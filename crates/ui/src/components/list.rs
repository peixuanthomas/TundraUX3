use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::{
    Borders, HighlightSpacing, List as RatatuiList, ListItem as RatatuiListItem,
    ListState as RatatuiListState, StatefulWidget,
};

use crate::TundraTheme;

use super::{
    ComponentEvent, ComponentId, ComponentState, ComponentTone, InputEvent, Key, MouseButton,
    MouseKind, contains_point, inner_area, item_style, tone_color,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub id: ComponentId,
    pub label: String,
    pub description: Option<String>,
    pub disabled: bool,
    pub tone: ComponentTone,
}

impl ListItem {
    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            disabled: false,
            tone: ComponentTone::Default,
        }
    }
    pub const fn tone(mut self, tone: ComponentTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    pub id: ComponentId,
    pub title: Option<String>,
    pub items: Vec<ListItem>,
    pub state: ComponentState,
    selected: Option<usize>,
    hovered: Option<usize>,
    viewport_start: Option<usize>,
    highlight_symbol: Option<String>,
}

impl List {
    /// Automatic viewport origin that keeps an absolute selection visible.
    pub const fn automatic_viewport_start(selected: usize, visible_height: usize) -> usize {
        if visible_height == 0 {
            0
        } else {
            selected.saturating_add(1).saturating_sub(visible_height)
        }
    }
    pub fn render_with_context(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        context: &crate::RenderContext,
    ) {
        self.render(area, buffer, &context.compatibility_theme());
    }

    pub fn render_borderless_with_context(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        context: &crate::RenderContext,
    ) {
        self.render_borderless(area, buffer, &context.compatibility_theme());
    }

    pub fn new(id: impl Into<ComponentId>, items: Vec<ListItem>) -> Self {
        let selected = items.iter().position(|item| !item.disabled);
        Self {
            id: id.into(),
            title: None,
            items,
            state: ComponentState::default(),
            selected,
            hovered: None,
            viewport_start: None,
            highlight_symbol: Some("> ".to_string()),
        }
    }
    pub const fn with_viewport_start(mut self, start: usize) -> Self {
        self.viewport_start = Some(start);
        self
    }
    pub fn with_highlight_symbol(mut self, symbol: Option<impl Into<String>>) -> Self {
        self.highlight_symbol = symbol.map(Into::into);
        self
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub fn selected_item(&self) -> Option<&ListItem> {
        self.selected.and_then(|index| self.items.get(index))
    }

    pub fn hovered_index(&self) -> Option<usize> {
        self.hovered
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.state.focused = focused;
    }

    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index
            .filter(|index| self.items.get(*index).is_some())
            .or_else(|| self.items.iter().position(|item| !item.disabled));
    }

    pub(crate) fn set_hovered(&mut self, index: Option<usize>) {
        self.hovered = index.filter(|index| self.items.get(*index).is_some());
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        self.handle_event_in(event, area, true)
    }

    /// Handles input for a list rendered without a surrounding block.
    pub fn handle_event_borderless(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        self.handle_event_in(event, area, false)
    }

    fn handle_event_in(&mut self, event: InputEvent, area: Rect, bordered: bool) -> ComponentEvent {
        match event {
            InputEvent::Key(key) if !key.is_press_like() => ComponentEvent::None,
            InputEvent::FocusGained => {
                self.set_focused(true);
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.set_focused(false);
                self.hovered = None;
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) if self.state.focused => match key.key {
                Key::Up => self.select_previous(),
                Key::Down => self.select_next(),
                Key::Home => self.select_first(),
                Key::End => self.select_last(),
                Key::Enter | Key::Space => self
                    .selected_item()
                    .map(|item| ComponentEvent::Activated(item.id.clone()))
                    .unwrap_or(ComponentEvent::Consumed),
                _ => ComponentEvent::None,
            },
            InputEvent::Mouse(mouse) => {
                let index = self.item_index_at(area, mouse.column(), mouse.row(), bordered);
                match mouse.kind {
                    MouseKind::Move => {
                        if self.hovered != index {
                            self.hovered = index;
                            ComponentEvent::Changed(self.id.clone())
                        } else {
                            ComponentEvent::None
                        }
                    }
                    MouseKind::ScrollUp if contains_point(area, mouse.column(), mouse.row()) => {
                        self.select_previous()
                    }
                    MouseKind::ScrollDown if contains_point(area, mouse.column(), mouse.row()) => {
                        self.select_next()
                    }
                    MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left) => {
                        self.select_from_pointer(index)
                    }
                    MouseKind::DoubleClick(MouseButton::Left) => {
                        self.select_from_pointer(index);
                        self.selected_item()
                            .map(|item| ComponentEvent::Activated(item.id.clone()))
                            .unwrap_or(ComponentEvent::None)
                    }
                    _ => ComponentEvent::None,
                }
            }
            _ => ComponentEvent::None,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        let mut state = self.ratatui_state(area, true);
        StatefulWidget::render(self.ratatui_widget(theme, true), area, buffer, &mut state);
    }

    /// Renders the bordered list through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        let mut state = self.ratatui_state(area, true);
        frame.render_stateful_widget(self.ratatui_widget(theme, true), area, &mut state);
    }

    /// Renders the list with Ratatui's official `List` widget and no surrounding block.
    pub fn render_borderless(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        let mut state = self.ratatui_state(area, false);
        StatefulWidget::render(self.ratatui_widget(theme, false), area, buffer, &mut state);
    }

    /// Renders a borderless list through a Ratatui [`Frame`].
    pub fn render_borderless_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        let mut state = self.ratatui_state(area, false);
        frame.render_stateful_widget(self.ratatui_widget(theme, false), area, &mut state);
    }

    fn select_from_pointer(&mut self, index: Option<usize>) -> ComponentEvent {
        let Some(index) = index else {
            return ComponentEvent::None;
        };
        if self.items.get(index).is_none_or(|item| item.disabled) {
            return ComponentEvent::Consumed;
        }

        self.state.focused = true;
        self.hovered = Some(index);
        self.selected = Some(index);
        ComponentEvent::Selected(self.id.clone(), index)
    }

    fn select_first(&mut self) -> ComponentEvent {
        self.select_index(self.items.iter().position(|item| !item.disabled))
    }

    fn select_last(&mut self) -> ComponentEvent {
        self.select_index(self.items.iter().rposition(|item| !item.disabled))
    }

    fn select_previous(&mut self) -> ComponentEvent {
        let selected = self.selected.unwrap_or(self.items.len());
        let next = self.items[..selected]
            .iter()
            .rposition(|item| !item.disabled);
        self.select_index(next)
    }

    fn select_next(&mut self) -> ComponentEvent {
        let start = self
            .selected
            .map(|index| index.saturating_add(1))
            .unwrap_or(0);
        let next = self.items[start..]
            .iter()
            .position(|item| !item.disabled)
            .map(|index| index + start);
        self.select_index(next)
    }

    fn select_index(&mut self, index: Option<usize>) -> ComponentEvent {
        let Some(index) = index else {
            return ComponentEvent::Consumed;
        };
        if self.selected == Some(index) {
            ComponentEvent::Consumed
        } else {
            self.selected = Some(index);
            ComponentEvent::Selected(self.id.clone(), index)
        }
    }

    fn item_index_at(&self, area: Rect, column: u16, row: u16, bordered: bool) -> Option<usize> {
        let inner = if bordered { inner_area(area) } else { area };
        if !contains_point(inner, column, row) {
            return None;
        }

        let index = self
            .viewport_offset(area, bordered)
            .saturating_add(row.saturating_sub(inner.y) as usize);
        self.items.get(index).map(|_| index)
    }

    fn ratatui_widget<'a>(&'a self, theme: &TundraTheme, bordered: bool) -> RatatuiList<'a> {
        let start = self.viewport_start.unwrap_or(0);
        let items = self
            .items
            .iter()
            .enumerate()
            .skip(start)
            .map(|(index, item)| {
                let label = match &item.description {
                    Some(description) => format!("{} - {}", item.label, description),
                    None => item.label.clone(),
                };
                let mut style = item_style(
                    self.state.focused,
                    self.hovered == Some(index),
                    false,
                    item.disabled,
                    theme,
                );
                if item.disabled {
                    style = style.add_modifier(Modifier::DIM);
                }
                if self.selected != Some(index) {
                    style = style.fg(tone_color(item.tone, theme));
                }
                RatatuiListItem::new(label).style(style)
            });
        let selected_disabled = self
            .selected
            .and_then(|index| self.items.get(index))
            .is_some_and(|item| item.disabled);
        let widget = RatatuiList::new(items)
            .style(theme.body_style())
            .highlight_symbol(self.highlight_symbol.as_deref().unwrap_or(""))
            .highlight_spacing(if self.highlight_symbol.is_some() {
                HighlightSpacing::Always
            } else {
                HighlightSpacing::Never
            })
            .highlight_style(item_style(
                self.state.focused,
                false,
                true,
                selected_disabled,
                theme,
            ));

        if !bordered {
            return widget;
        }

        let block = match self.title.as_deref() {
            Some(title) => theme
                .block()
                .title(title)
                .title_style(if self.state.focused {
                    theme.title_style()
                } else {
                    theme.body_style()
                })
                .borders(Borders::ALL)
                .border_style(theme.selectable_border_style(self.state.focused))
                .style(theme.body_style()),
            None => theme
                .block()
                .borders(Borders::ALL)
                .border_style(theme.selectable_border_style(self.state.focused))
                .style(theme.body_style()),
        };
        widget.block(block)
    }

    fn ratatui_state(&self, area: Rect, bordered: bool) -> RatatuiListState {
        if let Some(start) = self.viewport_start {
            return RatatuiListState::default()
                .with_selected(
                    self.selected
                        .and_then(|selected| selected.checked_sub(start)),
                )
                .with_offset(0);
        }
        RatatuiListState::default()
            .with_selected(self.selected)
            .with_offset(self.viewport_offset(area, bordered))
    }

    fn viewport_offset(&self, area: Rect, bordered: bool) -> usize {
        let height = if bordered {
            inner_area(area).height as usize
        } else {
            area.height as usize
        };
        if height == 0 {
            return 0;
        }

        self.viewport_start.unwrap_or_else(|| {
            self.selected
                .map(|selected| Self::automatic_viewport_start(selected, height))
                .unwrap_or(0)
        })
    }
}

#[cfg(test)]
mod viewport_tests {
    use super::List;
    #[test]
    fn automatic_viewport_boundaries() {
        assert_eq!(List::automatic_viewport_start(5, 4), 2);
        assert_eq!(List::automatic_viewport_start(5, 0), 0);
        assert_eq!(List::automatic_viewport_start(3, 4), 0);
    }
}
