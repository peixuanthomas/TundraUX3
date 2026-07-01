use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::{
    ExitConfirmViewModel, HomeDisplayMode, HomeViewModel, ShellChromeViewModel, ShellLayout,
    TundraTheme, compute_shell_layout,
};

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
        HomeDisplayMode::User => user_lines(home),
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
    ]
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
