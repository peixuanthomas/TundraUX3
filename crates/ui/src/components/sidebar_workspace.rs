use ratatui::{Frame, layout::Rect};

use super::Button;
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
}

impl SidebarWorkspaceLayout {
    pub fn new(area: Rect, labels: &[String]) -> Self {
        Self::layout(area, labels, None)
    }

    /// Keep the active navigation button visible when the sidebar needs scrolling.
    pub fn scrollable(area: Rect, labels: &[String], active: usize) -> Self {
        Self::layout(area, labels, Some(active))
    }

    fn layout(area: Rect, labels: &[String], active: Option<usize>) -> Self {
        let capacity =
            usize::from((area.height / 3).max(u16::from(area.height > 0))).min(labels.len());
        let vertical = area.width >= 92
            && capacity > 0
            && (active.is_some() || area.height >= labels.len() as u16 * 3);
        let mut navigation = Vec::new();
        let content = if vertical {
            let width = labels
                .iter()
                .map(|label| super::terminal_width(label) as u16 + 7)
                .max()
                .unwrap_or(0)
                .min(area.width / 3);
            let start = active
                .unwrap_or(0)
                .saturating_sub(capacity - 1)
                .min(labels.len() - capacity);
            let height = (area.height / capacity as u16).min(5);
            for index in 0..labels.len() {
                if index < start || index >= start + capacity {
                    navigation.push(Rect::default());
                    continue;
                }
                navigation.push(Rect::new(
                    area.x,
                    area.y + (index - start) as u16 * height,
                    width.saturating_sub(1),
                    if height < 3 {
                        height
                    } else {
                        height.saturating_sub(1).max(3)
                    },
                ));
            }
            Rect::new(
                area.x + width,
                area.y,
                area.width.saturating_sub(width),
                area.height,
            )
        } else {
            let mut x = area.x;
            let mut y = area.y;
            for label in labels {
                let width = (super::terminal_width(label) as u16 + 4).min(area.width);
                if x > area.x && x.saturating_add(width) > area.right() {
                    x = area.x;
                    y = y.saturating_add(1);
                }
                navigation
                    .push(Rect::new(x, y, width, u16::from(y < area.bottom())).intersection(area));
                x = x.saturating_add(width);
            }
            let top = if labels.is_empty() {
                area.y
            } else {
                y.saturating_add(1).min(area.bottom())
            };
            Rect::new(area.x, top, area.width, area.bottom().saturating_sub(top))
        };
        Self {
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
            let width = area.width * 3 / 5;
            (
                Rect::new(area.x, area.y, width, area.height),
                Rect::new(area.x + width, area.y, area.width - width, area.height),
            )
        } else {
            let height = area.height / 2;
            (
                Rect::new(area.x, area.y, area.width, height),
                Rect::new(area.x, area.y + height, area.width, area.height - height),
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
        for (index, (button, area)) in buttons.iter().zip(&self.navigation).enumerate() {
            if area.is_empty() {
                continue;
            }
            let mut button = button
                .clone()
                .with_centered_label(true)
                .with_bracketed_label(false)
                .with_current(index == active);
            button.label = format!(
                "{} {}",
                if index == active { "->" } else { "  " },
                button.label
            );
            if self.vertical && area.height >= 3 {
                button.render_frame(frame, *area, &theme);
            } else {
                button.render_borderless_frame(frame, *area, &theme);
            }
        }
    }
}
