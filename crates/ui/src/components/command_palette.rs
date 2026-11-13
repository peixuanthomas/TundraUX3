use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Widget};

use crate::{RenderContext, TundraTheme};

use super::{
    Button, ComponentEvent, ComponentId, ComponentState, DialogLayout, InputEvent, Key, List,
    ListItem, MouseButton, MouseKind, TextInput, contains_point,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPaletteCommand {
    pub id: ComponentId,
    pub title: String,
    pub hint: Option<String>,
    pub keywords: Vec<String>,
    pub disabled: bool,
}

impl CommandPaletteCommand {
    pub fn new(id: impl Into<ComponentId>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            hint: None,
            keywords: Vec::new(),
            disabled: false,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_keywords<I, S>(mut self, keywords: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPalette {
    pub id: ComponentId,
    pub commands: Vec<CommandPaletteCommand>,
    pub state: ComponentState,
    pub open: bool,
    query_input: TextInput,
    results: List,
    close_button: Button,
    pressed: Option<(ComponentId, Rect)>,
}

impl CommandPalette {
    pub fn render_with_context(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        if !self.open {
            return;
        }
        let title = i18n::tr!("ui-components-command-palette");
        let _scope = crate::automation::scope(self.id.as_str(), "dialog", &title, area);
        let layout = self.layout(area);
        let theme = DialogLayout::body_theme(context);
        layout.render(buffer, &title, context);
        self.close_button
            .clone()
            .with_background(context.theme.surface)
            .render_borderless(layout.close, buffer, &DialogLayout::close_theme(context));

        let mut input = self.query_input.clone();
        input.set_focused(self.state.focused);
        input.render_borderless_with_prefix(Self::query_area(layout), buffer, &theme, "⌕ ");

        let mut results = self
            .results_for_render()
            .with_background(context.theme.surface);
        if let Some((id, _)) = &self.pressed {
            results.set_hovered(results.items.iter().position(|item| &item.id == id));
        }
        let results_area = Self::results_area(layout);
        if results.items.is_empty() {
            let message = i18n::tr!("management-empty");
            Paragraph::new(message)
                .style(Style::default().fg(context.theme.muted))
                .render(
                    Rect::new(
                        layout.body.x,
                        results_area.y,
                        layout.body.width,
                        results_area.height,
                    ),
                    buffer,
                );
        } else {
            results.render_borderless(results_area, buffer, &theme);
        }
    }

    pub fn new(id: impl Into<ComponentId>, commands: Vec<CommandPaletteCommand>) -> Self {
        let id = id.into();
        let mut query_input = TextInput::new(format!("{}.query", id.as_str()))
            .with_placeholder(i18n::tr!("ui-components-type-a-command"));
        query_input.set_focused(false);
        let mut results = Self::make_results(&id, &commands, "");
        results.set_focused(false);
        Self {
            close_button: Button::new(format!("{}.close", id.as_str()), Key::Escape.label())
                .with_bracketed_label(false),
            id,
            commands,
            state: ComponentState::default(),
            open: false,
            query_input,
            results,
            pressed: None,
        }
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
        self.query_input.set_focused(open);
        self.results.set_focused(open);
        if !open {
            self.results.set_hovered(None);
            self.close_button.set_focused(false);
            self.close_button.state.hovered = false;
            self.pressed = None;
        } else {
            self.sync_results(false);
        }
    }

    pub fn query(&self) -> &str {
        self.query_input.value()
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query_input.set_value(query);
        self.sync_results(true);
    }

    pub fn set_selected(&mut self, index: Option<usize>) {
        self.sync_results(false);
        self.results.set_selected(index);
    }

    pub fn selected_command(&self) -> Option<&CommandPaletteCommand> {
        self.results
            .selected_index()
            .and_then(|visible| self.visible_indices().get(visible).copied())
            .and_then(|index| self.commands.get(index))
    }

    pub fn visible_commands(&self) -> Vec<&CommandPaletteCommand> {
        self.visible_indices()
            .into_iter()
            .filter_map(|index| self.commands.get(index))
            .collect()
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        if !self.open {
            return ComponentEvent::None;
        }

        self.sync_results(false);
        let layout = self.layout(area);
        if !matches!(event, InputEvent::Mouse(_)) {
            self.pressed = None;
            self.close_button.set_focused(false);
        }
        match event {
            InputEvent::Key(key) if !key.is_press_like() => ComponentEvent::None,
            InputEvent::FocusGained => {
                self.state.focused = true;
                self.query_input.set_focused(true);
                self.results.set_focused(true);
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
            InputEvent::Key(key) if key.key == Key::Enter => self.activate_selected(),
            InputEvent::Key(key)
                if matches!(key.key, Key::Up | Key::Down | Key::Tab | Key::BackTab)
                    || key.modifiers.is_control_only()
                        && matches!(key.key, Key::Home | Key::End) =>
            {
                let list_key = match key.key {
                    Key::Tab => Key::Down,
                    Key::BackTab => Key::Up,
                    other => other,
                };
                let event = self.results.handle_event_borderless(
                    InputEvent::key_with_modifiers(list_key, key.modifiers),
                    Self::results_area(layout),
                );
                self.translate_results_event(event)
            }
            event @ (InputEvent::Key(_) | InputEvent::Paste(_)) => {
                let event = self
                    .query_input
                    .handle_event_borderless(event, Self::query_edit_area(layout));
                if matches!(event, ComponentEvent::Changed(_)) {
                    self.sync_results(true);
                    ComponentEvent::Changed(self.id.clone())
                } else {
                    ComponentEvent::Consumed
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

                if matches!(mouse.kind, MouseKind::Down(_)) {
                    self.close_button.set_focused(false);
                } else if mouse.kind == MouseKind::Move
                    && !contains_point(layout.close, point.0, point.1)
                {
                    self.close_button.state.active = false;
                }
                if matches!(
                    self.close_button
                        .handle_event(InputEvent::Mouse(mouse), layout.close),
                    ComponentEvent::Activated(_)
                ) {
                    self.close();
                    return ComponentEvent::Dismissed(self.id.clone());
                }
                let results_area = Self::results_area(layout);
                let row = self
                    .results
                    .borderless_item_index_at(results_area, point.0, point.1)
                    .and_then(|index| self.results.items.get(index))
                    .filter(|item| !item.disabled)
                    .map(|item| {
                        (
                            item.id.clone(),
                            Rect::new(results_area.x, point.1, results_area.width, 1),
                        )
                    });
                match mouse.kind {
                    MouseKind::Down(MouseButton::Left) => {
                        self.pressed = row;
                        if self.pressed.is_some() {
                            return ComponentEvent::Changed(self.id.clone());
                        }
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
                    }
                    _ => self.pressed = None,
                }
                let input_event = self.query_input.handle_event_borderless(
                    InputEvent::Mouse(mouse),
                    Self::query_edit_area(layout),
                );
                let results_event = self
                    .results
                    .handle_event_borderless(InputEvent::Mouse(mouse), results_area);
                if mouse.kind == MouseKind::Click(MouseButton::Left)
                    && matches!(results_event, ComponentEvent::Selected(_, _))
                {
                    return self.activate_selected();
                }
                if let ComponentEvent::Activated(id) = results_event {
                    self.close();
                    return ComponentEvent::Activated(id);
                }
                if matches!(input_event, ComponentEvent::Changed(_)) {
                    self.sync_results(true);
                    return ComponentEvent::Changed(self.id.clone());
                }
                if matches!(
                    input_event,
                    ComponentEvent::Changed(_) | ComponentEvent::FocusRequested(_)
                ) || matches!(results_event, ComponentEvent::Changed(_))
                {
                    ComponentEvent::Changed(self.id.clone())
                } else {
                    self.translate_results_event(results_event)
                }
            }
            _ => ComponentEvent::Consumed,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.render_with_context(area, buffer, &DialogLayout::surface_theme(theme));
    }

    /// Renders the command palette through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        self.render(area, frame.buffer_mut(), theme);
    }

    fn visible_indices(&self) -> Vec<usize> {
        Self::visible_indices_for(self.query_input.value(), &self.commands)
    }

    fn visible_indices_for(query: &str, commands: &[CommandPaletteCommand]) -> Vec<usize> {
        if query.is_empty() {
            return (0..commands.len()).collect();
        }

        let query = query.to_lowercase();
        commands
            .iter()
            .enumerate()
            .filter_map(|(index, command)| {
                let title_matches = command.title.to_lowercase().contains(&query);
                let hint_matches = command
                    .hint
                    .as_ref()
                    .is_some_and(|hint| hint.to_lowercase().contains(&query));
                let keyword_matches = command
                    .keywords
                    .iter()
                    .any(|keyword| keyword.to_lowercase().contains(&query));
                (title_matches || hint_matches || keyword_matches).then_some(index)
            })
            .collect()
    }

    fn activate_selected(&mut self) -> ComponentEvent {
        let Some(command) = self.selected_command() else {
            return ComponentEvent::Consumed;
        };
        if command.disabled {
            return ComponentEvent::Consumed;
        }

        let id = command.id.clone();
        self.close();
        ComponentEvent::Activated(id)
    }

    fn translate_results_event(&mut self, event: ComponentEvent) -> ComponentEvent {
        match event {
            ComponentEvent::Selected(_, index) => ComponentEvent::Selected(self.id.clone(), index),
            ComponentEvent::Changed(_) => ComponentEvent::Changed(self.id.clone()),
            ComponentEvent::Activated(id) => {
                self.close();
                ComponentEvent::Activated(id)
            }
            ComponentEvent::None | ComponentEvent::FocusRequested(_) => ComponentEvent::Consumed,
            other => other,
        }
    }

    fn sync_results(&mut self, reset_selection: bool) {
        let selected_id = (!reset_selection)
            .then(|| self.results.selected_item().map(|item| item.id.clone()))
            .flatten();
        let hovered = self.results.hovered_index();
        let mut results = Self::make_results(&self.id, &self.commands, self.query_input.value());
        if let Some(selected_id) = selected_id {
            let selected = results
                .items
                .iter()
                .position(|item| item.id == selected_id && !item.disabled);
            results.set_selected(selected);
        }
        if !reset_selection {
            results.set_hovered(hovered);
        }
        results.state = self.state;
        results.set_focused(self.open && self.state.focused);
        self.results = results;
    }

    fn results_for_render(&self) -> List {
        let mut results = Self::make_results(&self.id, &self.commands, self.query_input.value());
        if let Some(selected) = self.results.selected_item() {
            let selected_index = results
                .items
                .iter()
                .position(|item| item.id == selected.id && !item.disabled);
            results.set_selected(selected_index);
        }
        results.set_hovered(self.results.hovered_index());
        results.state = self.state;
        results
    }

    fn make_results(id: &ComponentId, commands: &[CommandPaletteCommand], query: &str) -> List {
        let items = Self::visible_indices_for(query, commands)
            .into_iter()
            .filter_map(|index| commands.get(index))
            .map(|command| {
                let item = ListItem::new(command.id.clone(), command.title.clone())
                    .disabled(command.disabled);
                match &command.hint {
                    Some(hint) => item.with_description(hint.clone()),
                    None => item,
                }
            })
            .collect();
        List::new(format!("{}.results", id.as_str()), items).with_highlight_symbol(Some("▌   "))
    }

    fn layout(&self, area: Rect) -> DialogLayout {
        DialogLayout::with_min_body(area, 0, Some(&self.close_button.label), 2)
    }

    fn query_area(layout: DialogLayout) -> Rect {
        let body = layout.body;
        Rect::new(body.x, body.y, body.width, body.height.min(1))
    }

    fn query_edit_area(layout: DialogLayout) -> Rect {
        let query = Self::query_area(layout);
        Rect::new(
            query.x.saturating_add(2),
            query.y,
            query.width.saturating_sub(2),
            query.height,
        )
    }

    fn results_area(layout: DialogLayout) -> Rect {
        let body = layout.body;
        let offset = body.height.min(1) + u16::from(body.height >= 3);
        Rect::new(
            layout.area.x,
            body.y + offset,
            layout.area.width,
            body.height - offset,
        )
    }
}
