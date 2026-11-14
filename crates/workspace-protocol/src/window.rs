use super::{Hello, LayoutGeneration, WorkerId};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

mod cell_encoding;

/// Dimensions in terminal cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

impl Size {
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    pub const fn area(self) -> usize {
        self.width as usize * self.height as usize
    }
}

pub const MIN_CONTENT_SIZE: Size = Size::new(106, 16);

pub const MAX_IMAGE_BYTES: usize = 80_000_000;

/// Zero-based window-local coordinates and dimensions in terminal cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.width && y - self.y < self.height
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellToApp {
    Hello(Hello),
    Init(WindowInit),
    Viewport {
        layout: LayoutGeneration,
        /// `None` suspends viewport updates when the terminal cannot fit the workspace.
        size: Option<Size>,
        visible: bool,
        focused: bool,
    },
    Input {
        layout: LayoutGeneration,
        event: Value,
    },
    InputFence {
        serial: u64,
    },
    TaskAction {
        id: String,
        cancel: bool,
    },
    TaskCatalog {
        ids: Vec<String>,
    },
    Back {
        layout: LayoutGeneration,
    },
    Keyboard {
        layout: LayoutGeneration,
    },
    Help {
        layout: LayoutGeneration,
    },
    CancelPointer {
        layout: LayoutGeneration,
    },
    RequestClose,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppToShell {
    Welcome { worker: WorkerId },
    Frame(WindowFrame),
    InputFence { serial: u64 },
    TaskAction { id: String, cancel: bool },
    Status(WindowStatus),
    ToggleTasks,
    OpenApp(Value),
    LaunchFailed(String),
    BackgroundRequested,
    CloseRequested,
    CloseDeferred,
    CloseStarted,
    CloseCancelled,
    Closed,
    ShellAction(ShellAction),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellAction {
    SettingsChanged,
    ClockChanged,
    Logout,
    Restart,
    Reset,
    Exit,
    PowerOff,
    Reboot,
    UpdatePrepared(#[serde(with = "super::native_path")] PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInit {
    pub launch: Value,
    pub profile: Option<Value>,
    pub size: Size,
    pub layout: LayoutGeneration,
    pub focused: bool,
    #[serde(default)]
    pub image_cell_size: Option<[u16; 2]>,
    #[serde(default)]
    pub text_sizing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowStatus {
    pub minimum: Size,
    pub busy: bool,
    pub dirty: bool,
    #[serde(default)]
    pub running_operations: bool,
    #[serde(default)]
    pub awaiting_approval: bool,
    #[serde(default)]
    pub unresponsive: bool,
    #[serde(default)]
    pub task_ids: Vec<String>,
}

impl Default for WindowStatus {
    fn default() -> Self {
        Self {
            minimum: MIN_CONTENT_SIZE,
            busy: false,
            dirty: false,
            running_operations: false,
            awaiting_approval: false,
            unresponsive: false,
            task_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowFrame {
    pub layout: LayoutGeneration,
    pub size: Size,
    #[serde(default)]
    pub page: String,
    #[serde(default)]
    pub animation_running: bool,
    #[serde(default)]
    pub keyboard_available: bool,
    #[serde(default)]
    pub status: String,
    /// Exactly one cell per viewport position, in row-major order.
    #[serde(with = "cell_encoding")]
    pub cells: Vec<Cell>,
    pub cursor: Option<Cursor>,
    pub images: Vec<Image>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headings: Vec<Heading>,
    pub controls: Vec<ControlNode>,
    pub masks: Vec<Rect>,
    pub native_escape: Vec<Rect>,
    pub modal: bool,
}

impl WindowFrame {
    pub fn new(layout: LayoutGeneration, size: Size) -> Self {
        Self {
            layout,
            size,
            page: String::new(),
            animation_running: false,
            keyboard_available: false,
            status: String::new(),
            cells: vec![Cell::default(); size.area()],
            cursor: None,
            images: Vec::new(),
            headings: Vec::new(),
            controls: Vec::new(),
            masks: Vec::new(),
            native_escape: Vec::new(),
            modal: false,
        }
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.cells.len() != self.size.area() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid cell count",
            ));
        }
        if self
            .cells
            .iter()
            .any(|cell| cell.symbol.contains(char::is_control))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid control character",
            ));
        }
        if self
            .cursor
            .is_some_and(|cursor| cursor.x >= self.size.width || cursor.y >= self.size.height)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cursor outside window",
            ));
        }
        if self.headings.iter().any(|heading| {
            let rect = heading.area;
            rect.width == 0
                || rect.height != 2
                || !rect
                    .x
                    .checked_add(rect.width)
                    .is_some_and(|end| end <= self.size.width)
                || !rect
                    .y
                    .checked_add(rect.height)
                    .is_some_and(|end| end <= self.size.height)
                || !(1..=6).contains(&heading.level)
                || heading.text.contains(char::is_control)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid heading",
            ));
        }
        for image in &self.images {
            let pixels = u64::from(image.width) * u64::from(image.height);
            if pixels == 0
                || pixels > (MAX_IMAGE_BYTES / 4) as u64
                || (!image.rgba.is_empty()
                    && pixels.checked_mul(4) != Some(image.rgba.len() as u64))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid RGBA image",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    #[serde(default = "blank", skip_serializing_if = "is_blank")]
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Color::is_reset")]
    pub foreground: Color,
    #[serde(default, skip_serializing_if = "Color::is_reset")]
    pub background: Color,
    #[serde(default, skip_serializing_if = "Color::is_reset")]
    pub underline_color: Color,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub modifiers: u16,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            symbol: blank(),
            foreground: Color::Reset,
            background: Color::Reset,
            underline_color: Color::Reset,
            modifiers: 0,
            skip: false,
        }
    }
}

fn blank() -> String {
    " ".into()
}

fn is_blank(symbol: &str) -> bool {
    symbol == " "
}

fn is_zero(modifiers: &u16) -> bool {
    *modifiers == 0
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Color {
    #[default]
    Reset,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    fn is_reset(&self) -> bool {
        *self == Self::Reset
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub shape: CursorShape,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    #[default]
    Default,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub image_id: u64,
    pub width: u32,
    pub height: u32,
    #[serde(
        serialize_with = "serialize_pixels",
        deserialize_with = "deserialize_pixels"
    )]
    /// Pixels for a new image ID. Empty when an earlier frame supplied them.
    pub rgba: Arc<[u8]>,
    pub source: [u32; 4],
    pub padding: [u16; 4],
    pub cell_size: [u16; 2],
    pub area: Rect,
    pub visible: Vec<Rect>,
    pub z_index: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub area: Rect,
    pub text: String,
    pub level: u8,
    pub foreground: Color,
}

fn serialize_pixels<S: serde::Serializer>(pixels: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(&base64::display::Base64Display::new(pixels, &STANDARD))
}

fn deserialize_pixels<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<[u8]>, D::Error> {
    let pixels = std::borrow::Cow::<str>::deserialize(deserializer)?;
    STANDARD
        .decode(pixels.as_bytes())
        .map(Arc::from)
        .map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlNode {
    pub id: String,
    pub parent: Option<String>,
    pub role: String,
    pub name: String,
    pub text: String,
    pub rect: Rect,
    pub focused: bool,
    pub selected: bool,
    pub enabled: bool,
    pub actionable: bool,
    pub sensitive: bool,
    pub actions: Vec<String>,
}
