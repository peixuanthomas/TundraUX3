use ratatui::Frame;
use ratatui::buffer::{Buffer, CellDiffOption, CellWidth};
use ratatui::layout::Rect;
use ratatui::style::{Color as RenderedColor, Modifier, Style};
use ratatui::widgets::Clear;
use workspace_protocol::{Cell, Color, ControlNode, Cursor, LayoutGeneration, Size, WindowFrame};

use crate::{ColorCapability, RenderContext, automation};

const ANSI_COLORS: [RenderedColor; 16] = [
    RenderedColor::Black,
    RenderedColor::Red,
    RenderedColor::Green,
    RenderedColor::Yellow,
    RenderedColor::Blue,
    RenderedColor::Magenta,
    RenderedColor::Cyan,
    RenderedColor::Gray,
    RenderedColor::DarkGray,
    RenderedColor::LightRed,
    RenderedColor::LightGreen,
    RenderedColor::LightYellow,
    RenderedColor::LightBlue,
    RenderedColor::LightMagenta,
    RenderedColor::LightCyan,
    RenderedColor::White,
];

/// Captures content cells. The cursor, when present, already uses content-local coordinates.
pub fn capture_workspace_frame(
    layout: LayoutGeneration,
    buffer: &Buffer,
    content: Rect,
    cursor: Option<Cursor>,
) -> WindowFrame {
    let content = buffer.area.intersection(content);
    let size = Size::new(content.width, content.height);
    let mut cells = Vec::with_capacity(size.area());
    for y in content.y..content.bottom() {
        let mut next = content.x;
        for x in content.x..content.right() {
            let cell = &buffer[(x, y)];
            let width = cell.cell_width().max(1);
            let clipped = width > content.right() - x || cell.symbol().contains(char::is_control);
            let continuation = x < next;
            let symbol = if clipped || continuation {
                " "
            } else {
                cell.symbol()
            };
            cells.push(Cell {
                symbol: symbol.into(),
                foreground: capture_color(cell.fg),
                background: capture_color(cell.bg),
                underline_color: capture_color(cell.underline_color),
                modifiers: cell.modifier.bits(),
                skip: continuation,
            });
            if !clipped && !continuation {
                next = x + width;
            }
        }
    }
    WindowFrame {
        layout,
        size,
        cells,
        cursor: cursor.filter(|cursor| cursor.x < size.width && cursor.y < size.height),
        images: Vec::new(),
        headings: Vec::new(),
        controls: Vec::new(),
        masks: Vec::new(),
        native_escape: Vec::new(),
        modal: false,
        page: String::new(),
        animation_running: false,
        keyboard_available: false,
        status: String::new(),
    }
}

/// Adds visible control metadata and masks to a frame captured from the same content area.
pub fn capture_workspace_controls(
    frame: &mut WindowFrame,
    content: Rect,
    nodes: &[shell_control::Node],
) {
    let content = Rect::new(content.x, content.y, frame.size.width, frame.size.height);
    for node in nodes {
        let rect = Rect::new(node.rect.x, node.rect.y, node.rect.width, node.rect.height);
        let clipped = rect.intersection(content);
        if clipped.is_empty() {
            continue;
        }
        let relative = workspace_protocol::Rect {
            x: clipped.x - content.x,
            y: clipped.y - content.y,
            width: clipped.width,
            height: clipped.height,
        };
        if node.sensitive {
            frame.masks.push(relative);
        }
        let group = node.id.split('/').next();
        if !matches!(
            group,
            Some("page" | "page-overlay" | "modal" | "authorization" | "toast" | "keyboard")
        ) || node.role == "text"
        {
            continue;
        }
        let actionable = node.enabled && node.actionable && clipped == rect;
        frame.controls.push(ControlNode {
            id: node.id.clone(),
            parent: node.parent.clone(),
            role: node.role.clone(),
            name: node.name.clone(),
            text: if node.sensitive {
                "••••".into()
            } else {
                String::new()
            },
            rect: relative,
            focused: actionable && node.focused,
            selected: node.selected,
            enabled: node.enabled,
            actionable,
            sensitive: node.sensitive,
            actions: if actionable {
                node.actions.clone()
            } else {
                Vec::new()
            },
        });
    }
}

/// Paints a worker's content in a pane. The host retains cursor and overlay ownership.
pub fn render_workspace_frame(
    frame: &mut Frame<'_>,
    area: Rect,
    source: &WindowFrame,
    context: &RenderContext,
    actionable: bool,
) {
    let target = area.intersection(frame.area());
    if target.is_empty() || source.cells.len() != source.size.area() {
        return;
    }
    frame.render_widget(Clear, target);
    frame.buffer_mut().set_style(
        target,
        Style::default()
            .fg(context.theme.text)
            .bg(context.theme.canvas),
    );
    let visible = target.intersection(Rect::new(
        area.x,
        area.y,
        source.size.width,
        source.size.height,
    ));
    let stride = usize::from(source.size.width);
    let mut colors = FrameColors::new(context);
    for y in visible.y..visible.bottom() {
        let row = usize::from(y - area.y) * stride;
        let mut next = visible.x;
        for x in visible.x..visible.right() {
            let cell = &source.cells[row + usize::from(x - area.x)];
            let width = cell.symbol.cell_width();
            let continuation = x < next;
            let complete = width > 0
                && width <= visible.right() - x
                && !cell.symbol.contains(char::is_control)
                && !cell.skip;
            let rendered = &mut frame.buffer_mut()[(x, y)];
            rendered.set_symbol(if complete && !continuation {
                cell.symbol.as_str()
            } else {
                " "
            });
            rendered.fg = colors.render(cell.foreground);
            rendered.bg = colors.render(cell.background);
            rendered.underline_color = colors.render(cell.underline_color);
            rendered.modifier = Modifier::from_bits_truncate(cell.modifiers);
            if continuation {
                rendered.set_diff_option(CellDiffOption::Skip);
            } else if complete {
                next = x + width;
            }
        }
    }
    automation::import_workspace_controls(source, area, visible, actionable);
}

pub(crate) fn capture_color(color: RenderedColor) -> Color {
    match color {
        RenderedColor::Reset => Color::Reset,
        RenderedColor::Indexed(index) => Color::Indexed(index),
        RenderedColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
        named => Color::Indexed(
            ANSI_COLORS
                .iter()
                .position(|color| *color == named)
                .expect("named ANSI color") as u8,
        ),
    }
}

pub(crate) struct FrameColors<'a> {
    context: &'a RenderContext,
    ansi: std::collections::HashMap<(u8, u8, u8), RenderedColor>,
}

impl<'a> FrameColors<'a> {
    pub(crate) fn new(context: &'a RenderContext) -> Self {
        Self {
            context,
            ansi: std::collections::HashMap::new(),
        }
    }

    pub(crate) fn render(&mut self, color: Color) -> RenderedColor {
        match color {
            Color::Reset => RenderedColor::Reset,
            Color::Indexed(index @ 0..=15) => ANSI_COLORS[usize::from(index)],
            Color::Indexed(index) => self.limited_color(RenderedColor::Indexed(index)),
            Color::Rgb(r, g, b) => self.limited_color(RenderedColor::Rgb(r, g, b)),
        }
    }

    fn limited_color(&mut self, color: RenderedColor) -> RenderedColor {
        if self.context.capabilities.color == ColorCapability::TrueColor {
            return color;
        }
        let rgb = crate::theme::color_rgb(color).expect("color has RGB channels");
        *self.ansi.entry(rgb).or_insert_with(|| {
            ANSI_COLORS
                .iter()
                .enumerate()
                .min_by_key(|&(index, &color)| {
                    let (r, g, b) = self.context.capabilities.terminal_colors.palette[index]
                        .map(|[r, g, b]| (r, g, b))
                        .unwrap_or_else(|| crate::theme::color_rgb(color).unwrap());
                    let difference = |a, b| i32::from(a).abs_diff(i32::from(b)).pow(2);
                    difference(r, rgb.0) + difference(g, rgb.1) + difference(b, rgb.2)
                })
                .map(|(_, &color)| color)
                .expect("nonempty ANSI palette")
        })
    }
}
