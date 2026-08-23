use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::{RenderContext, TundraTheme};

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    contains_point,
};

/// A compact single-choice field. Unlike a page-local cycle control it owns
/// standard keyboard and mouse behavior, so settings and dialogs stay alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub id: ComponentId,
    pub options: Vec<String>,
    pub selected: Option<usize>,
    pub state: ComponentState,
    pub title: Option<String>,
}

impl Picker {
    pub fn new(
        id: impl Into<ComponentId>,
        options: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let options = options.into_iter().map(Into::into).collect::<Vec<_>>();
        Self {
            id: id.into(),
            selected: (!options.is_empty()).then_some(0),
            options,
            state: ComponentState::default(),
            title: None,
        }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn set_selected(&mut self, selected: Option<usize>) {
        self.selected = selected.filter(|index| *index < self.options.len());
    }

    pub fn selected_value(&self) -> Option<&str> {
        self.selected
            .and_then(|index| self.options.get(index).map(String::as_str))
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        if self.state.disabled {
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
            InputEvent::Key(key) if key.is_press_like() && self.state.focused => match key.key {
                Key::Left | Key::Up => self.select_offset(-1),
                Key::Right | Key::Down | Key::Space | Key::Enter => self.select_offset(1),
                Key::Home => self.select_index(0),
                Key::End => self.select_index(self.options.len().saturating_sub(1)),
                _ => ComponentEvent::None,
            },
            InputEvent::Mouse(mouse)
                if contains_point(area, mouse.column(), mouse.row())
                    && matches!(
                        mouse.kind,
                        MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left)
                    ) =>
            {
                self.state.focused = true;
                self.select_offset(1)
            }
            _ => ComponentEvent::None,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        let tokens = context.theme;
        let selected = self.selected_value().unwrap_or("—");
        let marker = if self.state.focused { "‹ " } else { "  " };
        let suffix = if self.state.focused { " ›" } else { "  " };
        let style = if self.state.disabled {
            ratatui::style::Style::default()
                .fg(tokens.muted)
                .bg(tokens.surface)
        } else if self.state.focused {
            ratatui::style::Style::default()
                .fg(tokens.focus)
                .bg(tokens.accent_soft)
        } else {
            ratatui::style::Style::default()
                .fg(tokens.text)
                .bg(tokens.surface)
        };
        let mut block = Block::default().style(style);
        if let Some(title) = &self.title {
            block = block
                .title(title.as_str())
                .borders(Borders::ALL)
                .border_style(
                    ratatui::style::Style::default()
                        .fg(if self.state.focused {
                            tokens.focus
                        } else {
                            tokens.border
                        })
                        .bg(tokens.surface),
                );
        }
        let inner = block.inner(area);
        block.render(area, buffer);
        Paragraph::new(Line::styled(format!("{marker}{selected}{suffix}"), style))
            .alignment(HorizontalAlignment::Left)
            .render(inner, buffer);
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        let mut buffer = frame.buffer_mut();
        self.render(area, &mut buffer, context);
    }

    pub fn render_with_theme(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.render(
            area,
            buffer,
            &RenderContext::from_theme(theme, Default::default(), Default::default()),
        );
    }

    fn select_offset(&mut self, offset: isize) -> ComponentEvent {
        let len = self.options.len();
        if len == 0 {
            return ComponentEvent::Consumed;
        }
        let current = self.selected.unwrap_or(0) as isize;
        let next = (current + offset).rem_euclid(len as isize) as usize;
        self.select_index(next)
    }

    fn select_index(&mut self, next: usize) -> ComponentEvent {
        if self.selected == Some(next) {
            ComponentEvent::Consumed
        } else {
            self.selected = Some(next);
            ComponentEvent::Selected(self.id.clone(), next)
        }
    }
}
