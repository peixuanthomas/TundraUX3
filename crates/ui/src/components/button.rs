use std::borrow::Cow;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonEmphasis {
    #[default]
    Normal,
    Primary,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub id: ComponentId,
    pub label: String,
    pub state: ComponentState,
    pub bracketed_label: bool,
    pub latched: bool,
    pub current: bool,
    pub centered_label: bool,
    pub pressed_fill: bool,
    pub emphasis: ButtonEmphasis,
    pub focus_fill: Option<ratatui::style::Color>,
    capture_role: &'static str,
    sensitive: bool,
    capture_selected: Option<bool>,
    capture_name: Option<String>,
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
            current: false,
            centered_label: false,
            pressed_fill: false,
            emphasis: ButtonEmphasis::Normal,
            focus_fill: None,
            capture_role: "button",
            sensitive: false,
            capture_selected: None,
            capture_name: None,
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

    /// Highlights the current page or value independently of input focus.
    pub fn with_current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }

    pub fn with_centered_label(mut self, centered: bool) -> Self {
        self.centered_label = centered;
        self
    }

    /// Fill held or latched keycaps and keep their legends readable.
    pub fn with_pressed_fill(mut self, filled: bool) -> Self {
        self.pressed_fill = filled;
        self
    }

    pub fn with_emphasis(mut self, emphasis: ButtonEmphasis) -> Self {
        self.emphasis = emphasis;
        self
    }

    pub fn with_focus_fill(mut self, fill: Option<ratatui::style::Color>) -> Self {
        self.focus_fill = fill;
        self
    }

    /// Describes fields and checkboxes that reuse the shared button painting.
    pub fn with_capture_role(mut self, role: &'static str) -> Self {
        self.capture_role = role;
        self
    }

    pub fn with_sensitive(mut self, sensitive: bool) -> Self {
        self.sensitive = sensitive;
        self
    }

    /// A checkbox value is independent of the field currently receiving input.
    pub fn with_capture_selected(mut self, selected: bool) -> Self {
        self.capture_selected = Some(selected);
        self
    }

    pub fn with_capture_name(mut self, name: impl Into<String>) -> Self {
        self.capture_name = Some(name.into());
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
        let style = self.style_for_state(state, theme);
        Paragraph::new(self.label_text(area.height.saturating_sub(2)))
            .alignment(HorizontalAlignment::Center)
            .style(style)
            .block(
                theme
                    .block()
                    .borders(Borders::ALL)
                    .style(style)
                    .border_style(self.border_style(state, theme)),
            )
    }

    fn borderless_widget<'a>(&'a self, area: Rect, theme: &TundraTheme) -> Paragraph<'a> {
        let style = self.style_for_state(self.render_state(area, theme), theme);
        Paragraph::new(self.label_text(area.height))
            .alignment(HorizontalAlignment::Center)
            .style(style)
    }

    fn label_text(&self, height: u16) -> Text<'_> {
        let mut lines = Text::from(self.display_label()).lines;
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
        let mut style = self.style_for_state(surface_state, theme);
        if !self.pressed_fill || !state.active || state.disabled {
            style = style.bg(theme.tokens().raised);
        }
        Paragraph::new("")
            .alignment(HorizontalAlignment::Left)
            .style(style)
            .block(
                theme
                    .block()
                    .borders(Borders::ALL)
                    .style(style)
                    .border_style(self.border_style(state, theme)),
            )
    }

    fn render_state(&self, area: Rect, theme: &TundraTheme) -> ComponentState {
        let mut captured_state = self.state;
        captured_state.focused |= self.state.selected && !self.current;
        captured_state.selected |= self.current || self.latched;
        if crate::automation::enabled() {
            let mut node = crate::automation::node(
                self.id.as_str(),
                self.capture_role,
                self.capture_name.as_deref().unwrap_or(&self.label),
                area,
            );
            node.focused = captured_state.focused;
            node.selected = self.capture_selected.unwrap_or(captured_state.selected);
            node.enabled = !captured_state.disabled;
            node.sensitive = self.sensitive;
            node.actions = if self.capture_role == "textbox" {
                vec!["click".into(), "text".into(), "key".into()]
            } else {
                vec!["click".into()]
            };
            crate::automation::record(node);
        }
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
        state.selected |= self.current;
        state
    }

    fn border_style(&self, state: ComponentState, theme: &TundraTheme) -> Style {
        if state.disabled {
            return theme.disabled_style();
        }
        if state.active {
            if self.pressed_fill {
                return theme
                    .border_style()
                    .fg(theme.button_pressed_color())
                    .bg(theme.button_pressed_color());
            }
            return theme.border_style().fg(theme.button_pressed_color());
        }
        if state.hovered || state.focused || state.selected {
            return theme.border_style().fg(theme.button_accent_color());
        }
        match self.emphasis {
            ButtonEmphasis::Primary => theme.border_style().fg(theme.button_accent_color()),
            ButtonEmphasis::Danger => theme.border_style().fg(theme.error),
            ButtonEmphasis::Normal => theme.border_style(),
        }
    }

    fn style_for_state(&self, state: ComponentState, theme: &TundraTheme) -> Style {
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
        if (state.hovered || state.focused || state.selected)
            && !state.active
            && let Some(fill) = self.focus_fill
        {
            style = style.bg(fill);
        }
        if state.active {
            style = if self.pressed_fill {
                theme.tokens().filled_style(theme.button_pressed_color())
            } else {
                style.fg(theme.button_pressed_color())
            };
        } else if state.hovered || state.focused || state.selected {
            style = style.fg(if self.emphasis == ButtonEmphasis::Danger {
                theme.error
            } else {
                theme.button_accent_color()
            });
        } else {
            match self.emphasis {
                ButtonEmphasis::Primary => {
                    style = style
                        .fg(theme.button_accent_color())
                        .add_modifier(Modifier::BOLD);
                }
                ButtonEmphasis::Danger => style = style.fg(theme.error),
                ButtonEmphasis::Normal => {}
            }
        }
        if state.selected || state.focused || state.active {
            style = style.add_modifier(Modifier::BOLD);
        }
        style
    }
}

#[cfg(test)]
#[path = "../../tests/unit/components/button_emphasis/tests.rs"]
mod emphasis_tests;
