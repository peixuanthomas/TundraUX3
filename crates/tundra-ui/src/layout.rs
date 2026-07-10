use ratatui::layout::Rect;

use crate::view_model::{
    ClockEntryViewModel, ClockViewModel, NotificationActionViewModel, NotificationViewModel,
};

const NOTIFICATION_DIALOG_WIDTH: u16 = 64;
const NOTIFICATION_DIALOG_MIN_WIDTH: u16 = 40;
const NOTIFICATION_DIALOG_MIN_HEIGHT: u16 = 7;
const NOTIFICATION_DIALOG_WITH_ACTIONS_MIN_HEIGHT: u16 = 9;
const NOTIFICATION_DIALOG_BORDER_CELLS: u16 = 2;
const NOTIFICATION_ACTION_GAP: u16 = 4;
const CLOCK_ANALOG_MIN_WIDTH: u16 = 76;
const CLOCK_ANALOG_MIN_HEIGHT: u16 = 18;
const CLOCK_PANEL_MIN_WIDTH: u16 = 28;
const CLOCK_PANEL_MAX_WIDTH: u16 = 34;
const CLOCK_COLUMN_GAP: u16 = 1;
const CLOCK_CREATE_DIALOG_WIDTH: u16 = 58;
const CLOCK_CREATE_DIALOG_HEIGHT: u16 = 11;

pub const NOTIFICATION_TOO_SMALL_MESSAGE: &str =
    "Terminal is too small to render this notification.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLayout {
    Compact(Rect),
    Full { top: Rect, main: Rect, status: Rect },
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationActionLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockPageMode {
    Analog,
    DigitalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockEntryKind {
    Alarm,
    Countdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockEntryRowLayout {
    pub id: u64,
    pub kind: ClockEntryKind,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockCreateDialogLayout {
    pub dialog: Rect,
    pub input: Rect,
    pub error: Rect,
    pub create_alarm: Rect,
    pub create_countdown: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockPageLayout {
    pub mode: ClockPageMode,
    /// Outer area of the clock block.
    pub clock: Rect,
    /// ASCII face canvas; absent in the narrow digital-only layout.
    pub analog: Option<Rect>,
    /// Exact date and digital-time content area.
    pub digital: Rect,
    /// Outer area of the alarms and countdowns block.
    pub panel: Rect,
    pub new_button: Rect,
    pub alarms_heading: Rect,
    pub countdowns_heading: Rect,
    pub entry_rows: Vec<ClockEntryRowLayout>,
    /// Effective offset into alarms followed by countdowns.
    pub entry_window_start: usize,
    pub entry_capacity: usize,
    pub create_dialog: Option<ClockCreateDialogLayout>,
}

pub fn compute_shell_layout(area: Rect) -> ShellLayout {
    if area.width < 50 || area.height < 12 {
        return ShellLayout::Compact(area);
    }

    let top = Rect::new(area.x, area.y, area.width, 3);
    let main_height = area.height.saturating_sub(6);
    let main = Rect::new(area.x, area.y.saturating_add(3), area.width, main_height);
    let status = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(3)),
        area.width,
        3,
    );

    ShellLayout::Full { top, main, status }
}

/// Computes every interactive rectangle used by the Clock page.
///
/// Callers should pass the `main` rectangle from [`compute_shell_layout`]. The
/// renderer and input routing can then share this value without duplicating
/// geometry or visible-entry window calculations.
pub fn clock_page_layout(main: Rect, model: &ClockViewModel) -> ClockPageLayout {
    let mode = if main.width >= CLOCK_ANALOG_MIN_WIDTH && main.height >= CLOCK_ANALOG_MIN_HEIGHT {
        ClockPageMode::Analog
    } else {
        ClockPageMode::DigitalOnly
    };

    let (clock, analog, digital, panel) = match mode {
        ClockPageMode::Analog => {
            let panel_width = (main.width / 3)
                .clamp(CLOCK_PANEL_MIN_WIDTH, CLOCK_PANEL_MAX_WIDTH)
                .min(main.width.saturating_sub(CLOCK_COLUMN_GAP));
            let clock_width = main
                .width
                .saturating_sub(panel_width)
                .saturating_sub(CLOCK_COLUMN_GAP);
            let clock = Rect::new(main.x, main.y, clock_width, main.height);
            let panel = Rect::new(
                main.x
                    .saturating_add(clock_width)
                    .saturating_add(CLOCK_COLUMN_GAP),
                main.y,
                panel_width,
                main.height,
            );
            let inner = inset_rect(clock, 1);
            let digital_height = 2.min(inner.height);
            let digital = Rect::new(
                inner.x,
                inner
                    .y
                    .saturating_add(inner.height.saturating_sub(digital_height)),
                inner.width,
                digital_height,
            );
            let face_height = inner
                .height
                .saturating_sub(digital_height)
                .saturating_sub(1);
            let analog = (inner.width > 0 && face_height > 0).then_some(Rect::new(
                inner.x,
                inner.y,
                inner.width,
                face_height,
            ));
            (clock, analog, digital, panel)
        }
        ClockPageMode::DigitalOnly => {
            let panel_width = (main.width / 2)
                .clamp(CLOCK_PANEL_MIN_WIDTH, CLOCK_PANEL_MAX_WIDTH)
                .min(main.width.saturating_sub(17));
            let clock_width = main
                .width
                .saturating_sub(panel_width)
                .saturating_sub(CLOCK_COLUMN_GAP);
            let clock = Rect::new(main.x, main.y, clock_width, main.height);
            let panel = Rect::new(
                main.x
                    .saturating_add(clock_width)
                    .saturating_add(CLOCK_COLUMN_GAP),
                main.y,
                panel_width,
                main.height,
            );
            let digital = inset_rect(clock, 1);
            (clock, None, digital, panel)
        }
    };

    let panel_inner = inset_rect(panel, 1);
    let new_button = line_in_rect(panel_inner, panel_inner.y);
    let condensed_panel = panel_inner.height < 7;
    let entry_capacity =
        usize::from(
            panel_inner
                .height
                .saturating_sub(if condensed_panel { 3 } else { 4 }),
        );
    let total_entries = model.alarms.len().saturating_add(model.countdowns.len());
    let entry_window_start = model.entry_window_start.min(total_entries);
    let visible = flattened_clock_entries(model)
        .into_iter()
        .skip(entry_window_start)
        .take(entry_capacity)
        .collect::<Vec<_>>();
    let visible_alarm_count = visible
        .iter()
        .filter(|(kind, _)| *kind == ClockEntryKind::Alarm)
        .count();

    let alarms_heading = line_in_rect(
        panel_inner,
        panel_inner
            .y
            .saturating_add(if condensed_panel { 1 } else { 2 }),
    );
    let alarm_rows_y = alarms_heading.y.saturating_add(alarms_heading.height);
    let countdowns_heading = line_in_rect(
        panel_inner,
        alarm_rows_y.saturating_add(usize_to_u16(visible_alarm_count)),
    );
    let countdown_rows_y = countdowns_heading
        .y
        .saturating_add(countdowns_heading.height);
    let mut alarm_row = 0_u16;
    let mut countdown_row = 0_u16;
    let entry_rows = visible
        .into_iter()
        .filter_map(|(kind, entry)| {
            let y = match kind {
                ClockEntryKind::Alarm => {
                    let y = alarm_rows_y.saturating_add(alarm_row);
                    alarm_row = alarm_row.saturating_add(1);
                    y
                }
                ClockEntryKind::Countdown => {
                    let y = countdown_rows_y.saturating_add(countdown_row);
                    countdown_row = countdown_row.saturating_add(1);
                    y
                }
            };
            let area = line_in_rect(panel_inner, y);
            (area.width > 0 && area.height > 0).then_some(ClockEntryRowLayout {
                id: entry.id,
                kind,
                area,
            })
        })
        .collect();

    ClockPageLayout {
        mode,
        clock,
        analog,
        digital,
        panel,
        new_button,
        alarms_heading,
        countdowns_heading,
        entry_rows,
        entry_window_start,
        entry_capacity,
        create_dialog: model
            .create_dialog
            .as_ref()
            .map(|_| clock_create_dialog_layout(main)),
    }
}

fn flattened_clock_entries(model: &ClockViewModel) -> Vec<(ClockEntryKind, &ClockEntryViewModel)> {
    model
        .alarms
        .iter()
        .map(|entry| (ClockEntryKind::Alarm, entry))
        .chain(
            model
                .countdowns
                .iter()
                .map(|entry| (ClockEntryKind::Countdown, entry)),
        )
        .collect()
}

fn clock_create_dialog_layout(area: Rect) -> ClockCreateDialogLayout {
    let dialog = centered_rect(
        area,
        area.width.min(CLOCK_CREATE_DIALOG_WIDTH),
        area.height.min(CLOCK_CREATE_DIALOG_HEIGHT),
    );
    let inner = inset_rect(dialog, 1);
    let (input_offset, error_offset, button_offset) = if inner.height >= 7 {
        (2, Some(4), 6)
    } else {
        let input_offset = u16::from(inner.height >= 2);
        let button_offset = inner.height.saturating_sub(1);
        let error_offset = (button_offset > input_offset.saturating_add(1))
            .then_some(button_offset.saturating_sub(1));
        (input_offset, error_offset, button_offset)
    };
    let input = line_in_rect(inner, inner.y.saturating_add(input_offset));
    let button_y = inner.y.saturating_add(button_offset);
    let error = error_offset.map_or_else(
        || Rect::new(inner.x, button_y, 0, 0),
        |offset| line_in_rect(inner, inner.y.saturating_add(offset)),
    );
    let buttons_width = inner.width.saturating_sub(2);
    let alarm_width = buttons_width / 2;
    let countdown_width = buttons_width.saturating_sub(alarm_width);
    let create_alarm = Rect::new(inner.x, button_y, alarm_width, u16::from(inner.height > 0));
    let create_countdown = Rect::new(
        inner.x.saturating_add(alarm_width).saturating_add(2),
        button_y,
        countdown_width,
        u16::from(inner.height > 0),
    );

    ClockCreateDialogLayout {
        dialog,
        input,
        error,
        create_alarm,
        create_countdown,
    }
}

fn inset_rect(area: Rect, margin: u16) -> Rect {
    let doubled = margin.saturating_mul(2);
    Rect::new(
        area.x.saturating_add(margin.min(area.width)),
        area.y.saturating_add(margin.min(area.height)),
        area.width.saturating_sub(doubled),
        area.height.saturating_sub(doubled),
    )
}

fn line_in_rect(area: Rect, y: u16) -> Rect {
    if area.width == 0 || area.height == 0 || y < area.y || y >= area.y.saturating_add(area.height)
    {
        return Rect::new(area.x, area.y.saturating_add(area.height), 0, 0);
    }
    Rect::new(area.x, y, area.width, 1)
}

fn usize_to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
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
