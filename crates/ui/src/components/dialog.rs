use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::widgets::{Borders, Clear, Paragraph, Widget};

use crate::TundraTheme;

use super::foundation::terminal_width;
use super::{
    Button, ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    clamp_index, contains_point, inner_area,
};

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
}

impl Dialog {
    pub fn render_with_context(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        context: &crate::RenderContext,
    ) {
        self.render(area, buffer, &context.compatibility_theme());
    }

    pub fn new(
        id: impl Into<ComponentId>,
        title: impl Into<String>,
        body: impl Into<String>,
        actions: Vec<DialogAction>,
    ) -> Self {
        let selected_action = clamp_index(0, actions.len());
        let mut dialog = Self {
            id: id.into(),
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
                self.sync_button_states();
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) => match key.key {
                Key::Escape => {
                    self.close();
                    ComponentEvent::Dismissed(self.id.clone())
                }
                Key::Tab | Key::Right | Key::Down => self.select_next_action(),
                Key::BackTab | Key::Left | Key::Up => self.select_previous_action(),
                Key::Enter | Key::Space => self.activate_selected_action(),
                _ => ComponentEvent::Consumed,
            },
            InputEvent::Mouse(mouse) => {
                if !contains_point(area, mouse.column(), mouse.row()) {
                    return ComponentEvent::Consumed;
                }

                let action = self.action_index_at(area, mouse.column(), mouse.row());
                if matches!(
                    mouse.kind,
                    MouseKind::Down(MouseButton::Left)
                        | MouseKind::Click(MouseButton::Left)
                        | MouseKind::DoubleClick(MouseButton::Left)
                ) && let Some(index) = action
                {
                    self.selected_action = Some(index);
                    self.sync_button_states();
                }

                let layout = self.action_layout(area);
                let mut changed = false;
                let mut activated = None;
                for (index, button_area) in layout {
                    let Some(button) = self.buttons.get_mut(index) else {
                        continue;
                    };
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
        if !self.open {
            return;
        }

        Clear.render(area, buffer);
        self.block(theme).render(area, buffer);
        self.render_body(area, buffer, theme);

        let buttons = self.buttons_for_render();
        for (index, action_area) in self.action_layout(area) {
            if let Some(button) = buttons.get(index) {
                button.render_borderless(action_area, buffer, theme);
            }
        }
    }

    /// Renders the dialog through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        if !self.open {
            return;
        }

        frame.render_widget(Clear, area);
        frame.render_widget(self.block(theme), area);
        let inner = inner_area(area);
        let body_height = inner.height.saturating_sub(1);
        frame.render_widget(
            Paragraph::new(self.body.join("\n"))
                .alignment(HorizontalAlignment::Left)
                .style(theme.body_style()),
            Rect::new(inner.x, inner.y, inner.width, body_height),
        );

        let buttons = self.buttons_for_render();
        for (index, action_area) in self.action_layout(area) {
            if let Some(button) = buttons.get(index) {
                button.render_borderless_frame(frame, action_area, theme);
            }
        }
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

    fn action_index_at(&self, area: Rect, column: u16, row: u16) -> Option<usize> {
        self.action_layout(area)
            .into_iter()
            .find_map(|(index, action_area)| {
                contains_point(action_area, column, row).then_some(index)
            })
    }

    fn action_layout(&self, area: Rect) -> Vec<(usize, Rect)> {
        let inner = inner_area(area);
        if inner.height == 0 {
            return Vec::new();
        }

        let action_y = inner.y.saturating_add(inner.height.saturating_sub(1));
        let mut action_x = inner.x;
        self.actions
            .iter()
            .enumerate()
            .map_while(|(index, action)| {
                let width = u16::try_from(terminal_width(&action.label))
                    .unwrap_or(u16::MAX)
                    .saturating_add(2);
                if action_x.saturating_add(width) > inner.x.saturating_add(inner.width) {
                    return None;
                }
                let area = Rect::new(action_x, action_y, width, 1);
                action_x = action_x.saturating_add(width.saturating_add(1));
                Some((index, area))
            })
            .collect()
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
                .map(|action| Button::new(action.id.clone(), action.label.clone()))
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

    fn buttons_for_render(&self) -> Vec<Button> {
        self.actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let mut button = self
                    .buttons
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| Button::new(action.id.clone(), action.label.clone()));
                button.state.focused = self.state.focused && self.selected_action == Some(index);
                button.state.selected = self.selected_action == Some(index);
                button
            })
            .collect()
    }

    fn render_body(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        let inner = inner_area(area);
        let body_height = inner.height.saturating_sub(1);
        Paragraph::new(self.body.join("\n"))
            .alignment(HorizontalAlignment::Left)
            .style(theme.body_style())
            .render(
                Rect::new(inner.x, inner.y, inner.width, body_height),
                buffer,
            );
    }

    fn block(&self, theme: &TundraTheme) -> ratatui::widgets::Block<'static> {
        theme
            .block()
            .title(self.title.clone())
            .borders(Borders::ALL)
            .style(theme.body_style())
    }
}
