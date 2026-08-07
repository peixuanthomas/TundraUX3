use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Borders, Clear, Widget};

use crate::TundraTheme;

use super::foundation::terminal_width;
use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, List, ListItem, MouseButton,
    MouseKind, contains_point, inner_area,
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
    list: List,
}

impl ContextMenu {
    pub fn new(id: impl Into<ComponentId>, items: Vec<ContextMenuItem>) -> Self {
        let id = id.into();
        let mut list = Self::make_list(&id, &items);
        list.set_focused(false);
        Self {
            id,
            title: None,
            items,
            state: ComponentState::default(),
            open: false,
            list,
        }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn open(&mut self) {
        self.set_open(true);
    }

    pub fn close(&mut self) {
        self.set_open(false);
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        self.state.focused = open;
        self.list.set_focused(open);
        if !open {
            self.list.set_hovered(None);
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.list.selected_index()
    }

    pub fn set_selected(&mut self, index: Option<usize>) {
        self.sync_list();
        self.list.set_selected(index);
    }

    pub fn preferred_area(&self, anchor_column: u16, anchor_row: u16, bounds: Rect) -> Rect {
        let content_width = self
            .items
            .iter()
            .map(|item| u16::try_from(terminal_width(&item.label)).unwrap_or(u16::MAX))
            .chain(
                self.title
                    .iter()
                    .map(|title| u16::try_from(terminal_width(title)).unwrap_or(u16::MAX)),
            )
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

        self.sync_list();
        match event {
            InputEvent::Key(key) if !key.is_press_like() => ComponentEvent::None,
            InputEvent::FocusGained => {
                self.state.focused = true;
                self.list.set_focused(true);
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.close();
                ComponentEvent::Dismissed(self.id.clone())
            }
            InputEvent::Key(key) if key.key == Key::Escape => {
                self.close();
                ComponentEvent::Dismissed(self.id.clone())
            }
            InputEvent::Key(key) => {
                let list_area = self.list_area(area);
                let event = self
                    .list
                    .handle_event_borderless(InputEvent::Key(key), list_area);
                let event = self.translate_list_event(event);
                if matches!(event, ComponentEvent::None) {
                    ComponentEvent::Consumed
                } else {
                    event
                }
            }
            InputEvent::Mouse(mouse) => {
                let inside = contains_point(area, mouse.column(), mouse.row());
                if !inside
                    && matches!(
                        mouse.kind,
                        MouseKind::Down(_) | MouseKind::Click(_) | MouseKind::DoubleClick(_)
                    )
                {
                    self.close();
                    return ComponentEvent::Dismissed(self.id.clone());
                }

                let activates = matches!(
                    mouse.kind,
                    MouseKind::Down(MouseButton::Left)
                        | MouseKind::Click(MouseButton::Left)
                        | MouseKind::DoubleClick(MouseButton::Left)
                );
                let list_area = self.list_area(area);
                let event = self
                    .list
                    .handle_event_borderless(InputEvent::Mouse(mouse), list_area);
                if activates && matches!(event, ComponentEvent::Selected(_, _)) {
                    return self.activate_selected();
                }
                self.translate_list_event(event)
            }
            _ => ComponentEvent::Consumed,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        if !self.open {
            return;
        }

        Clear.render(area, buffer);
        let block = self.block(theme);
        let inner = block.inner(area);
        block.render(area, buffer);

        let list = self.list_for_render();
        list.render_borderless(inner, buffer, theme);
    }

    /// Renders the context menu through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        if !self.open {
            return;
        }

        frame.render_widget(Clear, area);
        let block = self.block(theme);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let list = self.list_for_render();
        list.render_borderless_frame(frame, inner, theme);
    }

    fn activate_selected(&mut self) -> ComponentEvent {
        let Some(item) = self.list.selected_item() else {
            return ComponentEvent::Consumed;
        };
        if item.disabled {
            return ComponentEvent::Consumed;
        }

        let id = item.id.clone();
        self.close();
        ComponentEvent::Activated(id)
    }

    fn translate_list_event(&mut self, event: ComponentEvent) -> ComponentEvent {
        match event {
            ComponentEvent::Selected(_, index) => ComponentEvent::Selected(self.id.clone(), index),
            ComponentEvent::Changed(_) => ComponentEvent::Changed(self.id.clone()),
            ComponentEvent::Activated(id) => {
                self.close();
                ComponentEvent::Activated(id)
            }
            ComponentEvent::FocusRequested(_) => ComponentEvent::Consumed,
            other => other,
        }
    }

    fn sync_list(&mut self) {
        let selected = self.list.selected_index();
        let hovered = self.list.hovered_index();
        let mut list = Self::make_list(&self.id, &self.items);
        list.state = self.state;
        list.set_selected(selected);
        list.set_hovered(hovered);
        self.list = list;
    }

    fn list_for_render(&self) -> List {
        let mut list = Self::make_list(&self.id, &self.items);
        list.state = self.state;
        list.set_selected(self.list.selected_index());
        list.set_hovered(self.list.hovered_index());
        list
    }

    fn make_list(id: &ComponentId, items: &[ContextMenuItem]) -> List {
        List::new(
            format!("{}.items", id.as_str()),
            items
                .iter()
                .map(|item| {
                    ListItem::new(item.id.clone(), item.label.clone()).disabled(item.disabled)
                })
                .collect(),
        )
    }

    fn list_area(&self, area: Rect) -> Rect {
        inner_area(area)
    }

    fn block<'a>(&'a self, theme: &TundraTheme) -> ratatui::widgets::Block<'a> {
        match self.title.as_deref() {
            Some(title) => theme
                .block()
                .title(title)
                .borders(Borders::ALL)
                .style(theme.body_style()),
            None => theme
                .block()
                .borders(Borders::ALL)
                .style(theme.body_style()),
        }
    }
}
