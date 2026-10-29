use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::layout::{UserManagementColumnMode, UserManagementLayout, user_management_layout};
use super::model::{
    UserManagementFeedbackTone, UserManagementFocus, UserManagementUserViewModel,
    UserManagementViewModel,
};
use crate::components::{Button, ComponentTone, DataTable, Scrollbar, Surface};
use crate::screens::clock::render_clock_line;
use crate::screens::shell::fit_cell;
use crate::{RenderContext, TundraTheme};
pub fn render_user_management_content(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &UserManagementViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let layout = user_management_layout(main, model);
    Surface::new()
        .titled(i18n::tr!("ui-user-management-user-management"))
        .bordered(false)
        .render_frame(frame, layout.panel, context);

    render_clock_line(
        frame,
        layout.summary,
        i18n::tr!(
            "ui-user-management-summary",
            user = model.current_user.clone(),
            count = model.users.len()
        ),
        theme.body_style(),
        HorizontalAlignment::Left,
    );
    render_user_management_table(frame, &layout, model, context);
    if !layout.details.is_empty() {
        Surface::new()
            .bordered(true)
            .titled(i18n::tr!("management-touch-details"))
            .render_frame(frame, layout.details, context);
        let details = model
            .selected_user()
            .map(|user| {
                [
                    (
                        i18n::tr!("ui-user-management-username"),
                        user.username.clone(),
                    ),
                    (
                        i18n::tr!("ui-user-management-display-name"),
                        user.display_name.clone(),
                    ),
                    (i18n::tr!("ui-user-management-role"), user.role.clone()),
                    (
                        i18n::tr!("ui-user-management-status"),
                        user_management_status(user),
                    ),
                ]
                .into_iter()
                .map(|(label, value)| format!("{label}: {value}"))
                .collect::<Vec<_>>()
                .join("\n")
            })
            .unwrap_or_else(|| i18n::tr!("ui-user-management-no-user-selected"));
        frame.render_widget(
            Paragraph::new(details)
                .wrap(Wrap { trim: false })
                .style(theme.body_style()),
            crate::screens::shell::inset_rect(layout.details, 1),
        );
    }
    if !layout.actions_panel.is_empty() {
        Surface::new()
            .bordered(true)
            .titled(i18n::tr!("management-touch-actions"))
            .render_frame(frame, layout.actions_panel, context);
    }
    if let Some(track) = layout.scrollbar {
        Scrollbar::new(
            model.users.len(),
            layout.visible_capacity,
            layout.visible_start,
        )
        .render_frame(frame, track, context);
    }
    render_user_management_feedback(frame, &layout, model, theme);
    super::layout::user_management_action_bar(model).render(frame, layout.action_bar.area, context);
}

fn render_user_management_table(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    model: &UserManagementViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let widths = user_management_column_widths(layout.header.width, layout.column_mode);
    let headers = user_management_header_cells(layout.column_mode, &widths);
    let rows = model
        .users
        .iter()
        .map(|user| user_management_user_cells(user, layout.column_mode, &widths))
        .collect::<Vec<_>>();
    let tones = model
        .users
        .iter()
        .map(|user| {
            if !user.enabled || user.locked {
                ComponentTone::Warning
            } else {
                ComponentTone::Default
            }
        })
        .collect();
    let mut table = DataTable::new("user-management.users", headers.clone(), rows)
        .with_column_widths(widths)
        .with_viewport_start(layout.visible_start)
        .with_row_tones(tones)
        .bordered(false);
    table.row_ids = model
        .users
        .iter()
        .map(|user| user.username.clone())
        .collect();
    table.selected = Some(model.selected_index);
    table.state.focused = model.focus == UserManagementFocus::UserList;
    let table_area = Rect::new(
        layout.header.x,
        layout.header.y,
        layout.header.width,
        layout.header.height.saturating_add(layout.rows_area.height),
    );
    table.render_frame(frame, table_area, context);
    let columns = user_management_sort_headers(layout);
    let labels = vec![
        i18n::tr!("ui-user-management-header-username"),
        i18n::tr!("ui-user-management-header-display-name"),
        i18n::tr!("ui-user-management-role"),
        i18n::tr!("ui-user-management-status"),
    ];
    crate::render_table_headers(
        frame,
        &columns,
        &labels,
        model.sort,
        "user-management",
        context,
    );

    if layout.rows.is_empty() && model.users.is_empty() {
        let empty = Rect::new(
            layout.rows_area.x,
            layout.rows_area.y,
            layout.rows_area.width,
            u16::from(layout.rows_area.height > 0),
        );
        render_clock_line(
            frame,
            empty,
            i18n::tr!("ui-user-management-no-users-available-padded"),
            theme.muted_style(),
            HorizontalAlignment::Left,
        );
    }
}

fn render_user_management_feedback(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    let (text, style) = if let Some(message) = &model.message {
        let style = match model.feedback_tone {
            UserManagementFeedbackTone::Info => theme.body_style(),
            UserManagementFeedbackTone::Success => theme.title_style(),
            UserManagementFeedbackTone::Error => theme.error_style(),
        };
        (message.clone(), style)
    } else if let UserManagementFocus::Action(focused) = model.focus {
        let Some(reason) = model
            .actions
            .iter()
            .find(|action| action.action == focused && !action.enabled)
            .and_then(|action| action.disabled_reason.clone())
        else {
            return;
        };
        (reason, theme.muted_style())
    } else {
        return;
    };
    render_clock_line(
        frame,
        layout.feedback,
        text,
        style,
        HorizontalAlignment::Left,
    );
}

fn user_management_status(user: &UserManagementUserViewModel) -> String {
    let mut status = if !user.enabled {
        i18n::tr!("ui-user-management-disabled")
    } else if user.locked {
        i18n::tr!("ui-user-management-locked")
    } else {
        i18n::tr!("ui-user-management-enabled")
    };
    if user.is_current {
        status = i18n::tr!("ui-user-management-current-status", status = status);
    }
    status
}

fn user_management_column_widths(width: u16, mode: UserManagementColumnMode) -> Vec<u16> {
    let width = usize::from(width);
    let marker_width = 2_usize.min(width);
    let available = width.saturating_sub(marker_width);
    match mode {
        UserManagementColumnMode::Detailed => {
            let separators = 3_usize.min(available);
            let cells = available.saturating_sub(separators);
            let role_width = 10.min(cells / 4);
            let status_width = 18.min(cells.saturating_sub(role_width) / 2);
            let names_width = cells
                .saturating_sub(role_width)
                .saturating_sub(status_width);
            let username_width = names_width / 2;
            let display_width = names_width.saturating_sub(username_width);
            vec![username_width, display_width, role_width, status_width]
        }
        UserManagementColumnMode::Account => {
            let separators = 2_usize.min(available);
            let cells = available.saturating_sub(separators);
            let role_width = 9.min(cells / 3);
            let status_width = 16.min(cells.saturating_sub(role_width) / 2);
            let account_width = cells
                .saturating_sub(role_width)
                .saturating_sub(status_width);
            vec![account_width, role_width, status_width]
        }
    }
    .into_iter()
    .map(|width| u16::try_from(width).unwrap_or(u16::MAX))
    .collect()
}

fn user_management_header_cells(mode: UserManagementColumnMode, widths: &[u16]) -> Vec<String> {
    let labels = match mode {
        UserManagementColumnMode::Detailed => vec![
            i18n::tr!("ui-user-management-header-username"),
            i18n::tr!("ui-user-management-header-display-name"),
            i18n::tr!("ui-user-management-role"),
            i18n::tr!("ui-user-management-status"),
        ],
        UserManagementColumnMode::Account => vec![
            i18n::tr!("ui-user-management-account"),
            i18n::tr!("ui-user-management-role"),
            i18n::tr!("ui-user-management-status"),
        ],
    };
    labels
        .into_iter()
        .zip(widths)
        .map(|(label, width)| fit_cell(&label, usize::from(*width)))
        .collect()
}

fn user_management_user_cells(
    user: &UserManagementUserViewModel,
    mode: UserManagementColumnMode,
    widths: &[u16],
) -> Vec<String> {
    let status = user_management_status(user);
    let values = match mode {
        UserManagementColumnMode::Detailed => vec![
            user.username.clone(),
            user.display_name.clone(),
            user.role.clone(),
            status,
        ],
        UserManagementColumnMode::Account => {
            let account = if user.display_name.is_empty() || user.display_name == user.username {
                user.username.clone()
            } else {
                format!("{} — {}", user.username, user.display_name)
            };
            vec![account, user.role.clone(), status]
        }
    };
    values
        .into_iter()
        .zip(widths)
        .map(|(value, width)| fit_cell(&value, usize::from(*width)))
        .collect()
}

pub fn render_user_management_overlay(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &UserManagementViewModel,
    context: &RenderContext,
) {
    let layout = user_management_layout(main, model);
    if let Some(menu) = layout.more_menu {
        frame.render_widget(Clear, menu);
        Surface::new()
            .bordered(true)
            .raised(true)
            .titled(i18n::tr!("ui-user-management-more"))
            .render_frame(frame, menu, context);
        let theme = context.compatibility_theme();
        for action_layout in &layout.actions {
            let action = model
                .visible_actions()
                .into_iter()
                .find(|action| action.action == action_layout.action)
                .unwrap();
            let mut button = Button::new(
                format!("user-management.action.{:?}", action.action),
                super::layout::user_management_action_label(&action),
            )
            .with_focus_fill(Some(context.theme.accent_soft))
            .with_emphasis(if action.dangerous {
                crate::components::ButtonEmphasis::Danger
            } else {
                crate::components::ButtonEmphasis::Normal
            });
            button.set_disabled(!action.enabled);
            button.state.selected = model.focus == UserManagementFocus::Action(action.action);
            button.render_borderless_frame(frame, action_layout.area, &theme);
        }
        return;
    }
    let layout = user_management_layout(main, model);
    if let (Some(form_layout), Some(form)) = (layout.form.as_ref(), model.form.as_ref()) {
        form.dialog().render_frame(frame, form_layout.main, context);
    }
}

pub fn user_management_sort_headers(layout: &UserManagementLayout) -> Vec<(usize, Rect)> {
    let widths = user_management_column_widths(layout.header.width, layout.column_mode);
    crate::table_header_areas(layout.header, &widths)
        .into_iter()
        .map(|(index, area)| {
            (
                if layout.column_mode == UserManagementColumnMode::Account && index > 0 {
                    index + 1
                } else {
                    index
                },
                area,
            )
        })
        .collect()
}
