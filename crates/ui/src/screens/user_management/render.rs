use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::Clear;

use super::layout::{
    UserManagementColumnMode, UserManagementFormLayout, UserManagementLayout,
    user_management_layout,
};
use super::model::{
    UserManagementFeedbackTone, UserManagementField, UserManagementFocus, UserManagementFormKind,
    UserManagementFormViewModel, UserManagementUserViewModel, UserManagementViewModel,
};
use crate::components::{Button, ComponentTone, DataTable, Surface, TextInput};
use crate::screens::clock::render_clock_line;
use crate::screens::shell::{fit_cell, render_compact_home, render_status, render_top};
use crate::{RenderContext, ShellChromeViewModel, ShellLayout, TundraTheme, compute_shell_layout};
pub fn render_user_management(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_user_management_contextual(frame, area, chrome, model, &context);
}

pub fn render_user_management_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &UserManagementViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_user_management_main(frame, main, model, theme, context);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_user_management_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    let layout = user_management_layout(main, model);
    Surface::new()
        .titled("User Management")
        .bordered(true)
        .render_frame(frame, layout.panel, context);

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
    render_user_management_table(frame, &layout, model, context);
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
        render_user_management_form(frame, form_layout, form, theme, context);
    }
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
    let mut table = DataTable::new("user-management.users", headers, rows)
        .with_column_widths(widths)
        .with_viewport_start(layout.visible_start)
        .with_row_tones(tones)
        .bordered(false);
    table.selected = Some(model.selected_index);
    table.state.focused = model.focus == UserManagementFocus::UserList;
    let table_area = Rect::new(
        layout.header.x,
        layout.header.y,
        layout.header.width,
        layout.header.height.saturating_add(layout.rows_area.height),
    );
    table.render_frame(frame, table_area, context);

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
    context: &RenderContext,
) {
    frame.render_widget(Clear, layout.dialog);
    if !layout.compact {
        Surface::new()
            .titled(form.title.clone())
            .bordered(true)
            .raised(true)
            .render_frame(frame, layout.dialog, context);
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
