use std::fmt;
use std::path::Path;

use image::{DynamicImage, ImageReader};
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
}

#[derive(Debug)]
pub enum EditorMediaError {
    Protocol(ratatui_image::errors::Errors),
    Decode(String),
    TooLarge { width: u32, height: u32 },
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
        }
    }
}

impl std::error::Error for EditorMediaError {}

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
}
