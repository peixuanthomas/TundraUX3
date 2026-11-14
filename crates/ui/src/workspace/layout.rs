use ratatui::layout::Rect;
use workspace_protocol::{MIN_CONTENT_SIZE, SessionId, Size};

use crate::{MIN_SHELL_TERMINAL_HEIGHT, MIN_SHELL_TERMINAL_WIDTH};

const PANE_TITLE_HEIGHT: usize = 1;
const PANE_GAP: usize = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceSplitMode {
    LeftRight,
    TopBottom,
    #[default]
    Grid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspacePaneLayout {
    pub id: SessionId,
    pub title: Option<Rect>,
    pub content: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLayout {
    pub available: Rect,
    pub capacity: usize,
    pub blocked: bool,
    pub size_needed: Size,
    pub panes: Vec<WorkspacePaneLayout>,
}

impl WorkspaceLayout {
    /// All live windows, including background windows, contribute their content minimum.
    /// Visible IDs are supplied in their established display order.
    pub fn new(
        terminal: Rect,
        available: Rect,
        windows: &[(SessionId, Size)],
        visible: &[SessionId],
        focused: Option<SessionId>,
        mode: WorkspaceSplitMode,
    ) -> Self {
        let limits = Limits::new(terminal, available, windows, mode);
        let capacity = limits.capacity();
        let count = visible.len().min(capacity);
        let focused = focused.filter(|id| visible.contains(id));
        let mut panes = Vec::with_capacity(count);
        if count == 1 {
            panes.push(WorkspacePaneLayout {
                id: focused.unwrap_or(visible[0]),
                title: None,
                content: available,
            });
        } else if count > 1 {
            let mut visible = visible[..count].to_vec();
            if let Some(focused) = focused
                && !visible.contains(&focused)
            {
                visible[count - 1] = focused;
            }
            let (rows, columns) = limits.grid(count);
            for (row, ids) in visible.chunks(columns).enumerate() {
                let (y, height) = split_span(available.y, available.height, rows, row);
                for (column, &id) in ids.iter().enumerate() {
                    let (x, width) = split_span(available.x, available.width, ids.len(), column);
                    panes.push(WorkspacePaneLayout {
                        id,
                        title: Some(Rect::new(x, y, width, PANE_TITLE_HEIGHT as u16)),
                        content: Rect::new(
                            x,
                            y + PANE_TITLE_HEIGHT as u16,
                            width,
                            height - PANE_TITLE_HEIGHT as u16,
                        ),
                    });
                }
            }
        }
        Self {
            available,
            capacity,
            blocked: capacity == 0,
            size_needed: limits.size_needed,
            panes,
        }
    }

    pub fn capacity(
        terminal: Rect,
        available: Rect,
        windows: &[(SessionId, Size)],
        mode: WorkspaceSplitMode,
    ) -> usize {
        Limits::new(terminal, available, windows, mode).capacity()
    }
}

struct Limits {
    size_needed: Size,
    single_fits: bool,
    rows: usize,
    columns: usize,
}

impl Limits {
    fn new(
        terminal: Rect,
        available: Rect,
        windows: &[(SessionId, Size)],
        mode: WorkspaceSplitMode,
    ) -> Self {
        let minimum = windows
            .iter()
            .fold(MIN_CONTENT_SIZE, |minimum, (_, size)| Size {
                width: minimum.width.max(size.width),
                height: minimum.height.max(size.height),
            });
        let size_needed = Size {
            width: minimum
                .width
                .saturating_add(terminal.width.saturating_sub(available.width)),
            height: minimum
                .height
                .saturating_add(terminal.height.saturating_sub(available.height))
                .max(MIN_SHELL_TERMINAL_HEIGHT),
        };
        let rows = (usize::from(available.height) + PANE_GAP)
            / (usize::from(minimum.height) + PANE_TITLE_HEIGHT + PANE_GAP);
        let columns =
            (usize::from(available.width) + PANE_GAP) / (usize::from(minimum.width) + PANE_GAP);
        let (rows, columns) = match mode {
            WorkspaceSplitMode::LeftRight => (rows.min(1), columns.min(2)),
            WorkspaceSplitMode::TopBottom => (rows.min(2), columns.min(1)),
            WorkspaceSplitMode::Grid => (rows, columns),
        };
        Self {
            size_needed,
            single_fits: terminal.width >= MIN_SHELL_TERMINAL_WIDTH
                && terminal.height >= MIN_SHELL_TERMINAL_HEIGHT
                && available.width >= minimum.width
                && available.height >= minimum.height,
            rows,
            columns,
        }
    }

    fn capacity(&self) -> usize {
        if self.single_fits {
            (self.rows * self.columns).max(1)
        } else {
            0
        }
    }

    fn grid(&self, count: usize) -> (usize, usize) {
        (1..=self.columns.min(count))
            .map(|columns| (count.div_ceil(columns), columns))
            .filter(|(rows, _)| *rows <= self.rows)
            .min_by_key(|&(rows, columns)| (rows.abs_diff(columns), rows * columns - count, rows))
            .expect("window count fits the grid")
    }
}

fn split_span(origin: u16, length: u16, count: usize, index: usize) -> (u16, u16) {
    let span = usize::from(length) - (count - 1) * PANE_GAP;
    let base = span / count;
    let remainder = span % count;
    let offset = index * (base + PANE_GAP) + index.min(remainder);
    let length = base + usize::from(index < remainder);
    (origin + offset as u16, length as u16)
}
