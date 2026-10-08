use std::borrow::Cow;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Borders, Paragraph, Widget};

use crate::TundraTheme;

use super::foundation::terminal_width;
use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseKind, contains_point,
};

/// The exact rectangle painted by a shared button, reused by the host for
/// pointer capture. IDs distinguish adjacent controls across redraws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonRegion {
    pub id: ComponentId,
    pub area: Rect,
    pub disabled: bool,
}

/// Explicit per-frame interaction context. Compatibility themes share this
/// collector, so nested dialogs and legacy pages register the same rectangles
/// they paint without a second, potentially divergent button layout.
#[derive(Debug, Clone)]
pub struct ButtonFrame {
    pub keyboard_focus_visible: bool,
    pub background: ratatui::style::Color,
    pub accent: ratatui::style::Color,
    pub pressed_color: ratatui::style::Color,
    pub hovered: Option<ButtonRegion>,
    pub pressed: Option<ButtonRegion>,
    regions: std::sync::Arc<std::sync::Mutex<Vec<ButtonRegion>>>,
}

impl PartialEq for ButtonFrame {
    fn eq(&self, other: &Self) -> bool {
        self.background == other.background
            && self.keyboard_focus_visible == other.keyboard_focus_visible
            && self.accent == other.accent
            && self.pressed_color == other.pressed_color
            && self.hovered == other.hovered
            && self.pressed == other.pressed
            && std::sync::Arc::ptr_eq(&self.regions, &other.regions)
    }
}
impl Eq for ButtonFrame {}

impl ButtonFrame {
    pub fn new(
        hovered: Option<ButtonRegion>,
        pressed: Option<ButtonRegion>,
        theme: &TundraTheme,
    ) -> Self {
        Self {
            keyboard_focus_visible: true,
            background: theme.background,
            accent: theme.accent_color,
            pressed_color: theme.button_pressed_color(),
            hovered,
            pressed,
            regions: Default::default(),
        }
    }

    pub fn regions(&self) -> Vec<ButtonRegion> {
        self.regions.lock().expect("button frame registry").clone()
    }

    /// Registers a visible button after a scrolling page translates its canvas.
    pub fn register_region(&self, region: ButtonRegion) {
        if region.area.width > 0 && region.area.height > 0 {
            self.regions
                .lock()
                .expect("button frame registry")
                .push(region);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub id: ComponentId,
    pub label: String,
    pub state: ComponentState,
    pub bracketed_label: bool,
    pub latched: bool,
    pub centered_label: bool,
    pub alternate_label: Option<(String, bool)>,
}

impl Button {
    pub fn render_with_context(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        context: &crate::RenderContext,
    ) {
        self.render(area, buffer, &context.compatibility_theme());
    }

    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            state: ComponentState::default(),
            bracketed_label: true,
            latched: false,
            centered_label: false,
            alternate_label: None,
        }
    }

    /// Keeps the shared button's interaction styles while allowing literal keycaps.
    pub fn with_bracketed_label(mut self, bracketed: bool) -> Self {
        self.bracketed_label = bracketed;
        self
    }

    /// Persistent pressed feedback, independent of pointer or keyboard focus.
    pub fn with_latched(mut self, latched: bool) -> Self {
        self.latched = latched;
        self
    }

    pub fn with_centered_label(mut self, centered: bool) -> Self {
        self.centered_label = centered;
        self
    }

    /// Shows an alternate above the ordinary label, muting the inactive value.
    /// Small buttons show only the active value.
    pub fn with_alternate_label(mut self, label: impl Into<String>, active: bool) -> Self {
        self.alternate_label = Some((label.into(), active));
        self
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
                        self.state.hovered = true;
                        self.state.active = true;
                        ComponentEvent::FocusRequested(self.id.clone())
                    }
                    MouseKind::Up(super::MouseButton::Left) => {
                        self.state.hovered = false;
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
                    MouseKind::Drag(super::MouseButton::Left) if self.state.active && !inside => {
                        self.state.hovered = false;
                        self.state.active = false;
                        ComponentEvent::Consumed
                    }
                    MouseKind::Click(button) if inside && button == super::MouseButton::Left => {
                        self.state.focused = true;
                        self.state.hovered = false;
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
        self.bordered_widget(area, theme).render(area, buffer);
    }

    /// Renders the bordered button through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        frame.render_widget(self.bordered_widget(area, theme), area);
    }

    /// Renders a single-line button without a surrounding block.
    pub fn render_borderless(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.borderless_widget(area, theme).render(area, buffer);
    }

    /// Renders a single-line button without a surrounding block through a Ratatui [`Frame`].
    pub fn render_borderless_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        frame.render_widget(self.borderless_widget(area, theme), area);
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
        self.surface_widget(area, theme).render(area, buffer);
    }

    /// Frame variant of [`Button::render_surface`].
    pub fn render_surface_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        frame.render_widget(self.surface_widget(area, theme), area);
    }

    /// Width of the button label after applying its configured decoration.
    pub fn rendered_label_width(&self) -> usize {
        self.display_label()
            .lines()
            .map(terminal_width)
            .max()
            .unwrap_or(0)
    }

    fn bordered_widget<'a>(&'a self, area: Rect, theme: &TundraTheme) -> Paragraph<'a> {
        let state = self.render_state(area, theme);
        let style = Self::style_for_state(state, theme);
        Paragraph::new(self.label_text(area.height.saturating_sub(2), style, theme))
            .alignment(HorizontalAlignment::Center)
            .style(style)
            .block(
                theme
                    .block()
                    .borders(Borders::ALL)
                    .style(style)
                    .border_style(Self::border_style(state, theme)),
            )
    }

    fn borderless_widget<'a>(&'a self, area: Rect, theme: &TundraTheme) -> Paragraph<'a> {
        let style = Self::style_for_state(self.render_state(area, theme), theme);
        Paragraph::new(self.label_text(area.height, style, theme))
            .alignment(HorizontalAlignment::Center)
            .style(style)
    }

    fn label_text<'a>(&'a self, height: u16, style: Style, theme: &TundraTheme) -> Text<'a> {
        let mut lines = if let Some((alternate, active)) = &self.alternate_label {
            let ordinary = self.display_label();
            if height >= 2 {
                let muted = style
                    .fg(theme.tokens().muted)
                    .remove_modifier(Modifier::BOLD);
                vec![
                    Line::from(Span::styled(
                        alternate.as_str(),
                        if *active { style } else { muted },
                    )),
                    Line::from(Span::styled(ordinary, if *active { muted } else { style })),
                ]
            } else {
                vec![Line::from(if *active {
                    Cow::Borrowed(alternate.as_str())
                } else {
                    ordinary
                })]
            }
        } else {
            Text::from(self.display_label()).lines
        };
        if self.centered_label {
            let padding = usize::from(height).saturating_sub(lines.len()) / 2;
            lines.splice(0..0, std::iter::repeat_n(Line::default(), padding));
        }
        Text::from(lines)
    }

    fn display_label(&self) -> Cow<'_, str> {
        if !self.bracketed_label {
            return Cow::Borrowed(self.label.as_str());
        }
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

    fn surface_widget(&self, area: Rect, theme: &TundraTheme) -> Paragraph<'static> {
        let state = self.render_state(area, theme);
        let mut surface_state = state;
        surface_state.selected = false;
        let style = Self::style_for_state(surface_state, theme).bg(theme.tokens().raised);
        Paragraph::new("")
            .alignment(HorizontalAlignment::Left)
            .style(style)
            .block(
                theme
                    .block()
                    .borders(Borders::ALL)
                    .style(style)
                    .border_style(Self::border_style(state, theme)),
            )
    }

    fn render_state(&self, area: Rect, theme: &TundraTheme) -> ComponentState {
        let mut state = self.state;
        if let Some(frame) = &theme.buttons {
            state.focused &= frame.keyboard_focus_visible;
            state.selected &= frame.keyboard_focus_visible;
            let region = ButtonRegion {
                id: self.id.clone(),
                area,
                disabled: state.disabled,
            };
            frame
                .regions
                .lock()
                .expect("button frame registry")
                .push(region.clone());
            state.hovered = frame.hovered.as_ref() == Some(&region);
            state.active = state.hovered && frame.pressed.as_ref() == Some(&region);
        }
        state.active |= self.latched && !state.disabled;
        state
    }

    fn border_style(state: ComponentState, theme: &TundraTheme) -> Style {
        if state.disabled {
            return theme.disabled_style();
        }
        if state.active {
            return theme.border_style().fg(theme.button_pressed_color());
        }
        if state.hovered || state.focused || state.selected {
            return theme.border_style().fg(theme.button_accent_color());
        }
        theme.border_style()
    }

    fn style_for_state(state: ComponentState, theme: &TundraTheme) -> Style {
        if state.disabled {
            return theme.disabled_style();
        }

        let mut style = theme.body_style();
        if (state.hovered || state.active || state.focused || state.selected)
            && let Some(frame) = &theme.buttons
        {
            // Swatches and menu tabs may supply a local fill equal to the
            // shared accent. Use the frame canvas so emphasized text stays visible.
            style = style.bg(frame.background);
        }
        if state.active {
            style = style.fg(theme.button_pressed_color());
        } else if state.hovered || state.focused || state.selected {
            style = style.fg(theme.button_accent_color());
        }
        if state.selected || state.focused || state.active {
            style = style.add_modifier(Modifier::BOLD);
        }
        style
    }
}
