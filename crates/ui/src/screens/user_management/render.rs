use ratatui::Frame;
use ratatui::layout::{Constraint, HorizontalAlignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Borders, Clear, HighlightSpacing, Row, Table, TableState};

use super::layout::{
    UserManagementColumnMode, UserManagementFormLayout, UserManagementLayout,
    user_management_layout,
};
use super::model::{
    UserManagementFeedbackTone, UserManagementField, UserManagementFocus, UserManagementFormKind,
    UserManagementFormViewModel, UserManagementUserViewModel, UserManagementViewModel,
};
use crate::components::{Button, TextInput};
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
        HorizontalAlignment::Left,
    );
    render_user_management_table(frame, &layout, model, theme);
    render_user_management_feedback(frame, &layout, model, theme);
    render_user_management_actions(frame, &layout, model, theme);
    render_clock_line(
        frame,
        layout.help,
        "↑↓ Select · Tab Actions · Enter Activate · Esc Back".to_string(),
        theme.muted_style(),
        HorizontalAlignment::Left,
    );

    if let (Some(form_layout), Some(form)) = (layout.form.as_ref(), model.form.as_ref()) {
        render_user_management_form(frame, form_layout, form, theme);
    }
}

fn render_user_management_table(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    let widths = user_management_column_widths(layout.header.width, layout.column_mode);
    let header = Row::new(user_management_header_cells(layout.column_mode, &widths))
        .style(theme.title_style());
    let rows = layout
        .rows
        .iter()
        .filter_map(|row| {
            let user = model.users.get(row.index)?;
            Some(
                Row::new(user_management_user_cells(
                    user,
                    layout.column_mode,
                    &widths,
                ))
                .style(theme.body_style()),
            )
        })
        .collect::<Vec<_>>();
    let selected = layout
        .rows
        .iter()
        .position(|row| row.index == model.selected_index);
    let table = Table::new(rows, widths.iter().copied().map(Constraint::Length))
        .header(header)
        .style(theme.body_style())
        .column_spacing(1)
        .highlight_symbol("> ")
        .highlight_spacing(HighlightSpacing::Always)
        .row_highlight_style(theme.title_style());
    let table_area = Rect::new(
        layout.header.x,
        layout.header.y,
        layout.header.width,
        layout.header.height.saturating_add(layout.rows_area.height),
    );
    let mut state = TableState::default().with_selected(selected);
    frame.render_stateful_widget(table, table_area, &mut state);

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
        let mut button = Button::new(
            format!("user-management.action.{:?}", action.action),
            fit_cell(
                &action.button_label(),
                usize::from(action_layout.area.width),
            ),
        );
        button.set_disabled(!action.enabled);
        button.set_focused(focused);
        button.state.hovered = focused;

        let mut button_theme = *theme;
        if action.dangerous && action.enabled && !focused {
            button_theme.foreground = theme.error;
        }
        button.render_inline_frame(frame, action_layout.area, &button_theme);
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
        HorizontalAlignment::Left,
    );

    for field in &layout.fields {
        let input = match field.field {
            UserManagementField::Username => Some((
                "user-management.form.username",
                "Username",
                form.username.clone(),
            )),
            UserManagementField::DisplayName => Some((
                "user-management.form.display-name",
                "Display name",
                form.display_name.clone(),
            )),
            UserManagementField::Password => Some((
                "user-management.form.password",
                "Password",
                "*".repeat(form.password_len),
            )),
            UserManagementField::Role => None,
            UserManagementField::Submit | UserManagementField::Cancel => continue,
        };
        if let Some((id, label, value)) = input {
            render_user_management_input(
                frame,
                field.area,
                id,
                label,
                value,
                form.focused_field == field.field,
                theme,
            );
        } else {
            render_user_management_button(
                frame,
                field.area,
                "user-management.form.role",
                fit_cell(
                    &format!("Role: {}  ◀/▶", form.role),
                    usize::from(field.area.width),
                ),
                form.focused_field == field.field,
                theme,
            );
        }
    }
    if let Some(error) = &form.error {
        render_clock_line(
            frame,
            layout.error,
            error.clone(),
            theme.error_style(),
            HorizontalAlignment::Left,
        );
    }
    render_user_management_button(
        frame,
        layout.submit,
        "user-management.form.submit",
        format!("[ {} ]", form.submit_label()),
        form.focused_field == UserManagementField::Submit,
        theme,
    );
    render_user_management_button(
        frame,
        layout.cancel,
        "user-management.form.cancel",
        "[ Cancel ]",
        form.focused_field == UserManagementField::Cancel,
        theme,
    );
}

fn render_user_management_input(
    frame: &mut Frame<'_>,
    area: Rect,
    id: &'static str,
    label: &'static str,
    value: String,
    focused: bool,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let prefix = format!("[ {label}: ");
    let mut input = TextInput::new(id).with_cursor_symbol("_");
    input.set_value(&value);
    input.set_focused(focused);
    input.state.hovered = focused;
    let mut input_theme = *theme;
    input_theme.muted = theme.foreground;

    if area.width <= 2 {
        input.render_borderless_frame_with_prefix(frame, area, &input_theme, &prefix);
        return;
    }
    let input_area = Rect::new(area.x, area.y, area.width.saturating_sub(2), area.height);
    input.render_borderless_frame_with_prefix(frame, input_area, &input_theme, &prefix);

    let prefix_width = Line::from(prefix.as_str()).width();
    let input_capacity = usize::from(input_area.width).saturating_sub(prefix_width);
    let visible_value_width = Line::from(value.as_str())
        .width()
        .saturating_add(usize::from(focused))
        .min(input_capacity);
    let suffix_x = area
        .x
        .saturating_add(u16::try_from(prefix_width).unwrap_or(u16::MAX))
        .saturating_add(u16::try_from(visible_value_width).unwrap_or(u16::MAX))
        .min(area.right().saturating_sub(2));
    render_clock_line(
        frame,
        Rect::new(
            suffix_x,
            area.y,
            area.right().saturating_sub(suffix_x),
            area.height,
        ),
        " ]".to_string(),
        if focused {
            theme.title_style()
        } else {
            theme.body_style()
        },
        HorizontalAlignment::Left,
    );
}

fn render_user_management_button(
    frame: &mut Frame<'_>,
    area: Rect,
    id: &'static str,
    label: impl Into<String>,
    focused: bool,
    theme: &TundraTheme,
) {
    let mut button = Button::new(id, label);
    button.set_focused(focused);
    button.state.hovered = focused;
    button.render_inline_frame(frame, area, theme);
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
        UserManagementColumnMode::Detailed => vec!["USERNAME", "DISPLAY NAME", "ROLE", "STATUS"],
        UserManagementColumnMode::Account => vec!["ACCOUNT", "ROLE", "STATUS"],
    };
    labels
        .into_iter()
        .zip(widths)
        .map(|(label, width)| fit_cell(label, usize::from(*width)))
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
