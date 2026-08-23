use std::borrow::Cow;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Borders, Paragraph, Widget};

use crate::TundraTheme;

use super::foundation::terminal_width;
use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseKind, contains_point,
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
            InputEvent::Key(key) if !key.is_press_like() => ComponentEvent::None,
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
                let inside = contains_point(area, mouse.column(), mouse.row());
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
                    MouseKind::Up(super::MouseButton::Left) => {
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
        self.bordered_widget(theme).render(area, buffer);
    }

    /// Renders the bordered button through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        frame.render_widget(self.bordered_widget(theme), area);
    }

    /// Renders a single-line button without a surrounding block.
    pub fn render_borderless(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.borderless_widget(theme).render(area, buffer);
    }

    /// Renders a single-line button without a surrounding block through a Ratatui [`Frame`].
    pub fn render_borderless_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        frame.render_widget(self.borderless_widget(theme), area);
    }

    /// Alias for [`Button::render_borderless_frame`] for inline action rows.
    pub fn render_inline_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        self.render_borderless_frame(frame, area, theme);
    }

    /// Renders only the interactive surface for a rich-content button.
    ///
    /// The caller may overlay specialized content such as an application icon,
    /// while the shared component remains responsible for the themed border,
    /// background, focus, disabled, and selected states.
    pub fn render_surface(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.surface_widget(theme).render(area, buffer);
    }

    /// Frame variant of [`Button::render_surface`].
    pub fn render_surface_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        frame.render_widget(self.surface_widget(theme), area);
    }

    /// Width of the button label after applying the shared square-bracket affordance.
    pub fn rendered_label_width(&self) -> usize {
        self.display_label()
            .lines()
            .map(terminal_width)
            .max()
            .unwrap_or(0)
    }

    fn bordered_widget<'a>(&'a self, theme: &TundraTheme) -> Paragraph<'a> {
        let style = self.button_style(theme);
        Paragraph::new(self.display_label())
            .alignment(HorizontalAlignment::Center)
            .style(style)
            .block(
                theme
                    .block()
                    .borders(Borders::ALL)
                    .style(style)
                    .border_style(
                        theme.selectable_border_style(self.state.selected || self.state.active),
                    ),
            )
    }

    fn borderless_widget<'a>(&'a self, theme: &TundraTheme) -> Paragraph<'a> {
        Paragraph::new(self.display_label())
            .alignment(HorizontalAlignment::Center)
            .style(self.button_style(theme))
    }

    fn display_label(&self) -> Cow<'_, str> {
        let trimmed = self.label.trim();
        if trimmed.is_empty() || (trimmed.starts_with('[') && trimmed.ends_with(']')) {
            return Cow::Borrowed(self.label.as_str());
        }

        // Reuse up to two existing ASCII padding cells so fixed-width inline
        // buttons retain their established layout after adding the brackets.
        let leading_len = self
            .label
            .len()
            .saturating_sub(self.label.trim_start().len());
        let trailing_start = leading_len.saturating_add(trimmed.len());
        let mut leading = &self.label[..leading_len];
        let mut trailing = &self.label[trailing_start..];
        let mut reclaimed = 0;
        while reclaimed < 2 && trailing.ends_with(' ') {
            trailing = &trailing[..trailing.len().saturating_sub(1)];
            reclaimed += 1;
        }
        while reclaimed < 2 && leading.ends_with(' ') {
            leading = &leading[..leading.len().saturating_sub(1)];
            reclaimed += 1;
        }

        Cow::Owned(format!("{leading}[{trimmed}]{trailing}"))
    }

    fn surface_widget(&self, theme: &TundraTheme) -> Paragraph<'static> {
        let mut surface_state = self.state;
        surface_state.selected = false;
        let style = Self::style_for_state(surface_state, theme);
        Paragraph::new("")
            .alignment(HorizontalAlignment::Left)
            .style(style)
            .block(
                theme
                    .block()
                    .borders(Borders::ALL)
                    .style(style)
                    .border_style(
                        theme.selectable_border_style(self.state.selected || self.state.active),
                    ),
            )
    }

    fn button_style(&self, theme: &TundraTheme) -> Style {
        Self::style_for_state(self.state, theme)
    }

    fn style_for_state(state: ComponentState, theme: &TundraTheme) -> Style {
        if state.disabled {
            return theme.muted_style();
        }

        let mut style = theme.body_style();
        if state.selected || state.hovered || state.active {
            style = style.fg(theme.accent_color);
        }
        if state.selected || state.focused || state.active {
            style = style.add_modifier(Modifier::BOLD);
        }
        style
    }
}
