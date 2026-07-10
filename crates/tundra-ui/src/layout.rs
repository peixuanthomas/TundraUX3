use ratatui::layout::Rect;

use crate::view_model::{
    ClockEntryViewModel, ClockViewModel, NotificationActionViewModel, NotificationViewModel,
    UserManagementAction, UserManagementActionViewModel, UserManagementField, UserManagementFocus,
    UserManagementFormKind, UserManagementFormViewModel, UserManagementViewModel,
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
const USER_MANAGEMENT_DETAILED_MIN_WIDTH: u16 = 72;
const USER_MANAGEMENT_DIALOG_WIDTH: u16 = 60;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserManagementColumnMode {
    Detailed,
    Account,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserManagementRowLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserManagementActionLayout {
    pub action: UserManagementAction,
    pub area: Rect,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserManagementFieldLayout {
    pub field: UserManagementField,
    pub area: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserManagementFormLayout {
    pub dialog: Rect,
    pub compact: bool,
    pub prompt: Rect,
    pub fields: Vec<UserManagementFieldLayout>,
    pub error: Rect,
    pub submit: Rect,
    pub cancel: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserManagementLayout {
    pub panel: Rect,
    pub summary: Rect,
    pub header: Rect,
    pub rows_area: Rect,
    pub rows: Vec<UserManagementRowLayout>,
    pub actions_area: Rect,
    pub actions: Vec<UserManagementActionLayout>,
    pub feedback: Rect,
    pub help: Rect,
    pub column_mode: UserManagementColumnMode,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub form: Option<UserManagementFormLayout>,
}

impl UserManagementLayout {
    pub fn row_index_at(&self, x: u16, y: u16) -> Option<usize> {
        self.rows
            .iter()
            .find_map(|row| rect_contains(row.area, x, y).then_some(row.index))
    }

    pub fn action_at(&self, x: u16, y: u16) -> Option<UserManagementAction> {
        self.actions
            .iter()
            .find_map(|action| rect_contains(action.area, x, y).then_some(action.action))
    }

    pub fn form_control_at(&self, x: u16, y: u16) -> Option<UserManagementField> {
        let form = self.form.as_ref()?;
        form.fields
            .iter()
            .find_map(|field| rect_contains(field.area, x, y).then_some(field.field))
            .or_else(|| rect_contains(form.submit, x, y).then_some(UserManagementField::Submit))
            .or_else(|| rect_contains(form.cancel, x, y).then_some(UserManagementField::Cancel))
    }
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

/// Computes the User Management table, action row, and modal hit geometry.
///
/// The returned window start is clamped so the selected user is visible. Input
/// routing should use `visible_start` rather than duplicating this calculation.
pub fn user_management_layout(main: Rect, model: &UserManagementViewModel) -> UserManagementLayout {
    let panel = main;
    let inner = inset_rect(panel, 1);
    let summary_line = line_in_rect(inner, inner.y);
    let new_user_action = model
        .actions
        .iter()
        .find(|action| action.action == UserManagementAction::NewUser);
    let (summary, new_user_layout) = new_user_action.map_or((summary_line, None), |action| {
        let desired_width = u16::try_from(action.button_label().chars().count())
            .unwrap_or(u16::MAX)
            .min(summary_line.width);
        let gap = u16::from(summary_line.width > desired_width);
        let summary_width = summary_line
            .width
            .saturating_sub(desired_width)
            .saturating_sub(gap);
        let action_area = Rect::new(
            summary_line
                .x
                .saturating_add(summary_width)
                .saturating_add(gap),
            summary_line.y,
            desired_width,
            summary_line.height,
        );
        (
            Rect::new(
                summary_line.x,
                summary_line.y,
                summary_width,
                summary_line.height,
            ),
            Some(UserManagementActionLayout {
                action: action.action,
                area: action_area,
                enabled: action.enabled,
            }),
        )
    });
    let header = line_in_rect(inner, inner.y.saturating_add(1));
    let inner_bottom = inner.y.saturating_add(inner.height);

    let show_help = inner.height >= 5;
    let help_y = inner_bottom.saturating_sub(1);
    let help = if show_help {
        line_in_rect(inner, help_y)
    } else {
        Rect::new(inner.x, inner_bottom, 0, 0)
    };
    let actions_y = inner_bottom
        .saturating_sub(1)
        .saturating_sub(u16::from(show_help));
    let actions_area = line_in_rect(inner, actions_y);
    let show_feedback = (model.message.is_some()
        || matches!(model.focus, UserManagementFocus::Action(action) if model
            .actions
            .iter()
            .any(|candidate| candidate.action == action && !candidate.enabled && candidate.disabled_reason.is_some())))
        && actions_y > header.y.saturating_add(header.height).saturating_add(1);
    let feedback_y = actions_y.saturating_sub(1);
    let feedback = if show_feedback {
        line_in_rect(inner, feedback_y)
    } else {
        Rect::new(inner.x, actions_y, 0, 0)
    };

    let rows_y = header.y.saturating_add(header.height);
    let rows_end = if show_feedback { feedback_y } else { actions_y };
    let rows_height = rows_end.saturating_sub(rows_y);
    let rows_area = Rect::new(inner.x, rows_y, inner.width, rows_height);
    let visible_capacity = usize::from(rows_height);
    let visible_start = user_management_visible_start(model, visible_capacity);
    let rows = (visible_start..model.users.len())
        .take(visible_capacity)
        .enumerate()
        .map(|(offset, index)| UserManagementRowLayout {
            index,
            area: Rect::new(
                rows_area.x,
                rows_area.y.saturating_add(usize_to_u16(offset)),
                rows_area.width,
                1,
            ),
        })
        .collect();

    let bottom_actions = model
        .actions
        .iter()
        .filter(|action| action.action != UserManagementAction::NewUser)
        .cloned()
        .collect::<Vec<_>>();
    let mut actions = user_management_action_layouts(actions_area, &bottom_actions);
    if let Some(new_user_layout) = new_user_layout {
        actions.insert(0, new_user_layout);
    }

    UserManagementLayout {
        panel,
        summary,
        header,
        rows_area,
        rows,
        actions_area,
        actions,
        feedback,
        help,
        column_mode: if main.width >= USER_MANAGEMENT_DETAILED_MIN_WIDTH {
            UserManagementColumnMode::Detailed
        } else {
            UserManagementColumnMode::Account
        },
        visible_start,
        visible_capacity,
        form: model
            .form
            .as_ref()
            .map(|form| user_management_form_layout(main, form)),
    }
}

pub fn user_management_row_index_at(
    layout: &UserManagementLayout,
    coordinates: (u16, u16),
) -> Option<usize> {
    layout.row_index_at(coordinates.0, coordinates.1)
}

pub fn user_management_action_at(
    layout: &UserManagementLayout,
    coordinates: (u16, u16),
) -> Option<UserManagementAction> {
    layout.action_at(coordinates.0, coordinates.1)
}

pub fn user_management_form_control_at(
    layout: &UserManagementLayout,
    coordinates: (u16, u16),
) -> Option<UserManagementField> {
    layout.form_control_at(coordinates.0, coordinates.1)
}

fn user_management_visible_start(
    model: &UserManagementViewModel,
    visible_capacity: usize,
) -> usize {
    if model.users.is_empty() || visible_capacity == 0 {
        return 0;
    }

    let selected = model
        .selected_index
        .min(model.users.len().saturating_sub(1));
    let max_start = model.users.len().saturating_sub(visible_capacity);
    let mut start = model.user_window_start.min(max_start);
    if selected < start {
        start = selected;
    } else if selected >= start.saturating_add(visible_capacity) {
        start = selected.saturating_add(1).saturating_sub(visible_capacity);
    }
    start.min(max_start)
}

fn user_management_action_layouts(
    area: Rect,
    actions: &[UserManagementActionViewModel],
) -> Vec<UserManagementActionLayout> {
    if area.width == 0 || area.height == 0 || actions.is_empty() {
        return Vec::new();
    }

    let count = u16::try_from(actions.len()).unwrap_or(u16::MAX);
    let gap = u16::from(area.width >= count.saturating_mul(2).saturating_sub(1));
    let gaps_width = count.saturating_sub(1).saturating_mul(gap);
    let available = area.width.saturating_sub(gaps_width);
    let base_width = available / count.max(1);
    let remainder = available % count.max(1);
    let mut x = area.x;

    actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let extra = u16::from(usize::from(remainder) > index);
            let width = base_width.saturating_add(extra);
            let action_area = Rect::new(x, area.y, width, 1);
            x = x.saturating_add(width).saturating_add(gap);
            UserManagementActionLayout {
                action: action.action,
                area: action_area,
                enabled: action.enabled,
            }
        })
        .collect()
}

fn user_management_form_layout(
    area: Rect,
    form: &UserManagementFormViewModel,
) -> UserManagementFormLayout {
    let desired_height = match form.kind {
        UserManagementFormKind::Create => 12,
        UserManagementFormKind::EditInfo | UserManagementFormKind::Password => 9,
    };
    let compact = area.height < desired_height;
    let dialog = if compact {
        area
    } else {
        centered_rect(
            area,
            area.width.min(USER_MANAGEMENT_DIALOG_WIDTH),
            desired_height,
        )
    };
    let inner = if compact {
        dialog
    } else {
        inset_rect(dialog, 1)
    };
    let prompt = line_in_rect(inner, inner.y);
    let buttons_y = inner.y.saturating_add(inner.height.saturating_sub(1));
    let input_fields = form
        .field_order()
        .iter()
        .copied()
        .filter(|field| {
            !matches!(
                field,
                UserManagementField::Submit | UserManagementField::Cancel
            )
        })
        .collect::<Vec<_>>();
    let fields = input_fields
        .into_iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let y = inner
                .y
                .saturating_add(1)
                .saturating_add(usize_to_u16(index));
            (y < buttons_y).then_some(UserManagementFieldLayout {
                field,
                area: line_in_rect(inner, y),
            })
        })
        .collect::<Vec<_>>();
    let first_unused_y = fields.last().map_or(inner.y.saturating_add(1), |field| {
        field.area.y.saturating_add(field.area.height)
    });
    let error = if form.error.is_some() && first_unused_y < buttons_y {
        line_in_rect(inner, buttons_y.saturating_sub(1).max(first_unused_y))
    } else if form.error.is_some() && compact {
        prompt
    } else {
        Rect::new(inner.x, buttons_y, 0, 0)
    };
    let gap = u16::from(inner.width >= 3);
    let button_width = inner.width.saturating_sub(gap) / 2;
    let submit = Rect::new(
        inner.x,
        buttons_y,
        button_width,
        u16::from(inner.height > 0),
    );
    let cancel = Rect::new(
        inner.x.saturating_add(button_width).saturating_add(gap),
        buttons_y,
        inner.width.saturating_sub(button_width).saturating_sub(gap),
        u16::from(inner.height > 0),
    );

    UserManagementFormLayout {
        dialog,
        compact,
        prompt,
        fields,
        error,
        submit,
        cancel,
    }
}

fn rect_contains(area: Rect, x: u16, y: u16) -> bool {
    area.width > 0
        && area.height > 0
        && x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
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
