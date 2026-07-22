use ratatui::layout::Rect;

pub const MIN_SHELL_TERMINAL_WIDTH: u16 = 50;
pub const MIN_SHELL_TERMINAL_HEIGHT: u16 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLayout {
    Compact(Rect),
    Full { top: Rect, main: Rect, status: Rect },
}

pub fn compute_shell_layout(area: Rect) -> ShellLayout {
    if area.width < MIN_SHELL_TERMINAL_WIDTH || area.height < MIN_SHELL_TERMINAL_HEIGHT {
        return ShellLayout::Compact(area);
    }

    let top = Rect::new(area.x, area.y, area.width, 3);
    let main_height = area.height.saturating_sub(6);
    let main = Rect::new(area.x, area.y.saturating_add(3), area.width, main_height);
    let status = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(3)),
        area.width,
        3,
    );

    ShellLayout::Full { top, main, status }
}
pub(crate) fn rect_contains(area: Rect, x: u16, y: u16) -> bool {
    area.width > 0
        && area.height > 0
        && x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
}

pub(crate) fn inset_rect(area: Rect, margin: u16) -> Rect {
    let doubled = margin.saturating_mul(2);
    Rect::new(
        area.x.saturating_add(margin.min(area.width)),
        area.y.saturating_add(margin.min(area.height)),
        area.width.saturating_sub(doubled),
        area.height.saturating_sub(doubled),
    )
}

pub(crate) fn line_in_rect(area: Rect, y: u16) -> Rect {
    if area.width == 0 || area.height == 0 || y < area.y || y >= area.y.saturating_add(area.height)
    {
        return Rect::new(area.x, area.y.saturating_add(area.height), 0, 0);
    }
    Rect::new(area.x, y, area.width, 1)
}

pub(crate) fn usize_to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

pub(crate) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}
