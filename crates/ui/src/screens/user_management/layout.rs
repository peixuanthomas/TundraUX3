use ratatui::layout::Rect;

use super::model::{
    UserManagementAction, UserManagementActionViewModel, UserManagementField, UserManagementFocus,
    UserManagementFormViewModel, UserManagementViewModel,
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
    pub action_bar: crate::components::PageActionBarLayout,
    pub more_menu: Option<Rect>,
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
        if self.more_menu.is_some() {
            return None;
        }
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
        let bar = user_management_action_bar(model);
        let action_height = bar
            .height(right.width.saturating_sub(2))
            .saturating_add(2)
            .min(right.height);
        layout.details = Rect::new(
            right.x,
            right.y,
            right.width,
            right.height.saturating_sub(action_height),
        );
        layout.actions_panel =
            Rect::new(right.x, layout.details.bottom(), right.width, action_height);
        layout.actions_area = inset_rect(layout.actions_panel, 1);
        layout.action_bar = bar.layout(layout.actions_area);
        layout.actions = user_management_page_action_layouts(model, &layout.action_bar);
        apply_more_menu(&mut layout, main, model);
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

    let help = Rect::new(inner.x, inner_bottom, 0, 0);
    let bar = user_management_action_bar(model);
    let action_height = bar.height(inner.width).min(inner.height);
    let actions_y = inner_bottom.saturating_sub(action_height);
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

    let action_bar = bar.layout(actions_area);
    let actions = user_management_page_action_layouts(model, &action_bar);

    let mut layout = UserManagementLayout {
        panel,
        summary,
        header,
        rows_area,
        rows,
        details: Rect::default(),
        actions_panel: Rect::default(),
        actions_area,
        actions,
        action_bar,
        more_menu: None,
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
    };
    apply_more_menu(&mut layout, main, model);
    layout
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

pub(super) fn user_management_action_bar(
    model: &UserManagementViewModel,
) -> crate::components::PageActionBar {
    use crate::components::{PageAction, PageActionBar};
    let primary = if model.selected_user().is_some() {
        UserManagementAction::EditInfo
    } else {
        UserManagementAction::NewUser
    };
    let actions = model
        .page_actions()
        .into_iter()
        .map(|action| {
            let label = user_management_action_label(&action);
            let page = PageAction::new(format!("action.{:?}", action.action), label)
                .disabled(!action.enabled);
            if action.dangerous {
                page.danger()
            } else if action.action == primary {
                page.primary()
            } else {
                page
            }
        })
        .collect();
    let selected = match model.focus {
        UserManagementFocus::Action(action) => Some(format!("action.{action:?}").into()),
        _ => None,
    };
    PageActionBar::new("user-management")
        .with_actions(actions)
        .with_selected(selected)
}

pub(super) fn user_management_action_label(action: &UserManagementActionViewModel) -> String {
    action.shortcut.map_or_else(
        || action.label.clone(),
        |key| format!("{} {}", key.to_ascii_uppercase(), action.label),
    )
}

fn user_management_page_action_layouts(
    model: &UserManagementViewModel,
    bar: &crate::components::PageActionBarLayout,
) -> Vec<UserManagementActionLayout> {
    let actions = model.page_actions();
    bar.actions
        .iter()
        .map(|area| UserManagementActionLayout {
            action: actions[area.index].action,
            area: area.area,
            enabled: area.enabled,
        })
        .collect()
}

fn apply_more_menu(layout: &mut UserManagementLayout, main: Rect, model: &UserManagementViewModel) {
    if !model.more_open {
        return;
    }
    let actions = model.visible_actions();
    let width = actions
        .iter()
        .map(|action| {
            crate::components::terminal_width(&user_management_action_label(action)) as u16 + 6
        })
        .max()
        .unwrap_or(0)
        .min(main.width);
    let area = centered_rect(main, width, (actions.len() as u16 + 4).min(main.height));
    layout.more_menu = Some(area);
    let content = inset_rect(area, 1);
    layout.actions = actions
        .iter()
        .enumerate()
        .map(|(index, action)| UserManagementActionLayout {
            action: action.action,
            area: Rect::new(
                content.x + 1,
                content.y + 1 + index as u16,
                content.width.saturating_sub(2),
                1,
            ),
            enabled: action.enabled,
        })
        .collect();
}

fn user_management_form_layout(
    area: Rect,
    form: &UserManagementFormViewModel,
) -> UserManagementFormLayout {
    let desired_height =
        (form.field_order().len() as u16).saturating_add(3 + u16::from(form.error.is_some()));
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
