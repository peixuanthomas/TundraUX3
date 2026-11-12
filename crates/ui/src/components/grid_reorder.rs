use ratatui::{Frame, layout::Rect, widgets::Paragraph};

use super::{ScrollbarOrientation, contains_point};
use crate::TundraTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridDropSide {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridDropTarget {
    pub item_index: usize,
    pub side: GridDropSide,
}

impl GridDropTarget {
    pub const fn insertion_index(self) -> usize {
        match self.side {
            GridDropSide::Before => self.item_index,
            GridDropSide::After => self.item_index.saturating_add(1),
        }
    }

    pub fn indicator(self, area: Rect) -> Rect {
        let x = match self.side {
            GridDropSide::Before => area.x,
            GridDropSide::After => area.right().saturating_sub(1),
        };
        Rect::new(x, area.y, u16::from(area.width > 0), area.height)
    }
}

pub(crate) fn grid_drop_target(
    content: Rect,
    items: impl Iterator<Item = (usize, Rect)>,
    scrollbar: Option<Rect>,
    point: (u16, u16),
) -> Option<GridDropTarget> {
    if !contains_point(content, point.0, point.1)
        || scrollbar.is_some_and(|area| ScrollbarOrientation::VerticalRight.hit_test(area, point))
    {
        return None;
    }
    let (x, y) = point;
    let items = items.collect::<Vec<_>>();
    let mut row_end = None;
    for &(index, area) in &items {
        if y >= area.y && y < area.bottom() {
            if x < area.x.saturating_add(area.width / 2) {
                return Some(GridDropTarget {
                    item_index: index,
                    side: GridDropSide::Before,
                });
            }
            row_end = Some(GridDropTarget {
                item_index: index,
                side: GridDropSide::After,
            });
        }
    }
    if row_end.is_some() {
        return row_end;
    }
    // Row gaps insert before the following row, including on scrolled grids.
    if let Some(&(index, _)) = items.iter().find(|(_, area)| y < area.y) {
        return Some(GridDropTarget {
            item_index: index,
            side: GridDropSide::Before,
        });
    }
    items.last().map(|&(index, _)| GridDropTarget {
        item_index: index,
        side: GridDropSide::After,
    })
}

pub(crate) fn render_grid_drop_indicator(frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
    for y in area.y..area.bottom() {
        frame.render_widget(
            Paragraph::new("┃").style(theme.title_style().fg(theme.button_accent_color())),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}
