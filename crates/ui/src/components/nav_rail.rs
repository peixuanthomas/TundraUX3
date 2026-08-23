use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::RenderContext;

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    contains_point,
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

/// Vertical navigation with a one-cell selection indicator instead of an
/// accent-filled card.
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
        let tokens = context.theme;
        let mut block = Block::default()
            .style(Style::default().fg(tokens.text).bg(tokens.surface))
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(tokens.border).bg(tokens.surface));
        if let Some(title) = &self.title {
            block = block.title(title.as_str());
        }
        let inner = block.inner(area);
        block.render(area, buffer);
        for (index, item) in self.items.iter().enumerate().take(inner.height as usize) {
            let selected = self.selected == Some(index);
            let style = if item.disabled {
                Style::default().fg(tokens.muted).bg(tokens.surface)
            } else if selected {
                Style::default()
                    .fg(if self.state.focused {
                        tokens.focus
                    } else {
                        tokens.accent_strong
                    })
                    .bg(tokens.surface)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(tokens.text).bg(tokens.surface)
            };
            let marker = if selected { "▎" } else { " " };
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
                        inner.y.saturating_add(index as u16),
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
        if !contains_point(area, column, row) {
            return None;
        }
        let title_offset = u16::from(self.title.is_some());
        row.checked_sub(area.y.saturating_add(title_offset))
            .map(usize::from)
            .filter(|index| *index < self.items.len())
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
