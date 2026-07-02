use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::{
    AuthField, BootstrapAdminViewModel, ExitConfirmViewModel, ExplorerDialogViewModel,
    ExplorerEntryViewModel, ExplorerSearchViewModel, ExplorerViewModel, HomeDisplayMode,
    HomeViewModel, LoginViewModel, ShellChromeViewModel, ShellLayout, TundraTheme,
    UserManagementField, UserManagementFormKind, UserManagementFormViewModel,
    UserManagementViewModel, compute_shell_layout,
};

pub const EXPLORER_HELP_LINE: &str = "Enter: open    Backspace: parent    N: folder    T: text file    R: rename    X/Delete: delete    C: copy    V: paste    /: search    H: hidden    Esc: back";

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
    render_auth_screen(frame, area, chrome, "Login", auth_lines(model), theme);
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
    let lines = match home.display_mode() {
        HomeDisplayMode::Debug => debug_lines(home),
        HomeDisplayMode::User | HomeDisplayMode::Auth => user_lines(home),
    };
    let main = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Home")
                .borders(Borders::ALL)
                .style(theme.body_style()),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(main, area);
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

fn auth_lines(model: &LoginViewModel) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Tab / Down: password    Enter on password: login    Esc: exit"),
        Line::from(""),
        Line::from(format!(
            "{}Username: {}",
            focus_marker(model.focused_field == AuthField::Username),
            model.username
        )),
        Line::from(format!(
            "{}Password: {}",
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

fn user_lines(home: &HomeViewModel) -> Vec<Line<'static>> {
    let user = home.current_user.as_deref().unwrap_or("Unknown user");
    let time = home.current_time.as_deref().unwrap_or("Unknown time");
    let mut lines = vec![
        Line::from(format!("User: {user}")),
        Line::from(format!("Time: {time}")),
        Line::from(""),
    ];

    lines.extend(
        home.entries()
            .iter()
            .map(|entry| Line::from(format!("{} - {}", entry.label, entry.description))),
    );
    lines.push(Line::from(""));
    lines.push(Line::from(
        "Q / Esc: exit    U: user management when available",
    ));
    lines
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
