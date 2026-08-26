use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

use image::{DynamicImage, RgbaImage};
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui_image::FontSize;
use ratatui_image::Image;
use ratatui_image::Resize;
use ratatui_image::protocol::Protocol;
use ratatui_image::protocol::iterm2::Iterm2;
use ratatui_image::protocol::kitty::Kitty;
use ratatui_image::protocol::sixel::Sixel;

pub const EDITOR_IMAGE_MAX_PIXELS: u64 = 20_000_000;
static NEXT_KITTY_IMAGE_ID: AtomicU32 = AtomicU32::new(1);

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
    /// Builds the rendering-facing probe from pure capability values supplied
    /// by the Shell. Terminal I/O and platform detection stay outside UI.
    pub fn from_terminal_capabilities(
        protocol: EditorGraphicsProtocol,
        cell_width: u16,
        cell_height: u16,
        is_tmux: bool,
        text_sizing_protocol: bool,
    ) -> Self {
        Self {
            status: TerminalGraphicsProbeStatus::Verified(protocol),
            picker: Some(EditorImagePicker::from_terminal_capabilities(
                protocol,
                cell_width,
                cell_height,
                is_tmux,
            )),
            text_sizing_protocol,
        }
    }

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

    pub fn with_text_sizing_protocol(mut self, supported: bool) -> Self {
        self.text_sizing_protocol = supported;
        self
    }
}

#[derive(Debug, Clone)]
pub struct EditorImagePicker {
    font_size: FontSize,
    protocol: EditorGraphicsProtocol,
    is_tmux: bool,
}

impl EditorImagePicker {
    fn from_terminal_capabilities(
        protocol: EditorGraphicsProtocol,
        cell_width: u16,
        cell_height: u16,
        is_tmux: bool,
    ) -> Self {
        Self {
            font_size: FontSize::new(cell_width.max(1), cell_height.max(1)),
            protocol,
            is_tmux,
        }
    }

    pub fn protocol(&self) -> EditorGraphicsProtocol {
        self.protocol
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
        let resize = Resize::Fit(None);
        let size = resize.size_for(&image, self.font_size, area.as_size());
        let image = resize.resize(&image, self.font_size, size, None);
        let protocol = match self.protocol {
            EditorGraphicsProtocol::Kitty => Protocol::Kitty(
                Kitty::new(image, size, next_kitty_image_id(), self.is_tmux)
                    .map_err(EditorMediaError::Protocol)?,
            ),
            EditorGraphicsProtocol::Sixel => Protocol::Sixel(
                Sixel::new(image, size, self.is_tmux).map_err(EditorMediaError::Protocol)?,
            ),
            EditorGraphicsProtocol::Iterm2 => Protocol::ITerm2(
                Iterm2::new(image, size, self.is_tmux).map_err(EditorMediaError::Protocol)?,
            ),
        };
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
}

fn next_kitty_image_id() -> u32 {
    NEXT_KITTY_IMAGE_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            Some(id.saturating_add(1))
        })
        .expect("atomic image id update is infallible")
}

pub struct PreparedEditorImage {
    protocol: Protocol,
    kind: EditorGraphicsProtocol,
}

impl PreparedEditorImage {
    pub fn protocol(&self) -> EditorGraphicsProtocol {
        self.kind
    }

    pub fn render_size(&self) -> Size {
        self.protocol.size()
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
    fn pure_capabilities_select_protocols_and_measured_render_geometry() {
        for protocol in [
            EditorGraphicsProtocol::Kitty,
            EditorGraphicsProtocol::Sixel,
            EditorGraphicsProtocol::Iterm2,
        ] {
            let probe =
                TerminalGraphicsProbe::from_terminal_capabilities(protocol, 5, 10, true, false);
            let prepared = probe
                .picker()
                .unwrap()
                .prepare(DynamicImage::new_rgba8(100, 100), Rect::new(0, 0, 40, 40))
                .unwrap();
            assert_eq!(prepared.protocol(), protocol);
            assert_eq!(prepared.render_size(), Size::new(20, 10));
            assert!(matches!(
                (&prepared.protocol, protocol),
                (Protocol::Kitty(_), EditorGraphicsProtocol::Kitty)
                    | (Protocol::Sixel(_), EditorGraphicsProtocol::Sixel)
                    | (Protocol::ITerm2(_), EditorGraphicsProtocol::Iterm2)
            ));
        }

        let default = TerminalGraphicsProbe::from_terminal_capabilities(
            EditorGraphicsProtocol::Kitty,
            10,
            20,
            false,
            false,
        );
        let prepared = default
            .picker()
            .unwrap()
            .prepare(DynamicImage::new_rgba8(100, 100), Rect::new(0, 0, 40, 40))
            .unwrap();
        assert_eq!(prepared.render_size(), Size::new(10, 5));
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
