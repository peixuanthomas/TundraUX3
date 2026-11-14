use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use image::{DynamicImage, Pixel, Rgba, RgbaImage};
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui::style::{Color, Modifier};
use ratatui_image::FontSize;
use ratatui_image::Image;
use ratatui_image::Resize;
use ratatui_image::protocol::Protocol;
use ratatui_image::protocol::halfblocks::Halfblocks;
use ratatui_image::protocol::iterm2::Iterm2;
use ratatui_image::protocol::kitty::Kitty;
use ratatui_image::protocol::sixel::Sixel;

pub const EDITOR_IMAGE_MAX_PIXELS: u64 = 20_000_000;
static NEXT_KITTY_IMAGE_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        let Some(protocol) = picker.protocol() else {
            return Self::unsupported();
        };
        Self {
            status: TerminalGraphicsProbeStatus::Verified(protocol),
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
    protocol: Option<EditorGraphicsProtocol>,
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
            protocol: Some(protocol),
            is_tmux,
        }
    }

    /// Prepares icon pixels for the app host and leaves terminal encoding to Shell.
    pub fn for_workspace(cell_width: u16, cell_height: u16) -> Self {
        Self {
            font_size: FontSize::new(cell_width.max(1), cell_height.max(1)),
            protocol: None,
            is_tmux: false,
        }
    }

    pub fn protocol(&self) -> Option<EditorGraphicsProtocol> {
        self.protocol
    }

    pub fn cell_size(&self) -> (u16, u16) {
        (self.font_size.width, self.font_size.height)
    }

    pub(crate) fn is_tmux(&self) -> bool {
        self.is_tmux
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
        let Some(kind) = self.protocol else {
            return Ok(PreparedEditorImage {
                image: PreparedImage::Workspace(WorkspaceIcon {
                    source: image.into_rgba8(),
                    size,
                    cell_size: [self.font_size.width, self.font_size.height],
                    raster: RefCell::new(None),
                }),
            });
        };
        let protocol = match kind {
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
            image: PreparedImage::Native(protocol),
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

pub(crate) fn next_kitty_image_id() -> u32 {
    NEXT_KITTY_IMAGE_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            Some(id.saturating_add(1))
        })
        .expect("atomic image id update is infallible")
}

enum PreparedImage {
    Native(Protocol),
    Workspace(WorkspaceIcon),
}

pub struct PreparedEditorImage {
    image: PreparedImage,
}

impl PreparedEditorImage {
    pub fn protocol(&self) -> Option<EditorGraphicsProtocol> {
        match self.image {
            PreparedImage::Native(Protocol::Kitty(_)) => Some(EditorGraphicsProtocol::Kitty),
            PreparedImage::Native(Protocol::Sixel(_)) => Some(EditorGraphicsProtocol::Sixel),
            PreparedImage::Native(Protocol::ITerm2(_)) => Some(EditorGraphicsProtocol::Iterm2),
            _ => None,
        }
    }

    pub fn render_size(&self) -> Size {
        match &self.image {
            PreparedImage::Native(protocol) => protocol.size(),
            PreparedImage::Workspace(icon) => icon.size,
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        match &self.image {
            PreparedImage::Native(protocol) => frame.render_widget(Image::new(protocol), area),
            PreparedImage::Workspace(icon) => icon.render(frame, area),
        }
    }

    /// Render a fixed-size image centered inside a larger allocation.
    ///
    /// `ratatui-image` preserves the image aspect ratio but anchors the resulting
    /// protocol area at the allocation's left edge. Launcher and Home tiles
    /// allocate the whole tile width to an icon, so center the actual protocol
    /// footprint here.
    pub fn render_centered(&self, frame: &mut Frame<'_>, area: Rect) {
        self.render(frame, centered_protocol_area(area, self.render_size()));
    }
}

struct WorkspaceIcon {
    source: RgbaImage,
    size: Size,
    cell_size: [u16; 2],
    raster: RefCell<Option<WorkspaceIconRaster>>,
}

struct WorkspaceIconRaster {
    background: Color,
    image_id: u64,
    rgba: Arc<[u8]>,
    fallback: Protocol,
}

impl WorkspaceIcon {
    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        if self.size.width > area.width || self.size.height > area.height {
            return;
        }
        let area = Rect::new(area.x, area.y, self.size.width, self.size.height);
        if area.is_empty() || area.intersection(frame.area()) != area {
            return;
        }
        let cell = &frame.buffer_mut()[(area.x, area.y)];
        let background = if cell.modifier.contains(Modifier::REVERSED) {
            cell.fg
        } else {
            cell.bg
        };
        let mut cached = self.raster.borrow_mut();
        if cached
            .as_ref()
            .is_none_or(|cached| cached.background != background)
        {
            let (r, g, b) = crate::theme::color_rgb(background).unwrap_or_default();
            let mut pixels = self.source.clone();
            for pixel in pixels.pixels_mut() {
                let mut base = Rgba([r, g, b, 255]);
                base.blend(pixel);
                base[3] = 255;
                *pixel = base;
            }
            let rgba = Arc::from(pixels.as_raw().as_slice());
            let Ok(fallback) = Halfblocks::new(DynamicImage::ImageRgba8(pixels), self.size) else {
                return;
            };
            *cached = Some(WorkspaceIconRaster {
                background,
                image_id: (1 << 63) | u64::from(next_kitty_image_id()),
                rgba,
                fallback: Protocol::Halfblocks(fallback),
            });
        }
        let cached = cached.as_ref().unwrap();
        frame.render_widget(Image::new(&cached.fallback), area);
        WORKSPACE_ICONS.with_borrow_mut(|capture| {
            let Some((content, images)) = capture else {
                return;
            };
            if content.intersection(area) != area {
                return;
            }
            let area = workspace_protocol::Rect {
                x: area.x - content.x,
                y: area.y - content.y,
                width: area.width,
                height: area.height,
            };
            let (width, height) = self.source.dimensions();
            images.push(workspace_protocol::Image {
                image_id: cached.image_id,
                width,
                height,
                rgba: cached.rgba.clone(),
                source: [0, 0, width, height],
                padding: [0; 4],
                cell_size: self.cell_size,
                area,
                visible: vec![area],
                z_index: 0,
            });
        });
    }
}

thread_local! {
    static WORKSPACE_ICONS: RefCell<Option<(Rect, Vec<workspace_protocol::Image>)>> = const { RefCell::new(None) };
}

pub struct WorkspaceIconCapture;

impl WorkspaceIconCapture {
    pub fn begin(content: Rect) -> Self {
        WORKSPACE_ICONS.with_borrow_mut(|capture| *capture = Some((content, Vec::new())));
        Self
    }

    pub fn finish(self) -> Vec<workspace_protocol::Image> {
        WORKSPACE_ICONS.with_borrow_mut(|capture| capture.take().unwrap().1)
    }
}

impl Drop for WorkspaceIconCapture {
    fn drop(&mut self) {
        WORKSPACE_ICONS.with_borrow_mut(|capture| *capture = None);
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
#[path = "../tests/unit/editor_media/tests.rs"]
mod tests;
