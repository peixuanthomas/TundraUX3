use ratatui::layout::Rect;

use super::model::{NotificationActionViewModel, NotificationViewModel};

const NOTIFICATION_DIALOG_WIDTH: u16 = 64;
const NOTIFICATION_DIALOG_MIN_WIDTH: u16 = 40;
const NOTIFICATION_DIALOG_MIN_HEIGHT: u16 = 7;
const NOTIFICATION_DIALOG_WITH_ACTIONS_MIN_HEIGHT: u16 = 9;
const NOTIFICATION_DIALOG_BORDER_CELLS: u16 = 2;
const NOTIFICATION_ACTION_GAP: u16 = 4;

pub const NOTIFICATION_TOO_SMALL_MESSAGE: &str =
    "Terminal is too small to render this notification.";
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationActionLayout {
    pub index: usize,
    pub area: Rect,
}

pub fn notification_layout(area: Rect, model: &NotificationViewModel) -> NotificationLayout {
    let dialog_width = area
        .width
        .clamp(NOTIFICATION_DIALOG_MIN_WIDTH, NOTIFICATION_DIALOG_WIDTH);
    let inner_width = dialog_width.saturating_sub(NOTIFICATION_DIALOG_BORDER_CELLS);
    let message_height = notification_wrapped_line_count(&model.message, inner_width).max(1);
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
    let horizontal_actions = actions_fit_one_line(&action_metrics, inner_width);
    let action_height = if action_metrics.is_empty() {
        0
    } else if horizontal_actions {
        1
    } else {
        action_metrics
            .iter()
            .fold(0_u16, |height, metric| height.saturating_add(metric.height))
    };
    let content_height = NOTIFICATION_DIALOG_BORDER_CELLS
        .saturating_add(message_height)
        .saturating_add(u16::from(!action_metrics.is_empty()))
        .saturating_add(action_height);
    let required_height = if action_metrics.is_empty() {
        content_height.max(NOTIFICATION_DIALOG_MIN_HEIGHT)
    } else {
        content_height.max(NOTIFICATION_DIALOG_WITH_ACTIONS_MIN_HEIGHT)
    };

    if area.width < NOTIFICATION_DIALOG_MIN_WIDTH || area.height < required_height {
        return NotificationLayout::TooSmall {
            required_width: NOTIFICATION_DIALOG_MIN_WIDTH,
            required_height,
        };
    }

    let dialog = centered_rect(area, dialog_width, required_height);
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
    let message = Rect::new(inner.x, inner.y, inner.width, message_height);
    let action_y = message.y.saturating_add(message.height).saturating_add(1);
    let actions = if horizontal_actions {
        horizontal_action_layouts(inner, action_y, &action_metrics)
    } else {
        stacked_action_layouts(inner, action_y, &action_metrics)
    };

    NotificationLayout::Dialog(NotificationDialogLayout {
        dialog,
        message,
        actions,
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
        let characters = source_line.chars().collect::<Vec<_>>();
        if characters.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        wrapped.extend(
            characters
                .chunks(width)
                .map(|chunk| chunk.iter().collect::<String>()),
        );
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
        .map(|line| u16::try_from(line.chars().count()).unwrap_or(u16::MAX))
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
) -> Vec<NotificationActionLayout> {
    let mut y = y;
    metrics
        .iter()
        .enumerate()
        .map(|(index, metric)| {
            let width = metric.width.min(inner.width).max(1);
            let area = Rect::new(
                inner
                    .x
                    .saturating_add(inner.width.saturating_sub(width) / 2),
                y,
                width,
                metric.height,
            );
            y = y.saturating_add(metric.height);
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
