use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::TundraTheme;

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    contains_point, inner_area, item_style,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub id: ComponentId,
    pub label: String,
    pub description: Option<String>,
    pub disabled: bool,
}

impl ListItem {
    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            disabled: false,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    pub id: ComponentId,
    pub title: Option<String>,
    pub items: Vec<ListItem>,
    pub state: ComponentState,
    selected: Option<usize>,
    hovered: Option<usize>,
}

impl List {
    pub fn new(id: impl Into<ComponentId>, items: Vec<ListItem>) -> Self {
        let selected = items.iter().position(|item| !item.disabled);
        Self {
            id: id.into(),
            title: None,
            items,
            state: ComponentState::default(),
            selected,
            hovered: None,
        }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub fn selected_item(&self) -> Option<&ListItem> {
        self.selected.and_then(|index| self.items.get(index))
    }

    pub fn hovered_index(&self) -> Option<usize> {
        self.hovered
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.state.focused = focused;
    }

    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index
            .filter(|index| self.items.get(*index).is_some_and(|item| !item.disabled))
            .or_else(|| self.items.iter().position(|item| !item.disabled));
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        match event {
            InputEvent::FocusGained => {
                self.set_focused(true);
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.set_focused(false);
                self.hovered = None;
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) if self.state.focused => match key.key {
                Key::Up => self.select_previous(),
                Key::Down => self.select_next(),
                Key::Home => self.select_first(),
                Key::End => self.select_last(),
                Key::Enter | Key::Space => self
                    .selected_item()
                    .map(|item| ComponentEvent::Activated(item.id.clone()))
                    .unwrap_or(ComponentEvent::Consumed),
                _ => ComponentEvent::None,
            },
            InputEvent::Mouse(mouse) => {
                let index = self.item_index_at(area, mouse.column, mouse.row);
                match mouse.kind {
                    MouseKind::Move => {
                        if self.hovered != index {
                            self.hovered = index;
                            ComponentEvent::Changed(self.id.clone())
                        } else {
                            ComponentEvent::None
                        }
                    }
                    MouseKind::ScrollUp if contains_point(area, mouse.column, mouse.row) => {
                        self.select_previous()
                    }
                    MouseKind::ScrollDown if contains_point(area, mouse.column, mouse.row) => {
                        self.select_next()
                    }
                    MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left) => {
                        self.select_from_pointer(index)
                    }
                    MouseKind::DoubleClick(MouseButton::Left) => {
                        self.select_from_pointer(index);
                        self.selected_item()
                            .map(|item| ComponentEvent::Activated(item.id.clone()))
                            .unwrap_or(ComponentEvent::None)
                    }
                    _ => ComponentEvent::None,
                }
            }
            _ => ComponentEvent::None,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        let block = match self.title.as_deref() {
            Some(title) => Block::default()
                .title(title)
                .borders(Borders::ALL)
                .style(theme.body_style()),
            None => Block::default()
                .borders(Borders::ALL)
                .style(theme.body_style()),
        };
        let inner = block.inner(area);
        block.render(area, buffer);

        let visible_rows = inner.height as usize;
        for (row, item) in self.items.iter().take(visible_rows).enumerate() {
            let index = row;
            let selected = self.selected == Some(index);
            let hovered = self.hovered == Some(index);
            let style = item_style(self.state.focused, hovered, selected, item.disabled, theme);
            let label = match &item.description {
                Some(description) => format!("{} - {}", item.label, description),
                None => item.label.clone(),
            };
            let row_area = Rect::new(inner.x, inner.y.saturating_add(row as u16), inner.width, 1);
            Paragraph::new(label).style(style).render(row_area, buffer);
        }
    }

    fn select_from_pointer(&mut self, index: Option<usize>) -> ComponentEvent {
        let Some(index) = index else {
            return ComponentEvent::None;
        };
        if self.items.get(index).is_none_or(|item| item.disabled) {
            return ComponentEvent::Consumed;
        }

        self.state.focused = true;
        self.hovered = Some(index);
        self.selected = Some(index);
        ComponentEvent::Selected(self.id.clone(), index)
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
        if self.selected == Some(index) {
            ComponentEvent::Consumed
        } else {
            self.selected = Some(index);
            ComponentEvent::Selected(self.id.clone(), index)
        }
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
