use ratatui::layout::Rect;

use crate::view_model::{
    ClockEntryViewModel, ClockViewModel, DiagnosticsRepairDialogViewModel, DiagnosticsTab,
    DiagnosticsViewModel, ExplorerConflictChoice, ExplorerOverlayViewModel, ExplorerSortColumn,
    ExplorerToolbarAction, ExplorerViewModel, NotificationActionViewModel, NotificationViewModel,
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
const DIAGNOSTICS_LIST_MIN_WIDTH: u16 = 28;
const DIAGNOSTICS_LIST_MAX_WIDTH: u16 = 44;
const DIAGNOSTICS_REPAIR_DIALOG_WIDTH: u16 = 68;
const DIAGNOSTICS_REPAIR_DIALOG_MAX_HEIGHT: u16 = 13;
const EXPLORER_SIDEBAR_MIN_WIDTH: u16 = 96;
const EXPLORER_DETAILED_MIN_WIDTH: u16 = 72;
const EXPLORER_SIDEBAR_WIDTH: u16 = 20;
const EXPLORER_FOOTER_TALL_HEIGHT: u16 = 5;

pub const MIN_SHELL_TERMINAL_WIDTH: u16 = 50;
pub const MIN_SHELL_TERMINAL_HEIGHT: u16 = 12;
pub const NOTIFICATION_TOO_SMALL_MESSAGE: &str =
    "Terminal is too small to render this notification.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLayout {
    Compact(Rect),
    Full { top: Rect, main: Rect, status: Rect },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsTabLayout {
    pub tab: DiagnosticsTab,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsRowLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsScrollbarLayout {
    pub track: Rect,
    pub thumb: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsRepairDialogLayout {
    pub dialog: Rect,
    pub prompt: Rect,
    pub items_area: Rect,
    pub rows: Vec<DiagnosticsRowLayout>,
    pub help: Rect,
    pub confirm: Rect,
    pub cancel: Rect,
    pub visible_start: usize,
    pub visible_capacity: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticsHitTarget {
    Tab(DiagnosticsTab),
    Check(usize),
    Incident(usize),
    RepairItem(usize),
    RepairConfirm,
    RepairCancel,
    RepairDialogSurface,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsLayout {
    pub panel: Rect,
    pub active_tab: DiagnosticsTab,
    pub header: Rect,
    pub tabs_area: Rect,
    pub tabs: Vec<DiagnosticsTabLayout>,
    pub list_panel: Rect,
    pub list_rows_area: Rect,
    pub list_scrollbar: Option<DiagnosticsScrollbarLayout>,
    pub rows: Vec<DiagnosticsRowLayout>,
    pub detail_panel: Rect,
    pub footer: Rect,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub repair_dialog: Option<DiagnosticsRepairDialogLayout>,
}

impl DiagnosticsLayout {
    pub fn hit_test(&self, x: u16, y: u16) -> Option<DiagnosticsHitTarget> {
        if let Some(dialog) = &self.repair_dialog {
            if let Some(row) = dialog.rows.iter().find(|row| rect_contains(row.area, x, y)) {
                return Some(DiagnosticsHitTarget::RepairItem(row.index));
            }
            if rect_contains(dialog.confirm, x, y) {
                return Some(DiagnosticsHitTarget::RepairConfirm);
            }
            if rect_contains(dialog.cancel, x, y) {
                return Some(DiagnosticsHitTarget::RepairCancel);
            }
            return rect_contains(dialog.dialog, x, y)
                .then_some(DiagnosticsHitTarget::RepairDialogSurface);
        }

        if let Some(tab) = self.tabs.iter().find(|tab| rect_contains(tab.area, x, y)) {
            return Some(DiagnosticsHitTarget::Tab(tab.tab));
        }
        self.rows
            .iter()
            .find(|row| rect_contains(row.area, x, y))
            .map(|row| match self.active_tab {
                DiagnosticsTab::Health => DiagnosticsHitTarget::Check(row.index),
                DiagnosticsTab::Incidents => DiagnosticsHitTarget::Incident(row.index),
            })
    }
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
    if area.width < MIN_SHELL_TERMINAL_WIDTH || area.height < MIN_SHELL_TERMINAL_HEIGHT {
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

/// Computes the two-column Diagnostics page and modal hit geometry.
///
/// Callers should pass the `main` rectangle from [`compute_shell_layout`]. The
/// returned window start is clamped so the active selection remains visible.
pub fn diagnostics_layout(main: Rect, model: &DiagnosticsViewModel) -> DiagnosticsLayout {
    let panel = main;
    let inner = inset_rect(panel, 1);
    let inner_bottom = inner.y.saturating_add(inner.height);
    let header = line_in_rect(inner, inner.y);
    let tabs_area = line_in_rect(inner, inner.y.saturating_add(header.height));
    let footer = line_in_rect(inner, inner_bottom.saturating_sub(1));
    let content_y = tabs_area.y.saturating_add(tabs_area.height);
    let content_height = footer.y.saturating_sub(content_y);
    let content = Rect::new(inner.x, content_y, inner.width, content_height);

    let column_gap = u16::from(content.width >= 3);
    let available_width = content.width.saturating_sub(column_gap);
    let list_width = if available_width >= DIAGNOSTICS_LIST_MIN_WIDTH.saturating_mul(2) {
        (available_width.saturating_mul(2) / 5)
            .clamp(DIAGNOSTICS_LIST_MIN_WIDTH, DIAGNOSTICS_LIST_MAX_WIDTH)
    } else {
        available_width / 2
    };
    let detail_width = available_width.saturating_sub(list_width);
    let list_panel = Rect::new(content.x, content.y, list_width, content.height);
    let detail_panel = Rect::new(
        content
            .x
            .saturating_add(list_width)
            .saturating_add(column_gap),
        content.y,
        detail_width,
        content.height,
    );
    let list_inner = inset_rect(list_panel, 1);
    let visible_capacity = usize::from(list_inner.height);
    let visible_start = diagnostics_visible_start(
        model.item_count(),
        model.selected_index(),
        model.list_window_start,
        visible_capacity,
    );
    let list_scrollbar = diagnostics_scrollbar_layout(
        list_inner,
        model.item_count(),
        visible_start,
        visible_capacity,
    );
    let list_rows_area = if list_scrollbar.is_some() {
        Rect::new(
            list_inner.x,
            list_inner.y,
            list_inner.width.saturating_sub(2),
            list_inner.height,
        )
    } else {
        list_inner
    };
    let rows = (visible_start..model.item_count())
        .take(visible_capacity)
        .enumerate()
        .map(|(offset, index)| DiagnosticsRowLayout {
            index,
            area: Rect::new(
                list_rows_area.x,
                list_rows_area.y.saturating_add(usize_to_u16(offset)),
                list_rows_area.width,
                1,
            ),
        })
        .collect();

    let mut tab_x = tabs_area.x;
    let tabs = DiagnosticsTab::ALL
        .into_iter()
        .map(|tab| {
            let desired = usize_to_u16(tab.label().chars().count()).saturating_add(4);
            let width = desired.min(tabs_area.right().saturating_sub(tab_x));
            let area = Rect::new(tab_x, tabs_area.y, width, tabs_area.height);
            tab_x = tab_x.saturating_add(width);
            DiagnosticsTabLayout { tab, area }
        })
        .collect();

    DiagnosticsLayout {
        panel,
        active_tab: model.tab,
        header,
        tabs_area,
        tabs,
        list_panel,
        list_rows_area,
        list_scrollbar,
        rows,
        detail_panel,
        footer,
        visible_start,
        visible_capacity,
        repair_dialog: model
            .repair_dialog
            .as_ref()
            .map(|dialog| diagnostics_repair_dialog_layout(main, dialog)),
    }
}

fn diagnostics_scrollbar_layout(
    list_inner: Rect,
    item_count: usize,
    visible_start: usize,
    visible_capacity: usize,
) -> Option<DiagnosticsScrollbarLayout> {
    if item_count <= visible_capacity || list_inner.width < 3 || list_inner.height == 0 {
        return None;
    }

    let track = Rect::new(
        list_inner.right().saturating_sub(1),
        list_inner.y,
        1,
        list_inner.height,
    );
    let track_height = usize::from(track.height);
    let visible_count = visible_capacity.min(item_count);
    let thumb_height = track_height
        .saturating_mul(visible_count)
        .saturating_add(item_count / 2)
        .checked_div(item_count)
        .unwrap_or_default()
        .max(1)
        .min(track_height);
    let max_thumb_start = track_height.saturating_sub(thumb_height);
    let max_visible_start = item_count.saturating_sub(visible_count);
    let thumb_start = if max_visible_start == 0 {
        0
    } else {
        visible_start
            .min(max_visible_start)
            .saturating_mul(max_thumb_start)
            .saturating_add(max_visible_start / 2)
            / max_visible_start
    };
    let thumb = Rect::new(
        track.x,
        track.y.saturating_add(usize_to_u16(thumb_start)),
        1,
        usize_to_u16(thumb_height),
    );

    Some(DiagnosticsScrollbarLayout { track, thumb })
}

pub fn diagnostics_hit_test(
    layout: &DiagnosticsLayout,
    coordinates: (u16, u16),
) -> Option<DiagnosticsHitTarget> {
    layout.hit_test(coordinates.0, coordinates.1)
}

fn diagnostics_visible_start(
    item_count: usize,
    selected_index: usize,
    requested_start: usize,
    visible_capacity: usize,
) -> usize {
    if item_count == 0 || visible_capacity == 0 {
        return 0;
    }

    let selected = selected_index.min(item_count.saturating_sub(1));
    let max_start = item_count.saturating_sub(visible_capacity);
    let mut start = requested_start.min(max_start);
    if selected < start {
        start = selected;
    } else if selected >= start.saturating_add(visible_capacity) {
        start = selected.saturating_add(1).saturating_sub(visible_capacity);
    }
    start.min(max_start)
}

fn diagnostics_repair_dialog_layout(
    main: Rect,
    model: &DiagnosticsRepairDialogViewModel,
) -> DiagnosticsRepairDialogLayout {
    let desired_height = usize_to_u16(model.items.len().min(6))
        .saturating_add(7)
        .min(DIAGNOSTICS_REPAIR_DIALOG_MAX_HEIGHT);
    let dialog = centered_rect(
        main,
        main.width.min(DIAGNOSTICS_REPAIR_DIALOG_WIDTH),
        main.height.min(desired_height),
    );
    let inner = inset_rect(dialog, 1);
    let inner_bottom = inner.y.saturating_add(inner.height);
    let prompt_height = inner.height.min(2);
    let prompt = Rect::new(inner.x, inner.y, inner.width, prompt_height);
    let button_y = inner_bottom.saturating_sub(1);
    let help_y = button_y.saturating_sub(1).max(prompt.bottom());
    let help = line_in_rect(inner, help_y);
    let items_y = prompt.bottom();
    let items_area = Rect::new(
        inner.x,
        items_y,
        inner.width,
        help_y.saturating_sub(items_y),
    );
    let visible_capacity = usize::from(items_area.height);
    let visible_start = diagnostics_visible_start(
        model.items.len(),
        model.selected,
        model.scroll_offset,
        visible_capacity,
    );
    let rows = (visible_start..model.items.len())
        .take(visible_capacity)
        .enumerate()
        .map(|(offset, index)| DiagnosticsRowLayout {
            index,
            area: Rect::new(
                items_area.x,
                items_area.y.saturating_add(usize_to_u16(offset)),
                items_area.width,
                1,
            ),
        })
        .collect();
    let button_gap = u16::from(inner.width >= 3).saturating_mul(2);
    let buttons_width = inner.width.saturating_sub(button_gap);
    let confirm_width = buttons_width / 2;
    let cancel_width = buttons_width.saturating_sub(confirm_width);
    let button_height = u16::from(inner.height > 0);
    let confirm = Rect::new(inner.x, button_y, confirm_width, button_height);
    let cancel = Rect::new(
        inner
            .x
            .saturating_add(confirm_width)
            .saturating_add(button_gap),
        button_y,
        cancel_width,
        button_height,
    );

    DiagnosticsRepairDialogLayout {
        dialog,
        prompt,
        items_area,
        rows,
        help,
        confirm,
        cancel,
        visible_start,
        visible_capacity,
    }
}

/// Computes the User Management table, action row, and modal hit geometry.
///
/// The returned window start is clamped so the selected user is visible. Input
/// routing should use `visible_start` rather than duplicating this calculation.
pub fn user_management_layout(main: Rect, model: &UserManagementViewModel) -> UserManagementLayout {
    let panel = main;
    let inner = inset_rect(panel, 1);
    let summary = line_in_rect(inner, inner.y);
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

    let actions = user_management_action_layouts(actions_area, &model.actions);

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
    let new_button = if model.is_read_only() {
        Rect::new(panel_inner.x, panel_inner.y, 0, 0)
    } else {
        line_in_rect(panel_inner, panel_inner.y)
    };
    let condensed_panel = panel_inner.height < 7;
    let reserved_lines = if model.is_read_only() {
        2
    } else if condensed_panel {
        3
    } else {
        4
    };
    let entry_capacity = usize::from(panel_inner.height.saturating_sub(reserved_lines));
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
        panel_inner.y.saturating_add(if model.is_read_only() {
            0
        } else if condensed_panel {
            1
        } else {
            2
        }),
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
        create_dialog: (!model.is_read_only())
            .then_some(model.create_dialog.as_ref())
            .flatten()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerLayoutMode {
    DetailedWithSidebar,
    Detailed,
    Compact,
}

impl ExplorerLayoutMode {
    pub const fn shows_all_columns(self) -> bool {
        !matches!(self, Self::Compact)
    }

    pub const fn shows_sidebar(self) -> bool {
        matches!(self, Self::DetailedWithSidebar)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerToolbarButtonLayout {
    pub action: ExplorerToolbarAction,
    pub area: Rect,
    pub show_label: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerBreadcrumbLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerQuickLocationLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerColumnLayout {
    pub column: ExplorerSortColumn,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerRowLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerOverlayControl {
    ContextItem(usize),
    NameInput,
    Confirm,
    Cancel,
    Option(usize),
    OptionsClose,
    ConflictChoice(ExplorerConflictChoice),
    ApplyToRemaining,
    PropertiesClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOverlayControlLayout {
    pub control: ExplorerOverlayControl,
    pub area: Rect,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOverlayLayout {
    pub area: Rect,
    pub content: Rect,
    pub controls: Vec<ExplorerOverlayControlLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerHitTarget {
    Toolbar(ExplorerToolbarAction),
    Address,
    Breadcrumb(usize),
    Search,
    QuickLocation(usize),
    Column(ExplorerSortColumn),
    Entry(usize),
    Scrollbar,
    CancelOperation,
    Overlay(ExplorerOverlayControl),
    OverlaySurface,
    EmptyTable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerLayout {
    pub mode: ExplorerLayoutMode,
    pub panel: Rect,
    pub toolbar: Rect,
    pub toolbar_buttons: Vec<ExplorerToolbarButtonLayout>,
    pub path_bar: Rect,
    pub address_button: Rect,
    pub address_input: Rect,
    pub breadcrumbs: Vec<ExplorerBreadcrumbLayout>,
    pub search: Rect,
    pub sidebar: Option<Rect>,
    pub sidebar_header: Option<Rect>,
    pub quick_locations: Vec<ExplorerQuickLocationLayout>,
    pub table: Rect,
    pub table_header: Rect,
    pub columns: Vec<ExplorerColumnLayout>,
    pub table_body: Rect,
    pub rows: Vec<ExplorerRowLayout>,
    pub scrollbar: Option<Rect>,
    pub footer: Rect,
    pub cancel_operation: Option<Rect>,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub overlay: Option<ExplorerOverlayLayout>,
}

impl ExplorerLayout {
    pub fn hit_test(&self, x: u16, y: u16) -> Option<ExplorerHitTarget> {
        if let Some(overlay) = &self.overlay {
            if let Some(control) = overlay
                .controls
                .iter()
                .find(|control| control.enabled && rect_contains(control.area, x, y))
            {
                return Some(ExplorerHitTarget::Overlay(control.control.clone()));
            }
            return rect_contains(overlay.area, x, y).then_some(ExplorerHitTarget::OverlaySurface);
        }

        if let Some(button) = self
            .toolbar_buttons
            .iter()
            .find(|button| button.enabled && rect_contains(button.area, x, y))
        {
            return Some(ExplorerHitTarget::Toolbar(button.action));
        }
        if rect_contains(self.search, x, y) {
            return Some(ExplorerHitTarget::Search);
        }
        if rect_contains(self.address_button, x, y) {
            return Some(ExplorerHitTarget::Address);
        }
        if let Some(crumb) = self
            .breadcrumbs
            .iter()
            .find(|crumb| rect_contains(crumb.area, x, y))
        {
            return Some(ExplorerHitTarget::Breadcrumb(crumb.index));
        }
        if let Some(location) = self
            .quick_locations
            .iter()
            .find(|location| rect_contains(location.area, x, y))
        {
            return Some(ExplorerHitTarget::QuickLocation(location.index));
        }
        if let Some(column) = self
            .columns
            .iter()
            .find(|column| rect_contains(column.area, x, y))
        {
            return Some(ExplorerHitTarget::Column(column.column));
        }
        if let Some(row) = self.rows.iter().find(|row| rect_contains(row.area, x, y)) {
            return Some(ExplorerHitTarget::Entry(row.index));
        }
        if self
            .scrollbar
            .is_some_and(|scrollbar| rect_contains(scrollbar, x, y))
        {
            return Some(ExplorerHitTarget::Scrollbar);
        }
        if self
            .cancel_operation
            .is_some_and(|cancel| rect_contains(cancel, x, y))
        {
            return Some(ExplorerHitTarget::CancelOperation);
        }
        rect_contains(self.table_body, x, y).then_some(ExplorerHitTarget::EmptyTable)
    }

    pub fn row_index_at(&self, x: u16, y: u16) -> Option<usize> {
        self.rows
            .iter()
            .find_map(|row| rect_contains(row.area, x, y).then_some(row.index))
    }
}

/// Computes the complete Explorer render and mouse-hit geometry.
///
/// Widths are intentionally based on the page area rather than terminal-global
/// coordinates: `>=96` enables the 20-cell quick-access sidebar, `>=72` keeps
/// every metadata column, and narrower pages show only Name and Type.
pub fn explorer_layout(area: Rect, model: &ExplorerViewModel) -> ExplorerLayout {
    let mode = if area.width >= EXPLORER_SIDEBAR_MIN_WIDTH && model.show_sidebar {
        ExplorerLayoutMode::DetailedWithSidebar
    } else if area.width >= EXPLORER_DETAILED_MIN_WIDTH {
        ExplorerLayoutMode::Detailed
    } else {
        ExplorerLayoutMode::Compact
    };
    let panel = area;
    let inner = inset_rect(panel, 1);
    let toolbar = line_in_rect(inner, inner.y);
    let path_bar = line_in_rect(inner, inner.y.saturating_add(1));
    let toolbar_buttons = explorer_toolbar_button_layouts(toolbar, model, mode);

    let search_width = if path_bar.width >= 96 {
        32
    } else if path_bar.width >= 72 {
        26
    } else {
        18.min(path_bar.width / 2)
    }
    .min(path_bar.width);
    let search = Rect::new(
        path_bar
            .x
            .saturating_add(path_bar.width.saturating_sub(search_width)),
        path_bar.y,
        search_width,
        path_bar.height,
    );
    let address_area = Rect::new(
        path_bar.x,
        path_bar.y,
        path_bar.width.saturating_sub(search_width),
        path_bar.height,
    );
    let address_button_width = 6.min(address_area.width);
    let address_gap = u16::from(address_area.width > address_button_width);
    let address_button = Rect::new(
        address_area.x,
        address_area.y,
        address_button_width,
        address_area.height,
    );
    let address_input = Rect::new(
        address_area
            .x
            .saturating_add(address_button_width)
            .saturating_add(address_gap),
        address_area.y,
        address_area
            .width
            .saturating_sub(address_button_width)
            .saturating_sub(address_gap),
        address_area.height,
    );
    let breadcrumbs = if model.address_editing {
        Vec::new()
    } else {
        explorer_breadcrumb_layouts(address_input, model)
    };

    let remaining_height = inner.height.saturating_sub(2);
    let footer_height = if remaining_height >= 11 {
        EXPLORER_FOOTER_TALL_HEIGHT
    } else if remaining_height >= 4 {
        2
    } else {
        u16::from(remaining_height > 0)
    };
    let body_y = inner.y.saturating_add(2);
    let body_height = remaining_height.saturating_sub(footer_height);
    let body = Rect::new(inner.x, body_y, inner.width, body_height);
    let footer = Rect::new(
        inner.x,
        body_y.saturating_add(body_height),
        inner.width,
        footer_height,
    );

    let (sidebar, table) = if mode.shows_sidebar() {
        let sidebar_width = EXPLORER_SIDEBAR_WIDTH.min(body.width);
        let gap = u16::from(body.width > sidebar_width);
        (
            Some(Rect::new(body.x, body.y, sidebar_width, body.height)),
            Rect::new(
                body.x.saturating_add(sidebar_width).saturating_add(gap),
                body.y,
                body.width.saturating_sub(sidebar_width).saturating_sub(gap),
                body.height,
            ),
        )
    } else {
        (None, body)
    };
    let sidebar_header = sidebar.map(|sidebar| line_in_rect(sidebar, sidebar.y));
    let quick_locations = sidebar.map_or_else(Vec::new, |sidebar| {
        model
            .quick_locations
            .iter()
            .enumerate()
            .take(usize::from(sidebar.height.saturating_sub(1)))
            .map(|(index, _)| ExplorerQuickLocationLayout {
                index,
                area: Rect::new(
                    sidebar.x,
                    sidebar
                        .y
                        .saturating_add(1)
                        .saturating_add(usize_to_u16(index)),
                    sidebar.width,
                    1,
                ),
            })
            .collect()
    });

    let table_header = line_in_rect(table, table.y);
    let table_body = Rect::new(
        table.x,
        table.y.saturating_add(table_header.height),
        table.width,
        table.height.saturating_sub(table_header.height),
    );
    let visible_capacity = usize::from(table_body.height);
    let visible_start = explorer_visible_start(model, visible_capacity);
    let show_scrollbar = visible_capacity > 0 && model.entries.len() > visible_capacity;
    let scrollbar = show_scrollbar.then(|| {
        Rect::new(
            table_body
                .x
                .saturating_add(table_body.width.saturating_sub(1)),
            table_body.y,
            u16::from(table_body.width > 0),
            table_body.height,
        )
    });
    let row_width = table_body.width.saturating_sub(u16::from(show_scrollbar));
    let rows = (visible_start..model.entries.len())
        .take(visible_capacity)
        .enumerate()
        .map(|(offset, index)| ExplorerRowLayout {
            index,
            area: Rect::new(
                table_body.x,
                table_body.y.saturating_add(usize_to_u16(offset)),
                row_width,
                1,
            ),
        })
        .collect();
    let columns = explorer_column_layouts(
        Rect::new(
            table_header.x,
            table_header.y,
            row_width,
            table_header.height,
        ),
        mode,
    );

    let cancel_operation = model.operation.as_ref().and_then(|operation| {
        if !operation.cancellable || footer.width == 0 || footer.height == 0 {
            return None;
        }
        let desired = usize_to_u16(operation.cancel_label.chars().count())
            .saturating_add(4)
            .min(footer.width);
        Some(Rect::new(
            footer
                .x
                .saturating_add(footer.width.saturating_sub(desired)),
            footer.y,
            desired,
            1,
        ))
    });
    let overlay = explorer_overlay_layout(area, model);

    ExplorerLayout {
        mode,
        panel,
        toolbar,
        toolbar_buttons,
        path_bar,
        address_button,
        address_input,
        breadcrumbs,
        search,
        sidebar,
        sidebar_header,
        quick_locations,
        table,
        table_header,
        columns,
        table_body,
        rows,
        scrollbar,
        footer,
        cancel_operation,
        visible_start,
        visible_capacity,
        overlay,
    }
}

pub fn explorer_hit_test(
    layout: &ExplorerLayout,
    coordinates: (u16, u16),
) -> Option<ExplorerHitTarget> {
    layout.hit_test(coordinates.0, coordinates.1)
}

fn explorer_toolbar_button_layouts(
    area: Rect,
    model: &ExplorerViewModel,
    mode: ExplorerLayoutMode,
) -> Vec<ExplorerToolbarButtonLayout> {
    let compact = matches!(mode, ExplorerLayoutMode::Compact);
    let labelled_width = model
        .toolbar
        .buttons
        .iter()
        .map(|button| usize_to_u16(button.label.chars().count()).saturating_add(4))
        .fold(0u16, u16::saturating_add)
        .saturating_add(model.toolbar.buttons.len().saturating_sub(1) as u16);
    // Either label every action or collapse every action to its asset icon.  A greedy mix made
    // the important trailing Rename/Delete/Sort/Options controls disappear at common widths.
    let show_labels = !compact && labelled_width <= area.width;
    let mut x = area.x;
    model
        .toolbar
        .buttons
        .iter()
        .filter_map(|button| {
            let remaining = area.x.saturating_add(area.width).saturating_sub(x);
            if remaining < 3 || area.height == 0 {
                return None;
            }
            let labelled_width = usize_to_u16(button.label.chars().count()).saturating_add(4);
            let show_label = show_labels;
            let width = if show_label { labelled_width } else { 3 }.min(remaining);
            let layout = ExplorerToolbarButtonLayout {
                action: button.action,
                area: Rect::new(x, area.y, width, 1),
                show_label,
                enabled: button.enabled,
            };
            x = x
                .saturating_add(width)
                .saturating_add(u16::from(remaining > width));
            Some(layout)
        })
        .collect()
}

fn explorer_breadcrumb_layouts(
    area: Rect,
    model: &ExplorerViewModel,
) -> Vec<ExplorerBreadcrumbLayout> {
    let mut x = area.x;
    model
        .breadcrumbs
        .iter()
        .enumerate()
        .filter_map(|(index, crumb)| {
            let remaining = area.x.saturating_add(area.width).saturating_sub(x);
            if remaining == 0 || area.height == 0 {
                return None;
            }
            let desired = usize_to_u16(crumb.label.chars().count()).saturating_add(3);
            let width = desired.min(remaining);
            let result = ExplorerBreadcrumbLayout {
                index,
                area: Rect::new(x, area.y, width, 1),
            };
            x = x.saturating_add(width);
            Some(result)
        })
        .collect()
}

fn explorer_visible_start(model: &ExplorerViewModel, capacity: usize) -> usize {
    if model.entries.is_empty() || capacity == 0 {
        return 0;
    }
    let max_start = model.entries.len().saturating_sub(capacity);
    let mut start = model.viewport_offset.min(max_start);
    if model.viewport_follows_focus
        && let Some(focused) = model
            .selected_index
            .filter(|index| *index < model.entries.len())
    {
        if focused < start {
            start = focused;
        } else if focused >= start.saturating_add(capacity) {
            start = focused.saturating_add(1).saturating_sub(capacity);
        }
    }
    start.min(max_start)
}

fn explorer_column_layouts(area: Rect, mode: ExplorerLayoutMode) -> Vec<ExplorerColumnLayout> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let widths = if mode.shows_all_columns() {
        let kind = 16.min(area.width);
        let size = 12.min(area.width.saturating_sub(kind));
        let modified = 18.min(area.width.saturating_sub(kind).saturating_sub(size));
        let name = area
            .width
            .saturating_sub(kind)
            .saturating_sub(size)
            .saturating_sub(modified);
        vec![
            (ExplorerSortColumn::Name, name),
            (ExplorerSortColumn::Type, kind),
            (ExplorerSortColumn::Size, size),
            (ExplorerSortColumn::Modified, modified),
        ]
    } else {
        let kind = 16.min(area.width / 3).max(u16::from(area.width > 0));
        vec![
            (ExplorerSortColumn::Name, area.width.saturating_sub(kind)),
            (ExplorerSortColumn::Type, kind),
        ]
    };
    let mut x = area.x;
    widths
        .into_iter()
        .filter_map(|(column, width)| {
            if width == 0 {
                return None;
            }
            let result = ExplorerColumnLayout {
                column,
                area: Rect::new(x, area.y, width, 1),
            };
            x = x.saturating_add(width);
            Some(result)
        })
        .collect()
}

fn explorer_overlay_layout(area: Rect, model: &ExplorerViewModel) -> Option<ExplorerOverlayLayout> {
    match model.overlay.as_ref() {
        Some(ExplorerOverlayViewModel::ContextMenu(menu)) => {
            let content_width = menu
                .items
                .iter()
                .map(|item| {
                    item.label.chars().count().saturating_add(
                        item.shortcut
                            .as_ref()
                            .map_or(0, |value| value.chars().count().saturating_add(2)),
                    )
                })
                .max()
                .unwrap_or(12);
            let width = usize_to_u16(content_width)
                .saturating_add(4)
                .clamp(16, area.width.max(16));
            let height = usize_to_u16(menu.items.len())
                .saturating_add(2)
                .min(area.height);
            let x = menu
                .x
                .min(area.x.saturating_add(area.width.saturating_sub(width)));
            let y = menu
                .y
                .min(area.y.saturating_add(area.height.saturating_sub(height)));
            let dialog = Rect::new(x, y, width.min(area.width), height);
            let content = inset_rect(dialog, 1);
            let controls = menu
                .items
                .iter()
                .enumerate()
                .take(usize::from(content.height))
                .map(|(index, item)| ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::ContextItem(index),
                    area: Rect::new(
                        content.x,
                        content.y.saturating_add(usize_to_u16(index)),
                        content.width,
                        1,
                    ),
                    enabled: item.enabled,
                })
                .collect();
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls,
            })
        }
        Some(ExplorerOverlayViewModel::Name(_)) => {
            let dialog = centered_rect(area, area.width.min(60), area.height.min(9));
            let content = inset_rect(dialog, 1);
            let button_y = content.y.saturating_add(content.height.saturating_sub(1));
            let gap = u16::from(content.width > 2);
            let button_width = content.width.saturating_sub(gap) / 2;
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls: vec![
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::NameInput,
                        area: line_in_rect(content, content.y.saturating_add(2)),
                        enabled: true,
                    },
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::Confirm,
                        area: Rect::new(content.x, button_y, button_width, 1),
                        enabled: true,
                    },
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::Cancel,
                        area: Rect::new(
                            content.x.saturating_add(button_width).saturating_add(gap),
                            button_y,
                            content
                                .width
                                .saturating_sub(button_width)
                                .saturating_sub(gap),
                            1,
                        ),
                        enabled: true,
                    },
                ],
            })
        }
        Some(ExplorerOverlayViewModel::Options(options)) => {
            let desired_height = usize_to_u16(options.options.len()).saturating_add(4);
            let dialog = centered_rect(area, area.width.min(64), area.height.min(desired_height));
            let content = inset_rect(dialog, 1);
            let option_capacity = usize::from(content.height.saturating_sub(1));
            let mut controls = options
                .options
                .iter()
                .enumerate()
                .take(option_capacity)
                .map(|(index, option)| ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::Option(index),
                    area: Rect::new(
                        content.x,
                        content.y.saturating_add(usize_to_u16(index)),
                        content.width,
                        1,
                    ),
                    enabled: option.enabled,
                })
                .collect::<Vec<_>>();
            controls.push(ExplorerOverlayControlLayout {
                control: ExplorerOverlayControl::OptionsClose,
                area: line_in_rect(
                    content,
                    content.y.saturating_add(content.height.saturating_sub(1)),
                ),
                enabled: true,
            });
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls,
            })
        }
        Some(ExplorerOverlayViewModel::Conflict(conflict)) => {
            let dialog = centered_rect(area, area.width.min(68), area.height.min(10));
            let content = inset_rect(dialog, 1);
            let choices_y = content
                .y
                .saturating_add(4.min(content.height.saturating_sub(1)));
            let count = usize_to_u16(conflict.choices.len()).max(1);
            let choice_width = content.width / count;
            let mut controls = conflict
                .choices
                .iter()
                .enumerate()
                .map(|(index, choice)| ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::ConflictChoice(*choice),
                    area: Rect::new(
                        content
                            .x
                            .saturating_add(usize_to_u16(index).saturating_mul(choice_width)),
                        choices_y,
                        if index + 1 == conflict.choices.len() {
                            content
                                .width
                                .saturating_sub(usize_to_u16(index).saturating_mul(choice_width))
                        } else {
                            choice_width
                        },
                        1,
                    ),
                    enabled: true,
                })
                .collect::<Vec<_>>();
            if conflict.allow_apply_to_remaining {
                controls.push(ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::ApplyToRemaining,
                    area: line_in_rect(content, choices_y.saturating_add(1)),
                    enabled: true,
                });
            }
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls,
            })
        }
        Some(ExplorerOverlayViewModel::Properties(_)) => {
            let dialog = centered_rect(area, area.width.min(64), area.height.min(12));
            let content = inset_rect(dialog, 1);
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls: vec![ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::PropertiesClose,
                    area: line_in_rect(
                        content,
                        content.y.saturating_add(content.height.saturating_sub(1)),
                    ),
                    enabled: true,
                }],
            })
        }
        None if model.pending_dialog.is_some() => {
            let dialog = centered_rect(area, area.width.min(56), area.height.min(7));
            let content = inset_rect(dialog, 1);
            let button_y = content.y.saturating_add(content.height.saturating_sub(1));
            let gap = u16::from(content.width > 2);
            let confirm_width = content.width.saturating_sub(gap) / 2;
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls: vec![
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::Confirm,
                        area: Rect::new(content.x, button_y, confirm_width, 1),
                        enabled: true,
                    },
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::Cancel,
                        area: Rect::new(
                            content.x.saturating_add(confirm_width).saturating_add(gap),
                            button_y,
                            content
                                .width
                                .saturating_sub(confirm_width)
                                .saturating_sub(gap),
                            1,
                        ),
                        enabled: true,
                    },
                ],
            })
        }
        None => None,
    }
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
