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
}

pub type KeyCode = Key;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyModifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
    };

    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
    };

    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
    };

    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
    };

    pub const CTRL_SHIFT: Self = Self {
        ctrl: true,
        alt: false,
        shift: true,
    };

    pub fn new(ctrl: bool, alt: bool, shift: bool) -> Self {
        Self { ctrl, alt, shift }
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
        if self.modifiers.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_string());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_string());
        }
        parts.push(key_label(&self.key));
        parts.join("+")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: KeyModifiers,
    pub repeat: bool,
}

impl KeyEvent {
    pub fn new(key: Key) -> Self {
        Self {
            key,
            modifiers: KeyModifiers::NONE,
            repeat: false,
        }
    }

    pub fn with_modifiers(key: Key, modifiers: KeyModifiers) -> Self {
        Self {
            key,
            modifiers,
            repeat: false,
        }
    }

    pub fn repeated(mut self) -> Self {
        self.repeat = true;
        self
    }

    pub fn stroke(&self) -> KeyStroke {
        KeyStroke::new(self.key.clone(), self.modifiers)
    }
}

impl From<&KeyEvent> for KeyStroke {
    fn from(value: &KeyEvent) -> Self {
        value.stroke()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
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
    Hover,
    Down(MouseButton),
    Up(MouseButton),
    Click(MouseButton),
    DoubleClick(MouseButton),
    Drag(MouseButton),
    Scroll(ScrollDirection),
}

pub type MouseAction = MouseEventKind;

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
        Self::new(column, row, MouseEventKind::Hover)
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize { width: u16, height: u16 },
    Tick,
}

impl InputEvent {
    pub fn key(key: Key) -> Self {
        Self::Key(KeyEvent::new(key))
    }

    pub fn key_with_modifiers(key: Key, modifiers: KeyModifiers) -> Self {
        Self::Key(KeyEvent::with_modifiers(key, modifiers))
    }

    pub fn mouse(mouse: MouseEvent) -> Self {
        Self::Mouse(mouse)
    }

    pub fn resize(width: u16, height: u16) -> Self {
        Self::Resize { width, height }
    }

    pub fn is_keyboard(&self) -> bool {
        matches!(self, Self::Key(_))
    }

    pub fn is_mouse(&self) -> bool {
        matches!(self, Self::Mouse(_))
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
    }
}
