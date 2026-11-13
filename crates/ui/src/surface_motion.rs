use ratatui::{
    buffer::{Buffer, Cell, CellDiffOption},
    layout::{Margin, Rect},
    style::{Color, Modifier},
    widgets::{Block, BorderType, Borders, Widget},
};

/// Dim the covered cells while keeping their text and protocol data intact.
pub fn dim_modal_backdrop(buffer: &mut Buffer, area: Rect, context: &crate::RenderContext) {
    for position in area.intersection(buffer.area).positions() {
        let cell = &mut buffer[position];
        if cell.diff_option == CellDiffOption::Skip {
            continue;
        }
        if context.capabilities.color == crate::ColorCapability::TrueColor {
            let dim = |color, fallback| {
                let color = if color == Color::Reset {
                    fallback
                } else {
                    color
                };
                crate::theme::color_rgb(color).map_or(color, |(r, g, b)| {
                    let dim = |channel| (u16::from(channel) * 105 / 255) as u8;
                    Color::Rgb(dim(r), dim(g), dim(b))
                })
            };
            cell.fg = dim(cell.fg, context.theme.text);
            cell.bg = dim(cell.bg, context.theme.canvas);
        } else {
            cell.modifier.insert(Modifier::DIM);
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
    use ratatui::{buffer::CellDiffOption, style::Color};
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
    use ratatui::buffer::CellDiffOption;
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
