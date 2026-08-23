use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Widget};

use crate::{RenderContext, TundraTheme};

/// A neutral Glacier Night container. Accent colour is intentionally excluded
/// from normal surfaces; it is reserved for focus and interaction state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    pub title: Option<String>,
    pub bordered: bool,
    pub raised: bool,
    pub border_shape: Option<crate::BorderShape>,
}

impl Default for Surface {
    fn default() -> Self {
        Self::new()
    }
}

impl Surface {
    pub const fn new() -> Self {
        Self {
            title: None,
            bordered: false,
            raised: false,
            border_shape: None,
        }
    }
    pub const fn border_shape(mut self, border_shape: crate::BorderShape) -> Self {
        self.border_shape = Some(border_shape);
        self
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub const fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    pub const fn raised(mut self, raised: bool) -> Self {
        self.raised = raised;
        self
    }

    pub fn inner(&self, area: Rect) -> Rect {
        if self.bordered && area.width > 2 && area.height > 2 {
            Rect::new(
                area.x.saturating_add(1),
                area.y.saturating_add(1),
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
            )
        } else {
            area
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        self.block(context).render(area, buffer);
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        frame.render_widget(self.block(context), area);
    }

    pub fn render_with_theme(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.render(
            area,
            buffer,
            &RenderContext::from_theme(theme, Default::default(), Default::default()),
        );
    }

    fn block(&self, context: &RenderContext) -> Block<'_> {
        let tokens = context.theme;
        let style = Style::default().fg(tokens.text).bg(if self.raised {
            tokens.raised
        } else {
            tokens.surface
        });
        let mut block = Block::default().style(style);
        if self.bordered {
            block = block
                .borders(Borders::ALL)
                .border_type(
                    self.border_shape
                        .unwrap_or(context.theme.border_shape)
                        .border_type(),
                )
                .border_style(
                    Style::default()
                        .fg(tokens.border)
                        .bg(style.bg.unwrap_or(tokens.surface)),
                );
        }
        if let Some(title) = &self.title {
            block = block.title(title.as_str()).title_style(
                Style::default()
                    .fg(tokens.accent)
                    .bg(style.bg.unwrap_or(tokens.surface))
                    .add_modifier(Modifier::BOLD),
            );
        }
        block
    }
}

/// A named raised surface used for settings groups, cards, and inspectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panel {
    surface: Surface,
}

impl Panel {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            surface: Surface::new().titled(title).bordered(true).raised(true),
        }
    }

    pub fn inner(&self, area: Rect) -> Rect {
        self.surface.inner(area)
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        self.surface.render(area, buffer, context);
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.surface.render_frame(frame, area, context);
    }
}
