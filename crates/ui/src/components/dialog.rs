use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Clear, Paragraph, Widget, Wrap};

use crate::{RenderContext, TundraTheme};

use super::foundation::terminal_width;
use super::{
    Button, ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    PageAction, PageActionBar, clamp_index, contains_point,
};

/// Geometry shared by a dialog's rendering and hit testing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DialogLayout {
    pub area: Rect,
    pub header: Rect,
    pub close: Rect,
    pub body: Rect,
    pub footer: Rect,
    band: Rect,
}

impl DialogLayout {
    pub const INSET: u16 = 3;

    const fn inset(width: u16) -> u16 {
        if width >= 12 {
            Self::INSET
        } else if width >= 6 {
            1
        } else {
            0
        }
    }

    pub const fn content_width(width: u16) -> u16 {
        width - Self::inset(width) * 2
    }

    pub fn new(area: Rect, footer_rows: u16, close_label: Option<&str>) -> Self {
        Self::with_min_body(area, footer_rows, close_label, 1)
    }

    pub fn with_min_body(
        area: Rect,
        footer_rows: u16,
        close_label: Option<&str>,
        minimum_body_rows: u16,
    ) -> Self {
        let x = area.x + Self::inset(area.width);
        let width = Self::content_width(area.width);
        let header_height = u16::from(area.height > 0);
        let available = area.height.saturating_sub(header_height);
        let footer_rows = footer_rows.min(available);
        let spare = (available - footer_rows).saturating_sub(minimum_body_rows);
        let top = u16::from(spare >= 3);
        let bottom = u16::from(spare >= 3);
        let header_gap = u16::from(spare >= 1);
        let footer_gap = u16::from(footer_rows > 0 && spare >= 4);
        let footer_padding = u16::from(footer_rows > 0 && spare >= 5);
        let close_width = close_label.map_or(0, |label| {
            (terminal_width(label).saturating_add(2)).min(usize::from(width)) as u16
        });
        let header = Rect::new(
            x,
            area.y + top,
            width.saturating_sub(close_width.saturating_add(u16::from(close_width > 0))),
            header_height,
        );
        let close = Rect::new(
            x + width - close_width,
            header.y,
            close_width,
            header.height,
        );
        let footer = Rect::new(x, area.bottom() - bottom - footer_rows, width, footer_rows);
        let band = Rect::new(
            area.x,
            footer.y - footer_padding,
            area.width,
            if footer_rows > 0 {
                footer_padding + footer_rows + bottom
            } else {
                0
            },
        );
        let body_y = header.bottom() + header_gap;
        let body = Rect::new(
            x,
            body_y,
            width,
            band.y.saturating_sub(footer_gap).saturating_sub(body_y),
        );
        Self {
            area,
            header,
            close,
            body,
            footer,
            band,
        }
    }

    pub fn render(self, buffer: &mut Buffer, title: &str, context: &RenderContext) {
        Clear.render(self.area, buffer);
        buffer.set_style(
            self.area,
            Style::default()
                .fg(context.theme.text)
                .bg(context.theme.surface),
        );
        buffer.set_style(self.band, Style::default().bg(context.theme.raised));
        Paragraph::new(title)
            .style(Style::default().add_modifier(Modifier::BOLD))
            .render(self.header, buffer);
    }

    pub fn render_frame(self, frame: &mut Frame<'_>, title: &str, context: &RenderContext) {
        self.render(frame.buffer_mut(), title, context);
    }

    pub(crate) fn surface_theme(theme: &TundraTheme) -> RenderContext {
        RenderContext::from_theme(
            theme,
            Default::default(),
            crate::RenderCapabilities {
                color: theme.color_capability,
                ..Default::default()
            },
        )
    }

    pub fn body_theme(context: &RenderContext) -> TundraTheme {
        Self::theme_on(context, context.theme.surface)
    }

    pub fn footer_theme(context: &RenderContext) -> TundraTheme {
        Self::theme_on(context, context.theme.raised)
    }

    pub fn close_theme(context: &RenderContext) -> TundraTheme {
        let mut theme = Self::body_theme(context);
        theme.foreground = context.theme.muted;
        theme
    }

    pub(crate) fn action_areas(
        self,
        bar: &PageActionBar,
        selected: Option<usize>,
    ) -> Vec<(usize, Rect)> {
        let height = bar.height(self.footer.width);
        let canvas = Rect::new(self.footer.x, 0, self.footer.width, height);
        let actions = bar.layout(canvas).actions;
        let start = height.saturating_sub(self.footer.height);
        let start = selected
            .and_then(|index| actions.iter().find(|action| action.index == index))
            .map_or(start, |action| start.min(action.area.y));
        let visible = Rect::new(self.footer.x, start, self.footer.width, self.footer.height);
        actions
            .into_iter()
            .filter_map(|action| {
                let mut area = action.area.intersection(visible);
                if area.is_empty() {
                    return None;
                }
                area.y = self.footer.y + area.y - start;
                Some((action.index, area))
            })
            .collect()
    }

    fn theme_on(context: &RenderContext, background: Color) -> TundraTheme {
        let mut theme = context.compatibility_theme();
        theme.background = background;
        if let Some(buttons) = &mut theme.buttons {
            buttons.background = background;
        }
        theme
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogAction {
    pub id: ComponentId,
    pub label: String,
}

impl DialogAction {
    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub id: ComponentId,
    pub title: String,
    pub body: Vec<String>,
    pub actions: Vec<DialogAction>,
    pub state: ComponentState,
    pub open: bool,
    selected_action: Option<usize>,
    buttons: Vec<Button>,
    close_button: Button,
}

impl Dialog {
    pub fn render_with_context(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        context: &crate::RenderContext,
    ) {
        if !self.open {
            return;
        }
        let _scope = crate::automation::scope(self.id.as_str(), "dialog", &self.title, area);
        let (layout, actions) = self.layout(area);
        layout.render(buffer, &self.title, context);
        self.close_button
            .clone()
            .with_background(context.theme.surface)
            .render_borderless(layout.close, buffer, &DialogLayout::close_theme(context));
        Paragraph::new(self.body.join("\n"))
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(context.theme.muted))
            .render(layout.body, buffer);

        let theme = DialogLayout::footer_theme(context);
        for (index, action_area) in actions {
            let action = &self.actions[index];
            let mut button = self
                .buttons
                .get(index)
                .cloned()
                .unwrap_or_else(|| Button::new(action.id.clone(), action.label.clone()))
                .with_bracketed_label(false)
                .with_background(context.theme.control_background())
                .with_label_alignment(HorizontalAlignment::Center)
                .with_focus_fill(Some(context.theme.accent_soft));
            let label_width = action_area
                .width
                .saturating_sub(u16::from(action_area.width > 4) * 2);
            button.label = crate::management_wrapped_lines(&action.label, label_width).join("\n");
            button.state.focused = self.state.focused && self.selected_action == Some(index);
            button.state.selected = self.selected_action == Some(index);
            button.render_borderless(action_area, buffer, &theme);
        }
    }

    pub fn new(
        id: impl Into<ComponentId>,
        title: impl Into<String>,
        body: impl Into<String>,
        actions: Vec<DialogAction>,
    ) -> Self {
        let id = id.into();
        let selected_action = clamp_index(0, actions.len());
        let mut dialog = Self {
            close_button: Button::new(format!("{}.close", id.as_str()), Key::Escape.label())
                .with_bracketed_label(false)
                .with_centered_label(true),
            id,
            title: title.into(),
            body: vec![body.into()],
            actions,
            state: ComponentState::default(),
            open: false,
            selected_action,
            buttons: Vec::new(),
        };
        dialog.sync_buttons();
        dialog
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
        if !open {
            self.state.active = false;
            self.cancel_pointer();
        }
        self.sync_button_states();
    }

    pub fn selected_action_index(&self) -> Option<usize> {
        self.selected_action
    }

    pub fn set_selected_action(&mut self, index: Option<usize>) {
        self.selected_action = index
            .filter(|index| *index < self.actions.len())
            .or_else(|| clamp_index(0, self.actions.len()));
        self.sync_button_states();
    }

    pub fn set_selected(&mut self, index: Option<usize>) {
        self.set_selected_action(index);
    }

    /// Returns the button areas used by both rendering and hit testing.
    pub fn action_areas(&self, area: Rect) -> Vec<(usize, Rect)> {
        self.layout(area).1
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        if !self.open {
            return ComponentEvent::None;
        }

        self.sync_buttons();
        match event {
            InputEvent::Key(key) if !key.is_press_like() => ComponentEvent::None,
            InputEvent::FocusGained => {
                self.state.focused = true;
                self.sync_button_states();
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.state.focused = false;
                self.cancel_pointer();
                self.sync_button_states();
                ComponentEvent::Consumed
            }
            InputEvent::Resize { .. } => {
                self.cancel_pointer();
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) => {
                self.cancel_pointer();
                match key.key {
                    Key::Escape => self.dismiss(),
                    Key::Tab | Key::Right | Key::Down => self.select_next_action(),
                    Key::BackTab | Key::Left | Key::Up => self.select_previous_action(),
                    Key::Enter | Key::Space => self.activate_selected_action(),
                    _ => ComponentEvent::Consumed,
                }
            }
            InputEvent::Mouse(mouse) => {
                let (layout, actions) = self.layout(area);
                if matches!(
                    mouse.kind,
                    MouseKind::Down(MouseButton::Left)
                        | MouseKind::Click(MouseButton::Left)
                        | MouseKind::DoubleClick(MouseButton::Left)
                ) {
                    self.cancel_pointer();
                    if let Some((index, _)) = actions
                        .iter()
                        .find(|(_, action)| contains_point(*action, mouse.column(), mouse.row()))
                    {
                        self.selected_action = Some(*index);
                        self.sync_button_states();
                    }
                }

                let close = self
                    .close_button
                    .handle_event(InputEvent::Mouse(mouse), layout.close);
                if matches!(close, ComponentEvent::Activated(_)) {
                    return self.dismiss();
                }
                let mut changed = matches!(
                    close,
                    ComponentEvent::Changed(_) | ComponentEvent::FocusRequested(_)
                );
                let mut activated = None;
                for (index, button_area) in actions {
                    let button = &mut self.buttons[index];
                    match button.handle_event(InputEvent::Mouse(mouse), button_area) {
                        ComponentEvent::Activated(id) => activated = Some(id),
                        ComponentEvent::Changed(_) | ComponentEvent::FocusRequested(_) => {
                            changed = true;
                        }
                        _ => {}
                    }
                }

                if let Some(id) = activated {
                    ComponentEvent::Activated(id)
                } else if changed {
                    ComponentEvent::Changed(self.id.clone())
                } else {
                    ComponentEvent::Consumed
                }
            }
            _ => ComponentEvent::Consumed,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.render_with_context(area, buffer, &DialogLayout::surface_theme(theme));
    }

    /// Renders the dialog through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        self.render(area, frame.buffer_mut(), theme);
    }

    fn select_next_action(&mut self) -> ComponentEvent {
        if self.actions.is_empty() {
            return ComponentEvent::Consumed;
        }

        let index = self
            .selected_action
            .map(|index| (index + 1) % self.actions.len())
            .unwrap_or(0);
        self.selected_action = Some(index);
        self.sync_button_states();
        ComponentEvent::Selected(self.id.clone(), index)
    }

    fn select_previous_action(&mut self) -> ComponentEvent {
        if self.actions.is_empty() {
            return ComponentEvent::Consumed;
        }

        let index = self
            .selected_action
            .map(|index| {
                if index == 0 {
                    self.actions.len().saturating_sub(1)
                } else {
                    index.saturating_sub(1)
                }
            })
            .unwrap_or(0);
        self.selected_action = Some(index);
        self.sync_button_states();
        ComponentEvent::Selected(self.id.clone(), index)
    }

    fn activate_selected_action(&self) -> ComponentEvent {
        self.selected_action
            .and_then(|index| self.actions.get(index))
            .map(|action| ComponentEvent::Activated(action.id.clone()))
            .unwrap_or(ComponentEvent::Consumed)
    }

    fn dismiss(&mut self) -> ComponentEvent {
        self.close();
        ComponentEvent::Dismissed(self.id.clone())
    }

    fn cancel_pointer(&mut self) {
        self.close_button.state.active = false;
        self.close_button.state.hovered = false;
        for button in &mut self.buttons {
            button.state.active = false;
            button.state.hovered = false;
        }
    }

    fn layout(&self, area: Rect) -> (DialogLayout, Vec<(usize, Rect)>) {
        let bar = PageActionBar::new("").with_actions(
            self.actions
                .iter()
                .map(|action| PageAction::new(action.id.clone(), action.label.clone()))
                .collect(),
        );
        let footer_rows = bar
            .height(DialogLayout::content_width(area.width))
            .min(area.height.saturating_sub(2));
        let body_rows = area.height.saturating_sub(footer_rows + 1).min(3);
        let layout = DialogLayout::with_min_body(
            area,
            footer_rows,
            Some(&self.close_button.label),
            body_rows,
        );
        let mut actions = layout.action_areas(&bar, self.selected_action);
        actions.sort_unstable_by_key(|(index, _)| *index);
        (layout, actions)
    }

    fn sync_buttons(&mut self) {
        if self.buttons.len() != self.actions.len()
            || self
                .buttons
                .iter()
                .zip(&self.actions)
                .any(|(button, action)| {
                    button.id != action.id || button.label.trim() != action.label
                })
        {
            self.buttons = self
                .actions
                .iter()
                .map(|action| {
                    Button::new(action.id.clone(), action.label.clone()).with_bracketed_label(false)
                })
                .collect();
        }
        self.selected_action = self
            .selected_action
            .filter(|index| *index < self.actions.len())
            .or_else(|| clamp_index(0, self.actions.len()));
        self.sync_button_states();
    }

    fn sync_button_states(&mut self) {
        for (index, button) in self.buttons.iter_mut().enumerate() {
            button.state.focused = self.state.focused && self.selected_action == Some(index);
            button.state.selected = self.selected_action == Some(index);
            if !self.open {
                button.state.active = false;
                button.state.hovered = false;
            }
        }
    }
}
