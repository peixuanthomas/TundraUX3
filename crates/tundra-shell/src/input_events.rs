use crossterm::event::{KeyEventKind, KeyModifiers, MouseButton};
use std::time::Duration;

pub const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
pub(crate) const DOUBLE_CLICK_CELL_TOLERANCE: u16 = 1;

pub type CellPosition = (u16, u16);
pub type ShellInput = InputEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct InputModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
    pub hyper: bool,
    pub meta: bool,
}

impl InputModifiers {
    pub const fn none() -> Self {
        Self {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
            hyper: false,
            meta: false,
        }
    }
}

impl From<KeyModifiers> for InputModifiers {
    fn from(modifiers: KeyModifiers) -> Self {
        Self {
            shift: modifiers.contains(KeyModifiers::SHIFT),
            control: modifiers.contains(KeyModifiers::CONTROL),
            alt: modifiers.contains(KeyModifiers::ALT),
            super_key: modifiers.contains(KeyModifiers::SUPER),
            hyper: modifiers.contains(KeyModifiers::HYPER),
            meta: modifiers.contains(KeyModifiers::META),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InputKey {
    Character(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    BackTab,
    Left,
    Right,
    Up,
    Down,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Function(u8),
    Other(String),
}

impl InputKey {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Character(character) => character.to_string(),
            Self::Enter => "Enter".to_string(),
            Self::Escape => "Esc".to_string(),
            Self::Backspace => "Backspace".to_string(),
            Self::Tab => "Tab".to_string(),
            Self::BackTab => "Shift+Tab".to_string(),
            Self::Left => "Left".to_string(),
            Self::Right => "Right".to_string(),
            Self::Up => "Up".to_string(),
            Self::Down => "Down".to_string(),
            Self::Delete => "Delete".to_string(),
            Self::Insert => "Insert".to_string(),
            Self::Home => "Home".to_string(),
            Self::End => "End".to_string(),
            Self::PageUp => "PageUp".to_string(),
            Self::PageDown => "PageDown".to_string(),
            Self::Function(number) => format!("F({number})"),
            Self::Other(label) => label.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputPhase {
    Press,
    Repeat,
    Release,
}

impl InputPhase {
    pub(crate) const fn is_press_like(self) -> bool {
        matches!(self, Self::Press | Self::Repeat)
    }
}

impl From<KeyEventKind> for InputPhase {
    fn from(kind: KeyEventKind) -> Self {
        match kind {
            KeyEventKind::Press => Self::Press,
            KeyEventKind::Repeat => Self::Repeat,
            KeyEventKind::Release => Self::Release,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyInput {
    pub key: InputKey,
    pub modifiers: InputModifiers,
    pub phase: InputPhase,
}

impl KeyInput {
    pub fn new(key: InputKey, modifiers: InputModifiers, phase: InputPhase) -> Self {
        Self {
            key,
            modifiers,
            phase,
        }
    }

    pub fn from_label(label: impl AsRef<str>) -> Self {
        let label = label.as_ref();
        let (key, modifiers) = match label {
            "Ctrl+C" => (
                InputKey::Character('c'),
                InputModifiers {
                    control: true,
                    ..InputModifiers::none()
                },
            ),
            "Enter" => (InputKey::Enter, InputModifiers::none()),
            "Esc" => (InputKey::Escape, InputModifiers::none()),
            "Backspace" => (InputKey::Backspace, InputModifiers::none()),
            "Tab" => (InputKey::Tab, InputModifiers::none()),
            "Shift+Tab" => (
                InputKey::BackTab,
                InputModifiers {
                    shift: true,
                    ..InputModifiers::none()
                },
            ),
            "Left" => (InputKey::Left, InputModifiers::none()),
            "Right" => (InputKey::Right, InputModifiers::none()),
            "Up" => (InputKey::Up, InputModifiers::none()),
            "Down" => (InputKey::Down, InputModifiers::none()),
            "Delete" => (InputKey::Delete, InputModifiers::none()),
            "Home" => (InputKey::Home, InputModifiers::none()),
            "End" => (InputKey::End, InputModifiers::none()),
            "PageUp" => (InputKey::PageUp, InputModifiers::none()),
            "PageDown" => (InputKey::PageDown, InputModifiers::none()),
            function
                if function
                    .strip_prefix('F')
                    .and_then(|number| number.parse::<u8>().ok())
                    .is_some() =>
            {
                (
                    InputKey::Function(
                        function[1..]
                            .parse::<u8>()
                            .expect("function key guard validated the number"),
                    ),
                    InputModifiers::none(),
                )
            }
            single if single.chars().count() == 1 => (
                InputKey::Character(single.chars().next().expect("single char")),
                InputModifiers::none(),
            ),
            other => (InputKey::Other(other.to_string()), InputModifiers::none()),
        };

        Self::new(key, modifiers, InputPhase::Press)
    }

    pub fn label(&self) -> String {
        if matches!(&self.key, InputKey::BackTab) {
            return "Shift+Tab".to_string();
        }

        if self.modifiers.control
            && !self.modifiers.alt
            && !self.modifiers.shift
            && !self.modifiers.super_key
            && !self.modifiers.hyper
            && !self.modifiers.meta
            && let InputKey::Character(character) = &self.key
        {
            return format!("Ctrl+{}", character.to_ascii_uppercase());
        }

        let mut parts = Vec::new();
        if self.modifiers.control {
            parts.push("Ctrl");
        }
        if self.modifiers.alt {
            parts.push("Alt");
        }
        if self.modifiers.super_key {
            parts.push("Super");
        }
        if self.modifiers.hyper {
            parts.push("Hyper");
        }
        if self.modifiers.meta {
            parts.push("Meta");
        }
        if self.modifiers.shift {
            parts.push("Shift");
        }

        let key = self.key.label();
        if parts.is_empty() {
            key
        } else {
            parts.push(key.as_str());
            parts.join("+")
        }
    }

    pub(crate) fn is_ctrl_c(&self) -> bool {
        matches!(&self.key, InputKey::Character('c' | 'C')) && self.modifiers.control
    }

    pub(crate) fn is_character(&self, expected: char) -> bool {
        matches!(&self.key, InputKey::Character(character) if *character == expected)
    }

    pub(crate) fn has_non_shift_modifier(&self) -> bool {
        self.modifiers.control
            || self.modifiers.alt
            || self.modifiers.super_key
            || self.modifiers.hyper
            || self.modifiers.meta
    }

    pub(crate) fn is_unmodified_action_key(&self) -> bool {
        !self.has_non_shift_modifier() && !self.modifiers.shift
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

impl PointerButton {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Middle => "Middle",
        }
    }
}

impl From<MouseButton> for PointerButton {
    fn from(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => Self::Left,
            MouseButton::Right => Self::Right,
            MouseButton::Middle => Self::Middle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollDirection {
    Down,
    Up,
    Left,
    Right,
}

impl ScrollDirection {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Down => "Down",
            Self::Up => "Up",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DragDirection {
    Up,
    Down,
    Left,
    Right,
}

impl DragDirection {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseInput {
    Down {
        button: PointerButton,
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
    Up {
        button: PointerButton,
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
    Drag {
        button: PointerButton,
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
    Moved {
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
    Scroll {
        direction: ScrollDirection,
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
}

impl MouseInput {
    pub fn coordinates(self) -> CellPosition {
        match self {
            Self::Down { coordinates, .. }
            | Self::Up { coordinates, .. }
            | Self::Drag { coordinates, .. }
            | Self::Moved { coordinates, .. }
            | Self::Scroll { coordinates, .. } => coordinates,
        }
    }

    pub fn scroll_direction(self) -> Option<ScrollDirection> {
        match self {
            Self::Scroll { direction, .. } => Some(direction),
            _ => None,
        }
    }

    pub fn summary(self) -> String {
        match self {
            Self::Down { button, .. } => format!("Mouse Down {}", button.label()),
            Self::Up { button, .. } => format!("Mouse Up {}", button.label()),
            Self::Drag { button, .. } => format!("Mouse Drag {}", button.label()),
            Self::Moved { .. } => "Mouse Moved".to_string(),
            Self::Scroll { direction, .. } => format!("Mouse Scroll {}", direction.label()),
        }
    }

    pub(crate) fn down_button(self) -> Option<PointerButton> {
        match self {
            Self::Down { button, .. } => Some(button),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyInput),
    Mouse(MouseInput),
    Resize { width: u16, height: u16 },
    FocusGained,
    FocusLost,
    Paste(String),
    Tick,
    Shutdown,
}

impl InputEvent {
    pub fn from_key_label(label: impl AsRef<str>) -> Self {
        Self::Key(KeyInput::from_label(label))
    }

    pub fn mouse_down(button: PointerButton, coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Down {
            button,
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }

    pub fn mouse_up(button: PointerButton, coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Up {
            button,
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }

    pub fn mouse_drag(button: PointerButton, coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Drag {
            button,
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }

    pub fn mouse_moved(coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Moved {
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }

    pub fn mouse_scroll(direction: ScrollDirection, coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Scroll {
            direction,
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }
}
