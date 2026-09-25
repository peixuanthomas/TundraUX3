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
    let mut main = Rect::new(area.x, area.y.saturating_add(3), area.width, main_height);
    if area.width >= 80 && area.height >= 24 {
        main = inset_rect(main, crate::SpringStyle::PAGE_INSET);
    }
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

/// Geometry shared by composition, hit testing and effects. Chrome stays fixed;
/// any legacy page projection is applied only to the content rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellFrameLayout {
    pub bounds: Rect,
    pub shell: ShellLayout,
    pub main: Rect,
    pub back_button: Option<Rect>,
    pub status_message: Option<Rect>,
    pub time_button: Option<Rect>,
}

impl ShellFrameLayout {
    pub fn new(bounds: Rect, time_label: Option<&str>, context: &crate::RenderContext) -> Self {
        let shell = compute_shell_layout(bounds);
        let back_button = match shell {
            ShellLayout::Full { top, .. } => Some(Rect::new(
                top.right().saturating_sub(7),
                top.y,
                7,
                top.height,
            )),
            ShellLayout::Compact(_) => None,
        };
        let (main, status_message, time_button) = match shell {
            ShellLayout::Compact(main) => (main, None, None),
            ShellLayout::Full { main, status, .. } => {
                let time = time_label
                    .map(|label| super::status_time_button_area(status, label))
                    .filter(|area| !area.is_empty());
                let width = time.map_or(status.width, |button| button.x.saturating_sub(status.x));
                (
                    context.page_area(main),
                    Some(Rect::new(status.x, status.y, width, status.height)),
                    time,
                )
            }
        };
        let shell = match shell {
            ShellLayout::Full { top, status, .. } => ShellLayout::Full { top, main, status },
            compact => compact,
        };
        Self {
            bounds,
            shell,
            main,
            back_button,
            status_message,
            time_button,
        }
    }

    pub fn is_compact(self) -> bool {
        matches!(self.shell, ShellLayout::Compact(_))
    }
}

/// PTY and its scrollbar share the compositor's main panel geometry.
pub fn command_line_terminal_area(area: Rect) -> Option<Rect> {
    let layout = ShellFrameLayout::new(area, None, &crate::RenderContext::default());
    command_line_terminal_area_in(&layout)
}

pub fn command_line_terminal_area_in(layout: &ShellFrameLayout) -> Option<Rect> {
    (!layout.is_compact()
        && layout.bounds.width >= crate::MIN_COMMAND_LINE_TERMINAL_WIDTH
        && layout.bounds.height >= crate::MIN_COMMAND_LINE_TERMINAL_HEIGHT)
        .then(|| {
            crate::components::Surface::new()
                .bordered(true)
                .inner(layout.main)
        })
}
