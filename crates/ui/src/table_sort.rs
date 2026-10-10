use crate::{RenderContext, components::Button};
use ratatui::{Frame, layout::Rect};
use std::cmp::Ordering;

pub fn table_header_areas(area: Rect, widths: &[u16]) -> Vec<(usize, Rect)> {
    let mut x = area.x;
    widths
        .iter()
        .enumerate()
        .filter_map(|(index, width)| {
            let rect = Rect::new(
                x.min(area.right()),
                area.y,
                (*width).min(area.right().saturating_sub(x)),
                u16::from(area.height > 0),
            );
            x = x.saturating_add(*width);
            (!rect.is_empty()).then_some((index, rect))
        })
        .collect()
}

pub fn equal_table_widths(width: u16, count: usize) -> Vec<u16> {
    if count == 0 {
        return Vec::new();
    }
    let base = usize::from(width) / count;
    (0..count)
        .map(|i| (base + usize::from(i < usize::from(width) % count)) as u16)
        .collect()
}

/// Wrap full labels, then align each row against the right edge.
pub fn right_aligned_actions(area: Rect, widths: &[u16]) -> Vec<Rect> {
    let mut x = area.x;
    let mut y = area.y;
    let mut result = Vec::new();
    for width in widths {
        let width = (*width).min(area.width);
        if x > area.x && x.saturating_add(width) > area.right() {
            x = area.x;
            y = y.saturating_add(1);
        }
        result.push(Rect::new(
            x,
            y.min(area.bottom()),
            width,
            u16::from(y < area.bottom()),
        ));
        x = x.saturating_add(width).saturating_add(1);
    }
    for row in area.y..y.saturating_add(1).min(area.bottom()) {
        let end = result
            .iter()
            .filter(|r| r.y == row)
            .map(|r| r.right())
            .max()
            .unwrap_or(area.right());
        for rect in result.iter_mut().filter(|r| r.y == row) {
            rect.x += area.right().saturating_sub(end);
        }
    }
    result
}

pub fn render_table_headers(
    frame: &mut Frame<'_>,
    areas: &[(usize, Rect)],
    labels: &[String],
    sort: Option<TableSort>,
    id: &str,
    context: &RenderContext,
) {
    let theme = context.compatibility_theme();
    for (column, area) in areas {
        if let Some(label) = labels.get(*column) {
            let label = sort.map_or_else(|| label.clone(), |sort| sort.label(*column, label));
            Button::new(
                format!("{id}.sort.{column}"),
                table_header_text(&label, area.width),
            )
            .with_bracketed_label(false)
            .render_borderless_frame(frame, *area, &theme);
        }
    }
}

// Pad the entire cell so Button's centered text stays aligned with the column
// and replaces any header already drawn by the table underneath it.
pub(crate) fn table_header_text(label: &str, width: u16) -> String {
    use crate::components::{terminal_width, truncate_to_terminal_width};
    let width = usize::from(width);
    let arrow = [" ▲", " ▼", " ↑", " ↓"]
        .into_iter()
        .find(|arrow| label.ends_with(arrow));
    let text = if let Some(arrow) = arrow.filter(|_| width >= 2) {
        format!(
            "{}{}",
            truncate_to_terminal_width(label.strip_suffix(arrow).unwrap(), width - 2),
            arrow
        )
    } else {
        truncate_to_terminal_width(label, width)
    };
    let padding = width.saturating_sub(terminal_width(&text));
    format!("{text}{}", " ".repeat(padding))
}

/// A column selection shared by the built-in list pages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TableSort {
    pub column: usize,
    pub descending: bool,
}

impl TableSort {
    pub fn toggle(current: Option<Self>, column: usize) -> Self {
        Self {
            column,
            descending: current.is_some_and(|sort| sort.column == column && !sort.descending),
        }
    }

    pub fn label(self, column: usize, label: &str) -> String {
        if self.column == column {
            format!("{label} {}", if self.descending { "▼" } else { "▲" })
        } else {
            label.to_owned()
        }
    }

    pub fn order(self, order: Ordering) -> Ordering {
        if self.descending {
            order.reverse()
        } else {
            order
        }
    }

    pub fn compare(self, left: &str, right: &str) -> Ordering {
        self.order(compare_table_cells(left, right))
    }
}

/// Compare counts, percentages and sizes by value, other cells alphabetically.
/// Do not extract digits from names or version strings and mistake them for sizes.
pub fn compare_table_cells(left: &str, right: &str) -> Ordering {
    match (cell_number(left), cell_number(right)) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.to_lowercase().cmp(&right.to_lowercase()),
    }
}

fn cell_number(value: &str) -> Option<f64> {
    let value = value.trim().trim_end_matches("/s");
    let value = value.split_once(" / ").map_or(value, |(used, _)| used);
    let end = value
        .find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+'))
        .unwrap_or(value.len());
    let number = value[..end].parse::<f64>().ok()?;
    let multiplier = match value[end..].trim().to_ascii_lowercase().as_str() {
        "" | "%" | "b" | "bytes" | "°c" | "c" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "kib" => 1024.0,
        "mib" => 1024.0_f64.powi(2),
        "gib" => 1024.0_f64.powi(3),
        "tib" => 1024.0_f64.powi(4),
        "ms" => 0.001,
        "s" => 1.0,
        "min" => 60.0,
        "h" => 3600.0,
        _ => return None,
    };
    (number.is_finite()).then_some(number * multiplier)
}
