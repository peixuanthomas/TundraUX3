use ratatui::layout::Rect;
use ratatui::text::Line;

use super::model::{NotificationActionViewModel, NotificationViewModel};
use crate::components::DialogLayout;

const NOTIFICATION_DIALOG_WIDTH: u16 = 64;
const NOTIFICATION_DIALOG_MIN_WIDTH: u16 = 40;
const NOTIFICATION_ACTION_GAP: u16 = 2;

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
    pub(crate) surface: DialogLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationActionLayout {
    pub index: usize,
    pub area: Rect,
}

pub fn notification_layout(area: Rect, model: &NotificationViewModel) -> NotificationLayout {
    let action_texts = model
        .actions
        .iter()
        .map(notification_action_text)
        .collect::<Vec<_>>();
    let close = model
        .actions
        .iter()
        .position(|action| action.cancel || matches!(action.id.as_str(), "cancel" | "close"));
    let close_text = close.map(|index| action_texts[index].as_str());
    let footer_indices = (0..action_texts.len())
        .filter(|index| Some(*index) != close)
        .collect::<Vec<_>>();
    let padding = DialogLayout::INSET * 2;
    let required_width = notification_text_width(&super::model::notification_title(model))
        .saturating_add(padding)
        .clamp(3, NOTIFICATION_DIALOG_MIN_WIDTH);
    let message_width = notification_text_width(&model.message);
    let action_widths = footer_indices
        .iter()
        .map(|index| notification_text_width(&action_texts[*index]).saturating_add(2));
    let action_width = if model.stacked_actions {
        action_widths.max().unwrap_or(0)
    } else {
        action_widths
            .fold(0u16, u16::saturating_add)
            .saturating_add(
                (footer_indices.len().saturating_sub(1) as u16)
                    .saturating_mul(NOTIFICATION_ACTION_GAP),
            )
    };
    let header_width = notification_text_width(&super::model::notification_title(model))
        .saturating_add(
            close_text.map_or(0, |label| notification_text_width(label).saturating_add(3)),
        );
    let dialog_width = message_width
        .max(action_width)
        .max(header_width)
        .saturating_add(padding)
        .max(required_width)
        .min(NOTIFICATION_DIALOG_WIDTH)
        .min(area.width);
    let inner_width = DialogLayout::content_width(dialog_width);
    let unscrolled_line_count = wrap_notification_text(&model.message, inner_width)
        .len()
        .max(1);
    let action_metrics = footer_indices
        .iter()
        .map(|index| ActionMetric {
            width: notification_text_width(&action_texts[*index]).saturating_add(2),
            height: notification_wrapped_line_count(&action_texts[*index], inner_width).max(1),
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
    // Reserve space for the title, a message line, and every action in compact windows.
    let required_height = (if action_metrics.is_empty() {
        3u16
    } else {
        4u16
    })
    .saturating_add(action_height);
    if area.width < required_width || area.height < required_height {
        return NotificationLayout::TooSmall {
            required_width,
            required_height,
        };
    }
    let chrome_rows = if action_metrics.is_empty() { 4 } else { 6 };
    let dialog_height = u16::try_from(unscrolled_line_count)
        .unwrap_or(u16::MAX)
        .saturating_add(action_height)
        .saturating_add(chrome_rows)
        .min(area.height);
    let dialog = crate::centered_rect(area, dialog_width, dialog_height);
    let surface = DialogLayout::new(dialog, action_height, close_text);
    let overflowing = unscrolled_line_count > usize::from(surface.body.height);
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
        .min(surface.body.height);
    let max_scroll_offset = message_line_count.saturating_sub(usize::from(message_height));
    let scroll_offset = model.scroll_offset.min(max_scroll_offset);
    let message = Rect::new(
        surface.body.x,
        surface.body.y,
        message_width,
        message_height,
    );
    let scrollbar = overflowing.then(|| {
        Rect::new(
            surface.body.right().saturating_sub(1),
            message.y,
            1,
            message.height,
        )
    });
    let scroll_hint =
        (overflowing && !surface.footer.is_empty() && message.bottom() < surface.footer.y)
            .then(|| Rect::new(surface.body.x, message.bottom(), surface.body.width, 1));
    let mut actions = if horizontal_actions {
        horizontal_action_layouts(surface.footer, &action_metrics)
    } else {
        stacked_action_layouts(surface.footer, &action_metrics, model.stacked_actions)
    };
    for action in &mut actions {
        action.index = footer_indices[action.index];
    }
    if let Some(index) = close {
        actions.push(NotificationActionLayout {
            index,
            area: surface.close,
        });
        actions.sort_unstable_by_key(|action| action.index);
    }

    NotificationLayout::Dialog(NotificationDialogLayout {
        dialog,
        message,
        actions,
        message_line_count,
        scroll_offset,
        max_scroll_offset,
        scrollbar,
        scroll_hint,
        surface,
    })
}

pub(crate) fn notification_action_text(action: &NotificationActionViewModel) -> String {
    match &action.shortcut {
        Some(shortcut) => format!("{} {shortcut}", action.label),
        None => action.label.clone(),
    }
}

pub(crate) fn wrap_notification_text(text: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }

    crate::management_wrapped_lines(text, width)
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
    metrics: &[ActionMetric],
) -> Vec<NotificationActionLayout> {
    let labels_width = metrics
        .iter()
        .fold(0_u16, |width, metric| width.saturating_add(metric.width));
    let gaps = u16::try_from(metrics.len().saturating_sub(1))
        .unwrap_or(u16::MAX)
        .saturating_mul(NOTIFICATION_ACTION_GAP);
    let total_width = labels_width.saturating_add(gaps);
    let mut x = inner.right().saturating_sub(total_width);

    metrics
        .iter()
        .enumerate()
        .map(|(index, metric)| {
            let area = Rect::new(x, inner.y, metric.width, 1);
            x = x
                .saturating_add(metric.width)
                .saturating_add(NOTIFICATION_ACTION_GAP);
            NotificationActionLayout { index, area }
        })
        .collect()
}
fn stacked_action_layouts(
    inner: Rect,
    metrics: &[ActionMetric],
    uniform_width: bool,
) -> Vec<NotificationActionLayout> {
    let mut y = inner.y;
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
                if uniform_width {
                    inner.x
                } else {
                    inner.right() - width
                },
                y,
                width,
                metric.height,
            );
            y = y.saturating_add(metric.height);
            NotificationActionLayout { index, area }
        })
        .collect()
}
