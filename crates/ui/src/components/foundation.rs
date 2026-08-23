use ratatui::buffer::CellWidth;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::{ComponentVisualState, TundraTheme};

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ComponentTone {
    #[default]
    Default,
    Muted,
    Accent,
    Success,
    Warning,
    Danger,
}

pub(crate) fn tone_color(tone: ComponentTone, theme: &TundraTheme) -> ratatui::style::Color {
    let tokens = theme.tokens();
    match tone {
        ComponentTone::Default => tokens.text,
        ComponentTone::Muted => tokens.muted,
        ComponentTone::Accent => tokens.accent,
        ComponentTone::Success => tokens.success,
        ComponentTone::Warning => tokens.warning,
        ComponentTone::Danger => tokens.danger,
    }
}

impl From<ComponentState> for ComponentVisualState {
    fn from(state: ComponentState) -> Self {
        Self {
            focused: state.focused,
            selected: state.selected,
            pressed: state.active,
            disabled: state.disabled,
        }
    }
}

impl From<ComponentVisualState> for ComponentState {
    fn from(state: ComponentVisualState) -> Self {
        Self {
            focused: state.focused,
            hovered: false,
            active: state.pressed,
            selected: state.selected,
            disabled: state.disabled,
        }
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
    let tokens = theme.tokens();
    if state.disabled {
        return Style::default().fg(tokens.muted).bg(tokens.surface);
    }

    let mut style = Style::default().fg(tokens.text).bg(tokens.surface);

    if state.selected {
        style = style
            .fg(tokens.accent)
            .bg(tokens.accent_soft)
            .add_modifier(Modifier::BOLD);
    } else if state.hovered {
        style = style.fg(tokens.accent_strong);
    }

    if state.focused && !state.selected {
        style = style.fg(tokens.focus).add_modifier(Modifier::BOLD);
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
    usize::from(value.cell_width())
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

#[cfg(test)]
mod tests {
    use super::{terminal_width, truncate_to_terminal_width};

    #[test]
    fn terminal_width_uses_ratatui_cell_width_for_cjk_and_emoji() {
        assert_eq!(terminal_width("中文"), 4);
        assert_eq!(terminal_width("日本"), 4);
        assert_eq!(terminal_width("🙂"), 2);
    }

    #[test]
    fn truncation_keeps_wide_graphemes_intact() {
        assert_eq!(truncate_to_terminal_width("A中文B", 3), "A中");
        assert_eq!(truncate_to_terminal_width("A🙂B", 3), "A🙂");
    }
}
