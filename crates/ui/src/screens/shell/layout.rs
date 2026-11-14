use ratatui::layout::Rect;

pub const MIN_SHELL_TERMINAL_WIDTH: u16 = 108;
pub const MIN_SHELL_TERMINAL_HEIGHT: u16 = 20;

// Compact chrome is a rendering fallback; Shell enforces the terminal minimum.
const COMPACT_CHROME_WIDTH: u16 = 50;
const COMPACT_CHROME_HEIGHT: u16 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLayout {
    Compact(Rect),
    Full { top: Rect, main: Rect, status: Rect },
}

pub fn compute_shell_layout(area: Rect) -> ShellLayout {
    if area.width < COMPACT_CHROME_WIDTH || area.height < COMPACT_CHROME_HEIGHT {
        let header_height = area.height.min(1);
        return ShellLayout::Compact(Rect::new(
            area.x,
            area.y.saturating_add(header_height),
            area.width,
            area.height.saturating_sub(header_height),
        ));
    }

    let top = Rect::new(area.x, area.y, area.width, 1);
    let main = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(2),
    );
    let status = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(1)),
        area.width,
        1,
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

/// Geometry shared by composition, hit testing and effects, including keyboard
/// animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellFrameLayout {
    pub bounds: Rect,
    pub shell: ShellLayout,
    pub main: Rect,
    pub back_button: Option<Rect>,
    pub help_button: Option<Rect>,
    pub tasks_button: Option<Rect>,
    pub status_message: Option<Rect>,
    pub workspace_taskbar: Option<Rect>,
    pub time_button: Option<Rect>,
    pub keyboard_launcher: Option<Rect>,
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
            ShellLayout::Compact(_) => (!bounds.is_empty()).then(|| {
                let width = bounds.width.min(7);
                Rect::new(bounds.right().saturating_sub(width), bounds.y, width, 1)
            }),
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
            help_button: None,
            tasks_button: None,
            status_message,
            workspace_taskbar: None,
            time_button,
            keyboard_launcher: None,
        }
    }

    pub fn is_compact(self) -> bool {
        matches!(self.shell, ShellLayout::Compact(_))
    }

    /// Chrome and page geometry follow the same progress as the visible keys.
    pub fn with_input_keyboard(mut self, keyboard: Rect, visibility: u16) -> Self {
        if visibility == 0 {
            return self;
        }
        match self.shell {
            ShellLayout::Full {
                top, mut status, ..
            } => {
                status.y = self.bounds.bottom();
                status.height = 0;
                let bottom = keyboard.y.min(self.bounds.bottom());
                self.main.height = bottom.saturating_sub(self.main.y);
                self.shell = ShellLayout::Full {
                    top,
                    main: self.main,
                    status,
                };
                self.status_message = None;
                self.workspace_taskbar = None;
                self.time_button = None;
                self.keyboard_launcher = None;
            }
            ShellLayout::Compact(_) => {
                self.main.height = keyboard
                    .y
                    .min(self.bounds.bottom())
                    .saturating_sub(self.main.y);
                self.shell = ShellLayout::Compact(self.main);
                self.keyboard_launcher = None;
            }
        }
        self
    }

    /// The keyboard entry shares the status row, immediately before the clock.
    pub fn with_keyboard_launcher(mut self, available: bool) -> Self {
        if !available || self.keyboard_launcher.is_some() {
            return self;
        }
        let Some(mut message) = self.status_message else {
            if self.is_compact() && !self.main.is_empty() {
                self.keyboard_launcher = Some(crate::screen_keyboard_launcher_area(self.main));
                self.main.height = self.main.height.saturating_sub(1);
                self.shell = ShellLayout::Compact(self.main);
            }
            return self;
        };
        let launcher = crate::screen_keyboard_launcher_area(message);
        message.width = launcher.x.saturating_sub(message.x);
        self.status_message = (!message.is_empty()).then_some(message);
        self.keyboard_launcher = (!launcher.is_empty()).then_some(launcher);
        self
    }

    /// Size the button from the same shortcut that the compositor paints.
    pub fn with_back_shortcut(mut self, shortcut: &str) -> Self {
        if let Some(button) = self.back_button.as_mut() {
            let width = crate::components::terminal_width(shortcut).saturating_add(4);
            button.width = usize_to_u16(width).min(self.bounds.width);
            button.x = self.bounds.right().saturating_sub(button.width);
        }
        if let Some(help) = self.help_button.as_mut() {
            let right = self.back_button.map_or(self.bounds.right(), |back| back.x);
            help.width = help.width.min(right.saturating_sub(self.bounds.x + 1));
            help.x = right.saturating_sub(help.width.saturating_add(1));
        }
        if let Some(tasks) = self.tasks_button.as_mut() {
            let right = self
                .help_button
                .or(self.back_button)
                .map_or(self.bounds.right(), |button| button.x);
            tasks.width = tasks.width.min(right.saturating_sub(self.bounds.x + 1));
            tasks.x = right.saturating_sub(tasks.width.saturating_add(1));
        }
        self
    }

    pub fn with_help_button(mut self, available: bool) -> Self {
        if !available || self.is_compact() {
            self.help_button = None;
            return self;
        }
        let Some(back) = self.back_button else {
            self.help_button = None;
            return self;
        };
        let label = i18n::tr!("ui-shell-help-button");
        let width = usize_to_u16(crate::components::terminal_width(&label).saturating_add(2))
            .min(back.x.saturating_sub(self.bounds.x + 1));
        let x = back.x.saturating_sub(width.saturating_add(1));
        self.help_button =
            (width > 0 && x >= self.bounds.x).then(|| Rect::new(x, back.y, width, back.height));
        self
    }

    pub fn with_tasks_button(mut self, label: Option<&str>) -> Self {
        self.tasks_button = label.and_then(|label| {
            let next = self.help_button.or(self.back_button)?;
            let width = usize_to_u16(crate::components::terminal_width(label).saturating_add(2))
                .min(next.x.saturating_sub(self.bounds.x + 1));
            let x = next.x.checked_sub(width.saturating_add(1))?;
            (width > 0 && x >= self.bounds.x).then_some(Rect::new(x, next.y, width, next.height))
        });
        self
    }

    /// Running windows share the status row, keeping errors and global controls reachable.
    pub fn with_workspace_taskbar(mut self, available: bool, status_visible: bool) -> Self {
        if available && self.workspace_taskbar.is_none() {
            self.workspace_taskbar = self.status_message.take();
            if status_visible && let Some(taskbar) = self.workspace_taskbar.as_mut() {
                let width = taskbar.width / 3;
                self.status_message = Some(Rect::new(taskbar.x, taskbar.y, width, 1));
                taskbar.x += width;
                taskbar.width -= width;
            }
        }
        self
    }

    /// Keep the compact Escape button available even while a shell dialog is open.
    pub fn modal_area(self) -> Rect {
        if self.is_compact() || self.status_message.or(self.workspace_taskbar).is_none() {
            self.main
        } else {
            self.bounds
        }
    }
}

/// PTY viewport on a standalone terminal, with the physical size limit.
pub fn command_line_terminal_area(area: Rect) -> Option<Rect> {
    if area.width < crate::MIN_COMMAND_LINE_TERMINAL_WIDTH
        || area.height < crate::MIN_COMMAND_LINE_TERMINAL_HEIGHT
    {
        return None;
    }
    let layout = ShellFrameLayout::new(area, None, &crate::RenderContext::default());
    command_line_terminal_area_in(&layout)
}

/// PTY and scrollbar geometry in the compositor, including AppHost virtual layouts.
pub fn command_line_terminal_area_in(layout: &ShellFrameLayout) -> Option<Rect> {
    let minimum = workspace_protocol::MIN_CONTENT_SIZE;
    (!layout.is_compact()
        && layout.bounds.width >= minimum.width
        && layout.bounds.height >= minimum.height + 2
        && !layout.main.is_empty())
    .then_some(layout.main)
}
