use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::{Borders, Clear};

use super::layout::{
    UserManagementColumnMode, UserManagementFormLayout, UserManagementLayout,
    user_management_layout,
};
use super::model::{
    UserManagementFeedbackTone, UserManagementField, UserManagementFocus, UserManagementFormKind,
    UserManagementFormViewModel, UserManagementUserViewModel, UserManagementViewModel,
};
use crate::screens::clock::render_clock_line;
use crate::screens::shell::{fit_cell, render_compact_home, render_status, render_top};
use crate::{ShellChromeViewModel, ShellLayout, TundraTheme, compute_shell_layout};
pub fn render_user_management(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_user_management_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_user_management_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    let layout = user_management_layout(main, model);
    frame.render_widget(
        theme
            .block()
            .title("User Management")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.panel,
    );

    render_clock_line(
        frame,
        layout.summary,
        format!(
            "Signed in: {}    {} {}",
            model.current_user,
            model.users.len(),
            if model.users.len() == 1 {
                "user"
            } else {
                "users"
            }
        ),
        theme.body_style(),
        Alignment::Left,
    );
    render_user_management_header(frame, &layout, theme);
    render_user_management_rows(frame, &layout, model, theme);
    render_user_management_feedback(frame, &layout, model, theme);
    render_user_management_actions(frame, &layout, model, theme);
    render_clock_line(
        frame,
        layout.help,
        "↑↓ Select · Tab Actions · Enter Activate · Esc Back".to_string(),
        theme.muted_style(),
        Alignment::Left,
    );

    if let (Some(form_layout), Some(form)) = (layout.form.as_ref(), model.form.as_ref()) {
        render_user_management_form(frame, form_layout, form, theme);
    }
}

fn render_user_management_header(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    theme: &TundraTheme,
) {
    let (username, display_name) = match layout.column_mode {
        UserManagementColumnMode::Detailed => ("USERNAME", "DISPLAY NAME"),
        UserManagementColumnMode::Account => ("ACCOUNT", ""),
    };
    let text = user_management_table_line(
        layout.header.width,
        layout.column_mode,
        " ",
        username,
        display_name,
        "ROLE",
        "STATUS",
    );
    render_clock_line(
        frame,
        layout.header,
        text,
        theme.title_style(),
        Alignment::Left,
    );
}

fn render_user_management_rows(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
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
            "  No users available".to_string(),
            theme.muted_style(),
            Alignment::Left,
        );
        return;
    }

    for row in &layout.rows {
        let Some(user) = model.users.get(row.index) else {
            continue;
        };
        let selected = row.index == model.selected_index;
        let marker = if selected { ">" } else { " " };
        let status = user_management_status(user);
        let text = user_management_table_line(
            row.area.width,
            layout.column_mode,
            marker,
            &user.username,
            &user.display_name,
            &user.role,
            &status,
        );
        render_clock_line(
            frame,
            row.area,
            text,
            if selected {
                theme.title_style()
            } else {
                theme.body_style()
            },
            Alignment::Left,
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
    render_clock_line(frame, layout.feedback, text, style, Alignment::Left);
}

fn render_user_management_actions(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    for action_layout in &layout.actions {
        let Some(action) = model
            .actions
            .iter()
            .find(|action| action.action == action_layout.action)
        else {
            continue;
        };
        let focused = model.focus == UserManagementFocus::Action(action.action);
        let style = if !action.enabled {
            theme.muted_style()
        } else if focused {
            theme.title_style()
        } else if action.dangerous {
            theme.error_style()
        } else {
            theme.body_style()
        };
        render_clock_line(
            frame,
            action_layout.area,
            fit_cell(
                &action.button_label(),
                usize::from(action_layout.area.width),
            ),
            style,
            Alignment::Center,
        );
    }
}

fn render_user_management_form(
    frame: &mut Frame<'_>,
    layout: &UserManagementFormLayout,
    form: &UserManagementFormViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(Clear, layout.dialog);
    if !layout.compact {
        frame.render_widget(
            theme
                .block()
                .title(form.title.clone())
                .borders(Borders::ALL)
                .style(theme.body_style()),
            layout.dialog,
        );
    }

    let prompt = match (layout.compact, form.kind) {
        (true, UserManagementFormKind::Create) => "Create user — User or Admin account".to_string(),
        (true, _) => form.title.clone(),
        (false, UserManagementFormKind::Create) => "Create a User or Admin account.".to_string(),
        (false, UserManagementFormKind::EditInfo) => format!("Editing: {}", form.username),
        (false, UserManagementFormKind::Password) => {
            format!("Set a new password for {}.", form.username)
        }
    };
    render_clock_line(
        frame,
        layout.prompt,
        prompt,
        theme.body_style(),
        Alignment::Left,
    );

    for field in &layout.fields {
        let value = match field.field {
            UserManagementField::Username => format!("Username: {}", form.username),
            UserManagementField::DisplayName => {
                format!("Display name: {}", form.display_name)
            }
            UserManagementField::Role => format!("Role: {}  ◀/▶", form.role),
            UserManagementField::Password => {
                format!("Password: {}", "*".repeat(form.password_len))
            }
            UserManagementField::Submit | UserManagementField::Cancel => continue,
        };
        render_clock_line(
            frame,
            field.area,
            format!("[ {value} ]"),
            if form.focused_field == field.field {
                theme.title_style()
            } else {
                theme.body_style()
            },
            Alignment::Left,
        );
    }
    if let Some(error) = &form.error {
        render_clock_line(
            frame,
            layout.error,
            error.clone(),
            theme.error_style(),
            Alignment::Left,
        );
    }
    render_clock_line(
        frame,
        layout.submit,
        format!("[ {} ]", form.submit_label()),
        if form.focused_field == UserManagementField::Submit {
            theme.title_style()
        } else {
            theme.body_style()
        },
        Alignment::Center,
    );
    render_clock_line(
        frame,
        layout.cancel,
        "[ Cancel ]".to_string(),
        if form.focused_field == UserManagementField::Cancel {
            theme.title_style()
        } else {
            theme.body_style()
        },
        Alignment::Center,
    );
}

fn user_management_status(user: &UserManagementUserViewModel) -> String {
    let mut status = if !user.enabled {
        "Disabled".to_string()
    } else if user.locked {
        "Locked".to_string()
    } else {
        "Enabled".to_string()
    };
    if user.is_current {
        status.push_str(" · You");
    }
    status
}

fn user_management_table_line(
    width: u16,
    mode: UserManagementColumnMode,
    marker: &str,
    username: &str,
    display_name: &str,
    role: &str,
    status: &str,
) -> String {
    let width = usize::from(width);
    if width == 0 {
        return String::new();
    }
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
            format!(
                "{}{} {} {} {}",
                fit_cell(marker, marker_width),
                fit_cell(username, username_width),
                fit_cell(display_name, display_width),
                fit_cell(role, role_width),
                fit_cell(status, status_width),
            )
        }
        UserManagementColumnMode::Account => {
            let separators = 2_usize.min(available);
            let cells = available.saturating_sub(separators);
            let role_width = 9.min(cells / 3);
            let status_width = 16.min(cells.saturating_sub(role_width) / 2);
            let account_width = cells
                .saturating_sub(role_width)
                .saturating_sub(status_width);
            let account = if display_name.is_empty() || display_name == username {
                username.to_string()
            } else {
                format!("{username} — {display_name}")
            };
            format!(
                "{}{} {} {}",
                fit_cell(marker, marker_width),
                fit_cell(&account, account_width),
                fit_cell(role, role_width),
                fit_cell(status, status_width),
            )
        }
    }
}
