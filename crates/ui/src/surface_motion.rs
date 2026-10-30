use ratatui::{
    buffer::{Buffer, Cell},
    layout::{Margin, Rect},
    widgets::{Block, BorderType, Borders, Widget},
};

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
