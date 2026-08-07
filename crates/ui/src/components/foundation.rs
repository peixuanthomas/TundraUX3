use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::TundraTheme;

pub use crate::UiId as ComponentId;

pub use crate::input::{
    InputEvent, Key, KeyEvent as KeyInput, KeyModifiers, MouseButton, MouseEvent as MouseInput,
    MouseEventKind as MouseKind,
};

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

/// Returns the number of terminal columns Ratatui uses to render `value`.
pub(crate) fn terminal_width(value: &str) -> usize {
    Span::raw(value).width()
}

/// Truncates text at a Ratatui grapheme boundary without splitting a wide cell.
pub(crate) fn truncate_to_terminal_width(value: &str, max_width: usize) -> String {
    if terminal_width(value) <= max_width {
        return value.to_string();
    }

    let span = Span::raw(value);
    let mut width = 0_usize;
    let mut truncated = String::new();
    for grapheme in span.styled_graphemes(Style::default()) {
        let grapheme_width = terminal_width(grapheme.symbol);
        if width.saturating_add(grapheme_width) > max_width {
            break;
        }
        truncated.push_str(grapheme.symbol);
        width = width.saturating_add(grapheme_width);
    }
    truncated
}

/// Maps a terminal column offset to an editable character boundary.
///
/// A cursor cannot occupy the trailing cell of a wide glyph, so clicks inside
/// that glyph stay on the boundary before it. Clicking its first following
/// column moves the cursor after it.
pub(crate) fn char_index_for_terminal_column(value: &str, column: usize) -> usize {
    if column == 0 {
        return 0;
    }

    let mut char_index = 0_usize;
    for (byte_index, character) in value.char_indices() {
        let end = byte_index.saturating_add(character.len_utf8());
        if terminal_width(&value[..end]) > column {
            break;
        }
        char_index = char_index.saturating_add(1);
    }
    char_index
}

pub(crate) fn byte_index_for_char(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(value.len())
}
