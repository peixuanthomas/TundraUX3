use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
};

use super::{Button, Surface};
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
    pub navigation: Vec<Rect>,
    pub content: Rect,
    pub vertical: bool,
    sidebar: Rect,
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
        let capacity = usize::from(area.height).min(labels.len());
        let vertical = area.width >= 92
            && capacity > 0
            && (active.is_some() || usize::from(area.height) >= labels.len());
        let mut navigation = Vec::new();
        let mut sidebar = Rect::default();
        let content = if vertical {
            let width = labels
                .iter()
                .map(|label| (super::terminal_width(label) as u16).saturating_add(2))
                .max()
                .unwrap_or(0)
                .min(area.width / 3);
            sidebar = Rect::new(area.x, area.y, width, area.height);
            let start = active
                .unwrap_or(0)
                .saturating_sub(capacity - 1)
                .min(labels.len() - capacity);
            for index in 0..labels.len() {
                if index < start || index >= start + capacity {
                    navigation.push(Rect::default());
                    continue;
                }
                navigation.push(Rect::new(area.x, area.y + (index - start) as u16, width, 1));
            }
            Rect::new(
                area.x + width + 2,
                area.y,
                area.width.saturating_sub(width + 2),
                area.height,
            )
        } else {
            let mut x = area.x;
            let mut y = area.y;
            for label in labels {
                let width = (super::terminal_width(label) as u16 + 2).min(area.width);
                if x > area.x && x.saturating_add(width) > area.right() {
                    x = area.x;
                    y = y.saturating_add(1);
                }
                navigation
                    .push(Rect::new(x, y, width, u16::from(y < area.bottom())).intersection(area));
                x = x.saturating_add(width).saturating_add(2);
            }
            let top = if labels.is_empty() {
                area.y
            } else {
                y.saturating_add(2).min(area.bottom())
            };
            Rect::new(area.x, top, area.width, area.bottom().saturating_sub(top))
        };
        Self {
            navigation,
            content,
            vertical,
            sidebar,
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
        let theme = context.compatibility_theme();
        if !self.sidebar.is_empty() {
            Surface::new().render_frame(frame, self.sidebar, context);
        }
        for (index, (button, area)) in buttons.iter().zip(&self.navigation).enumerate() {
            if area.is_empty() {
                continue;
            }
            let mut button = button
                .clone()
                .with_label_alignment(HorizontalAlignment::Left)
                .with_bracketed_label(false)
                .with_capture_name(button.label.clone())
                .with_background(context.theme.row_background(index))
                .with_current(index == active);
            button.label = format!(
                "{} {}",
                if button.state.focused && theme.keyboard_focus_visible() {
                    ">"
                } else {
                    " "
                },
                button.label
            );
            button.render_borderless_frame(frame, *area, &theme);
        }
    }
}
