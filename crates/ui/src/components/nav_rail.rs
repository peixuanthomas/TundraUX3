use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::widgets::{Paragraph, Widget};

use crate::RenderContext;

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind, Surface,
    contains_point, interactive_style,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavRailItem {
    pub id: ComponentId,
    pub label: String,
    pub icon: Option<String>,
    pub disabled: bool,
}

impl NavRailItem {
    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            disabled: false,
        }
    }

    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Compact navigation with persistent current-row fill and a separate focus marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavRail {
    pub id: ComponentId,
    pub items: Vec<NavRailItem>,
    pub selected: Option<usize>,
    pub state: ComponentState,
    pub title: Option<String>,
}

impl NavRail {
    pub fn new(id: impl Into<ComponentId>, items: Vec<NavRailItem>) -> Self {
        let selected = items.iter().position(|item| !item.disabled);
        Self {
            id: id.into(),
            items,
            selected,
            state: ComponentState::default(),
            title: None,
        }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        match event {
            InputEvent::FocusGained => {
                self.state.focused = true;
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.state.focused = false;
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) if key.is_press_like() && self.state.focused => match key.key {
                Key::Up => self.select_step(-1),
                Key::Down => self.select_step(1),
                Key::Home => self.select_first(),
                Key::End => self.select_last(),
                Key::Enter | Key::Space => self
                    .selected_item()
                    .map(|item| ComponentEvent::Activated(item.id.clone()))
                    .unwrap_or(ComponentEvent::Consumed),
                _ => ComponentEvent::None,
            },
            InputEvent::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left)
                ) =>
            {
                let index = self.index_at(area, mouse.column(), mouse.row());
                match index.and_then(|index| self.items.get(index).map(|item| (index, item))) {
                    Some((index, item)) if !item.disabled => {
                        self.state.focused = true;
                        self.selected = Some(index);
                        ComponentEvent::Selected(self.id.clone(), index)
                    }
                    _ => ComponentEvent::None,
                }
            }
            _ => ComponentEvent::None,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        let _scope = crate::automation::scope(
            self.id.as_str(),
            "navigation",
            self.title.as_deref().unwrap_or(""),
            area,
        );
        let surface = self.surface();
        let inner = surface.inner(area);
        surface.render(area, buffer, context);
        let theme = context.compatibility_theme();
        let start = self.viewport_start(inner);
        for (index, item) in self
            .items
            .iter()
            .enumerate()
            .skip(start)
            .take(inner.height as usize)
        {
            let selected = self.selected == Some(index);
            let mut state = self.state;
            state.selected = selected;
            state.focused &= selected;
            state.disabled |= item.disabled;
            crate::automation::control(
                item.id.as_str(),
                "tab",
                &item.label,
                Rect::new(inner.x, inner.y + (index - start) as u16, inner.width, 1),
                state,
                &["click"],
            );
            let style = interactive_style(state, &theme);
            let marker = if state.focused && theme.keyboard_focus_visible() {
                ">"
            } else {
                " "
            };
            let icon = item.icon.as_deref().unwrap_or("");
            let text = if icon.is_empty() {
                format!("{marker} {}", item.label)
            } else {
                format!("{marker} {icon} {}", item.label)
            };
            Paragraph::new(text)
                .alignment(HorizontalAlignment::Left)
                .style(style)
                .render(
                    Rect::new(
                        inner.x,
                        inner.y.saturating_add((index - start) as u16),
                        inner.width,
                        1,
                    ),
                    buffer,
                );
        }
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }

    pub fn selected_item(&self) -> Option<&NavRailItem> {
        self.selected.and_then(|index| self.items.get(index))
    }

    fn index_at(&self, area: Rect, column: u16, row: u16) -> Option<usize> {
        let inner = self.surface().inner(area);
        if !contains_point(inner, column, row) {
            return None;
        }
        row.checked_sub(inner.y)
            .map(|offset| self.viewport_start(inner) + usize::from(offset))
            .filter(|index| *index < self.items.len())
    }

    fn surface(&self) -> Surface {
        let mut surface = Surface::new();
        surface.title = self.title.clone();
        surface
    }

    fn viewport_start(&self, inner: Rect) -> usize {
        super::List::automatic_viewport_start(self.selected.unwrap_or(0), usize::from(inner.height))
    }

    fn select_first(&mut self) -> ComponentEvent {
        self.set_selected(self.items.iter().position(|item| !item.disabled))
    }
    fn select_last(&mut self) -> ComponentEvent {
        self.set_selected(self.items.iter().rposition(|item| !item.disabled))
    }
    fn select_step(&mut self, step: isize) -> ComponentEvent {
        if self.items.is_empty() {
            return ComponentEvent::Consumed;
        }
        let start = self.selected.unwrap_or(0) as isize;
        for offset in 1..=self.items.len() {
            let next =
                (start + step * offset as isize).rem_euclid(self.items.len() as isize) as usize;
            if !self.items[next].disabled {
                return self.set_selected(Some(next));
            }
        }
        ComponentEvent::Consumed
    }
    fn set_selected(&mut self, selected: Option<usize>) -> ComponentEvent {
        let Some(selected) = selected else {
            return ComponentEvent::Consumed;
        };
        if self.selected == Some(selected) {
            ComponentEvent::Consumed
        } else {
            self.selected = Some(selected);
            ComponentEvent::Selected(self.id.clone(), selected)
        }
    }
}
