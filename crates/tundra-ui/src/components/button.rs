use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::TundraTheme;

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseKind, contains_point,
    interactive_style,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub id: ComponentId,
    pub label: String,
    pub state: ComponentState,
}

impl Button {
    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            state: ComponentState::default(),
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.state.focused = focused;
        if !focused {
            self.state.active = false;
        }
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.state.disabled = disabled;
        if disabled {
            self.state.active = false;
            self.state.hovered = false;
        }
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
                self.state.hovered = false;
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) if self.state.focused => match key.key {
                Key::Enter | Key::Space => ComponentEvent::Activated(self.id.clone()),
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
                    MouseKind::Down(button) if inside && button == super::MouseButton::Left => {
                        self.state.focused = true;
                        self.state.active = true;
                        ComponentEvent::FocusRequested(self.id.clone())
                    }
                    MouseKind::Up(button) if button == super::MouseButton::Left => {
                        let was_active = self.state.active;
                        self.state.active = false;
                        if was_active && inside {
                            ComponentEvent::Activated(self.id.clone())
                        } else if was_active {
                            ComponentEvent::Consumed
                        } else {
                            ComponentEvent::None
                        }
                    }
                    MouseKind::Click(button) if inside && button == super::MouseButton::Left => {
                        self.state.focused = true;
                        self.state.active = false;
                        ComponentEvent::Activated(self.id.clone())
                    }
                    _ => ComponentEvent::None,
                }
            }
            _ => ComponentEvent::None,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        let style = interactive_style(self.state, theme);
        let button = Paragraph::new(self.label.as_str())
            .alignment(Alignment::Center)
            .style(style)
            .block(Block::default().borders(Borders::ALL).style(style));

        button.render(area, buffer);
    }
}
