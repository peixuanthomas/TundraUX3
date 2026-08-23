use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::widgets::{Borders, Paragraph, Widget};

use crate::TundraTheme;

use super::foundation::{
    char_index_for_terminal_column, terminal_width, truncate_to_terminal_width,
};
use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    byte_index_for_char, char_count, contains_point, inner_area, interactive_style,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput {
    pub id: ComponentId,
    pub placeholder: String,
    pub state: ComponentState,
    text: String,
    cursor: usize,
    cursor_symbol: String,
    placeholder_when_focused: bool,
}

impl TextInput {
    pub fn render_with_context(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        context: &crate::RenderContext,
    ) {
        self.render(area, buffer, &context.compatibility_theme());
    }

    pub fn new(id: impl Into<ComponentId>) -> Self {
        Self {
            id: id.into(),
            placeholder: String::new(),
            state: ComponentState::default(),
            text: String::new(),
            cursor: 0,
            cursor_symbol: "|".to_string(),
            placeholder_when_focused: false,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_cursor_symbol(mut self, cursor_symbol: impl Into<String>) -> Self {
        self.cursor_symbol = cursor_symbol.into();
        self
    }

    /// Controls how an empty focused input is rendered.
    ///
    /// The default (`false`) renders the cursor instead of the placeholder.
    /// When enabled, the placeholder remains visible and the character cursor
    /// is suppressed so the two never overlap.
    pub fn with_placeholder_when_focused(mut self, visible: bool) -> Self {
        self.placeholder_when_focused = visible;
        self
    }

    pub fn value(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        self.text = value.into();
        self.cursor = char_count(&self.text);
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.state.focused = focused;
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(char_count(&self.text));
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        self.handle_event_in(event, area, true)
    }

    /// Handles input for a text field rendered without a surrounding block.
    pub fn handle_event_borderless(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        self.handle_event_in(event, area, false)
    }

    fn handle_event_in(&mut self, event: InputEvent, area: Rect, bordered: bool) -> ComponentEvent {
        if self.state.disabled {
            return ComponentEvent::None;
        }

        match event {
            InputEvent::Key(key) if !key.is_press_like() => ComponentEvent::None,
            InputEvent::FocusGained => {
                self.set_focused(true);
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.set_focused(false);
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) if self.state.focused => match key.key {
                Key::Char(character) => {
                    self.insert_char(character);
                    ComponentEvent::Changed(self.id.clone())
                }
                Key::Space => {
                    self.insert_char(' ');
                    ComponentEvent::Changed(self.id.clone())
                }
                Key::Backspace => {
                    if self.delete_before_cursor() {
                        ComponentEvent::Changed(self.id.clone())
                    } else {
                        ComponentEvent::Consumed
                    }
                }
                Key::Delete => {
                    if self.delete_at_cursor() {
                        ComponentEvent::Changed(self.id.clone())
                    } else {
                        ComponentEvent::Consumed
                    }
                }
                Key::Left => {
                    self.cursor = self.cursor.saturating_sub(1);
                    ComponentEvent::Consumed
                }
                Key::Right => {
                    self.cursor = self.cursor.saturating_add(1).min(char_count(&self.text));
                    ComponentEvent::Consumed
                }
                Key::Home => {
                    self.cursor = 0;
                    ComponentEvent::Consumed
                }
                Key::End => {
                    self.cursor = char_count(&self.text);
                    ComponentEvent::Consumed
                }
                Key::Enter => ComponentEvent::Activated(self.id.clone()),
                _ => ComponentEvent::None,
            },
            InputEvent::Mouse(mouse) => {
                let inside = contains_point(area, mouse.column(), mouse.row());
                match mouse.kind {
                    MouseKind::Move => {
                        if self.state.hovered != inside {
                            self.state.hovered = inside;
                            ComponentEvent::Changed(self.id.clone())
                        } else {
                            ComponentEvent::None
                        }
                    }
                    MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left)
                        if inside =>
                    {
                        self.state.focused = true;
                        self.cursor = self.cursor_for_column(area, mouse.column(), bordered);
                        ComponentEvent::FocusRequested(self.id.clone())
                    }
                    _ => ComponentEvent::None,
                }
            }
            _ => ComponentEvent::None,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.bordered_widget(area, theme).render(area, buffer);
    }

    /// Renders the bordered text input through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        frame.render_widget(self.bordered_widget(area, theme), area);
    }

    /// Renders the text input without a surrounding block.
    pub fn render_borderless(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.render_borderless_with_prefix(area, buffer, theme, "");
    }

    /// Renders a borderless text input with a non-editable prefix.
    pub fn render_borderless_with_prefix(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        theme: &TundraTheme,
        prefix: &str,
    ) {
        self.borderless_widget(area, theme, prefix)
            .render(area, buffer);
    }

    /// Renders the borderless text input through a Ratatui [`Frame`].
    pub fn render_borderless_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        self.render_borderless_frame_with_prefix(frame, area, theme, "");
    }

    /// Renders a borderless text input with a non-editable prefix through a Ratatui [`Frame`].
    pub fn render_borderless_frame_with_prefix(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: &TundraTheme,
        prefix: &str,
    ) {
        frame.render_widget(self.borderless_widget(area, theme, prefix), area);
    }

    fn display_text(&self, max_width: usize) -> String {
        let placeholder_visible = self.placeholder_is_visible();
        let mut display = if placeholder_visible {
            self.placeholder.clone()
        } else {
            self.text.clone()
        };

        if self.state.focused && !placeholder_visible {
            let insert_at = byte_index_for_char(&display, self.cursor);
            display.insert_str(insert_at, &self.cursor_symbol);
        }

        truncate_to_terminal_width(&display, max_width)
    }

    fn bordered_widget(&self, area: Rect, theme: &TundraTheme) -> Paragraph<'static> {
        let style = interactive_style(self.state, theme);
        let block = theme
            .block()
            .borders(Borders::ALL)
            .style(style)
            .border_style(theme.selectable_border_style(self.state.selected));
        let line = self.display_text(block.inner(area).width as usize);
        let text_style = if self.placeholder_is_visible() {
            theme.muted_style()
        } else {
            theme.body_style()
        };
        Paragraph::new(line)
            .alignment(HorizontalAlignment::Left)
            .style(text_style)
            .block(block)
    }

    fn borderless_widget(
        &self,
        area: Rect,
        theme: &TundraTheme,
        prefix: &str,
    ) -> Paragraph<'static> {
        let prefix_width = terminal_width(prefix);
        let line = format!(
            "{prefix}{}",
            self.display_text((area.width as usize).saturating_sub(prefix_width))
        );
        let style = if self.placeholder_is_visible() {
            theme.muted_style()
        } else {
            interactive_style(self.state, theme)
        };
        Paragraph::new(line)
            .alignment(HorizontalAlignment::Left)
            .style(style)
    }

    fn insert_char(&mut self, character: char) {
        let byte_index = byte_index_for_char(&self.text, self.cursor);
        self.text.insert(byte_index, character);
        self.cursor = self.cursor.saturating_add(1);
    }

    fn delete_before_cursor(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }

        let remove_at = byte_index_for_char(&self.text, self.cursor.saturating_sub(1));
        self.text.remove(remove_at);
        self.cursor = self.cursor.saturating_sub(1);
        true
    }

    fn delete_at_cursor(&mut self) -> bool {
        if self.cursor >= char_count(&self.text) {
            return false;
        }

        let remove_at = byte_index_for_char(&self.text, self.cursor);
        self.text.remove(remove_at);
        true
    }

    fn cursor_for_column(&self, area: Rect, column: u16, bordered: bool) -> usize {
        let inner = if bordered { inner_area(area) } else { area };
        if inner.width == 0 || column <= inner.x {
            return 0;
        }

        char_index_for_terminal_column(&self.text, usize::from(column.saturating_sub(inner.x)))
    }

    fn placeholder_is_visible(&self) -> bool {
        self.text.is_empty()
            && !self.placeholder.is_empty()
            && (!self.state.focused || self.placeholder_when_focused)
    }
}
