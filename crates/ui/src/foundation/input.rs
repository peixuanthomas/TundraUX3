//! Normalized, framework-level input events.
//!
//! This is the single input model shared by UI routing and reusable components.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UiId(String);

impl UiId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<&str> for UiId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for UiId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for UiId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    pub column: u16,
    pub row: u16,
}

impl Point {
    pub fn new(column: u16, row: u16) -> Self {
        Self { column, row }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    BackTab,
    Delete,
    Insert,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
    F(u8),
    Other(String),
}

pub type KeyCode = Key;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyModifiers {
    pub shift: bool,
    /// Preferred spelling for the Control modifier.
    pub control: bool,
    /// Backwards-compatible spelling for [`Self::control`].
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
    pub hyper: bool,
    pub meta: bool,
}

impl KeyModifiers {
    pub const NONE: Self = Self {
        shift: false,
        control: false,
        ctrl: false,
        alt: false,
        super_key: false,
        hyper: false,
        meta: false,
    };

    pub const CTRL: Self = Self {
        shift: false,
        control: true,
        ctrl: true,
        alt: false,
        super_key: false,
        hyper: false,
        meta: false,
    };

    pub const ALT: Self = Self {
        shift: false,
        control: false,
        ctrl: false,
        alt: true,
        super_key: false,
        hyper: false,
        meta: false,
    };

    pub const SHIFT: Self = Self {
        shift: true,
        control: false,
        ctrl: false,
        alt: false,
        super_key: false,
        hyper: false,
        meta: false,
    };

    pub const CTRL_SHIFT: Self = Self {
        shift: true,
        control: true,
        ctrl: true,
        alt: false,
        super_key: false,
        hyper: false,
        meta: false,
    };

    pub fn new(ctrl: bool, alt: bool, shift: bool) -> Self {
        Self {
            shift,
            control: ctrl,
            ctrl,
            alt,
            super_key: false,
            hyper: false,
            meta: false,
        }
    }

    pub fn ctrl() -> Self {
        Self::CTRL
    }

    pub fn shift() -> Self {
        Self::SHIFT
    }

    pub fn alt() -> Self {
        Self::ALT
    }

    pub fn is_control(self) -> bool {
        self.control || self.ctrl
    }

    pub fn has_non_shift_modifier(self) -> bool {
        self.is_control() || self.alt || self.super_key || self.hyper || self.meta
    }

    pub const fn none() -> Self {
        Self::NONE
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyStroke {
    pub key: Key,
    pub modifiers: KeyModifiers,
}

impl KeyStroke {
    pub fn new(key: Key, modifiers: KeyModifiers) -> Self {
        Self { key, modifiers }
    }

    pub fn plain(key: Key) -> Self {
        Self::new(key, KeyModifiers::NONE)
    }

    pub fn char(character: char) -> Self {
        Self::plain(Key::Char(character))
    }

    pub fn ctrl_char(character: char) -> Self {
        Self::new(Key::Char(character), KeyModifiers::CTRL)
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.is_control() {
            parts.push("Ctrl".to_string());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_string());
        }
        if self.modifiers.super_key {
            parts.push("Super".to_string());
        }
        if self.modifiers.hyper {
            parts.push("Hyper".to_string());
        }
        if self.modifiers.meta {
            parts.push("Meta".to_string());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_string());
        }
        parts.push(key_label(&self.key));
        parts.join("+")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputPhase {
    Press,
    Repeat,
    Release,
}

impl InputPhase {
    pub const fn is_press_like(self) -> bool {
        matches!(self, Self::Press | Self::Repeat)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: KeyModifiers,
    pub phase: InputPhase,
}

impl KeyEvent {
    pub fn new(key: Key) -> Self {
        Self::with_phase(key, KeyModifiers::NONE, InputPhase::Press)
    }

    pub fn with_modifiers(key: Key, modifiers: KeyModifiers) -> Self {
        Self::with_phase(key, modifiers, InputPhase::Press)
    }

    pub fn with_phase(key: Key, modifiers: KeyModifiers, phase: InputPhase) -> Self {
        Self {
            key,
            modifiers,
            phase,
        }
    }

    pub fn repeated(mut self) -> Self {
        self.phase = InputPhase::Repeat;
        self
    }

    pub fn released(mut self) -> Self {
        self.phase = InputPhase::Release;
        self
    }

    pub fn is_press_like(&self) -> bool {
        self.phase.is_press_like()
    }

    pub fn stroke(&self) -> KeyStroke {
        KeyStroke::new(self.key.clone(), self.modifiers)
    }

    pub fn from_label(label: impl AsRef<str>) -> Self {
        let label = label.as_ref();
        let (key, modifiers) = match label {
            "Ctrl+C" => (Key::Char('c'), KeyModifiers::CTRL),
            "Enter" => (Key::Enter, KeyModifiers::NONE),
            "Esc" => (Key::Escape, KeyModifiers::NONE),
            "Backspace" => (Key::Backspace, KeyModifiers::NONE),
            "Tab" => (Key::Tab, KeyModifiers::NONE),
            "Shift+Tab" => (Key::BackTab, KeyModifiers::SHIFT),
            "Left" => (Key::Left, KeyModifiers::NONE),
            "Right" => (Key::Right, KeyModifiers::NONE),
            "Up" => (Key::Up, KeyModifiers::NONE),
            "Down" => (Key::Down, KeyModifiers::NONE),
            "Delete" => (Key::Delete, KeyModifiers::NONE),
            "Insert" => (Key::Insert, KeyModifiers::NONE),
            "Home" => (Key::Home, KeyModifiers::NONE),
            "End" => (Key::End, KeyModifiers::NONE),
            "PageUp" => (Key::PageUp, KeyModifiers::NONE),
            "PageDown" => (Key::PageDown, KeyModifiers::NONE),
            function
                if function
                    .strip_prefix('F')
                    .and_then(|number| number.parse::<u8>().ok())
                    .is_some() =>
            {
                (
                    Key::F(
                        function[1..]
                            .parse()
                            .expect("function key guard validated the number"),
                    ),
                    KeyModifiers::NONE,
                )
            }
            single if single.chars().count() == 1 => (
                Key::Char(single.chars().next().expect("single char")),
                KeyModifiers::NONE,
            ),
            other => (Key::Other(other.to_string()), KeyModifiers::NONE),
        };
        Self::with_phase(key, modifiers, InputPhase::Press)
    }

    pub fn label(&self) -> String {
        if matches!(self.key, Key::BackTab) {
            return "Shift+Tab".to_string();
        }

        if let Key::F(number) = &self.key {
            return format!("F({number})");
        }

        if self.modifiers.is_control()
            && !self.modifiers.alt
            && !self.modifiers.shift
            && !self.modifiers.super_key
            && !self.modifiers.hyper
            && !self.modifiers.meta
            && let Key::Char(character) = self.key
        {
            return format!("Ctrl+{}", character.to_ascii_uppercase());
        }

        self.stroke().label()
    }
    pub fn is_ctrl_c(&self) -> bool {
        matches!(self.key, Key::Char('c' | 'C')) && self.modifiers.is_control()
    }
    pub fn is_character(&self, expected: char) -> bool {
        matches!(self.key, Key::Char(character) if character == expected)
    }
    pub fn has_non_shift_modifier(&self) -> bool {
        self.modifiers.has_non_shift_modifier()
    }
    pub fn is_unmodified_action_key(&self) -> bool {
        !self.has_non_shift_modifier() && !self.modifiers.shift
    }
}

impl From<&KeyEvent> for KeyStroke {
    fn from(value: &KeyEvent) -> Self {
        value.stroke()
    }
}

impl Key {
    pub fn label(&self) -> String {
        key_label(self)
    }
}

/// Compatibility alias for the component interaction API.
pub type KeyInput = KeyEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Middle => "Middle",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseEventKind {
    Moved,
    Down(MouseButton),
    Up(MouseButton),
    Click(MouseButton),
    DoubleClick(MouseButton),
    Drag(MouseButton),
    Scroll(ScrollDirection),
}

pub type MouseAction = MouseEventKind;
/// Compatibility alias for the component interaction API.
pub type MouseKind = MouseEventKind;

impl MouseEventKind {
    #[allow(non_upper_case_globals)]
    pub const Move: Self = Self::Moved;
    #[allow(non_upper_case_globals)]
    #[allow(non_upper_case_globals)]
    pub const Hover: Self = Self::Moved;
    #[allow(non_upper_case_globals)]
    pub const ScrollUp: Self = Self::Scroll(ScrollDirection::Up);
    #[allow(non_upper_case_globals)]
    pub const ScrollDown: Self = Self::Scroll(ScrollDirection::Down);
    #[allow(non_upper_case_globals)]
    pub const ScrollLeft: Self = Self::Scroll(ScrollDirection::Left);
    #[allow(non_upper_case_globals)]
    pub const ScrollRight: Self = Self::Scroll(ScrollDirection::Right);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    pub position: Point,
    pub kind: MouseEventKind,
    pub modifiers: KeyModifiers,
}

impl MouseEvent {
    pub fn new(column: u16, row: u16, kind: MouseEventKind) -> Self {
        Self {
            position: Point::new(column, row),
            kind,
            modifiers: KeyModifiers::NONE,
        }
    }

    pub fn with_modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    pub fn column(&self) -> u16 {
        self.position.column
    }

    pub fn row(&self) -> u16 {
        self.position.row
    }

    pub fn hover(column: u16, row: u16) -> Self {
        Self::new(column, row, MouseEventKind::Moved)
    }

    pub fn moved(column: u16, row: u16) -> Self {
        Self::hover(column, row)
    }

    pub fn down(column: u16, row: u16, button: MouseButton) -> Self {
        Self::new(column, row, MouseEventKind::Down(button))
    }

    pub fn up(column: u16, row: u16, button: MouseButton) -> Self {
        Self::new(column, row, MouseEventKind::Up(button))
    }

    pub fn drag(column: u16, row: u16, button: MouseButton) -> Self {
        Self::new(column, row, MouseEventKind::Drag(button))
    }

    pub fn click(column: u16, row: u16, button: MouseButton) -> Self {
        Self::new(column, row, MouseEventKind::Click(button))
    }

    pub fn double_click(column: u16, row: u16, button: MouseButton) -> Self {
        Self::new(column, row, MouseEventKind::DoubleClick(button))
    }

    pub fn right_click(column: u16, row: u16) -> Self {
        Self::click(column, row, MouseButton::Right)
    }

    pub fn scroll(column: u16, row: u16, direction: ScrollDirection) -> Self {
        Self::new(column, row, MouseEventKind::Scroll(direction))
    }

    pub fn is_primary_click(self) -> bool {
        matches!(
            self.kind,
            MouseEventKind::Click(MouseButton::Left)
                | MouseEventKind::DoubleClick(MouseButton::Left)
        )
    }

    pub fn is_primary_down(self) -> bool {
        matches!(self.kind, MouseEventKind::Down(MouseButton::Left))
    }

    pub fn is_primary_up(self) -> bool {
        matches!(self.kind, MouseEventKind::Up(MouseButton::Left))
    }

    pub fn is_context_request(self) -> bool {
        matches!(
            self.kind,
            MouseEventKind::Click(MouseButton::Right)
                | MouseEventKind::Down(MouseButton::Right)
                | MouseEventKind::DoubleClick(MouseButton::Right)
        )
    }

    pub fn coordinates(self) -> (u16, u16) {
        (self.position.column, self.position.row)
    }

    pub fn scroll_direction(self) -> Option<ScrollDirection> {
        match self.kind {
            MouseEventKind::Scroll(direction) => Some(direction),
            _ => None,
        }
    }

    pub fn down_button(self) -> Option<MouseButton> {
        match self.kind {
            MouseEventKind::Down(button) => Some(button),
            _ => None,
        }
    }

    pub fn summary(self) -> String {
        match self.kind {
            MouseEventKind::Down(button) => format!("Mouse Down {}", button.label()),
            MouseEventKind::Up(button) => format!("Mouse Up {}", button.label()),
            MouseEventKind::Drag(button) => format!("Mouse Drag {}", button.label()),
            MouseEventKind::Moved => "Mouse Moved".to_string(),
            MouseEventKind::Click(button) => format!("Mouse Click {}", button.label()),
            MouseEventKind::DoubleClick(button) => format!("Mouse Double Click {}", button.label()),
            MouseEventKind::Scroll(direction) => format!("Mouse Scroll {:?}", direction),
        }
    }
}

/// Compatibility alias for the component interaction API.
pub type MouseInput = MouseEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize { width: u16, height: u16 },
    FocusGained,
    FocusLost,
    Paste(String),
    Tick,
    Shutdown,
}

impl InputEvent {
    pub fn key(key: Key) -> Self {
        Self::Key(KeyEvent::new(key))
    }

    pub fn key_with_modifiers(key: Key, modifiers: KeyModifiers) -> Self {
        Self::Key(KeyEvent::with_modifiers(key, modifiers))
    }

    pub fn key_with_phase(key: Key, modifiers: KeyModifiers, phase: InputPhase) -> Self {
        Self::Key(KeyEvent::with_phase(key, modifiers, phase))
    }

    pub fn mouse(mouse: MouseEvent) -> Self {
        Self::Mouse(mouse)
    }

    pub fn mouse_down(button: MouseButton, coordinates: (u16, u16)) -> Self {
        Self::Mouse(MouseEvent::down(coordinates.0, coordinates.1, button))
    }

    pub fn mouse_up(button: MouseButton, coordinates: (u16, u16)) -> Self {
        Self::Mouse(MouseEvent::up(coordinates.0, coordinates.1, button))
    }

    pub fn mouse_drag(button: MouseButton, coordinates: (u16, u16)) -> Self {
        Self::Mouse(MouseEvent::drag(coordinates.0, coordinates.1, button))
    }

    pub fn mouse_moved(coordinates: (u16, u16)) -> Self {
        Self::Mouse(MouseEvent::moved(coordinates.0, coordinates.1))
    }

    pub fn mouse_scroll(direction: ScrollDirection, coordinates: (u16, u16)) -> Self {
        Self::Mouse(MouseEvent::scroll(coordinates.0, coordinates.1, direction))
    }

    pub fn resize(width: u16, height: u16) -> Self {
        Self::Resize { width, height }
    }

    pub fn paste(value: impl Into<String>) -> Self {
        Self::Paste(value.into())
    }

    pub fn is_keyboard(&self) -> bool {
        matches!(self, Self::Key(_))
    }

    pub fn is_mouse(&self) -> bool {
        matches!(self, Self::Mouse(_))
    }

    pub fn from_key_label(label: impl AsRef<str>) -> Self {
        Self::Key(KeyEvent::from_label(label))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedTarget {
    Global,
    Focused(UiId),
    Hit(UiId),
    ModalBackdrop { modal: UiId },
    Unmatched,
}

pub type RouteTarget = RoutedTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedEvent {
    pub event: InputEvent,
    pub target: RoutedTarget,
}

impl RoutedEvent {
    pub fn new(event: InputEvent, target: RoutedTarget) -> Self {
        Self { event, target }
    }

    pub fn global(event: InputEvent) -> Self {
        Self::new(event, RoutedTarget::Global)
    }

    pub fn focused(event: InputEvent, target: impl Into<UiId>) -> Self {
        Self::new(event, RoutedTarget::Focused(target.into()))
    }

    pub fn hit(event: InputEvent, target: impl Into<UiId>) -> Self {
        Self::new(event, RoutedTarget::Hit(target.into()))
    }

    pub fn modal_backdrop(event: InputEvent, modal: impl Into<UiId>) -> Self {
        Self::new(
            event,
            RoutedTarget::ModalBackdrop {
                modal: modal.into(),
            },
        )
    }

    pub fn unmatched(event: InputEvent) -> Self {
        Self::new(event, RoutedTarget::Unmatched)
    }
}

fn key_label(key: &Key) -> String {
    match key {
        Key::Char(character) => character.to_string(),
        Key::Enter => "Enter".to_string(),
        Key::Escape => "Esc".to_string(),
        Key::Backspace => "Backspace".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::BackTab => "Shift+Tab".to_string(),
        Key::Delete => "Delete".to_string(),
        Key::Insert => "Insert".to_string(),
        Key::Left => "Left".to_string(),
        Key::Right => "Right".to_string(),
        Key::Up => "Up".to_string(),
        Key::Down => "Down".to_string(),
        Key::Home => "Home".to_string(),
        Key::End => "End".to_string(),
        Key::PageUp => "PageUp".to_string(),
        Key::PageDown => "PageDown".to_string(),
        Key::Space => "Space".to_string(),
        Key::F(index) => format!("F{index}"),
        Key::Other(label) => label.clone(),
    }
}
