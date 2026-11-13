use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    widgets::Paragraph,
};

use super::{Button, Surface, foundation::row_marker, terminal_width, truncate_to_terminal_width};
use crate::RenderContext;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceFocus {
    Navigation,
    Search,
    #[default]
    List,
    Details,
    Actions,
    Advanced,
}

impl WorkspaceFocus {
    pub fn step(self, backwards: bool) -> Self {
        let order = [
            Self::Navigation,
            Self::Search,
            Self::List,
            Self::Details,
            Self::Actions,
            Self::Advanced,
        ];
        let index = order.iter().position(|focus| *focus == self).unwrap();
        order[(index + if backwards { order.len() - 1 } else { 1 }) % order.len()]
    }
}

/// Navigation and content rectangles shared by painting and pointer capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarWorkspaceLayout {
    pub navigation_area: Rect,
    pub navigation: Vec<Rect>,
    pub content: Rect,
    pub vertical: bool,
}

impl SidebarWorkspaceLayout {
    /// Ordinary navigation and list items occupy one terminal row.
    pub const fn list_item_height(_available_height: u16) -> u16 {
        1
    }

    pub fn new(area: Rect, labels: &[String]) -> Self {
        Self::layout(area, labels, None)
    }

    /// Keep the active navigation button visible when the sidebar needs scrolling.
    pub fn scrollable(area: Rect, labels: &[String], active: usize) -> Self {
        Self::layout(area, labels, Some(active))
    }

    fn layout(area: Rect, labels: &[String], active: Option<usize>) -> Self {
        let vertical_padding = u16::from(area.height >= 10);
        let capacity =
            usize::from(area.height.saturating_sub(vertical_padding * 2)).min(labels.len());
        let vertical =
            area.width >= 92 && capacity > 0 && (active.is_some() || capacity == labels.len());
        let mut navigation = Vec::new();
        let (navigation_area, content) = if vertical {
            let width = labels
                .iter()
                .map(|label| (terminal_width(label) as u16).saturating_add(5))
                .max()
                .unwrap_or(0)
                .max(20)
                .min(area.width / 3);
            let start = active
                .unwrap_or(0)
                .saturating_sub(capacity - 1)
                .min(labels.len() - capacity);
            for index in 0..labels.len() {
                if index < start || index >= start + capacity {
                    navigation.push(Rect::default());
                    continue;
                }
                navigation.push(Rect::new(
                    area.x,
                    area.y + vertical_padding + (index - start) as u16,
                    width,
                    1,
                ));
            }
            (
                Rect::new(area.x, area.y, width, area.height),
                Rect::new(
                    area.x + width + 2,
                    area.y,
                    area.width.saturating_sub(width + 2),
                    area.height,
                ),
            )
        } else {
            let mut x = area.x;
            let mut y = area.y;
            for label in labels {
                let width = (terminal_width(label) as u16)
                    .saturating_add(4)
                    .min(area.width);
                if x > area.x && x.saturating_add(width) > area.right() {
                    x = area.x;
                    y = y.saturating_add(1);
                }
                navigation
                    .push(Rect::new(x, y, width, u16::from(y < area.bottom())).intersection(area));
                x = x.saturating_add(width).saturating_add(2);
            }
            let bottom = if labels.is_empty() {
                area.y
            } else {
                y.saturating_add(1).min(area.bottom())
            };
            let top = bottom
                .saturating_add(u16::from(!labels.is_empty()))
                .min(area.bottom());
            (
                Rect::new(area.x, area.y, area.width, bottom.saturating_sub(area.y)),
                Rect::new(area.x, top, area.width, area.bottom().saturating_sub(top)),
            )
        };
        Self {
            navigation_area,
            navigation,
            content,
            vertical,
        }
    }

    pub fn panes(area: Rect, details_only: bool) -> (Rect, Rect) {
        if details_only {
            return (Rect::default(), area);
        }
        if area.width >= 74 {
            let available = area.width.saturating_sub(2);
            let width = available * 3 / 5;
            (
                Rect::new(area.x, area.y, width, area.height),
                Rect::new(area.x + width + 2, area.y, available - width, area.height),
            )
        } else {
            let available = area.height.saturating_sub(1);
            let height = available / 2;
            (
                Rect::new(area.x, area.y, area.width, height),
                Rect::new(
                    area.x,
                    area.y + height + u16::from(area.height > 0),
                    area.width,
                    available - height,
                ),
            )
        }
    }

    pub fn render_navigation(
        &self,
        frame: &mut Frame<'_>,
        buttons: &[Button],
        active: usize,
        context: &RenderContext,
    ) {
        Surface::new()
            .raised(true)
            .render_frame(frame, self.navigation_area, context);
        for (index, (button, area)) in buttons.iter().zip(&self.navigation).enumerate() {
            Self::render_navigation_button(frame, button, *area, index == active, context);
        }
    }

    pub(crate) fn render_navigation_button(
        frame: &mut Frame<'_>,
        button: &Button,
        area: Rect,
        current: bool,
        context: &RenderContext,
    ) {
        Self::render_workspace_row(frame, button, area, current, 1, context);
    }

    pub(crate) fn render_workspace_row(
        frame: &mut Frame<'_>,
        button: &Button,
        area: Rect,
        current: bool,
        right_padding: u16,
        context: &RenderContext,
    ) {
        if area.is_empty() {
            return;
        }
        let theme = context.compatibility_theme();
        let label = truncate_to_terminal_width(
            &button.label,
            usize::from(area.width.saturating_sub(2 + right_padding)),
        );
        let mut row = button
            .clone()
            .with_label_alignment(HorizontalAlignment::Left)
            .with_bracketed_label(false)
            .with_capture_name(button.label.clone())
            .with_background(context.theme.raised)
            .with_current(current);
        row.label = format!("  {label}");
        row.render_borderless_frame(frame, area, &theme);
        if current || (button.state.focused && theme.keyboard_focus_visible()) {
            frame.render_widget(
                Paragraph::new(ratatui::text::Line::from(row_marker(
                    button.state.selected(current),
                    &theme,
                ))),
                Rect::new(area.x, area.y, 1, 1),
            );
        }
    }
}
