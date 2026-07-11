use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::{TundraTheme, theme::solid_border_style};

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
}

impl TextInput {
    pub fn new(id: impl Into<ComponentId>) -> Self {
        Self {
            id: id.into(),
            placeholder: String::new(),
            state: ComponentState::default(),
            text: String::new(),
            cursor: 0,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
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
        if self.state.disabled {
            return ComponentEvent::None;
        }

        match event {
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
                let inside = contains_point(area, mouse.column, mouse.row);
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
                        self.cursor = self.cursor_for_column(area, mouse.column);
                        ComponentEvent::FocusRequested(self.id.clone())
                    }
                    _ => ComponentEvent::None,
                }
            }
            _ => ComponentEvent::None,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        let style = interactive_style(self.state, theme);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(solid_border_style(style))
            .style(style);
        let inner = block.inner(area);
        block.render(area, buffer);

        let line = self.display_text(inner.width as usize);
        let text_style = if self.text.is_empty() && !self.state.focused {
            theme.muted_style()
        } else {
            theme.body_style()
        };
        Paragraph::new(line)
            .style(text_style)
            .render(Rect::new(inner.x, inner.y, inner.width, 1), buffer);
    }

    fn display_text(&self, max_width: usize) -> String {
        let mut display = if self.text.is_empty() && !self.state.focused {
            self.placeholder.clone()
        } else {
            self.text.clone()
        };

        if self.state.focused {
            let insert_at = byte_index_for_char(&display, self.cursor);
            display.insert(insert_at, '|');
        }

        display.chars().take(max_width).collect()
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

    fn cursor_for_column(&self, area: Rect, column: u16) -> usize {
        let inner = inner_area(area);
        if inner.width == 0 || column <= inner.x {
            return 0;
        }

        column
            .saturating_sub(inner.x)
            .min(char_count(&self.text) as u16) as usize
    }
}
