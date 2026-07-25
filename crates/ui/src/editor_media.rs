use std::fmt;
use std::path::Path;
#[cfg(unix)]
use std::time::{Duration, Instant};

use image::{DynamicImage, ImageReader, RgbaImage};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui_image::Image;
use ratatui_image::Resize;
#[cfg(unix)]
use ratatui_image::picker::cap_parser::{
    Parser as CapabilityParser, QueryStdioOptions, Response as CapabilityResponse,
};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;

pub const EDITOR_IMAGE_MAX_PIXELS: u64 = 20_000_000;
#[cfg(unix)]
const TERMINAL_CAPABILITY_QUERY_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorGraphicsProtocol {
    Kitty,
    Sixel,
    Iterm2,
}

impl EditorGraphicsProtocol {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Kitty => "Kitty",
            Self::Sixel => "Sixel",
            Self::Iterm2 => "iTerm2",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EditorImagePicker {
    picker: Picker,
    protocol: EditorGraphicsProtocol,
}

impl EditorImagePicker {
    /// Query after entering the alternate screen and before the event loop starts.
    /// Half-block rendering is intentionally treated as unsupported: the Editor
    /// contract requires raw Markdown fallback when no graphics protocol exists.
    pub fn detect_stdio() -> Result<Option<Self>, EditorMediaError> {
        #[cfg(unix)]
        let picker = query_unix_terminal_capabilities(TERMINAL_CAPABILITY_QUERY_TIMEOUT)?;
        #[cfg(not(unix))]
        let picker = Picker::from_query_stdio().map_err(EditorMediaError::Protocol)?;
        Self::from_picker(picker)
    }

    pub fn protocol(&self) -> EditorGraphicsProtocol {
        self.protocol
    }

    pub fn prepare_path(
        &self,
        path: &Path,
        area: Rect,
    ) -> Result<PreparedEditorImage, EditorMediaError> {
        let image = ImageReader::open(path)
            .map_err(|error| EditorMediaError::Decode(error.to_string()))?
            .with_guessed_format()
            .map_err(|error| EditorMediaError::Decode(error.to_string()))?
            .decode()
            .map_err(|error| EditorMediaError::Decode(error.to_string()))?;
        self.prepare(image, area)
    }

    pub fn prepare(
        &self,
        image: DynamicImage,
        area: Rect,
    ) -> Result<PreparedEditorImage, EditorMediaError> {
        let pixels = u64::from(image.width()).saturating_mul(u64::from(image.height()));
        if pixels > EDITOR_IMAGE_MAX_PIXELS {
            return Err(EditorMediaError::TooLarge {
                width: image.width(),
                height: image.height(),
            });
        }
        let protocol = self
            .picker
            .new_protocol(image, area, Resize::Fit(None))
            .map_err(EditorMediaError::Protocol)?;
        Ok(PreparedEditorImage {
            protocol,
            kind: self.protocol,
        })
    }

    /// Prepares a native RGBA icon for rendering through the detected terminal
    /// graphics protocol. `rgba` must contain exactly four bytes per pixel.
    ///
    /// This is intentionally owned input: platform icon APIs commonly hand out
    /// temporary buffers, while a prepared terminal image may outlive that API
    /// call until the next render pass.
    pub fn prepare_rgba(
        &self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        area: Rect,
    ) -> Result<PreparedEditorImage, EditorMediaError> {
        self.prepare(rgba_image(width, height, rgba)?, area)
    }

    fn from_picker(picker: Picker) -> Result<Option<Self>, EditorMediaError> {
        let protocol = match picker.protocol_type() {
            ProtocolType::Halfblocks => return Ok(None),
            ProtocolType::Kitty => EditorGraphicsProtocol::Kitty,
            ProtocolType::Sixel => EditorGraphicsProtocol::Sixel,
            ProtocolType::Iterm2 => EditorGraphicsProtocol::Iterm2,
        };
        Ok(Some(Self { picker, protocol }))
    }
}

pub struct PreparedEditorImage {
    protocol: Protocol,
    kind: EditorGraphicsProtocol,
}

impl PreparedEditorImage {
    pub fn protocol(&self) -> EditorGraphicsProtocol {
        self.kind
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Image::new(&self.protocol), area);
    }

    /// Render a fixed-size image centered inside a larger allocation.
    ///
    /// `ratatui-image` preserves the image aspect ratio but anchors the resulting
    /// protocol area at the allocation's left edge. Launcher tiles allocate the
    /// whole tile width to an icon, so center the actual protocol footprint here.
    pub fn render_centered(&self, frame: &mut Frame<'_>, area: Rect) {
        let centered = centered_protocol_area(area, self.protocol.area());
        frame.render_widget(Image::new(&self.protocol), centered);
    }
}

#[derive(Debug)]
pub enum EditorMediaError {
    Protocol(ratatui_image::errors::Errors),
    TerminalQuery(std::io::Error),
    Decode(String),
    TooLarge {
        width: u32,
        height: u32,
    },
    InvalidRgbaLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for EditorMediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => error.fmt(formatter),
            Self::TerminalQuery(error) => {
                write!(
                    formatter,
                    "could not query terminal image capabilities: {error}"
                )
            }
            Self::Decode(message) => write!(formatter, "could not decode image: {message}"),
            Self::TooLarge { width, height } => write!(
                formatter,
                "image dimensions {width}x{height} exceed the Editor safety limit"
            ),
            Self::InvalidRgbaLength {
                width,
                height,
                expected,
                actual,
            } => write!(
                formatter,
                "RGBA buffer for {width}x{height} image has {actual} bytes; expected {expected}"
            ),
        }
    }
}

impl std::error::Error for EditorMediaError {}

#[cfg(unix)]
fn query_unix_terminal_capabilities(timeout: Duration) -> Result<Picker, EditorMediaError> {
    use std::io::{self, Write};
    use std::os::fd::AsRawFd;

    let is_tmux = std::env::var_os("TMUX").is_some_and(|value| !value.is_empty());
    let query = CapabilityParser::query(
        is_tmux,
        QueryStdioOptions {
            text_sizing_protocol: false,
        },
    );
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(query.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(EditorMediaError::TerminalQuery)?;

    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();
    let deadline = Instant::now() + timeout;
    let mut parser = CapabilityParser::new();
    let mut responses = Vec::new();
    let mut complete = false;

    while !complete {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        let timeout_ms = remaining.as_millis().max(1).min(i32::MAX as u128) as i32;
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let poll_result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if poll_result == 0 {
            break;
        }
        if poll_result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(EditorMediaError::TerminalQuery(error));
        }
        if descriptor.revents & libc::POLLIN == 0 {
            break;
        }

        let mut buffer = [0_u8; 256];
        let read_result = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read_result == 0 {
            break;
        }
        if read_result < 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(EditorMediaError::TerminalQuery(error));
        }

        for byte in &buffer[..read_result as usize] {
            for response in parser.push(char::from(*byte)) {
                if response == CapabilityResponse::Status {
                    complete = true;
                    break;
                }
                responses.push(response);
            }
            if complete {
                break;
            }
        }
    }

    Ok(picker_from_terminal_responses(&responses))
}

#[cfg(unix)]
fn picker_from_terminal_responses(responses: &[CapabilityResponse]) -> Picker {
    let font_size = responses
        .iter()
        .find_map(|response| match response {
            CapabilityResponse::CellSize(Some(font_size)) => Some(*font_size),
            _ => None,
        })
        .or_else(terminal_font_size)
        .unwrap_or((10, 20));
    let mut picker = Picker::from_fontsize(font_size);

    // Explicit environment detection performed by `from_fontsize` takes
    // precedence for terminals such as iTerm2/WezTerm. Otherwise prefer Kitty
    // over Sixel, matching ratatui-image's normal capability selection.
    if picker.protocol_type() == ProtocolType::Halfblocks {
        if responses.contains(&CapabilityResponse::Kitty) {
            picker.set_protocol_type(ProtocolType::Kitty);
        } else if responses.contains(&CapabilityResponse::Sixel) {
            picker.set_protocol_type(ProtocolType::Sixel);
        }
    }
    picker
}

#[cfg(unix)]
fn terminal_font_size() -> Option<(u16, u16)> {
    let mut window = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut window) };
    if result < 0
        || window.ws_col == 0
        || window.ws_row == 0
        || window.ws_xpixel == 0
        || window.ws_ypixel == 0
    {
        return None;
    }
    Some((
        window.ws_xpixel / window.ws_col,
        window.ws_ypixel / window.ws_row,
    ))
}

fn centered_protocol_area(allocation: Rect, protocol_area: Rect) -> Rect {
    let width = protocol_area.width.min(allocation.width);
    let height = protocol_area.height.min(allocation.height);
    Rect::new(
        allocation
            .x
            .saturating_add(allocation.width.saturating_sub(width) / 2),
        allocation
            .y
            .saturating_add(allocation.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn rgba_image(width: u32, height: u32, rgba: Vec<u8>) -> Result<DynamicImage, EditorMediaError> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if pixels > EDITOR_IMAGE_MAX_PIXELS {
        return Err(EditorMediaError::TooLarge { width, height });
    }
    // The pixel limit guarantees this conversion and multiplication are safe on
    // every supported target, including 32-bit builds.
    let expected = usize::try_from(pixels.saturating_mul(4)).expect("bounded RGBA byte count");
    let actual = rgba.len();
    if actual != expected {
        return Err(EditorMediaError::InvalidRgbaLength {
            width,
            height,
            expected,
            actual,
        });
    }
    let image = RgbaImage::from_raw(width, height, rgba).expect("validated RGBA dimensions");
    Ok(DynamicImage::ImageRgba8(image))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halfblocks_are_reported_as_unsupported() {
        let picker = Picker::from_fontsize((8, 16));
        if picker.protocol_type() == ProtocolType::Halfblocks {
            assert!(EditorImagePicker::from_picker(picker).unwrap().is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn terminal_responses_prefer_kitty_then_sixel() {
        let picker = picker_from_terminal_responses(&[
            CapabilityResponse::Sixel,
            CapabilityResponse::Kitty,
            CapabilityResponse::CellSize(Some((9, 18))),
        ]);
        if Picker::from_fontsize((9, 18)).protocol_type() == ProtocolType::Halfblocks {
            assert_eq!(picker.protocol_type(), ProtocolType::Kitty);
            assert_eq!(picker.font_size(), (9, 18));
        }

        let picker = picker_from_terminal_responses(&[CapabilityResponse::Sixel]);
        if Picker::from_fontsize((10, 20)).protocol_type() == ProtocolType::Halfblocks {
            assert_eq!(picker.protocol_type(), ProtocolType::Sixel);
        }
    }

    #[test]
    fn rgba_preparation_rejects_wrong_buffer_length() {
        let error = rgba_image(2, 3, vec![0; 23]).unwrap_err();
        assert!(matches!(
            error,
            EditorMediaError::InvalidRgbaLength {
                width: 2,
                height: 3,
                expected: 24,
                actual: 23,
            }
        ));
    }

    #[test]
    fn rgba_preparation_constructs_an_rgba_image() {
        let image = rgba_image(2, 1, vec![255; 8]).expect("valid RGBA bytes");
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 1);
    }

    #[test]
    fn protocol_footprint_is_centered_and_clamped_inside_its_allocation() {
        assert_eq!(
            centered_protocol_area(Rect::new(10, 5, 20, 6), Rect::new(0, 0, 8, 4)),
            Rect::new(16, 6, 8, 4)
        );
        assert_eq!(
            centered_protocol_area(Rect::new(10, 5, 4, 2), Rect::new(0, 0, 8, 4)),
            Rect::new(10, 5, 4, 2)
        );
    }
}
