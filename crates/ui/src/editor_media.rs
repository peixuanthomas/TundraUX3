use std::fmt;
use std::path::Path;
#[cfg(unix)]
use std::time::{Duration, Instant};

use image::{DynamicImage, ImageReader, RgbaImage};
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui_image::Image;
use ratatui_image::Resize;
#[cfg(unix)]
use ratatui_image::picker::cap_parser::{
    Parser as CapabilityParser, QueryStdioOptions, Response as CapabilityResponse,
};
use ratatui_image::picker::{Capability, Picker, ProtocolType};
use ratatui_image::protocol::Protocol;

pub const EDITOR_IMAGE_MAX_PIXELS: u64 = 20_000_000;
#[cfg(unix)]
const TERMINAL_CAPABILITY_QUERY_TIMEOUT: Duration = Duration::from_secs(1);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalGraphicsProbeStatus {
    Verified(EditorGraphicsProtocol),
    Unsupported,
    NoResponse { reason: String },
}

impl TerminalGraphicsProbeStatus {
    pub const fn protocol(&self) -> Option<EditorGraphicsProtocol> {
        match self {
            Self::Verified(protocol) => Some(*protocol),
            Self::Unsupported | Self::NoResponse { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerminalGraphicsProbe {
    status: TerminalGraphicsProbeStatus,
    picker: Option<EditorImagePicker>,
    text_sizing_protocol: bool,
}

impl TerminalGraphicsProbe {
    pub fn verified(picker: EditorImagePicker) -> Self {
        Self {
            status: TerminalGraphicsProbeStatus::Verified(picker.protocol()),
            picker: Some(picker),
            text_sizing_protocol: false,
        }
    }

    pub fn unsupported() -> Self {
        Self {
            status: TerminalGraphicsProbeStatus::Unsupported,
            picker: None,
            text_sizing_protocol: false,
        }
    }

    pub fn no_response(reason: impl Into<String>) -> Self {
        Self {
            status: TerminalGraphicsProbeStatus::NoResponse {
                reason: reason.into(),
            },
            picker: None,
            text_sizing_protocol: false,
        }
    }

    pub fn status(&self) -> &TerminalGraphicsProbeStatus {
        &self.status
    }

    pub fn picker(&self) -> Option<&EditorImagePicker> {
        self.picker.as_ref()
    }

    pub const fn protocol(&self) -> Option<EditorGraphicsProtocol> {
        self.status.protocol()
    }

    pub const fn text_sizing_protocol(&self) -> bool {
        self.text_sizing_protocol
    }

    fn with_text_sizing_protocol(mut self, supported: bool) -> Self {
        self.text_sizing_protocol = supported;
        self
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
        match Self::probe_stdio() {
            TerminalGraphicsProbe {
                picker: Some(picker),
                ..
            } => Ok(Some(picker)),
            TerminalGraphicsProbe {
                status: TerminalGraphicsProbeStatus::Unsupported,
                ..
            } => Ok(None),
            TerminalGraphicsProbe {
                status: TerminalGraphicsProbeStatus::NoResponse { reason },
                ..
            } => Err(EditorMediaError::TerminalQuery(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                reason,
            ))),
            TerminalGraphicsProbe {
                status: TerminalGraphicsProbeStatus::Verified(_),
                picker: None,
                ..
            } => unreachable!("verified terminal graphics probes always carry a picker"),
        }
    }

    /// Performs a live capability handshake and preserves the distinction
    /// between an explicit text-only response and a terminal that never
    /// answered the query.
    pub fn probe_stdio() -> TerminalGraphicsProbe {
        #[cfg(unix)]
        {
            match query_unix_terminal_capabilities(TERMINAL_CAPABILITY_QUERY_TIMEOUT) {
                Ok(query) => terminal_probe_from_unix_query(query),
                Err(error) => TerminalGraphicsProbe::no_response(error.to_string()),
            }
        }
        #[cfg(not(unix))]
        {
            match Picker::from_query_stdio_with_options(
                ratatui_image::picker::cap_parser::QueryStdioOptions {
                    text_sizing_protocol: true,
                    ..Default::default()
                },
            ) {
                Ok(picker) => {
                    let text_sizing_protocol = picker
                        .capabilities()
                        .contains(&Capability::TextSizingProtocol);
                    match Self::from_picker(picker) {
                        Ok(Some(picker)) => TerminalGraphicsProbe::verified(picker)
                            .with_text_sizing_protocol(text_sizing_protocol),
                        Ok(None) => TerminalGraphicsProbe::unsupported()
                            .with_text_sizing_protocol(text_sizing_protocol),
                        Err(error) => TerminalGraphicsProbe::no_response(error.to_string()),
                    }
                }
                Err(error) => TerminalGraphicsProbe::no_response(error.to_string()),
            }
        }
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

    pub fn prepare_bytes(
        &self,
        bytes: &[u8],
        area: Rect,
    ) -> Result<PreparedEditorImage, EditorMediaError> {
        let image = image::load_from_memory(bytes)
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
            .new_protocol(image, area.as_size(), Resize::Fit(None))
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
    /// protocol area at the allocation's left edge. Launcher and Home tiles
    /// allocate the whole tile width to an icon, so center the actual protocol
    /// footprint here.
    pub fn render_centered(&self, frame: &mut Frame<'_>, area: Rect) {
        let centered = centered_protocol_area(area, self.protocol.size());
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
struct UnixTerminalCapabilityQuery {
    picker: Picker,
    complete: bool,
    had_unverified_graphics_hint: bool,
    text_sizing_protocol: bool,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Iterm2GraphicsCapabilities {
    file: bool,
    sixel: bool,
}

#[cfg(unix)]
fn query_unix_terminal_capabilities(
    timeout: Duration,
) -> Result<UnixTerminalCapabilityQuery, EditorMediaError> {
    use std::io::{self, Write};
    use std::os::fd::AsRawFd;

    let mut picker = Picker::from_query_stdio_with_options(QueryStdioOptions {
        timeout,
        text_sizing_protocol: true,
        ..Default::default()
    })
    .map_err(EditorMediaError::Protocol)?;
    let text_sizing_protocol = picker
        .capabilities()
        .contains(&Capability::TextSizingProtocol);
    let is_tmux = std::env::var_os("TMUX").is_some_and(|value| !value.is_empty());
    let confirmation_query = graphics_capability_confirmation_query(is_tmux);
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(confirmation_query.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(EditorMediaError::TerminalQuery)?;

    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();
    let deadline = Instant::now() + timeout;
    let mut parser = CapabilityParser::new();
    let mut responses = Vec::new();
    let mut raw_responses = Vec::new();
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
            if raw_responses.len() < 8 * 1024 {
                raw_responses.push(*byte);
            }
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

    let iterm2_graphics = parse_iterm2_graphics_capabilities(&raw_responses);
    let standard_protocol = standard_protocol_from_capabilities(picker.capabilities());
    let iterm2_protocol = iterm2_graphics.and_then(|capabilities| {
        capabilities
            .sixel
            .then_some(ProtocolType::Sixel)
            .or_else(|| capabilities.file.then_some(ProtocolType::Iterm2))
    });
    let had_unverified_graphics_hint = standard_protocol.is_none()
        && iterm2_protocol.is_none()
        && picker.protocol_type() != ProtocolType::Halfblocks;
    picker.set_protocol_type(
        standard_protocol
            .or(iterm2_protocol)
            .unwrap_or(ProtocolType::Halfblocks),
    );

    Ok(UnixTerminalCapabilityQuery {
        picker,
        complete,
        had_unverified_graphics_hint,
        text_sizing_protocol,
    })
}

#[cfg(unix)]
fn terminal_probe_from_unix_query(query: UnixTerminalCapabilityQuery) -> TerminalGraphicsProbe {
    let has_live_protocol_response = query.picker.protocol_type() != ProtocolType::Halfblocks;
    let text_sizing_protocol = query.text_sizing_protocol;
    let probe = match EditorImagePicker::from_picker(query.picker) {
        Ok(Some(picker)) if query.complete || has_live_protocol_response => {
            TerminalGraphicsProbe::verified(picker)
        }
        Ok(_) if query.complete && query.had_unverified_graphics_hint => {
            TerminalGraphicsProbe::no_response(
                "terminal responded, but the hinted graphics protocol did not answer its capability query",
            )
        }
        Ok(_) if query.complete => TerminalGraphicsProbe::unsupported(),
        Ok(_) => TerminalGraphicsProbe::no_response(
            "terminal did not return the graphics capability query terminator",
        ),
        Err(error) => TerminalGraphicsProbe::no_response(error.to_string()),
    };
    probe.with_text_sizing_protocol(text_sizing_protocol)
}

#[cfg(unix)]
fn graphics_capability_confirmation_query(is_tmux: bool) -> String {
    let (start, escape, end) = CapabilityParser::tmux_start_escape_end(is_tmux);
    format!("{start}{escape}]1337;Capabilities{escape}\\{escape}[0n{end}")
}

#[cfg(unix)]
fn parse_iterm2_graphics_capabilities(response: &[u8]) -> Option<Iterm2GraphicsCapabilities> {
    const PREFIX: &[u8] = b"\x1b]1337;Capabilities=";
    let start = response
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)?
        + PREFIX.len();
    let tail = &response[start..];
    let end = tail
        .iter()
        .position(|byte| *byte == b'\x07')
        .or_else(|| tail.windows(2).position(|window| window == b"\x1b\\"))?;
    let features = tail[..end]
        .iter()
        .copied()
        .take_while(u8::is_ascii_alphanumeric)
        .collect::<Vec<_>>();
    Some(Iterm2GraphicsCapabilities {
        file: iterm2_feature_present(&features, b"F"),
        sixel: iterm2_feature_present(&features, b"Sx"),
    })
}

#[cfg(unix)]
fn iterm2_feature_present(features: &[u8], expected: &[u8]) -> bool {
    let mut start = 0;
    for end in 1..=features.len() {
        if end == features.len() || features[end].is_ascii_uppercase() {
            if &features[start..end] == expected {
                return true;
            }
            start = end;
        }
    }
    false
}

#[cfg(unix)]
fn standard_protocol_from_capabilities(capabilities: &[Capability]) -> Option<ProtocolType> {
    if capabilities.contains(&Capability::Kitty) {
        Some(ProtocolType::Kitty)
    } else if capabilities.contains(&Capability::Sixel) {
        Some(ProtocolType::Sixel)
    } else {
        None
    }
}

fn centered_protocol_area(allocation: Rect, protocol_size: Size) -> Rect {
    let width = protocol_size.width.min(allocation.width);
    let height = protocol_size.height.min(allocation.height);
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
        assert!(
            EditorImagePicker::from_picker(Picker::halfblocks())
                .unwrap()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn standard_capabilities_prefer_kitty_then_sixel() {
        assert_eq!(
            standard_protocol_from_capabilities(&[Capability::Sixel, Capability::Kitty]),
            Some(ProtocolType::Kitty)
        );
        assert_eq!(
            standard_protocol_from_capabilities(&[Capability::Sixel]),
            Some(ProtocolType::Sixel)
        );
    }

    #[cfg(unix)]
    #[test]
    fn terminal_probe_distinguishes_unsupported_from_no_response() {
        let text_only_picker = Picker::halfblocks();
        let unsupported = terminal_probe_from_unix_query(UnixTerminalCapabilityQuery {
            picker: text_only_picker.clone(),
            complete: true,
            had_unverified_graphics_hint: false,
            text_sizing_protocol: false,
        });
        assert_eq!(
            unsupported.status(),
            &TerminalGraphicsProbeStatus::Unsupported
        );

        let no_response = terminal_probe_from_unix_query(UnixTerminalCapabilityQuery {
            picker: text_only_picker,
            complete: false,
            had_unverified_graphics_hint: false,
            text_sizing_protocol: false,
        });
        assert!(matches!(
            no_response.status(),
            TerminalGraphicsProbeStatus::NoResponse { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn live_protocol_response_verifies_without_the_final_terminator() {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let verified = terminal_probe_from_unix_query(UnixTerminalCapabilityQuery {
            picker,
            complete: false,
            had_unverified_graphics_hint: false,
            text_sizing_protocol: false,
        });
        assert_eq!(
            verified.status(),
            &TerminalGraphicsProbeStatus::Verified(EditorGraphicsProtocol::Kitty)
        );
    }

    #[cfg(unix)]
    #[test]
    fn iterm2_capability_response_reports_graphics_protocol_support() {
        assert_eq!(
            parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=AFN\x1b\\"),
            Some(Iterm2GraphicsCapabilities {
                file: true,
                sixel: false,
            })
        );
        assert_eq!(
            parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=ASxN\x07"),
            Some(Iterm2GraphicsCapabilities {
                file: false,
                sixel: true,
            })
        );
        assert_eq!(
            parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=FooN\x07"),
            Some(Iterm2GraphicsCapabilities {
                file: false,
                sixel: false,
            })
        );
        assert_eq!(parse_iterm2_graphics_capabilities(b"\x1b[?1;0c"), None);
    }

    #[cfg(unix)]
    #[test]
    fn unconfirmed_environment_graphics_hint_is_no_response() {
        let mut hinted_picker = Picker::halfblocks();
        hinted_picker.set_protocol_type(ProtocolType::Halfblocks);
        let probe = terminal_probe_from_unix_query(UnixTerminalCapabilityQuery {
            picker: hinted_picker,
            complete: true,
            had_unverified_graphics_hint: true,
            text_sizing_protocol: false,
        });
        assert!(matches!(
            probe.status(),
            TerminalGraphicsProbeStatus::NoResponse { .. }
        ));
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
            centered_protocol_area(
                Rect::new(10, 5, 20, 6),
                Size {
                    width: 8,
                    height: 4,
                },
            ),
            Rect::new(16, 6, 8, 4)
        );
        assert_eq!(
            centered_protocol_area(
                Rect::new(10, 5, 4, 2),
                Size {
                    width: 8,
                    height: 4,
                },
            ),
            Rect::new(10, 5, 4, 2)
        );
    }
}
