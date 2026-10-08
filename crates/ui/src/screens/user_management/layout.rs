use ratatui::layout::Rect;

use super::model::{
    UserManagementAction, UserManagementActionViewModel, UserManagementField, UserManagementFocus,
    UserManagementFormKind, UserManagementFormViewModel, UserManagementViewModel,
};
use crate::screens::shell::{centered_rect, inset_rect, line_in_rect, rect_contains, usize_to_u16};

const USER_MANAGEMENT_DETAILED_MIN_WIDTH: u16 = 72;
const USER_MANAGEMENT_DIALOG_WIDTH: u16 = 60;
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
    pub scrollbar: Option<Rect>,
    pub field_window_start: usize,
    pub field_capacity: usize,
    pub field_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserManagementLayout {
    pub panel: Rect,
    pub summary: Rect,
    pub header: Rect,
    pub rows_area: Rect,
    pub rows: Vec<UserManagementRowLayout>,
    pub details: Rect,
    pub actions_panel: Rect,
    pub actions_area: Rect,
    pub actions: Vec<UserManagementActionLayout>,
    pub feedback: Rect,
    pub help: Rect,
    pub column_mode: UserManagementColumnMode,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub form: Option<UserManagementFormLayout>,
    pub scrollbar: Option<Rect>,
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

/// Computes the User Management table, action row, and modal hit geometry.
///
/// The returned window start is clamped so the selected user is visible. Input
/// routing should use `visible_start` rather than duplicating this calculation.
pub fn user_management_layout(main: Rect, model: &UserManagementViewModel) -> UserManagementLayout {
    if main.width >= 100 && main.height >= 14 {
        let left_width = main.width / 2;
        let mut table_model = model.clone();
        if table_model.message.is_none() {
            if let UserManagementFocus::Action(focused) = model.focus {
                table_model.message = model
                    .actions
                    .iter()
                    .find(|action| action.action == focused && !action.enabled)
                    .and_then(|action| action.disabled_reason.clone());
            }
        }
        table_model.actions.clear();
        let mut layout = user_management_compact_layout(
            Rect::new(main.x, main.y, left_width, main.height),
            &table_model,
        );
        layout.panel = main;
        layout.summary.width = main.width.saturating_sub(2);
        layout.help.width = main.width.saturating_sub(2);
        layout.feedback.width = if layout.feedback.height > 0 {
            main.width.saturating_sub(2)
        } else {
            0
        };
        let bottom = if layout.feedback.height > 0 {
            layout.feedback.y
        } else {
            layout.help.y
        };
        let right = Rect::new(
            main.x + left_width,
            layout.header.y,
            main.width.saturating_sub(left_width + 1),
            bottom.saturating_sub(layout.header.y),
        );
        let columns = usize::from(right.width.saturating_sub(2) / 22).clamp(1, 2);
        let button_rows = model.actions.len().div_ceil(columns) as u16;
        let action_height = button_rows.saturating_add(2).min(right.height);
        layout.details = Rect::new(
            right.x,
            right.y,
            right.width,
            right.height.saturating_sub(action_height),
        );
        layout.actions_panel =
            Rect::new(right.x, layout.details.bottom(), right.width, action_height);
        layout.actions_area = inset_rect(layout.actions_panel, 1);
        layout.actions = user_management_action_layouts_with_columns(
            layout.actions_area,
            &model.actions,
            columns,
        );
        layout.form = model
            .form
            .as_ref()
            .map(|form| user_management_form_layout(main, form));
        return layout;
    }
    user_management_compact_layout(main, model)
}

fn user_management_compact_layout(
    main: Rect,
    model: &UserManagementViewModel,
) -> UserManagementLayout {
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
    let action_columns = if inner.height < 5 && inner.width >= 60 {
        model.actions.len().max(1)
    } else {
        usize::from(inner.width / 14)
            .max(1)
            .min(model.actions.len().max(1))
    };
    let action_height = u16::try_from(model.actions.len().div_ceil(action_columns))
        .unwrap_or(u16::MAX)
        .min(inner.height);
    let actions_y = inner_bottom
        .saturating_sub(action_height)
        .saturating_sub(u16::from(show_help));
    let actions_area = Rect::new(
        inner.x,
        actions_y.max(inner.y),
        inner.width,
        action_height.min(inner_bottom.saturating_sub(actions_y.max(inner.y))),
    );
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
    let scrollbar =
        (model.users.len() > usize::from(rows_height) && inner.width > 1 && rows_height > 0)
            .then(|| Rect::new(inner.right() - 1, rows_y, 1, rows_height));
    let rows_area = Rect::new(
        inner.x,
        rows_y,
        inner.width.saturating_sub(u16::from(scrollbar.is_some())),
        rows_height,
    );
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
        details: Rect::default(),
        actions_panel: Rect::default(),
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
        scrollbar,
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

    let max_start = model.users.len().saturating_sub(visible_capacity);
    model.user_window_start.min(max_start)
}

fn user_management_action_layouts(
    area: Rect,
    actions: &[UserManagementActionViewModel],
) -> Vec<UserManagementActionLayout> {
    if area.width == 0 || area.height == 0 || actions.is_empty() {
        return Vec::new();
    }

    let columns = if area.height == 1 {
        actions.len()
    } else {
        usize::from(area.width / 14).max(1).min(actions.len())
    };
    user_management_action_layouts_with_columns(area, actions, columns)
}

fn user_management_action_layouts_with_columns(
    area: Rect,
    actions: &[UserManagementActionViewModel],
    columns: usize,
) -> Vec<UserManagementActionLayout> {
    let count = u16::try_from(columns).unwrap_or(u16::MAX);
    let gap = u16::from(area.width >= count.saturating_mul(2).saturating_sub(1));
    let gaps_width = count.saturating_sub(1).saturating_mul(gap);
    let available = area.width.saturating_sub(gaps_width);
    let base_width = available / count.max(1);
    let remainder = available % count.max(1);

    actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let column = index % columns;
            let extra = u16::from(usize::from(remainder) > column);
            let width = base_width.saturating_add(extra);
            let x = area.x + (base_width + gap) * column as u16 + (column as u16).min(remainder);
            let y = area.y + (index / columns) as u16;
            let action_area = Rect::new(x, y, width, u16::from(y < area.bottom()));
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
    let field_count = input_fields.len();
    let field_capacity = usize::from(buttons_y.saturating_sub(inner.y + 1));
    let focused_index = input_fields
        .iter()
        .position(|field| *field == form.focused_field)
        .unwrap_or(0);
    let field_window_start = focused_index
        .saturating_add(1)
        .saturating_sub(field_capacity.max(1))
        .min(field_count.saturating_sub(field_capacity));
    let scrollbar = (field_count > field_capacity && field_capacity > 0 && inner.width > 1)
        .then(|| Rect::new(inner.right() - 1, inner.y + 1, 1, field_capacity as u16));
    let fields = input_fields
        .into_iter()
        .skip(field_window_start)
        .enumerate()
        .filter_map(|(index, field)| {
            let y = inner
                .y
                .saturating_add(1)
                .saturating_add(usize_to_u16(index));
            (y < buttons_y).then_some(UserManagementFieldLayout {
                field,
                area: Rect::new(
                    inner.x,
                    y,
                    inner.width.saturating_sub(u16::from(scrollbar.is_some())),
                    1,
                ),
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
        scrollbar,
        field_window_start,
        field_capacity,
        field_count,
    }
}
