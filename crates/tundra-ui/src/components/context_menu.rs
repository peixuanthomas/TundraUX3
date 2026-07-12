use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Borders, Clear, Paragraph, Widget};

use crate::TundraTheme;

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    contains_point, inner_area, item_style,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuItem {
    pub id: ComponentId,
    pub label: String,
    pub disabled: bool,
}

impl ContextMenuItem {
    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenu {
    pub id: ComponentId,
    pub title: Option<String>,
    pub items: Vec<ContextMenuItem>,
    pub state: ComponentState,
    pub open: bool,
    selected: Option<usize>,
    hovered: Option<usize>,
}

impl ContextMenu {
    pub fn new(id: impl Into<ComponentId>, items: Vec<ContextMenuItem>) -> Self {
        let selected = items.iter().position(|item| !item.disabled);
        Self {
            id: id.into(),
            title: None,
            items,
            state: ComponentState::default(),
            open: false,
            selected,
            hovered: None,
        }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn open(&mut self) {
        self.open = true;
        self.state.focused = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.state.focused = false;
        self.hovered = None;
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub fn preferred_area(&self, anchor_column: u16, anchor_row: u16, bounds: Rect) -> Rect {
        let content_width = self
            .items
            .iter()
            .map(|item| item.label.chars().count() as u16)
            .max()
            .unwrap_or(0);
        let width = content_width.saturating_add(2).max(4);
        let height = (self.items.len() as u16).saturating_add(2).max(3);
        let max_x = bounds.x.saturating_add(bounds.width.saturating_sub(width));
        let max_y = bounds
            .y
            .saturating_add(bounds.height.saturating_sub(height));

        Rect::new(
            anchor_column.min(max_x).max(bounds.x),
            anchor_row.min(max_y).max(bounds.y),
            width.min(bounds.width),
            height.min(bounds.height),
        )
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
                self.close();
                ComponentEvent::Dismissed(self.id.clone())
            }
            InputEvent::Key(key) => match key.key {
                Key::Escape => {
                    self.close();
                    ComponentEvent::Dismissed(self.id.clone())
                }
                Key::Up => self.select_previous(),
                Key::Down => self.select_next(),
                Key::Home => self.select_first(),
                Key::End => self.select_last(),
                Key::Enter | Key::Space => self.activate_selected(),
                _ => ComponentEvent::Consumed,
            },
            InputEvent::Mouse(mouse) => {
                let inside = contains_point(area, mouse.column, mouse.row);
                if !inside
                    && matches!(
                        mouse.kind,
                        MouseKind::Down(_) | MouseKind::Click(_) | MouseKind::DoubleClick(_)
                    )
                {
                    self.close();
                    return ComponentEvent::Dismissed(self.id.clone());
                }

                let index = self.item_index_at(area, mouse.column, mouse.row);
                match mouse.kind {
                    MouseKind::Move => {
                        if self.hovered != index {
                            self.hovered = index;
                            ComponentEvent::Changed(self.id.clone())
                        } else {
                            ComponentEvent::Consumed
                        }
                    }
                    MouseKind::Down(MouseButton::Left)
                    | MouseKind::Click(MouseButton::Left)
                    | MouseKind::DoubleClick(MouseButton::Left) => self.activate_pointer(index),
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
        let block = match self.title.as_deref() {
            Some(title) => theme
                .block()
                .title(title)
                .borders(Borders::ALL)
                .style(theme.body_style()),
            None => theme
                .block()
                .borders(Borders::ALL)
                .style(theme.body_style()),
        };
        let inner = block.inner(area);
        block.render(area, buffer);

        for (row, item) in self.items.iter().take(inner.height as usize).enumerate() {
            let style = item_style(
                self.state.focused,
                self.hovered == Some(row),
                self.selected == Some(row),
                item.disabled,
                theme,
            );
            Paragraph::new(item.label.as_str()).style(style).render(
                Rect::new(inner.x, inner.y.saturating_add(row as u16), inner.width, 1),
                buffer,
            );
        }
    }

    fn activate_pointer(&mut self, index: Option<usize>) -> ComponentEvent {
        let Some(index) = index else {
            return ComponentEvent::Consumed;
        };
        if self.items.get(index).is_none_or(|item| item.disabled) {
            return ComponentEvent::Consumed;
        }

        self.selected = Some(index);
        self.activate_selected()
    }

    fn activate_selected(&mut self) -> ComponentEvent {
        let Some(index) = self.selected else {
            return ComponentEvent::Consumed;
        };
        let Some(item) = self.items.get(index) else {
            return ComponentEvent::Consumed;
        };
        if item.disabled {
            return ComponentEvent::Consumed;
        }

        let id = item.id.clone();
        self.close();
        ComponentEvent::Activated(id)
    }

    fn select_first(&mut self) -> ComponentEvent {
        self.select_index(self.items.iter().position(|item| !item.disabled))
    }

    fn select_last(&mut self) -> ComponentEvent {
        self.select_index(self.items.iter().rposition(|item| !item.disabled))
    }

    fn select_previous(&mut self) -> ComponentEvent {
        let selected = self.selected.unwrap_or(self.items.len());
        let next = self.items[..selected]
            .iter()
            .rposition(|item| !item.disabled);
        self.select_index(next)
    }

    fn select_next(&mut self) -> ComponentEvent {
        let start = self
            .selected
            .map(|index| index.saturating_add(1))
            .unwrap_or(0);
        let next = self.items[start..]
            .iter()
            .position(|item| !item.disabled)
            .map(|index| index + start);
        self.select_index(next)
    }

    fn select_index(&mut self, index: Option<usize>) -> ComponentEvent {
        let Some(index) = index else {
            return ComponentEvent::Consumed;
        };
        self.selected = Some(index);
        ComponentEvent::Selected(self.id.clone(), index)
    }

    fn item_index_at(&self, area: Rect, column: u16, row: u16) -> Option<usize> {
        let inner = inner_area(area);
        if !contains_point(inner, column, row) {
            return None;
        }

        let index = row.saturating_sub(inner.y) as usize;
        self.items.get(index).map(|_| index)
    }
}
