use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::model::{NotificationActionViewModel, NotificationViewModel};

const NOTIFICATION_DIALOG_WIDTH: u16 = 64;
const NOTIFICATION_DIALOG_MIN_WIDTH: u16 = 40;
const NOTIFICATION_DIALOG_MIN_HEIGHT: u16 = 7;
const NOTIFICATION_DIALOG_WITH_ACTIONS_MIN_HEIGHT: u16 = 9;
const NOTIFICATION_DIALOG_BORDER_CELLS: u16 = 2;
const NOTIFICATION_ACTION_GAP: u16 = 4;

pub fn notification_too_small_message() -> String {
    i18n::tr!("ui-notifications-terminal-is-too-small-to-render-this-notification")
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationLayout {
    Dialog(NotificationDialogLayout),
    TooSmall {
        required_width: u16,
        required_height: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDialogLayout {
    pub dialog: Rect,
    pub message: Rect,
    pub actions: Vec<NotificationActionLayout>,
    pub message_line_count: usize,
    pub scroll_offset: usize,
    pub max_scroll_offset: usize,
    pub scrollbar: Option<Rect>,
    pub scroll_hint: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationActionLayout {
    pub index: usize,
    pub area: Rect,
}

pub fn notification_layout(area: Rect, model: &NotificationViewModel) -> NotificationLayout {
    let required_width = notification_text_width(&super::model::notification_title(model))
        .saturating_add(NOTIFICATION_DIALOG_BORDER_CELLS)
        .clamp(3, NOTIFICATION_DIALOG_MIN_WIDTH);
    let dialog_width = area
        .width
        .max(required_width)
        .min(NOTIFICATION_DIALOG_WIDTH);
    let inner_width = dialog_width.saturating_sub(NOTIFICATION_DIALOG_BORDER_CELLS);
    let unscrolled_line_count = wrap_notification_text(&model.message, inner_width)
        .len()
        .max(1);
    let action_texts = model
        .actions
        .iter()
        .map(notification_action_text)
        .collect::<Vec<_>>();
    let action_metrics = action_texts
        .iter()
        .map(|text| ActionMetric {
            width: notification_text_width(text),
            height: notification_wrapped_line_count(text, inner_width).max(1),
        })
        .collect::<Vec<_>>();
    let horizontal_actions =
        !model.stacked_actions && actions_fit_one_line(&action_metrics, inner_width);
    let action_height = if action_metrics.is_empty() {
        0
    } else if horizontal_actions {
        1
    } else {
        action_metrics
            .iter()
            .fold(0_u16, |height, metric| height.saturating_add(metric.height))
    };
    let separator_height = u16::from(!action_metrics.is_empty());
    // Only the title/borders, at least one message line, and all actions are mandatory.
    // Message length must never hide the recovery actions behind TooSmall.
    let required_height = NOTIFICATION_DIALOG_BORDER_CELLS
        .saturating_add(1)
        .saturating_add(separator_height)
        .saturating_add(action_height);
    if area.width < required_width || area.height < required_height {
        return NotificationLayout::TooSmall {
            required_width,
            required_height,
        };
    }
    let gaps = u16::try_from(action_metrics.len().saturating_sub(1)).unwrap_or(u16::MAX);
    let full_height =
        usize::from(required_height.saturating_sub(1)).saturating_add(unscrolled_line_count);
    // Preserve spacious menu actions when the entire message fits without scrolling.
    let action_gap = u16::from(
        model.stacked_actions
            && usize::from(area.height) >= full_height.saturating_add(usize::from(gaps)),
    );
    let fixed_height = required_height
        .saturating_sub(1)
        .saturating_add(action_gap.saturating_mul(gaps));
    let message_capacity = area.height.saturating_sub(fixed_height);
    let overflowing = unscrolled_line_count > usize::from(message_capacity);
    // Reserve the existing scrollbar's terminal column before wrapping the message.
    let message_width = inner_width.saturating_sub(u16::from(overflowing));
    let message_line_count = if overflowing {
        wrap_notification_text(&model.message, message_width)
            .len()
            .max(1)
    } else {
        unscrolled_line_count
    };
    let message_height = u16::try_from(message_line_count)
        .unwrap_or(u16::MAX)
        .min(message_capacity);
    let max_scroll_offset = message_line_count.saturating_sub(usize::from(message_height));
    let scroll_offset = model.scroll_offset.min(max_scroll_offset);
    let nominal_height = if action_metrics.is_empty() {
        NOTIFICATION_DIALOG_MIN_HEIGHT
    } else {
        NOTIFICATION_DIALOG_WITH_ACTIONS_MIN_HEIGHT
    };
    let dialog_height = fixed_height
        .saturating_add(message_height)
        .max(nominal_height)
        .min(area.height);
    let dialog = centered_rect(area, dialog_width, dialog_height);
    let inner = Rect::new(
        dialog.x.saturating_add(1),
        dialog.y.saturating_add(1),
        dialog
            .width
            .saturating_sub(NOTIFICATION_DIALOG_BORDER_CELLS),
        dialog
            .height
            .saturating_sub(NOTIFICATION_DIALOG_BORDER_CELLS),
    );
    let message = Rect::new(inner.x, inner.y, message_width, message_height);
    let scrollbar = overflowing.then(|| {
        Rect::new(
            inner.right().saturating_sub(1),
            message.y,
            1,
            message.height,
        )
    });
    let scroll_hint = (overflowing && separator_height > 0)
        .then(|| Rect::new(inner.x, message.bottom(), inner.width, 1));
    let action_y = message.y.saturating_add(message.height).saturating_add(1);
    let actions = if horizontal_actions {
        horizontal_action_layouts(inner, action_y, &action_metrics)
    } else {
        stacked_action_layouts(
            inner,
            action_y,
            &action_metrics,
            action_gap,
            model.stacked_actions,
        )
    };

    NotificationLayout::Dialog(NotificationDialogLayout {
        dialog,
        message,
        actions,
        message_line_count,
        scroll_offset,
        max_scroll_offset,
        scrollbar,
        scroll_hint,
    })
}

pub(crate) fn notification_action_text(action: &NotificationActionViewModel) -> String {
    let label = match &action.shortcut {
        Some(shortcut) => format!("{shortcut}: {}", action.label),
        None => action.label.clone(),
    };
    if action.selected {
        format!("[{label}]")
    } else {
        format!(" {label} ")
    }
}

pub(crate) fn wrap_notification_text(text: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }

    let width = usize::from(width);
    let mut wrapped = Vec::new();
    for source_line in text.split('\n') {
        if source_line.is_empty() {
            wrapped.push(String::new());
            continue;
        }

        let span = Span::raw(source_line);
        let mut line = String::new();
        let mut line_width = 0usize;
        for grapheme in span.styled_graphemes(Style::default()) {
            let grapheme_width = Line::from(grapheme.symbol).width();
            if !line.is_empty() && line_width.saturating_add(grapheme_width) > width {
                wrapped.push(std::mem::take(&mut line));
                line_width = 0;
            }
            if grapheme_width > width && line.is_empty() {
                wrapped.push(grapheme.symbol.to_string());
                continue;
            }
            line.push_str(grapheme.symbol);
            line_width = line_width.saturating_add(grapheme_width);
        }
        if !line.is_empty() {
            wrapped.push(line);
        }
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

fn notification_wrapped_line_count(text: &str, width: u16) -> u16 {
    u16::try_from(wrap_notification_text(text, width).len()).unwrap_or(u16::MAX)
}

fn notification_text_width(text: &str) -> u16 {
    text.split('\n')
        .map(|line| u16::try_from(Line::from(line).width()).unwrap_or(u16::MAX))
        .max()
        .unwrap_or(0)
        .max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActionMetric {
    width: u16,
    height: u16,
}

fn actions_fit_one_line(metrics: &[ActionMetric], inner_width: u16) -> bool {
    if metrics.is_empty() {
        return false;
    }
    if metrics.iter().any(|metric| metric.height > 1) {
        return false;
    }

    let labels_width = metrics
        .iter()
        .fold(0_u16, |width, metric| width.saturating_add(metric.width));
    let gaps = u16::try_from(metrics.len().saturating_sub(1))
        .unwrap_or(u16::MAX)
        .saturating_mul(NOTIFICATION_ACTION_GAP);
    labels_width.saturating_add(gaps) <= inner_width
}

fn horizontal_action_layouts(
    inner: Rect,
    y: u16,
    metrics: &[ActionMetric],
) -> Vec<NotificationActionLayout> {
    let labels_width = metrics
        .iter()
        .fold(0_u16, |width, metric| width.saturating_add(metric.width));
    let gaps = u16::try_from(metrics.len().saturating_sub(1))
        .unwrap_or(u16::MAX)
        .saturating_mul(NOTIFICATION_ACTION_GAP);
    let total_width = labels_width.saturating_add(gaps);
    let mut x = inner
        .x
        .saturating_add(inner.width.saturating_sub(total_width) / 2);

    metrics
        .iter()
        .enumerate()
        .map(|(index, metric)| {
            let area = Rect::new(x, y, metric.width, 1);
            x = x
                .saturating_add(metric.width)
                .saturating_add(NOTIFICATION_ACTION_GAP);
            NotificationActionLayout { index, area }
        })
        .collect()
}

fn stacked_action_layouts(
    inner: Rect,
    y: u16,
    metrics: &[ActionMetric],
    gap: u16,
    uniform_width: bool,
) -> Vec<NotificationActionLayout> {
    let mut y = y;
    let menu_width = metrics.iter().map(|metric| metric.width).max().unwrap_or(1);
    metrics
        .iter()
        .enumerate()
        .map(|(index, metric)| {
            let width = if uniform_width {
                menu_width
            } else {
                metric.width
            }
            .min(inner.width)
            .max(1);
            let area = Rect::new(
                inner
                    .x
                    .saturating_add(inner.width.saturating_sub(width) / 2),
                y,
                width,
                metric.height,
            );
            y = y.saturating_add(metric.height).saturating_add(gap);
            NotificationActionLayout { index, area }
        })
        .collect()
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}
