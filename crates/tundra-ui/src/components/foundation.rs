use std::fmt;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::TundraTheme;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentId(String);

impl ComponentId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<&str> for ComponentId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ComponentId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComponentState {
    pub focused: bool,
    pub hovered: bool,
    pub active: bool,
    pub selected: bool,
    pub disabled: bool,
}

impl ComponentState {
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentEvent {
    None,
    Consumed,
    Changed(ComponentId),
    FocusRequested(ComponentId),
    Selected(ComponentId, usize),
    Activated(ComponentId),
    Dismissed(ComponentId),
}

impl ComponentEvent {
    pub fn is_consumed(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyInput),
    Mouse(MouseInput),
    FocusGained,
    FocusLost,
}

impl InputEvent {
    pub fn key(key: Key) -> Self {
        Self::Key(KeyInput::new(key))
    }

    pub fn key_with_modifiers(key: Key, modifiers: KeyModifiers) -> Self {
        Self::Key(KeyInput { key, modifiers })
    }

    pub fn mouse(kind: MouseKind, column: u16, row: u16) -> Self {
        Self::Mouse(MouseInput { kind, column, row })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    pub key: Key,
    pub modifiers: KeyModifiers,
}

impl KeyInput {
    pub fn new(key: Key) -> Self {
        Self {
            key,
            modifiers: KeyModifiers::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
}

impl KeyModifiers {
    pub fn shift() -> Self {
        Self {
            shift: true,
            control: false,
            alt: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Space,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseInput {
    pub kind: MouseKind,
    pub column: u16,
    pub row: u16,
}

impl MouseInput {
    pub fn is_primary_click(self) -> bool {
        matches!(
            self.kind,
            MouseKind::Click(MouseButton::Left) | MouseKind::DoubleClick(MouseButton::Left)
        )
    }

    pub fn is_primary_down(self) -> bool {
        matches!(self.kind, MouseKind::Down(MouseButton::Left))
    }

    pub fn is_primary_up(self) -> bool {
        matches!(self.kind, MouseKind::Up(MouseButton::Left))
    }

    pub fn is_context_request(self) -> bool {
        matches!(
            self.kind,
            MouseKind::Click(MouseButton::Right)
                | MouseKind::Down(MouseButton::Right)
                | MouseKind::DoubleClick(MouseButton::Right)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Move,
    Down(MouseButton),
    Up(MouseButton),
    Click(MouseButton),
    DoubleClick(MouseButton),
    ScrollUp,
    ScrollDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

pub fn contains_point(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

pub(crate) fn inner_area(area: Rect) -> Rect {
    if area.width <= 2 || area.height <= 2 {
        return Rect::new(area.x, area.y, 0, 0);
    }

    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

pub(crate) fn interactive_style(state: ComponentState, theme: &TundraTheme) -> Style {
    if state.disabled {
        return theme.muted_style();
    }

    let mut style = theme.body_style();

    if state.selected {
        style = style
            .fg(theme.background)
            .bg(theme.accent_color)
            .add_modifier(Modifier::BOLD);
    } else if state.hovered {
        style = style.fg(theme.accent_color);
    }

    if state.focused {
        style = style.add_modifier(Modifier::BOLD);
    }

    if state.active {
        style = style.add_modifier(Modifier::REVERSED);
    }

    style
}

pub(crate) fn item_style(
    focused: bool,
    hovered: bool,
    selected: bool,
    disabled: bool,
    theme: &TundraTheme,
) -> Style {
    interactive_style(
        ComponentState {
            focused,
            hovered,
            active: false,
            selected,
            disabled,
        },
        theme,
    )
}

pub(crate) fn clamp_index(index: usize, len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some(index.min(len.saturating_sub(1)))
    }
}

pub(crate) fn char_count(value: &str) -> usize {
    value.chars().count()
}

pub(crate) fn byte_index_for_char(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(value.len())
}
