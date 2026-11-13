use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Borders, Widget};

use crate::TundraTheme;

use super::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, MouseButton, MouseKind,
    contains_point,
    foundation::{row_marker, row_style},
    inner_area, terminal_width, truncate_to_terminal_width,
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
    pub fn render_with_context(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        context: &crate::RenderContext,
    ) {
        self.render(area, buffer, &context.compatibility_theme());
    }

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

    /// Returns the clipped cell geometry used by borderless tab rendering and input.
    pub fn borderless_item_areas(&self, area: Rect) -> Vec<Rect> {
        self.item_areas(area, false)
    }

    /// Returns the tab index at a terminal cell using borderless render geometry.
    pub fn borderless_index_at(&self, area: Rect, column: u16, row: u16) -> Option<usize> {
        self.index_at(area, column, row, false)
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
            InputEvent::Key(key) if self.state.focused => {
                let result = match key.key {
                    Key::Left | Key::Up => self.select_previous(),
                    Key::Right | Key::Down => self.select_next(),
                    Key::Home => self.select_first(),
                    Key::End => self.select_last(),
                    Key::Enter | Key::Space => self
                        .selected_tab()
                        .map(|tab| ComponentEvent::Activated(tab.id.clone()))
                        .unwrap_or(ComponentEvent::Consumed),
                    _ => ComponentEvent::None,
                };
                if result.is_consumed() {
                    self.hovered = None;
                }
                result
            }
            InputEvent::Mouse(mouse) => {
                let index = self.index_at(area, mouse.column(), mouse.row(), bordered);
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
        self.render_in(area, buffer, theme, true);
    }

    /// Renders bordered tabs through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        self.render(area, frame.buffer_mut(), theme);
    }

    /// Renders tabs without a surrounding block.
    pub fn render_borderless(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.render_in(area, buffer, theme, false);
    }

    /// Renders borderless tabs through a Ratatui [`Frame`].
    pub fn render_borderless_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        self.render_borderless(area, frame.buffer_mut(), theme);
    }

    fn select_from_pointer(&mut self, index: Option<usize>) -> ComponentEvent {
        let Some(index) = index else {
            return ComponentEvent::None;
        };
        if self.tabs.get(index).is_none_or(|tab| tab.disabled) {
            return ComponentEvent::Consumed;
        }

        self.state.focused = true;
        self.hovered = None;
        self.selected = Some(index);
        ComponentEvent::Selected(self.id.clone(), index)
    }

    fn capture(&self, area: Rect, bordered: bool) {
        if !crate::automation::enabled() {
            return;
        }
        let _scope = crate::automation::scope(self.id.as_str(), "tablist", "", area);
        for (index, (item, rect)) in self
            .tabs
            .iter()
            .zip(self.item_areas(area, bordered))
            .enumerate()
        {
            let mut state = self.state;
            state.selected = self.selected == Some(index);
            state.focused &= state.selected;
            state.disabled |= item.disabled;
            crate::automation::control(
                item.id.as_str(),
                "tab",
                &item.label,
                rect,
                state,
                &["click"],
            );
        }
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

    fn item_areas(&self, area: Rect, bordered: bool) -> Vec<Rect> {
        let inner = if bordered { inner_area(area) } else { area };
        let mut tab_x = inner.x;
        self.tabs
            .iter()
            .map(|tab| {
                let width = u16::try_from(terminal_width(&tab.label))
                    .unwrap_or(u16::MAX)
                    .saturating_add(2)
                    .min(inner.right().saturating_sub(tab_x));
                let item = Rect::new(tab_x, inner.y, width, u16::from(inner.height > 0));
                tab_x = tab_x.saturating_add(width);
                item
            })
            .collect()
    }

    fn index_at(&self, area: Rect, column: u16, row: u16, bordered: bool) -> Option<usize> {
        self.item_areas(area, bordered)
            .iter()
            .position(|item| contains_point(*item, column, row))
    }

    fn render_in(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme, bordered: bool) {
        self.capture(area, bordered);
        let mut block = theme.block().style(theme.body_style());
        if bordered {
            block = block.borders(Borders::ALL);
        }
        block.render(area, buffer);
        for (index, (tab, rect)) in self
            .tabs
            .iter()
            .zip(self.item_areas(area, bordered))
            .enumerate()
        {
            if rect.is_empty() {
                continue;
            }
            let selected = self.selected == Some(index);
            let state = ComponentState {
                focused: self.state.focused && selected,
                hovered: self.hovered == Some(index),
                selected,
                disabled: self.state.disabled || tab.disabled,
                ..Default::default()
            };
            let style = row_style(state, theme, theme.tokens().raised);
            let label =
                truncate_to_terminal_width(&tab.label, usize::from(rect.width.saturating_sub(2)));
            buffer.set_style(rect, style.remove_modifier(Modifier::UNDERLINED));
            buffer.set_line(
                rect.x,
                rect.y,
                &Line::from(vec![row_marker(state, theme), Span::styled(label, style)]),
                rect.width,
            );
        }
    }
}
