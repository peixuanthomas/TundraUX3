use crate::TundraTheme;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::foundation::terminal_width;
use super::{
    Button, ComponentEvent, ComponentId, ComponentState, DialogLayout, InputEvent, Key, List,
    ListItem, MouseButton, MouseKind, contains_point,
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
    close_button: Button,
    pressed: Option<(ComponentId, Rect)>,
}

impl ContextMenu {
    pub fn new(id: impl Into<ComponentId>, items: Vec<ContextMenuItem>) -> Self {
        let id = id.into();
        let mut list = Self::make_list(&id, &items);
        list.set_focused(false);
        Self {
            close_button: Button::new(format!("{}.close", id.as_str()), Key::Escape.label())
                .with_bracketed_label(false),
            id,
            title: None,
            items,
            state: ComponentState::default(),
            open: false,
            list,
            pressed: None,
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
            self.pressed = None;
            self.close_button.set_focused(false);
            self.close_button.state.hovered = false;
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
            .chain(self.title.iter().map(|title| {
                u16::try_from(terminal_width(title) + terminal_width(&self.close_button.label) + 3)
                    .unwrap_or(u16::MAX)
            }))
            .max()
            .unwrap_or(0);
        let width = content_width.saturating_add(6).max(12);
        let extra_rows = if self.title.is_some() { 4 } else { 2 };
        let height = (self.items.len() as u16).saturating_add(extra_rows).max(3);
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
            InputEvent::Resize { .. } => {
                self.pressed = None;
                self.close_button.set_focused(false);
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) if key.key == Key::Escape => {
                self.close();
                ComponentEvent::Dismissed(self.id.clone())
            }
            InputEvent::Key(key) => {
                self.pressed = None;
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
                let point = (mouse.column(), mouse.row());
                let inside = contains_point(area, point.0, point.1);
                if !inside
                    && matches!(
                        mouse.kind,
                        MouseKind::Down(_) | MouseKind::Click(_) | MouseKind::DoubleClick(_)
                    )
                {
                    self.close();
                    return ComponentEvent::Dismissed(self.id.clone());
                }
                let close = self.layout(area).close;
                if matches!(mouse.kind, MouseKind::Down(_)) {
                    self.close_button.set_focused(false);
                } else if mouse.kind == MouseKind::Move && !contains_point(close, point.0, point.1)
                {
                    self.close_button.state.active = false;
                }
                if self.title.is_some()
                    && matches!(
                        self.close_button
                            .handle_event(InputEvent::Mouse(mouse), close),
                        ComponentEvent::Activated(_)
                    )
                {
                    self.close();
                    return ComponentEvent::Dismissed(self.id.clone());
                }
                let list_area = self.list_area(area);
                let row = self
                    .list
                    .borderless_item_index_at(list_area, point.0, point.1)
                    .and_then(|index| self.items.get(index))
                    .filter(|item| !item.disabled)
                    .map(|item| {
                        (
                            item.id.clone(),
                            Rect::new(list_area.x, point.1, list_area.width, 1),
                        )
                    });
                match mouse.kind {
                    MouseKind::Down(MouseButton::Left) => {
                        self.pressed = row;
                        return ComponentEvent::Changed(self.id.clone());
                    }
                    MouseKind::Drag(MouseButton::Left) | MouseKind::Move => {
                        if self.pressed != row {
                            self.pressed = None;
                        }
                    }
                    MouseKind::Up(MouseButton::Left) => {
                        if let Some(pressed) = self.pressed.take()
                            && row.as_ref() == Some(&pressed)
                        {
                            self.close();
                            return ComponentEvent::Activated(pressed.0);
                        }
                        return ComponentEvent::Consumed;
                    }
                    _ => self.pressed = None,
                }
                let event = self
                    .list
                    .handle_event_borderless(InputEvent::Mouse(mouse), list_area);
                if mouse.kind == MouseKind::Click(MouseButton::Left)
                    && matches!(event, ComponentEvent::Selected(_, _))
                {
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
        let _scope = crate::automation::scope(
            self.id.as_str(),
            "menu",
            self.title.as_deref().unwrap_or(""),
            area,
        );

        let context = DialogLayout::surface_theme(theme);
        let layout = self.layout(area);
        layout.render(buffer, self.title.as_deref().unwrap_or(""), &context);
        self.close_button
            .clone()
            .with_background(context.theme.surface)
            .render_borderless(layout.close, buffer, &DialogLayout::close_theme(&context));
        let mut list = self.list_for_render();
        if let Some((id, _)) = &self.pressed {
            list.set_hovered(list.items.iter().position(|item| &item.id == id));
        }
        list.with_background(context.theme.surface)
            .render_borderless(
                self.list_area(area),
                buffer,
                &DialogLayout::body_theme(&context),
            );
    }

    /// Renders the context menu through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        self.render(area, frame.buffer_mut(), theme);
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
        list.set_selected(
            selected.filter(|index| self.items.get(*index).is_some_and(|item| !item.disabled)),
        );
        list.set_hovered(hovered);
        self.list = list;
    }

    fn list_for_render(&self) -> List {
        let mut list = Self::make_list(&self.id, &self.items);
        list.state = self.state;
        list.set_selected(
            self.list
                .selected_index()
                .filter(|index| self.items.get(*index).is_some_and(|item| !item.disabled)),
        );
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
        .with_highlight_symbol(Some("▌   "))
    }

    fn list_area(&self, area: Rect) -> Rect {
        let body = self.layout(area).body;
        if self.title.is_some() {
            Rect::new(area.x, body.y, area.width, body.height)
        } else {
            let padding = u16::from(area.height > 2);
            Rect::new(
                area.x,
                area.y + padding,
                area.width,
                area.height - padding * 2,
            )
        }
    }

    fn layout(&self, area: Rect) -> DialogLayout {
        DialogLayout::new(
            area,
            0,
            self.title
                .as_ref()
                .map(|_| self.close_button.label.as_str()),
        )
    }
}
