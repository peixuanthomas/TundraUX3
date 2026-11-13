use ratatui::{
    buffer::{Buffer, Cell, CellDiffOption},
    layout::{Margin, Rect},
    style::{Color, Modifier},
    widgets::{Block, BorderType, Borders, Widget},
};

use crate::{ColorCapability, RenderContext, theme::color_rgb};

/// Shade the page behind a modal while keeping its text and layout visible.
pub fn dim_backdrop(buffer: &mut Buffer, area: Rect, context: &RenderContext) {
    let colors = context.capabilities.terminal_colors;
    let shade = |color, foreground| {
        let (r, g, b) = color_rgb(colors.resolve(color, foreground))
            .or_else(|| {
                color_rgb(if foreground {
                    context.theme.text
                } else {
                    context.theme.canvas
                })
            })
            .unwrap_or_default();
        // A black layer at 150/255 opacity, as in OpenCode's dialog.
        let channel = |value| (u16::from(value) * 105 / 255) as u8;
        Color::Rgb(channel(r), channel(g), channel(b))
    };
    for position in area.intersection(buffer.area).positions() {
        let cell = &mut buffer[position];
        if cell.diff_option == CellDiffOption::Skip {
            continue;
        }
        if cell.modifier.contains(Modifier::REVERSED) {
            std::mem::swap(&mut cell.fg, &mut cell.bg);
            cell.modifier.remove(Modifier::REVERSED);
        }
        if context.capabilities.color == ColorCapability::TrueColor {
            cell.fg = shade(cell.fg, true);
            cell.bg = shade(cell.bg, false);
            cell.underline_color = shade(cell.underline_color, true);
        } else {
            cell.fg = Color::DarkGray;
            cell.bg = context.theme.shadow;
            cell.underline_color = Color::DarkGray;
            cell.modifier.remove(Modifier::BOLD);
        }
    }
}

/// Move a surface's border, leaving content for the settled frame.
pub fn morph_surface(source: &Buffer, area: Rect) -> Buffer {
    let mut result = Buffer::empty(area);
    if area.is_empty() || source.area.is_empty() {
        return result;
    }
    let bounds = source.area;
    let corner = &source[(bounds.x, bounds.y)];
    let border = match corner.symbol() {
        "╭" => Some(BorderType::Rounded),
        "┌" => Some(BorderType::Plain),
        "╔" => Some(BorderType::Double),
        "┏" => Some(BorderType::Thick),
        _ => None,
    };
    let margin = u16::from(border.is_some());
    let content = bounds.inner(Margin::new(margin, margin));
    if !content.is_empty() {
        let blank = Cell::new(" ")
            .set_style(source[(content.x, content.y)].style())
            .clone();
        result.content.fill(blank);
    }
    if let Some(border) = border {
        Block::default()
            .borders(Borders::ALL)
            .border_type(border)
            .border_style(corner.style())
            .render(area, &mut result);
    }
    result
}

/// A light fixed-layout transition; text and image protocol cells remain intact.
pub fn light_surface(source: &Buffer, amount: u16) -> Buffer {
    use animate::Interpolate;
    let mut result = source.clone();
    for cell in &mut result.content {
        if cell.diff_option == CellDiffOption::Skip {
            continue;
        }
        if let Color::Rgb(r, g, b) = cell.bg {
            let raised = Color::Rgb(
                r.saturating_add(8),
                g.saturating_add(8),
                b.saturating_add(8),
            );
            cell.bg = Color::lerp(&raised, &cell.bg, f32::from(amount.min(1_000)) / 1_000.0);
        }
    }
    result
}

/// Fade a closing surface into the live underlay without changing its geometry.
pub fn fade_surface(source: &Buffer, underlay: &Buffer, amount: u16) -> Buffer {
    use animate::Interpolate;
    let mut result = source.clone();
    let alpha = f32::from(amount.min(1_000)) / 1_000.0;
    for position in source.area.intersection(underlay.area).positions() {
        let cell = &mut result[position];
        if cell.diff_option != CellDiffOption::Skip {
            let backdrop = &underlay[position];
            cell.fg = ratatui::style::Color::lerp(&backdrop.bg, &cell.fg, alpha);
            cell.bg = ratatui::style::Color::lerp(&backdrop.bg, &cell.bg, alpha);
        }
    }
    result
}
