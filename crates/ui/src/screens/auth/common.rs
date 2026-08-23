use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

use crate::components::Surface;
use crate::screens::shell::{
    ShellChromeViewModel, ShellLayout, compute_shell_layout, render_compact_home, render_status,
    render_top,
};
use crate::{RenderContext, TundraTheme};

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
            let context = RenderContext::from_theme(theme, Default::default(), Default::default());
            let surface = Surface::new().titled(title).bordered(true);
            let inner = surface.inner(main);
            surface.render_frame(frame, main, &context);
            frame.render_widget(
                Paragraph::new(lines)
                    .alignment(HorizontalAlignment::Left)
                    .wrap(Wrap { trim: true }),
                inner,
            );
            render_status(frame, status, chrome, theme);
        }
    }
}
