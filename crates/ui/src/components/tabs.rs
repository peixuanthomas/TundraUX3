use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Borders, Tabs as RatatuiTabs, Widget};

use crate::TundraTheme;

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    contains_point, inner_area, item_style,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabItem {
    pub id: ComponentId,
    pub label: String,
    pub disabled: bool,
}

impl TabItem {
    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tabs {
    pub id: ComponentId,
    pub tabs: Vec<TabItem>,
    pub state: ComponentState,
    selected: Option<usize>,
    hovered: Option<usize>,
}

impl Tabs {
    pub fn new(id: impl Into<ComponentId>, tabs: Vec<TabItem>) -> Self {
        let selected = tabs.iter().position(|tab| !tab.disabled);
        Self {
            id: id.into(),
            tabs,
            state: ComponentState::default(),
            selected,
            hovered: None,
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub fn selected_tab(&self) -> Option<&TabItem> {
        self.selected.and_then(|index| self.tabs.get(index))
    }

    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index
            .filter(|index| self.tabs.get(*index).is_some_and(|tab| !tab.disabled))
            .or_else(|| self.tabs.iter().position(|tab| !tab.disabled));
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.state.focused = focused;
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        self.handle_event_in(event, area, true)
    }

    /// Handles input for tabs rendered without a surrounding block.
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
                Key::Left | Key::Up => self.select_previous(),
                Key::Right | Key::Down => self.select_next(),
                Key::Home => self.select_first(),
                Key::End => self.select_last(),
                Key::Enter | Key::Space => self
                    .selected_tab()
                    .map(|tab| ComponentEvent::Activated(tab.id.clone()))
                    .unwrap_or(ComponentEvent::Consumed),
                _ => ComponentEvent::None,
            },
            InputEvent::Mouse(mouse) => {
                let index = self.tab_index_at(area, mouse.column(), mouse.row(), bordered);
                match mouse.kind {
                    MouseKind::Move => {
                        if self.hovered != index {
                            self.hovered = index;
                            ComponentEvent::Changed(self.id.clone())
                        } else {
                            ComponentEvent::None
                        }
                    }
                    MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left) => {
                        self.select_from_pointer(index)
                    }
                    _ => ComponentEvent::None,
                }
            }
            _ => ComponentEvent::None,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.ratatui_widget(theme, true).render(area, buffer);
    }

    /// Renders bordered tabs through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        frame.render_widget(self.ratatui_widget(theme, true), area);
    }

    /// Renders tabs using Ratatui's official `Tabs` widget without a surrounding block.
    pub fn render_borderless(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.ratatui_widget(theme, false).render(area, buffer);
    }

    /// Renders borderless tabs through a Ratatui [`Frame`].
    pub fn render_borderless_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        frame.render_widget(self.ratatui_widget(theme, false), area);
    }

    fn select_from_pointer(&mut self, index: Option<usize>) -> ComponentEvent {
        let Some(index) = index else {
            return ComponentEvent::None;
        };
        if self.tabs.get(index).is_none_or(|tab| tab.disabled) {
            return ComponentEvent::Consumed;
        }

        self.state.focused = true;
        self.selected = Some(index);
        ComponentEvent::Selected(self.id.clone(), index)
    }

    fn select_first(&mut self) -> ComponentEvent {
        self.select_index(self.tabs.iter().position(|tab| !tab.disabled))
    }

    fn select_last(&mut self) -> ComponentEvent {
        self.select_index(self.tabs.iter().rposition(|tab| !tab.disabled))
    }

    fn select_previous(&mut self) -> ComponentEvent {
        let selected = self.selected.unwrap_or(self.tabs.len());
        let next = self.tabs[..selected].iter().rposition(|tab| !tab.disabled);
        self.select_index(next)
    }

    fn select_next(&mut self) -> ComponentEvent {
        let start = self
            .selected
            .map(|index| index.saturating_add(1))
            .unwrap_or(0);
        let next = self.tabs[start..]
            .iter()
            .position(|tab| !tab.disabled)
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

    fn tab_index_at(&self, area: Rect, column: u16, row: u16, bordered: bool) -> Option<usize> {
        let inner = if bordered { inner_area(area) } else { area };
        if !contains_point(inner, column, row) || row != inner.y {
            return None;
        }

        let mut tab_x = inner.x;
        for (index, tab) in self.tabs.iter().enumerate() {
            let width = u16::try_from(Line::from(tab.label.as_str()).width())
                .unwrap_or(u16::MAX)
                .saturating_add(2);
            if column >= tab_x && column < tab_x.saturating_add(width) {
                return Some(index);
            }
            tab_x = tab_x.saturating_add(width);
        }
        None
    }

    fn ratatui_widget<'a>(&'a self, theme: &TundraTheme, bordered: bool) -> RatatuiTabs<'a> {
        let titles = self.tabs.iter().enumerate().map(|(index, tab)| {
            Line::styled(
                tab.label.as_str(),
                item_style(
                    self.state.focused,
                    self.hovered == Some(index),
                    self.selected == Some(index),
                    tab.disabled,
                    theme,
                ),
            )
        });
        let widget = RatatuiTabs::new(titles)
            .select(self.selected)
            .style(theme.body_style())
            .highlight_style(item_style(self.state.focused, false, true, false, theme))
            .divider("")
            .padding(" ", " ");

        if bordered {
            widget.block(
                theme
                    .block()
                    .borders(Borders::ALL)
                    .style(theme.body_style()),
            )
        } else {
            widget
        }
    }
}
