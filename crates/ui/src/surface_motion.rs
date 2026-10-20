use ratatui::{
    buffer::{Buffer, Cell, CellDiffOption},
    layout::{Margin, Rect},
    widgets::{Block, BorderType, Borders, Widget},
};

/// Move a surface's border while revealing whole characters at their native size.
pub fn morph_surface(source: &Buffer, area: Rect) -> Buffer {
    if area == source.area {
        return source.clone();
    }
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
    let inner = area.inner(Margin::new(margin, margin));
    let content = bounds.inner(Margin::new(margin, margin));
    if !content.is_empty() {
        let blank = Cell::new(" ")
            .set_style(source[(content.x, content.y)].style())
            .clone();
        result.content.fill(blank);
        let width = inner.width.min(content.width);
        let height = inner.height.min(content.height);
        let crop = Rect::new(
            content.x + (content.width - width) / 2,
            content.y + (content.height - height) / 2,
            width,
            height,
        );
        let destination = Rect::new(
            inner.x + (inner.width - width) / 2,
            inner.y + (inner.height - height) / 2,
            width,
            height,
        );
        for y in crop.y..crop.bottom() {
            let mut x = content.x;
            while x < crop.right() {
                let cell = &source[(x, y)];
                let width = ratatui::text::Span::raw(cell.symbol()).width().max(1) as u16;
                // Scan from the row's start so a cropped Chinese glyph is never split.
                if x >= crop.x && x + width <= crop.right() {
                    for offset in 0..width {
                        let cell = &source[(x + offset, y)];
                        if cell.diff_option != CellDiffOption::Skip {
                            result[(
                                destination.x + x - crop.x + offset,
                                destination.y + y - crop.y,
                            )] = cell.clone();
                        }
                    }
                }
                x += width;
            }
        }
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
