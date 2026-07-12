use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Borders, Clear, Paragraph, Widget};

use crate::TundraTheme;

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    clamp_index, contains_point, inner_area, item_style,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogAction {
    pub id: ComponentId,
    pub label: String,
}

impl DialogAction {
    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub id: ComponentId,
    pub title: String,
    pub body: Vec<String>,
    pub actions: Vec<DialogAction>,
    pub state: ComponentState,
    pub open: bool,
    selected_action: Option<usize>,
    hovered_action: Option<usize>,
}

impl Dialog {
    pub fn new(
        id: impl Into<ComponentId>,
        title: impl Into<String>,
        body: impl Into<String>,
        actions: Vec<DialogAction>,
    ) -> Self {
        let selected_action = clamp_index(0, actions.len());
        Self {
            id: id.into(),
            title: title.into(),
            body: vec![body.into()],
            actions,
            state: ComponentState::default(),
            open: false,
            selected_action,
            hovered_action: None,
        }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.state.focused = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.state.focused = false;
        self.state.active = false;
        self.hovered_action = None;
    }

    pub fn selected_action_index(&self) -> Option<usize> {
        self.selected_action
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        if !self.open {
            return ComponentEvent::None;
        }

        match event {
            InputEvent::FocusGained => {
                self.state.focused = true;
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.state.focused = false;
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) => match key.key {
                Key::Escape => {
                    self.close();
                    ComponentEvent::Dismissed(self.id.clone())
                }
                Key::Tab | Key::Right | Key::Down => self.select_next_action(),
                Key::BackTab | Key::Left | Key::Up => self.select_previous_action(),
                Key::Enter | Key::Space => self.activate_selected_action(),
                _ => ComponentEvent::Consumed,
            },
            InputEvent::Mouse(mouse) => {
                if !contains_point(area, mouse.column, mouse.row) {
                    return ComponentEvent::Consumed;
                }

                let action = self.action_index_at(area, mouse.column, mouse.row);
                match mouse.kind {
                    MouseKind::Move => {
                        if self.hovered_action != action {
                            self.hovered_action = action;
                            ComponentEvent::Changed(self.id.clone())
                        } else {
                            ComponentEvent::Consumed
                        }
                    }
                    MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left) => {
                        if let Some(index) = action {
                            self.selected_action = Some(index);
                            self.activate_selected_action()
                        } else {
                            ComponentEvent::Consumed
                        }
                    }
                    _ => ComponentEvent::Consumed,
                }
            }
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        if !self.open {
            return;
        }

        Clear.render(area, buffer);
        let block = theme
            .block()
            .title(self.title.as_str())
            .borders(Borders::ALL)
            .style(theme.body_style());
        let inner = block.inner(area);
        block.render(area, buffer);

        for (row, line) in self.body.iter().enumerate().take(inner.height as usize) {
            Paragraph::new(line.as_str())
                .style(theme.body_style())
                .render(
                    Rect::new(inner.x, inner.y.saturating_add(row as u16), inner.width, 1),
                    buffer,
                );
        }

        if inner.height == 0 {
            return;
        }

        let action_y = inner.y.saturating_add(inner.height.saturating_sub(1));
        let mut action_x = inner.x;
        for (index, action) in self.actions.iter().enumerate() {
            let label = format!(" {} ", action.label);
            let width = label.chars().count() as u16;
            if action_x.saturating_add(width) > inner.x.saturating_add(inner.width) {
                break;
            }
            let style = item_style(
                self.state.focused,
                self.hovered_action == Some(index),
                self.selected_action == Some(index),
                false,
                theme,
            );
            Paragraph::new(label)
                .style(style)
                .render(Rect::new(action_x, action_y, width, 1), buffer);
            action_x = action_x.saturating_add(width.saturating_add(1));
        }
    }

    fn select_next_action(&mut self) -> ComponentEvent {
        if self.actions.is_empty() {
            return ComponentEvent::Consumed;
        }

        let index = self
            .selected_action
            .map(|index| (index + 1) % self.actions.len())
            .unwrap_or(0);
        self.selected_action = Some(index);
        ComponentEvent::Selected(self.id.clone(), index)
    }

    fn select_previous_action(&mut self) -> ComponentEvent {
        if self.actions.is_empty() {
            return ComponentEvent::Consumed;
        }

        let index = self
            .selected_action
            .map(|index| {
                if index == 0 {
                    self.actions.len().saturating_sub(1)
                } else {
                    index.saturating_sub(1)
                }
            })
            .unwrap_or(0);
        self.selected_action = Some(index);
        ComponentEvent::Selected(self.id.clone(), index)
    }

    fn activate_selected_action(&self) -> ComponentEvent {
        self.selected_action
            .and_then(|index| self.actions.get(index))
            .map(|action| ComponentEvent::Activated(action.id.clone()))
            .unwrap_or(ComponentEvent::Consumed)
    }

    fn action_index_at(&self, area: Rect, column: u16, row: u16) -> Option<usize> {
        let inner = inner_area(area);
        if inner.height == 0 || row != inner.y.saturating_add(inner.height.saturating_sub(1)) {
            return None;
        }

        let mut action_x = inner.x;
        for (index, action) in self.actions.iter().enumerate() {
            let width = action.label.chars().count() as u16 + 2;
            if column >= action_x && column < action_x.saturating_add(width) {
                return Some(index);
            }
            action_x = action_x.saturating_add(width.saturating_add(1));
        }
        None
    }
}
