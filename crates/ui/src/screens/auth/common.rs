use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Borders, Paragraph, Wrap};

use crate::TundraTheme;
use crate::screens::shell::{
    ShellChromeViewModel, ShellLayout, compute_shell_layout, render_compact_home, render_status,
    render_top,
};

pub(super) fn render_auth_screen(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    title: &'static str,
    lines: Vec<Line<'static>>,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            let widget = Paragraph::new(lines)
                .block(
                    theme
                        .block()
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

pub(super) fn focus_marker(focused: bool) -> &'static str {
    if focused { "> " } else { "  " }
}
