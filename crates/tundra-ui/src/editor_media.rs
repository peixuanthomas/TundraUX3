use std::fmt;
use std::path::Path;

use image::{DynamicImage, ImageReader, RgbaImage};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui_image::Image;
use ratatui_image::Resize;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;

pub const EDITOR_IMAGE_MAX_PIXELS: u64 = 20_000_000;

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
