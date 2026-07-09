use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::{
    AuthField, BootstrapAdminViewModel, ExitConfirmViewModel, ExplorerDialogViewModel,
    ExplorerEntryViewModel, ExplorerSearchViewModel, ExplorerViewModel, HomeDisplayMode,
    HomeViewModel, LoginField, LoginViewModel, SetupField, SetupStep, SetupViewModel,
    ShellChromeViewModel, ShellLayout, TundraTheme, UserManagementField, UserManagementFormKind,
    UserManagementFormViewModel, UserManagementViewModel, compute_shell_layout,
    timezone_map::{TimezoneMapWidget, boundary_id_for_timezone},
};

pub const EXPLORER_HELP_LINE: &str = "Enter: open    Backspace: parent    N: folder    T: text file    R: rename    X/Delete: delete    C: copy    V: paste    /: search    H: hidden    Esc: back";
const SETUP_WIDE_MAP_MIN_WIDTH: u16 = 90;
const SETUP_WIDE_MAP_MIN_HEIGHT: u16 = 14;
const SETUP_CONTROLS_WIDTH: u16 = 48;
const SETUP_TIMEZONE_HEADER_HEIGHT: u16 = 5;
const SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT: u16 = 1;
const SETUP_TIMEZONE_BOTTOM_INDICATOR_HEIGHT: u16 = 1;
const SETUP_TIMEZONE_FOOTER_HEIGHT: u16 = 3;
const SETUP_LANGUAGE_LIST_LINE: u16 = 4;
const SETUP_ADMIN_HEADER_HEIGHT: u16 = 3;
const SETUP_ADMIN_FIELD_HEIGHT: u16 = 3;
const LOGIN_USER_LIST_WIDTH: u16 = 30;
const LOGIN_USERNAME_FIELD_HEIGHT: u16 = 5;
const LOGIN_PASSWORD_FIELD_HEIGHT: u16 = 3;
const LOGIN_FORM_GAP: u16 = 1;
const SETUP_ADMIN_CHECKLIST_HEIGHT: u16 = 7;
const SETUP_ADMIN_SIDE_CHECKLIST_MIN_WIDTH: u16 = 68;
const SETUP_ADMIN_CHECKLIST_WIDTH: u16 = 32;
const SETUP_ADMIN_COLUMN_GAP: u16 = 2;
const SETUP_ADMIN_USERNAME_LINE: u16 = 3;
const SETUP_ADMIN_PASSWORD_LINE: u16 = 7;
const SETUP_ADMIN_CONFIRM_PASSWORD_LINE: u16 = 11;
const SETUP_ADMIN_HINT_LINE: u16 = 15;
const SETUP_ADMIN_SUBMIT_LINE: u16 = 19;
const SETUP_ADMIN_ERROR_LINE: u16 = 21;
const SETUP_ADMIN_STACKED_CHECKLIST_LINE: u16 = 21;
const HOME_SUMMARY_HEIGHT: u16 = 2;
const HOME_CONTROLS_HEIGHT: u16 = 2;
const HOME_TILE_MAX_HEIGHT: u16 = 8;
const HOME_TILE_MIN_HEIGHT: u16 = 3;
const HOME_TILE_GAP: u16 = 1;

pub fn render_home(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    home: &HomeViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_main(frame, main, home, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

pub fn render_exit_confirmation(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExitConfirmViewModel,
    theme: &TundraTheme,
) {
    let dialog = centered_rect(area, area.width.min(46), area.height.min(7));
    let lines = vec![
        Line::from(model.message.clone()),
        Line::from(""),
        Line::from(format!("{}    {}", model.confirm_label, model.cancel_label)),
    ];
    let dialog_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(model.title.as_str())
                .borders(Borders::ALL)
                .style(theme.body_style()),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, dialog);
    frame.render_widget(dialog_widget, dialog);
}

pub fn render_login(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_login_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

pub fn render_bootstrap_admin(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &BootstrapAdminViewModel,
    theme: &TundraTheme,
) {
    render_auth_screen(
        frame,
        area,
        chrome,
        "Create Admin",
        bootstrap_lines(model),
        theme,
    );
}

pub fn render_setup(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_setup_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

pub fn render_user_management(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            let mut lines = vec![Line::from(format!("Current user: {}", model.current_user))];
            lines.push(Line::from(if model.can_manage_all {
                "N: new user    A: new admin    G: new debug    E: edit info    R: password    D: disable    U: enable/unlock    C: role    X/Delete: delete    Esc: back"
            } else {
                "E: edit info    R: password    X/Delete: delete account    Esc: back"
            }));
            lines.push(Line::from(""));
            for (index, user) in model.users.iter().enumerate() {
                let marker = if index == model.selected_index {
                    ">"
                } else {
                    " "
                };
                let enabled = if user.enabled { "enabled" } else { "disabled" };
                let locked = if user.locked { " locked" } else { "" };
                lines.push(Line::from(format!(
                    "{marker} {} ({}) | {} | {enabled}{locked}",
                    user.username, user.display_name, user.role
                )));
            }
            if let Some(form) = &model.form {
                lines.push(Line::from(""));
                lines.extend(user_management_form_lines(form));
            }
            if let Some(message) = &model.message {
                lines.push(Line::from(""));
                lines.push(Line::from(message.clone()));
            }
            let main_widget = Paragraph::new(lines)
                .block(
                    Block::default()
                        .title("User Management")
                        .borders(Borders::ALL)
                        .style(theme.body_style()),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(main_widget, main);
            render_status(frame, status, chrome, theme);
        }
    }
}

pub fn render_explorer(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_explorer_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);

            if let Some(dialog) = &model.pending_dialog {
                render_explorer_dialog(frame, main, dialog, theme);
            }
        }
    }
}

fn user_management_form_lines(form: &UserManagementFormViewModel) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(form.title.clone()),
        Line::from("Tab / Down: next    Shift+Tab / Up: previous    Enter: submit    Esc: cancel"),
    ];
    match form.kind {
        UserManagementFormKind::Create => {
            lines.push(Line::from(format!(
                "{}Username: {}",
                focus_marker(form.focused_field == UserManagementField::Username),
                form.username
            )));
            lines.push(Line::from(format!(
                "{}Display name: {}",
                focus_marker(form.focused_field == UserManagementField::DisplayName),
                form.display_name
            )));
            lines.push(Line::from(format!(
                "{}Password: {}",
                focus_marker(form.focused_field == UserManagementField::Password),
                "*".repeat(form.password_len)
            )));
            lines.push(Line::from(format!("Role: {}", form.role)));
        }
        UserManagementFormKind::EditInfo => {
            lines.push(Line::from(format!("Username: {}", form.username)));
            lines.push(Line::from(format!(
                "{}Display name: {}",
                focus_marker(form.focused_field == UserManagementField::DisplayName),
                form.display_name
            )));
        }
        UserManagementFormKind::Password => {
            lines.push(Line::from(format!("Username: {}", form.username)));
            lines.push(Line::from(format!(
                "{}Password: {}",
                focus_marker(form.focused_field == UserManagementField::Password),
                "*".repeat(form.password_len)
            )));
        }
    }
    lines
}

fn render_login_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    let outer = Block::default()
        .title("Login")
        .borders(Borders::ALL)
        .style(theme.body_style());
    frame.render_widget(outer, main);

    let list_area = login_user_list_area(main);
    let username_area = login_selected_username_area(main);
    let password_area = login_password_area(main);
    let form_area = login_form_area(main);

    render_login_user_list(frame, list_area, model, theme);
    render_login_username_field(frame, username_area, model, theme);
    render_login_password_field(frame, password_area, model, theme);

    let help_y = password_area
        .y
        .saturating_add(password_area.height)
        .saturating_add(LOGIN_FORM_GAP);
    let help_height = form_area
        .y
        .saturating_add(form_area.height)
        .saturating_sub(help_y);
    if help_height > 0 {
        let help_area = Rect::new(form_area.x, help_y, form_area.width, help_height);
        let mut lines = vec![
            Line::from("Users: Up/Down/Home/End    Tab: password"),
            Line::from("Enter on password: login    Esc: exit"),
        ];
        if let Some(error) = &model.error {
            lines.push(Line::from(""));
            lines.push(Line::styled(error.clone(), theme.error_style()));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme.muted_style())
                .wrap(Wrap { trim: true }),
            help_area,
        );
    }
}

fn render_login_user_list(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let visible_rows = area.height.saturating_sub(2) as usize;
    let (start, end) = login_user_window_bounds(model, visible_rows);
    let items: Vec<ListItem<'static>> = if model.users.is_empty() {
        vec![ListItem::new(Line::from("(no local users)"))]
    } else {
        model.users[start..end]
            .iter()
            .map(|user| {
                let mut suffix = String::new();
                if !user.enabled {
                    suffix.push_str(" disabled");
                }
                if user.locked {
                    suffix.push_str(" locked");
                }
                let label = if suffix.is_empty() {
                    format!("{} ({})", user.username, user.role)
                } else {
                    format!("{} ({}) |{}", user.username, user.role, suffix)
                };
                ListItem::new(Line::from(label))
            })
            .collect()
    };

    let mut state = ListState::default();
    if model.selected_index >= start && model.selected_index < end {
        state.select(Some(model.selected_index - start));
    }

    let block_style = if model.focused_field == LoginField::UserList {
        theme.title_style()
    } else {
        theme.body_style()
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title("Users")
                .borders(Borders::ALL)
                .style(block_style),
        )
        .highlight_symbol("> ")
        .highlight_style(theme.title_style());
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_login_username_field(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let selected = model.selected_user();
    let username = selected
        .map(|user| user.username.clone())
        .unwrap_or_else(|| "No user selected".to_string());
    let display = selected
        .map(|user| user.display_name.clone())
        .unwrap_or_else(|| "Choose a local account".to_string());
    let role = selected
        .map(|user| user.role.clone())
        .unwrap_or_else(|| "Unavailable".to_string());

    let lines = vec![
        Line::styled(username, theme.title_style()),
        Line::from(display),
        Line::styled(role, theme.muted_style()),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title("Selected User")
                    .borders(Borders::ALL)
                    .style(theme.body_style()),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_login_password_field(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let block_style = if model.focused_field == LoginField::Password {
        theme.title_style()
    } else {
        theme.body_style()
    };
    let password = if model.password_len == 0 {
        "Enter password".to_string()
    } else {
        "*".repeat(model.password_len)
    };
    let password_style = if model.password_len == 0 {
        theme.muted_style()
    } else {
        theme.body_style()
    };
    frame.render_widget(
        Paragraph::new(password)
            .style(password_style)
            .block(
                Block::default()
                    .title("Password")
                    .borders(Borders::ALL)
                    .style(block_style),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

pub fn login_user_list_area(main: Rect) -> Rect {
    login_columns(main).0
}

pub fn login_selected_username_area(main: Rect) -> Rect {
    let form = login_form_area(main);
    let reserved_password_height = LOGIN_PASSWORD_FIELD_HEIGHT.saturating_add(LOGIN_FORM_GAP);
    let height = if form.height > reserved_password_height {
        form.height
            .saturating_sub(reserved_password_height)
            .min(LOGIN_USERNAME_FIELD_HEIGHT)
    } else {
        0
    };

    Rect::new(form.x, form.y, form.width, height)
}

pub fn login_password_area(main: Rect) -> Rect {
    let form = login_form_area(main);
    let username = login_selected_username_area(main);
    let gap = if username.height > 0 {
        LOGIN_FORM_GAP.min(form.height.saturating_sub(username.height))
    } else {
        0
    };
    let y = username
        .y
        .saturating_add(username.height)
        .saturating_add(gap);
    let max_height = form.y.saturating_add(form.height).saturating_sub(y);
    Rect::new(
        form.x,
        y,
        form.width,
        max_height.min(LOGIN_PASSWORD_FIELD_HEIGHT),
    )
}

pub fn login_user_list_visible_rows(main: Rect) -> usize {
    login_user_list_area(main).height.saturating_sub(2) as usize
}

fn login_form_area(main: Rect) -> Rect {
    login_columns(main).1
}

fn login_columns(main: Rect) -> (Rect, Rect) {
    let inner = login_inner_area(main);
    if inner.width <= LOGIN_USER_LIST_WIDTH.saturating_add(10) {
        let [list, form] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(inner);
        return (list, form);
    }

    let [list, form] = Layout::horizontal([
        Constraint::Length(LOGIN_USER_LIST_WIDTH),
        Constraint::Min(30),
    ])
    .areas(inner);
    (list, form)
}

fn login_inner_area(main: Rect) -> Rect {
    Block::default().borders(Borders::ALL).inner(main)
}

fn login_user_window_bounds(model: &LoginViewModel, visible_rows: usize) -> (usize, usize) {
    if model.users.is_empty() || visible_rows == 0 {
        return (0, 0);
    }

    let selected = model.selected_index.min(model.users.len() - 1);
    let max_start = model.users.len().saturating_sub(visible_rows);
    let mut start = model.user_window_start.min(max_start);
    if selected < start {
        start = selected;
    } else if selected >= start.saturating_add(visible_rows) {
        start = selected.saturating_add(1).saturating_sub(visible_rows);
    }
    start = start.min(max_start);

    let end = start.saturating_add(visible_rows).min(model.users.len());
    (start, end)
}

fn render_auth_screen(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    title: &'static str,
    lines: Vec<Line<'static>>,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            let widget = Paragraph::new(lines)
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .style(theme.body_style()),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(widget, main);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_compact_home(frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
    let compact = Paragraph::new("TundraUX 3 needs at least 50x12 terminal cells.")
        .block(
            Block::default()
                .title("TundraUX 3")
                .borders(Borders::ALL)
                .style(theme.body_style()),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(compact, area);
}

fn render_setup_main(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    if model.step == SetupStep::Timezone
        && area.width >= SETUP_WIDE_MAP_MIN_WIDTH
        && area.height >= SETUP_WIDE_MAP_MIN_HEIGHT
    {
        let [controls, map] = Layout::horizontal([
            Constraint::Length(SETUP_CONTROLS_WIDTH),
            Constraint::Min(30),
        ])
        .areas(area);
        render_setup_controls(frame, area, controls, model, theme);
        render_setup_timezone_map(frame, map, model, theme);
    } else {
        render_setup_controls(frame, area, area, model, theme);
    }
}

fn render_setup_controls(
    frame: &mut Frame<'_>,
    main: Rect,
    controls: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    match model.step {
        SetupStep::Language => render_setup_language_page(frame, controls, model, theme),
        SetupStep::Timezone => render_setup_timezone_page(frame, main, controls, model, theme),
        SetupStep::Admin => render_setup_admin_page(frame, controls, model, theme),
    }
}

fn render_setup_timezone_map(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let selected_timezone = model.selected_timezone();
    let selected_timezone_id = selected_timezone.map(|timezone| timezone.id.as_str());
    let selected_boundary_id = selected_timezone_id.map(boundary_id_for_timezone);
    let mut widget = TimezoneMapWidget::themed(&[], theme)
        .selected_timezone_id(selected_timezone_id)
        .selected_boundary_id(selected_boundary_id);

    if let Some(timezone) = selected_timezone {
        widget = widget.city(timezone.longitude, timezone.latitude);
    }

    frame.render_widget(widget, area);
}

fn render_setup_language_page(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let controls = Paragraph::new(setup_language_lines(model, theme))
        .block(setup_block(theme))
        .wrap(Wrap { trim: true });

    frame.render_widget(controls, area);
}

fn render_setup_timezone_page(
    frame: &mut Frame<'_>,
    main: Rect,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(setup_block(theme), area);

    let content = setup_inner_area(area);
    let list_area = setup_timezone_list_area(main);
    let visible_rows = setup_timezone_visible_rows(main);
    let (start, end) = setup_timezone_window_bounds(model, visible_rows);

    let header = Rect::new(
        content.x,
        content.y,
        content.width,
        SETUP_TIMEZONE_HEADER_HEIGHT.min(content.height),
    );
    let top_indicator = Rect::new(
        content.x,
        list_area
            .y
            .saturating_sub(SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT),
        content.width,
        SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT.min(content.height),
    );
    let bottom_indicator = Rect::new(
        content.x,
        list_area.y.saturating_add(list_area.height),
        content.width,
        SETUP_TIMEZONE_BOTTOM_INDICATOR_HEIGHT.min(content.height),
    );
    let footer = Rect::new(
        content.x,
        content
            .y
            .saturating_add(content.height.saturating_sub(SETUP_TIMEZONE_FOOTER_HEIGHT)),
        content.width,
        SETUP_TIMEZONE_FOOTER_HEIGHT.min(content.height),
    );

    frame.render_widget(
        Paragraph::new(setup_timezone_header_lines(model, theme)),
        header,
    );
    frame.render_widget(
        Paragraph::new(setup_timezone_indicator_line(
            start > 0,
            "^ more timezones",
            theme,
        )),
        top_indicator,
    );
    frame.render_widget(
        Paragraph::new(setup_timezone_window_lines(model, start, end, theme)),
        list_area,
    );
    frame.render_widget(
        Paragraph::new(setup_timezone_indicator_line(
            end < model.timezones.len(),
            "v more timezones",
            theme,
        )),
        bottom_indicator,
    );
    frame.render_widget(
        Paragraph::new(setup_timezone_footer_lines(model, theme)).wrap(Wrap { trim: true }),
        footer,
    );
}

fn render_setup_admin_page(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(setup_block(theme), area);

    let content = setup_inner_area(area);
    let header = Rect::new(
        content.x,
        content.y,
        content.width,
        SETUP_ADMIN_HEADER_HEIGHT.min(content.height),
    );
    frame.render_widget(
        Paragraph::new(setup_admin_header_lines(model, theme)).wrap(Wrap { trim: true }),
        header,
    );

    render_setup_admin_field(
        frame,
        area,
        model,
        SetupField::AdminUsername,
        "Admin username",
        model.admin_username.clone(),
        "Enter admin username",
        theme,
    );
    render_setup_admin_field(
        frame,
        area,
        model,
        SetupField::AdminPassword,
        "Admin password",
        "*".repeat(model.admin_password_len),
        "Enter admin password",
        theme,
    );
    render_setup_admin_field(
        frame,
        area,
        model,
        SetupField::AdminPasswordConfirm,
        "Re-enter password",
        "*".repeat(model.admin_password_confirm_len),
        "Re-enter admin password",
        theme,
    );
    render_setup_admin_field(
        frame,
        area,
        model,
        SetupField::PasswordHint,
        "Password hint",
        model.password_hint.clone(),
        "Optional recovery hint, not the password",
        theme,
    );

    render_setup_password_checklist(frame, area, model, theme);

    frame.render_widget(
        Paragraph::new(setup_submit_line(model, theme)),
        setup_admin_field_area(area, SetupField::Submit),
    );

    if let Some(error) = &model.error {
        frame.render_widget(
            Paragraph::new(Line::styled(format!("Error: {error}"), theme.error_style()))
                .wrap(Wrap { trim: true }),
            setup_admin_error_area(area),
        );
    }
}

fn setup_block(theme: &TundraTheme) -> Block<'static> {
    Block::default()
        .title("First Run Setup")
        .title_style(theme.title_style())
        .borders(Borders::ALL)
        .style(theme.body_style())
}

fn setup_language_lines(model: &SetupViewModel, theme: &TundraTheme) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(
            format!("Step: {}", setup_step_label(model.step)),
            theme.title_style(),
        ),
        Line::from("Choose a language, then continue."),
        Line::styled(
            "Enter / Space: continue    Up / Down: choose    F1: help",
            theme.muted_style(),
        ),
        Line::from(""),
    ];

    if model.languages.is_empty() {
        lines.push(Line::styled(
            "  No languages available",
            theme.muted_style(),
        ));
    } else {
        for (index, language) in model.languages.iter().enumerate() {
            let text = format!(
                "{}{} ({})",
                selection_marker(index == model.selected_language_index),
                language.label,
                language.code
            );
            if index == model.selected_language_index {
                lines.push(Line::styled(text, theme.title_style()));
            } else {
                lines.push(Line::from(text));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::styled(
        selected_language_summary(model),
        theme.muted_style(),
    ));
    append_setup_error(&mut lines, model, theme);

    lines
}

fn setup_timezone_header_lines(model: &SetupViewModel, theme: &TundraTheme) -> Vec<Line<'static>> {
    vec![
        Line::styled(
            format!("Step: {}", setup_step_label(model.step)),
            theme.title_style(),
        ),
        Line::from("Choose a city or IANA zone, then continue."),
        Line::styled(
            "Enter: continue    Up / Down: choose    PgUp / PgDn: jump    F1: help",
            theme.muted_style(),
        ),
        Line::from(selected_timezone_id_summary(model)),
        Line::styled(
            selected_timezone_description_summary(model),
            theme.muted_style(),
        ),
    ]
}

fn setup_timezone_footer_lines(model: &SetupViewModel, theme: &TundraTheme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(error) = &model.error {
        lines.push(Line::styled(format!("Error: {error}"), theme.error_style()));
    }
    lines
}

fn setup_admin_header_lines(model: &SetupViewModel, theme: &TundraTheme) -> Vec<Line<'static>> {
    vec![
        Line::styled(
            format!("Step: {}", setup_step_label(model.step)),
            theme.title_style(),
        ),
        Line::from("Create the first administrator account."),
        Line::styled(
            "Tab / Down / Enter: next    Shift+Tab / Up: previous    Enter on submit: finish",
            theme.muted_style(),
        ),
    ]
}

fn render_setup_admin_field(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    field: SetupField,
    title: &'static str,
    value: String,
    placeholder: &'static str,
    theme: &TundraTheme,
) {
    let field_area = setup_admin_field_area(area, field);
    if field_area.width == 0 || field_area.height == 0 {
        return;
    }

    let focused = model.focused_field == field;
    let box_style = if focused {
        theme.title_style()
    } else {
        theme.body_style()
    };
    let block = Block::default()
        .title(title)
        .title_style(box_style)
        .borders(Borders::ALL)
        .style(box_style);
    let inner = block.inner(field_area);
    frame.render_widget(block, field_area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let is_placeholder = value.is_empty();
    let display = if is_placeholder {
        placeholder.to_string()
    } else {
        value
    };
    let text_style = if is_placeholder {
        theme.muted_style()
    } else {
        theme.body_style()
    };
    frame.render_widget(
        Paragraph::new(display).style(text_style),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
}

fn render_setup_password_checklist(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let checklist_area = setup_admin_checklist_area(area);
    if checklist_area.width == 0 || checklist_area.height == 0 {
        return;
    }

    let block = Block::default()
        .title("Password checklist")
        .title_style(theme.title_style())
        .borders(Borders::ALL)
        .style(theme.body_style());
    let inner = block.inner(checklist_area);
    frame.render_widget(block, checklist_area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(setup_password_checklist_lines(model, theme)).wrap(Wrap { trim: true }),
        inner,
    );
}

fn append_setup_error(lines: &mut Vec<Line<'static>>, model: &SetupViewModel, theme: &TundraTheme) {
    if let Some(error) = &model.error {
        lines.push(Line::from(""));
        lines.push(Line::styled(format!("Error: {error}"), theme.error_style()));
    }
}

pub fn setup_timezone_list_area(main: Rect) -> Rect {
    let controls = setup_timezone_controls_area(main);
    let content = setup_inner_area(controls);
    let reserved_height = SETUP_TIMEZONE_HEADER_HEIGHT
        .saturating_add(SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT)
        .saturating_add(SETUP_TIMEZONE_BOTTOM_INDICATOR_HEIGHT)
        .saturating_add(SETUP_TIMEZONE_FOOTER_HEIGHT);
    Rect::new(
        content.x,
        content
            .y
            .saturating_add(SETUP_TIMEZONE_HEADER_HEIGHT)
            .saturating_add(SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT),
        content.width,
        content.height.saturating_sub(reserved_height),
    )
}

pub fn setup_timezone_visible_rows(main: Rect) -> usize {
    usize::from(setup_timezone_list_area(main).height)
}

pub fn setup_language_list_area(main: Rect, language_count: usize) -> Rect {
    setup_line_area(
        main,
        SETUP_LANGUAGE_LIST_LINE,
        setup_rendered_row_count(language_count),
    )
}

pub fn setup_admin_field_area(main: Rect, field: SetupField) -> Rect {
    let (line, height) = match field {
        SetupField::AdminUsername => (SETUP_ADMIN_USERNAME_LINE, SETUP_ADMIN_FIELD_HEIGHT),
        SetupField::AdminPassword => (SETUP_ADMIN_PASSWORD_LINE, SETUP_ADMIN_FIELD_HEIGHT),
        SetupField::AdminPasswordConfirm => {
            (SETUP_ADMIN_CONFIRM_PASSWORD_LINE, SETUP_ADMIN_FIELD_HEIGHT)
        }
        SetupField::PasswordHint => (SETUP_ADMIN_HINT_LINE, SETUP_ADMIN_FIELD_HEIGHT),
        SetupField::Submit => (SETUP_ADMIN_SUBMIT_LINE, 1),
        SetupField::LanguageList | SetupField::TimezoneList => {
            (SETUP_ADMIN_USERNAME_LINE, SETUP_ADMIN_FIELD_HEIGHT)
        }
    };
    setup_admin_line_area(main, line, height)
}

fn setup_admin_form_area(area: Rect) -> Rect {
    let content = setup_inner_area(area);
    if content.width < SETUP_ADMIN_SIDE_CHECKLIST_MIN_WIDTH {
        return content;
    }

    let reserved_checklist_width =
        SETUP_ADMIN_CHECKLIST_WIDTH.saturating_add(SETUP_ADMIN_COLUMN_GAP);
    Rect::new(
        content.x,
        content.y,
        content.width.saturating_sub(reserved_checklist_width),
        content.height,
    )
}

fn setup_admin_checklist_area(area: Rect) -> Rect {
    let content = setup_inner_area(area);
    if content.width >= SETUP_ADMIN_SIDE_CHECKLIST_MIN_WIDTH {
        let form = setup_admin_form_area(area);
        let x = form
            .x
            .saturating_add(form.width)
            .saturating_add(SETUP_ADMIN_COLUMN_GAP);
        let width = content.x.saturating_add(content.width).saturating_sub(x);
        let line = SETUP_ADMIN_PASSWORD_LINE;
        return Rect::new(
            x,
            content.y.saturating_add(line),
            width,
            SETUP_ADMIN_CHECKLIST_HEIGHT.min(content.height.saturating_sub(line)),
        );
    }

    setup_line_area(
        area,
        SETUP_ADMIN_STACKED_CHECKLIST_LINE,
        SETUP_ADMIN_CHECKLIST_HEIGHT,
    )
}

fn setup_admin_error_area(area: Rect) -> Rect {
    let content = setup_inner_area(area);
    if content.width >= SETUP_ADMIN_SIDE_CHECKLIST_MIN_WIDTH {
        return setup_admin_line_area(area, SETUP_ADMIN_ERROR_LINE, 2);
    }

    setup_line_area(
        area,
        SETUP_ADMIN_STACKED_CHECKLIST_LINE.saturating_add(SETUP_ADMIN_CHECKLIST_HEIGHT),
        2,
    )
}

fn setup_admin_line_area(area: Rect, line: u16, desired_height: u16) -> Rect {
    let form = setup_admin_form_area(area);
    if line >= form.height || desired_height == 0 {
        return Rect::new(form.x, form.y.saturating_add(form.height), form.width, 0);
    }

    Rect::new(
        form.x,
        form.y.saturating_add(line),
        form.width,
        desired_height.min(form.height.saturating_sub(line)),
    )
}

fn setup_timezone_controls_area(main: Rect) -> Rect {
    if main.width >= SETUP_WIDE_MAP_MIN_WIDTH && main.height >= SETUP_WIDE_MAP_MIN_HEIGHT {
        Layout::horizontal([
            Constraint::Length(SETUP_CONTROLS_WIDTH),
            Constraint::Min(30),
        ])
        .split(main)[0]
    } else {
        main
    }
}

fn setup_inner_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

fn setup_line_area(area: Rect, line: u16, desired_height: u16) -> Rect {
    let content = setup_inner_area(area);
    if line >= content.height || desired_height == 0 {
        return Rect::new(
            content.x,
            content.y.saturating_add(content.height),
            content.width,
            0,
        );
    }

    Rect::new(
        content.x,
        content.y.saturating_add(line),
        content.width,
        desired_height.min(content.height.saturating_sub(line)),
    )
}

fn setup_rendered_row_count(count: usize) -> u16 {
    u16::try_from(count.max(1)).unwrap_or(u16::MAX)
}

fn setup_timezone_window_bounds(model: &SetupViewModel, visible_rows: usize) -> (usize, usize) {
    if model.timezones.is_empty() || visible_rows == 0 {
        return (0, 0);
    }

    let selected = model.selected_timezone_index.min(model.timezones.len() - 1);
    let max_start = model.timezones.len().saturating_sub(visible_rows);
    let mut start = model.timezone_window_start.min(max_start);

    if selected < start {
        start = selected;
    } else if selected >= start.saturating_add(visible_rows) {
        start = selected.saturating_add(1).saturating_sub(visible_rows);
    }
    start = start.min(max_start);

    let end = start
        .saturating_add(visible_rows)
        .min(model.timezones.len());
    (start, end)
}

fn setup_timezone_indicator_line(
    visible: bool,
    text: &'static str,
    theme: &TundraTheme,
) -> Line<'static> {
    if visible {
        Line::styled(text, theme.muted_style())
    } else {
        Line::from("")
    }
}

fn setup_timezone_window_lines(
    model: &SetupViewModel,
    start: usize,
    end: usize,
    theme: &TundraTheme,
) -> Vec<Line<'static>> {
    if model.timezones.is_empty() {
        return vec![Line::styled(
            "  No timezones available",
            theme.muted_style(),
        )];
    }

    if start >= end {
        return Vec::new();
    }

    model.timezones[start..end]
        .iter()
        .enumerate()
        .map(|(offset, timezone)| {
            let index = start + offset;
            let text = format!(
                "{}{} ({})",
                selection_marker(index == model.selected_timezone_index),
                timezone.label,
                timezone.id
            );
            if index == model.selected_timezone_index {
                Line::styled(text, theme.title_style())
            } else {
                Line::from(text)
            }
        })
        .collect()
}

fn setup_submit_line(model: &SetupViewModel, theme: &TundraTheme) -> Line<'static> {
    let label = if model.can_submit {
        "Submit: ready"
    } else {
        "Submit: incomplete"
    };
    let text = format!(
        "{}{}",
        focus_marker(model.focused_field == SetupField::Submit),
        label
    );

    if model.focused_field == SetupField::Submit {
        Line::styled(text, theme.title_style())
    } else if model.can_submit {
        Line::from(text)
    } else {
        Line::styled(text, theme.muted_style())
    }
}

fn setup_password_checklist_lines(
    model: &SetupViewModel,
    theme: &TundraTheme,
) -> Vec<Line<'static>> {
    if model.password_requirements.is_empty() {
        return vec![Line::styled(
            "No password rules available",
            theme.muted_style(),
        )];
    }

    model
        .password_requirements
        .iter()
        .map(|requirement| {
            let marker = if requirement.met { "[x]" } else { "[ ]" };
            let style = if requirement.met {
                theme.title_style()
            } else {
                theme.muted_style()
            };
            Line::styled(format!("{marker} {}", requirement.label), style)
        })
        .collect()
}

fn selected_language_summary(model: &SetupViewModel) -> String {
    model
        .selected_language()
        .map(|language| format!("Selected language: {}", language.code))
        .unwrap_or_else(|| "Selected language: none".to_string())
}

fn selected_timezone_id_summary(model: &SetupViewModel) -> String {
    model
        .selected_timezone()
        .map(|timezone| format!("Selected timezone: {}", timezone.id))
        .unwrap_or_else(|| "Selected timezone: none".to_string())
}

fn selected_timezone_description_summary(model: &SetupViewModel) -> String {
    model
        .selected_timezone()
        .map(|timezone| format!("{} - {}", timezone.label, timezone.description))
        .unwrap_or_else(|| "No timezone selected".to_string())
}

fn setup_step_label(step: SetupStep) -> &'static str {
    match step {
        SetupStep::Language => "Language",
        SetupStep::Timezone => "Timezone",
        SetupStep::Admin => "Admin",
    }
}

fn selection_marker(selected: bool) -> &'static str {
    if selected { "> " } else { "  " }
}

fn render_explorer_main(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    let main = Paragraph::new(explorer_lines(model, theme))
        .block(
            Block::default()
                .title("Explorer")
                .borders(Borders::ALL)
                .style(theme.body_style()),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(main, area);
}

fn render_explorer_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExplorerDialogViewModel,
    theme: &TundraTheme,
) {
    let dialog = centered_rect(area, area.width.min(56), area.height.min(7));
    let lines = vec![
        Line::from(model.message.clone()),
        Line::from(""),
        Line::from(format!("{}    {}", model.confirm_label, model.cancel_label)),
    ];
    let dialog_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(model.title.as_str())
                .borders(Borders::ALL)
                .style(theme.body_style()),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, dialog);
    frame.render_widget(dialog_widget, dialog);
}

fn render_top(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    theme: &TundraTheme,
) {
    let stack = if chrome.screen_stack.is_empty() {
        "Home".to_string()
    } else {
        chrome.screen_stack.join(" > ")
    };
    let lines = vec![
        Line::styled(chrome.app_name.clone(), theme.title_style()),
        Line::styled(
            format!(
                "{} | {:?} | {}x{} | {}",
                chrome.build_mode,
                chrome.display_mode,
                chrome.terminal_size.0,
                chrome.terminal_size.1,
                stack
            ),
            theme.muted_style(),
        ),
    ];
    let top = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .style(theme.body_style()),
    );

    frame.render_widget(top, area);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, home: &HomeViewModel, theme: &TundraTheme) {
    match home.display_mode() {
        HomeDisplayMode::Debug => {
            let main = Paragraph::new(debug_lines(home))
                .block(
                    Block::default()
                        .title("Home")
                        .borders(Borders::ALL)
                        .style(theme.body_style()),
                )
                .wrap(Wrap { trim: true });

            frame.render_widget(main, area);
        }
        HomeDisplayMode::User | HomeDisplayMode::Auth => render_user_main(frame, area, home, theme),
    }
}

fn render_user_main(frame: &mut Frame<'_>, area: Rect, home: &HomeViewModel, theme: &TundraTheme) {
    let outer = Block::default()
        .title("Home")
        .borders(Borders::ALL)
        .style(theme.body_style());
    frame.render_widget(outer, area);

    let content = home_content_area(area);
    if content.width == 0 || content.height == 0 {
        return;
    }

    let summary = home_summary_area(area);
    let controls = home_controls_area(area);
    let user = home.current_user.as_deref().unwrap_or("Unknown user");
    let time = home.current_time.as_deref().unwrap_or("Unknown time");

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!("User: {user}")),
            Line::from(format!("Time: {time}")),
        ])
        .style(theme.body_style())
        .wrap(Wrap { trim: true }),
        summary,
    );

    for (index, (entry, tile)) in home
        .entries()
        .iter()
        .zip(home_entry_tile_areas(area, home.entries().len()))
        .enumerate()
    {
        let selected = index == home.selected_entry_index();
        let style = if selected {
            theme.title_style()
        } else {
            theme.body_style()
        };
        let mut lines: Vec<Line<'static>> = home
            .home_icon_for_label(&entry.label)
            .map(|icon| {
                icon.lines
                    .iter()
                    .map(|line| Line::from(line.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        lines.push(Line::styled(entry.label.clone(), style));
        lines.push(Line::from(entry.description.clone()));

        let tile_widget = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(style)
                    .title(if selected { "Selected" } else { "" }),
            )
            .style(style)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(tile_widget, tile);
    }

    frame.render_widget(
        Paragraph::new(Line::from(
            "Arrows: select    Enter: open    E: explorer    U: users    Q / Esc: exit",
        ))
        .style(theme.muted_style())
        .wrap(Wrap { trim: true }),
        controls,
    );
}

fn render_status(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    theme: &TundraTheme,
) {
    let mut lines = vec![Line::from(chrome.status.status.clone())];
    if let Some(toast) = &chrome.status.toast {
        lines.push(Line::styled(toast.clone(), theme.muted_style()));
    }
    if let Some(error) = &chrome.status.error {
        lines.push(Line::styled(error.clone(), theme.error_style()));
    }

    let status = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Status")
                .borders(Borders::ALL)
                .style(theme.body_style()),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(status, area);
}

fn debug_lines(home: &HomeViewModel) -> Vec<Line<'static>> {
    let Some(diagnostics) = home.diagnostics() else {
        return vec![Line::from("Diagnostics unavailable")];
    };

    vec![
        Line::from(format!("Tick: {}", diagnostics.tick_count)),
        Line::from(format!(
            "Last key: {}",
            optional_text(&diagnostics.last_key_event)
        )),
        Line::from(format!(
            "Last mouse: {}",
            optional_text(&diagnostics.last_mouse_event)
        )),
        Line::from(format!(
            "Last resize: {}",
            optional_text(&diagnostics.last_resize_event)
        )),
        Line::from(format!(
            "Mouse: {}",
            diagnostics
                .mouse_coordinates
                .map(|(x, y)| format!("{x},{y}"))
                .unwrap_or_else(|| "none".to_string())
        )),
        Line::from(format!(
            "Scroll: {}",
            optional_text(&diagnostics.scroll_direction)
        )),
        Line::from(format!(
            "Drag: {}",
            optional_text(&diagnostics.drag_direction)
        )),
        Line::from(format!(
            "Flags: {}",
            if diagnostics.terminal_flags.is_empty() {
                "none".to_string()
            } else {
                diagnostics.terminal_flags.join(", ")
            }
        )),
        Line::from(format!(
            "Platform capabilities: {}",
            diagnostics.platform_capability_summary
        )),
    ]
}

fn bootstrap_lines(model: &BootstrapAdminViewModel) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Tab / Down: password    Enter on password: create admin    Esc: exit"),
        Line::from(""),
        Line::from(format!(
            "{}Admin username: {}",
            focus_marker(model.focused_field == AuthField::Username),
            model.username
        )),
        Line::from(format!(
            "{}Admin password: {}",
            focus_marker(model.focused_field == AuthField::Password),
            "*".repeat(model.password_len)
        )),
    ];
    if let Some(error) = &model.error {
        lines.push(Line::from(""));
        lines.push(Line::from(error.clone()));
    }
    lines
}

fn focus_marker(focused: bool) -> &'static str {
    if focused { "> " } else { "  " }
}

fn explorer_lines(model: &ExplorerViewModel, theme: &TundraTheme) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("Path: {}", model.current_path)),
        Line::from(format!(
            "Hidden files: {}",
            if model.show_hidden { "shown" } else { "hidden" }
        )),
    ];
    if let Some(search) = &model.search {
        lines.push(Line::from(explorer_search_line(search)));
    }
    lines.push(Line::from(EXPLORER_HELP_LINE));
    lines.push(Line::from(""));
    lines.push(Line::styled("Entries", theme.title_style()));

    if model.entries.is_empty() {
        lines.push(Line::from("(empty directory)"));
    } else {
        for (index, entry) in model.entries.iter().enumerate() {
            let line = explorer_entry_line(index, model.selected_index, entry);
            if model.selected_index == Some(index) {
                lines.push(Line::styled(line, theme.title_style()));
            } else {
                lines.push(Line::from(line));
            }
        }
    }

    let selected_names = selected_entry_names(model);
    if !selected_names.is_empty() {
        lines.push(Line::from(format!(
            "Selected: {}",
            selected_names.join(", ")
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::styled("Details", theme.title_style()));
    match model.selected_entry() {
        Some(entry) => {
            lines.push(Line::from(format!("Name: {}", entry.name)));
            lines.push(Line::from(format!("Type: {}", entry.kind)));
            lines.push(Line::from(format!(
                "Size: {}",
                entry.size.as_deref().unwrap_or("-")
            )));
            lines.push(Line::from(format!(
                "Modified: {}",
                entry.modified.as_deref().unwrap_or("-")
            )));
            lines.push(Line::from(format!(
                "Attributes: {}",
                format_attributes(&entry.attributes)
            )));
        }
        None => lines.push(Line::from("No entry selected")),
    }

    if let Some(message) = &model.message {
        lines.push(Line::from(""));
        lines.push(Line::styled(message.clone(), theme.muted_style()));
    }
    if let Some(error) = &model.error {
        lines.push(Line::from(""));
        lines.push(Line::styled(format!("Error: {error}"), theme.error_style()));
    }

    lines
}

pub fn explorer_first_entry_content_line(model: &ExplorerViewModel, content_width: u16) -> usize {
    let width = usize::from(content_width.max(1));
    let mut line = 0usize;
    line += wrapped_line_count(&format!("Path: {}", model.current_path), width);
    line += wrapped_line_count(
        &format!(
            "Hidden files: {}",
            if model.show_hidden { "shown" } else { "hidden" }
        ),
        width,
    );
    if let Some(search) = &model.search {
        line += wrapped_line_count(&explorer_search_line(search), width);
    }
    line += wrapped_line_count(EXPLORER_HELP_LINE, width);
    line += 1;
    line += wrapped_line_count("Entries", width);
    line
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    text.chars().count().max(1).div_ceil(width.max(1))
}

fn explorer_entry_line(
    index: usize,
    selected_index: Option<usize>,
    entry: &ExplorerEntryViewModel,
) -> String {
    let cursor_marker = if selected_index == Some(index) {
        ">"
    } else {
        " "
    };
    let selected_marker = if entry.selected { "*" } else { " " };
    format!(
        "{cursor_marker}{selected_marker} {} | {} | {} | {} | {}",
        entry.name,
        entry.kind,
        entry.size.as_deref().unwrap_or("-"),
        entry.modified.as_deref().unwrap_or("-"),
        format_attributes(&entry.attributes)
    )
}

fn explorer_search_line(search: &ExplorerSearchViewModel) -> String {
    let query = if search.query.is_empty() {
        "<empty>"
    } else {
        search.query.as_str()
    };
    let mode = if search.active { "active" } else { "inactive" };
    match search.match_count {
        Some(1) => format!("Search: {query} (1 match, {mode})"),
        Some(count) => format!("Search: {query} ({count} matches, {mode})"),
        None => format!("Search: {query} ({mode})"),
    }
}

fn selected_entry_names(model: &ExplorerViewModel) -> Vec<String> {
    model
        .entries
        .iter()
        .filter(|entry| entry.selected)
        .map(|entry| entry.name.clone())
        .collect()
}

fn format_attributes(attributes: &[String]) -> String {
    if attributes.is_empty() {
        "none".to_string()
    } else {
        attributes.join(", ")
    }
}

pub fn home_entry_tile_areas(main: Rect, entry_count: usize) -> Vec<Rect> {
    if entry_count == 0 {
        return Vec::new();
    }

    let grid = home_entry_grid_area(main);
    if grid.width == 0 || grid.height == 0 {
        return Vec::new();
    }

    let columns = home_entry_column_count(grid.width, entry_count);
    let rows = entry_count.div_ceil(columns);
    let horizontal_gap = if columns > 1 { HOME_TILE_GAP } else { 0 };
    let vertical_gap = if rows > 1 { HOME_TILE_GAP } else { 0 };
    let total_horizontal_gap = horizontal_gap.saturating_mul(columns.saturating_sub(1) as u16);
    let total_vertical_gap = vertical_gap.saturating_mul(rows.saturating_sub(1) as u16);
    let tile_width = grid
        .width
        .saturating_sub(total_horizontal_gap)
        .checked_div(columns as u16)
        .unwrap_or(0);
    let available_height = grid.height.saturating_sub(total_vertical_gap);
    let tile_height = available_height
        .checked_div(rows as u16)
        .unwrap_or(0)
        .min(HOME_TILE_MAX_HEIGHT)
        .max(HOME_TILE_MIN_HEIGHT.min(grid.height));

    let mut areas = Vec::with_capacity(entry_count);
    for index in 0..entry_count {
        let row = index / columns;
        let column = index % columns;
        let x = grid.x.saturating_add(
            (column as u16).saturating_mul(tile_width.saturating_add(horizontal_gap)),
        );
        let y = grid
            .y
            .saturating_add((row as u16).saturating_mul(tile_height.saturating_add(vertical_gap)));
        if x >= grid.x.saturating_add(grid.width) || y >= grid.y.saturating_add(grid.height) {
            break;
        }
        let width = tile_width.min(grid.x.saturating_add(grid.width).saturating_sub(x));
        let height = tile_height.min(grid.y.saturating_add(grid.height).saturating_sub(y));
        if width > 0 && height > 0 {
            areas.push(Rect::new(x, y, width, height));
        }
    }

    areas
}

pub fn home_entry_index_at(
    main: Rect,
    entry_count: usize,
    coordinates: (u16, u16),
) -> Option<usize> {
    home_entry_tile_areas(main, entry_count)
        .into_iter()
        .enumerate()
        .find_map(|(index, area)| rect_contains(area, coordinates).then_some(index))
}

fn home_content_area(main: Rect) -> Rect {
    Rect::new(
        main.x.saturating_add(1),
        main.y.saturating_add(1),
        main.width.saturating_sub(2),
        main.height.saturating_sub(2),
    )
}

fn home_summary_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    Rect::new(
        content.x,
        content.y,
        content.width,
        HOME_SUMMARY_HEIGHT.min(content.height),
    )
}

fn home_controls_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    let height = HOME_CONTROLS_HEIGHT.min(content.height);
    Rect::new(
        content.x,
        content
            .y
            .saturating_add(content.height.saturating_sub(height)),
        content.width,
        height,
    )
}

fn home_entry_grid_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    let reserved = HOME_SUMMARY_HEIGHT.saturating_add(HOME_CONTROLS_HEIGHT);
    let y = content
        .y
        .saturating_add(HOME_SUMMARY_HEIGHT.min(content.height));
    let bottom = content.y.saturating_add(
        content
            .height
            .saturating_sub(HOME_CONTROLS_HEIGHT.min(content.height)),
    );
    Rect::new(
        content.x,
        y,
        content.width,
        bottom
            .saturating_sub(y)
            .min(content.height.saturating_sub(reserved.min(content.height))),
    )
}

fn home_entry_column_count(width: u16, entry_count: usize) -> usize {
    let max_columns = if width >= 96 {
        4
    } else if width >= 72 {
        3
    } else if width >= 48 {
        2
    } else {
        1
    };

    max_columns.min(entry_count.max(1))
}

fn rect_contains(rect: Rect, coordinates: (u16, u16)) -> bool {
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);

    coordinates.0 >= rect.x
        && coordinates.0 < right
        && coordinates.1 >= rect.y
        && coordinates.1 < bottom
}

fn optional_text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("none")
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
