use crate::{RenderContext, components::Surface};
use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    layout::{HorizontalAlignment, Rect},
    text::Line,
    widgets::{Paragraph, Wrap},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthViewport {
    pub area: Rect,
    pub content: Rect,
    pub scrollbar: Option<Rect>,
    pub offset: u16,
}

impl AuthViewport {
    pub fn project(self, rect: Rect) -> Option<Rect> {
        (rect.y >= self.offset
            && rect.bottom() <= self.offset + self.area.height
            && rect.right() <= self.content.width)
            .then(|| {
                Rect::new(
                    self.area.x + rect.x,
                    self.area.y + rect.y - self.offset,
                    rect.width,
                    rect.height,
                )
            })
    }
    pub fn content_point(self, point: (u16, u16)) -> Option<(u16, u16)> {
        (point.0 >= self.area.x
            && point.0 < self.area.x.saturating_add(self.content.width)
            && point.1 >= self.area.y
            && point.1 < self.area.bottom())
        .then(|| (point.0 - self.area.x, point.1 - self.area.y + self.offset))
    }
}

pub fn auth_viewport(area: Rect, minimum_height: u16, offset: u16) -> AuthViewport {
    let height = area.height.max(minimum_height);
    let scrolling = height > area.height && area.width > 1 && area.height > 0;
    AuthViewport {
        area,
        content: Rect::new(
            0,
            0,
            area.width.saturating_sub(u16::from(scrolling)),
            height,
        ),
        scrollbar: scrolling.then(|| Rect::new(area.right() - 1, area.y, 1, area.height)),
        offset: if scrolling {
            offset.min(height - area.height)
        } else {
            0
        },
    }
}

/// Paint the whole form into a bounded buffer, then copy only its visible rows.
/// Button rectangles are translated with the same offset as the pixels.
pub(super) fn render_auth_viewport(
    frame: &mut Frame<'_>,
    viewport: AuthViewport,
    context: &RenderContext,
    render: impl FnMut(&mut Frame<'_>, Rect, &RenderContext),
) {
    render_auth_viewport_inner(frame, viewport, context, false, render);
}

pub(super) fn render_auth_overlay_viewport(
    frame: &mut Frame<'_>,
    viewport: AuthViewport,
    context: &RenderContext,
    render: impl FnMut(&mut Frame<'_>, Rect, &RenderContext),
) {
    render_auth_viewport_inner(frame, viewport, context, true, render);
}

fn render_auth_viewport_inner(
    frame: &mut Frame<'_>,
    viewport: AuthViewport,
    context: &RenderContext,
    overlay: bool,
    mut render: impl FnMut(&mut Frame<'_>, Rect, &RenderContext),
) {
    if viewport.area.is_empty() {
        return;
    }
    let mut local = context.clone();
    local.buttons = context.buttons.as_ref().map(|buttons| {
        let translate = |region: &crate::components::ButtonRegion| {
            let mut region = region.clone();
            region.area.x = region.area.x.saturating_sub(viewport.area.x);
            region.area.y = region
                .area
                .y
                .saturating_sub(viewport.area.y)
                .saturating_add(viewport.offset);
            region
        };
        let mut result = crate::components::ButtonFrame::new(
            buttons.hovered.as_ref().map(translate),
            buttons.pressed.as_ref().map(translate),
            &context.compatibility_theme(),
        );
        result.keyboard_focus_visible = buttons.keyboard_focus_visible;
        result
    });
    let mut terminal = Terminal::new(TestBackend::new(
        viewport.content.width,
        viewport.content.height,
    ))
    .expect("bounded authentication viewport");
    terminal
        .draw(|virtual_frame| render(virtual_frame, viewport.content, &local))
        .expect("in-memory authentication rendering");
    let buffer = terminal.backend().buffer();
    for y in 0..viewport.area.height {
        for x in 0..viewport.content.width {
            if overlay && buffer[(x, y + viewport.offset)] == ratatui::buffer::Cell::default() {
                continue;
            }
            frame.buffer_mut()[(viewport.area.x + x, viewport.area.y + y)] =
                buffer[(x, y + viewport.offset)].clone();
        }
    }
    if let (Some(original), Some(local)) = (&context.buttons, &local.buttons) {
        for mut region in local.regions() {
            // Partly clipped buttons cannot activate a hidden action.
            if region.area.y < viewport.offset
                || region.area.bottom() > viewport.offset + viewport.area.height
            {
                continue;
            }
            region.area.x += viewport.area.x;
            region.area.y = region.area.y - viewport.offset + viewport.area.y;
            original.register_region(region);
        }
    }
    if let Some(track) = viewport.scrollbar {
        crate::components::Scrollbar::new(
            usize::from(viewport.content.height),
            usize::from(viewport.area.height),
            usize::from(viewport.offset),
        )
        .render_frame(frame, track, context);
    }
}
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
