use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Borders, Clear, Paragraph, Widget};

use crate::TundraTheme;

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    byte_index_for_char, char_count, contains_point, inner_area, item_style,
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
    query: String,
    cursor: usize,
    selected_visible: Option<usize>,
    hovered_visible: Option<usize>,
}

impl CommandPalette {
    pub fn new(id: impl Into<ComponentId>, commands: Vec<CommandPaletteCommand>) -> Self {
        let selected_visible = commands.iter().position(|command| !command.disabled);
        Self {
            id: id.into(),
            commands,
            state: ComponentState::default(),
            open: false,
            query: String::new(),
            cursor: 0,
            selected_visible,
            hovered_visible: None,
        }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.state.focused = true;
        self.ensure_selection();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.state.focused = false;
        self.hovered_visible = None;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.cursor = char_count(&self.query);
        self.selected_visible = self.first_enabled_visible();
    }

    pub fn selected_command(&self) -> Option<&CommandPaletteCommand> {
        self.selected_visible
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

        match event {
            InputEvent::FocusGained => {
                self.state.focused = true;
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.close();
                ComponentEvent::Dismissed(self.id.clone())
            }
            InputEvent::Key(key) => match key.key {
                Key::Escape => {
                    self.close();
                    ComponentEvent::Dismissed(self.id.clone())
                }
                Key::Char(character) => {
                    self.insert_char(character);
                    self.selected_visible = self.first_enabled_visible();
                    ComponentEvent::Changed(self.id.clone())
                }
                Key::Space => {
                    self.insert_char(' ');
                    self.selected_visible = self.first_enabled_visible();
                    ComponentEvent::Changed(self.id.clone())
                }
                Key::Backspace => {
                    if self.delete_before_cursor() {
                        self.selected_visible = self.first_enabled_visible();
                        ComponentEvent::Changed(self.id.clone())
                    } else {
                        ComponentEvent::Consumed
                    }
                }
                Key::Delete => {
                    if self.delete_at_cursor() {
                        self.selected_visible = self.first_enabled_visible();
                        ComponentEvent::Changed(self.id.clone())
                    } else {
                        ComponentEvent::Consumed
                    }
                }
                Key::Left => {
                    self.cursor = self.cursor.saturating_sub(1);
                    ComponentEvent::Consumed
                }
                Key::Right => {
                    self.cursor = self.cursor.saturating_add(1).min(char_count(&self.query));
                    ComponentEvent::Consumed
                }
                Key::Home => {
                    self.cursor = 0;
                    ComponentEvent::Consumed
                }
                Key::End => {
                    self.cursor = char_count(&self.query);
                    ComponentEvent::Consumed
                }
                Key::Up => self.select_previous(),
                Key::Down | Key::Tab => self.select_next(),
                Key::BackTab => self.select_previous(),
                Key::Enter => self.activate_selected(),
            },
            InputEvent::Mouse(mouse) => {
                let inside = contains_point(area, mouse.column, mouse.row);
                if !inside
                    && matches!(
                        mouse.kind,
                        MouseKind::Down(_) | MouseKind::Click(_) | MouseKind::DoubleClick(_)
                    )
                {
                    self.close();
                    return ComponentEvent::Dismissed(self.id.clone());
                }

                let index = self.command_index_at(area, mouse.column, mouse.row);
                match mouse.kind {
                    MouseKind::Move => {
                        if self.hovered_visible != index {
                            self.hovered_visible = index;
                            ComponentEvent::Changed(self.id.clone())
                        } else {
                            ComponentEvent::Consumed
                        }
                    }
                    MouseKind::Down(MouseButton::Left)
                    | MouseKind::Click(MouseButton::Left)
                    | MouseKind::DoubleClick(MouseButton::Left) => self.activate_pointer(index),
                    _ => ComponentEvent::Consumed,
                }
            }
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        if !self.open {
            return;
        }

        Clear.render(area, buffer);
        let block = theme
            .block()
            .title("Command Palette")
            .borders(Borders::ALL)
            .style(theme.body_style());
        let inner = block.inner(area);
        block.render(area, buffer);

        if inner.height == 0 {
            return;
        }

        Paragraph::new(format!("> {}", self.query_with_cursor()))
            .style(theme.title_style())
            .render(Rect::new(inner.x, inner.y, inner.width, 1), buffer);

        let list_y = inner.y.saturating_add(2);
        let visible = self.visible_indices();
        for (visible_index, command_index) in visible
            .iter()
            .copied()
            .take(inner.height.saturating_sub(2) as usize)
            .enumerate()
        {
            let Some(command) = self.commands.get(command_index) else {
                continue;
            };
            let style = item_style(
                self.state.focused,
                self.hovered_visible == Some(visible_index),
                self.selected_visible == Some(visible_index),
                command.disabled,
                theme,
            );
            let label = match &command.hint {
                Some(hint) => format!("{} - {}", command.title, hint),
                None => command.title.clone(),
            };
            Paragraph::new(label).style(style).render(
                Rect::new(
                    inner.x,
                    list_y.saturating_add(visible_index as u16),
                    inner.width,
                    1,
                ),
                buffer,
            );
        }
    }

    fn visible_indices(&self) -> Vec<usize> {
        if self.query.is_empty() {
            return (0..self.commands.len()).collect();
        }

        let query = self.query.to_lowercase();
        self.commands
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

    fn first_enabled_visible(&self) -> Option<usize> {
        self.visible_indices()
            .iter()
            .enumerate()
            .find_map(|(visible_index, command_index)| {
                self.commands
                    .get(*command_index)
                    .is_some_and(|command| !command.disabled)
                    .then_some(visible_index)
            })
    }

    fn ensure_selection(&mut self) {
        let visible = self.visible_indices();
        if self
            .selected_visible
            .and_then(|index| visible.get(index))
            .and_then(|index| self.commands.get(*index))
            .is_some_and(|command| !command.disabled)
        {
            return;
        }
        self.selected_visible = self.first_enabled_visible();
    }

    fn activate_pointer(&mut self, index: Option<usize>) -> ComponentEvent {
        let Some(index) = index else {
            return ComponentEvent::Consumed;
        };
        self.selected_visible = Some(index);
        self.activate_selected()
    }

    fn activate_selected(&mut self) -> ComponentEvent {
        self.ensure_selection();
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

    fn select_previous(&mut self) -> ComponentEvent {
        let visible = self.visible_indices();
        let selected = self.selected_visible.unwrap_or(visible.len());
        let next = visible[..selected].iter().enumerate().rev().find_map(
            |(visible_index, command_index)| {
                self.commands
                    .get(*command_index)
                    .is_some_and(|command| !command.disabled)
                    .then_some(visible_index)
            },
        );
        self.select_visible(next)
    }

    fn select_next(&mut self) -> ComponentEvent {
        let visible = self.visible_indices();
        let start = self
            .selected_visible
            .map(|index| index.saturating_add(1))
            .unwrap_or(0);
        let next = visible[start..]
            .iter()
            .enumerate()
            .find_map(|(offset, command_index)| {
                self.commands
                    .get(*command_index)
                    .is_some_and(|command| !command.disabled)
                    .then_some(start + offset)
            });
        self.select_visible(next)
    }

    fn select_visible(&mut self, index: Option<usize>) -> ComponentEvent {
        let Some(index) = index else {
            return ComponentEvent::Consumed;
        };
        self.selected_visible = Some(index);
        ComponentEvent::Selected(self.id.clone(), index)
    }

    fn command_index_at(&self, area: Rect, column: u16, row: u16) -> Option<usize> {
        let inner = inner_area(area);
        let list_y = inner.y.saturating_add(2);
        if column < inner.x
            || column >= inner.x.saturating_add(inner.width)
            || row < list_y
            || row >= inner.y.saturating_add(inner.height)
        {
            return None;
        }

        let visible_index = row.saturating_sub(list_y) as usize;
        self.visible_indices()
            .get(visible_index)
            .map(|_| visible_index)
    }

    fn query_with_cursor(&self) -> String {
        let mut query = self.query.clone();
        let index = byte_index_for_char(&query, self.cursor);
        query.insert(index, '|');
        query
    }

    fn insert_char(&mut self, character: char) {
        let byte_index = byte_index_for_char(&self.query, self.cursor);
        self.query.insert(byte_index, character);
        self.cursor = self.cursor.saturating_add(1);
    }

    fn delete_before_cursor(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }

        let remove_at = byte_index_for_char(&self.query, self.cursor.saturating_sub(1));
        self.query.remove(remove_at);
        self.cursor = self.cursor.saturating_sub(1);
        true
    }

    fn delete_at_cursor(&mut self) -> bool {
        if self.cursor >= char_count(&self.query) {
            return false;
        }

        let remove_at = byte_index_for_char(&self.query, self.cursor);
        self.query.remove(remove_at);
        true
    }
}
