use crate::{RenderContext, components::Surface};
use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    text::Line,
    widgets::{Paragraph, Wrap},
};
pub(super) fn render_auth_screen(
    frame: &mut Frame<'_>,
    main: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    context: &RenderContext,
) {
    let surface = Surface::new().titled(title).bordered(true);
    let inner = surface.inner(main);
    surface.render_frame(frame, main, &context);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Left)
            .wrap(Wrap { trim: true }),
        inner,
    );
}
