//! Safe structured output for terminal rendering.
pub use vt100::CursorStyle as TerminalCursorStyle;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl From<vt100::Color> for TerminalColor {
    fn from(value: vt100::Color) -> Self {
        match value {
            vt100::Color::Default => Self::Default,
            vt100::Color::Idx(index) => Self::Indexed(index),
            vt100::Color::Rgb(red, green, blue) => Self::Rgb(red, green, blue),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCell {
    pub text: String,
    pub foreground: TerminalColor,
    pub background: TerminalColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub wide: bool,
    pub wide_continuation: bool,
    pub command_status: Option<Option<bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub rows: u16,
    pub columns: u16,
    pub cells: Vec<Vec<TerminalCell>>,
    pub scrollback_rows: usize,
    pub scrollback_offset: usize,
    pub cursor_row: u16,
    pub cursor_column: u16,
    pub cursor_visible: bool,
    pub cursor_style: TerminalCursorStyle,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub mouse_protocol_mode: vt100::MouseProtocolMode,
    pub mouse_protocol_encoding: vt100::MouseProtocolEncoding,
    pub title: String,
    pub images: Vec<ui::TerminalImageFragment>,
}

impl TerminalSnapshot {
    pub fn from_parser(parser: &mut vt100::Parser) -> Self {
        let scrollback_offset = parser.screen().scrollback();
        parser.set_scrollback(usize::MAX);
        let scrollback_rows = parser.screen().scrollback();
        parser.set_scrollback(scrollback_offset);
        let screen = parser.screen();
        let (rows, columns) = screen.size();
        let mut cells = Vec::with_capacity(usize::from(rows));
        for row in 0..rows {
            let mut snapshot_row = Vec::with_capacity(usize::from(columns));
            for column in 0..columns {
                let cell = screen.cell(row, column);
                snapshot_row.push(match cell {
                    Some(cell) => TerminalCell {
                        text: cell.contents(),
                        foreground: cell.fgcolor().into(),
                        background: cell.bgcolor().into(),
                        bold: cell.bold(),
                        italic: cell.italic(),
                        underline: cell.underline(),
                        inverse: cell.inverse(),
                        wide: cell.is_wide(),
                        wide_continuation: cell.is_wide_continuation(),
                        command_status: cell.command_status(),
                    },
                    None => TerminalCell {
                        text: String::new(),
                        foreground: TerminalColor::Default,
                        background: TerminalColor::Default,
                        bold: false,
                        italic: false,
                        underline: false,
                        inverse: false,
                        wide: false,
                        wide_continuation: false,
                        command_status: None,
                    },
                });
            }
            cells.push(snapshot_row);
        }
        let (cursor_row, cursor_column) = screen.cursor_position();
        Self {
            rows,
            columns,
            cells,
            scrollback_rows,
            scrollback_offset,
            cursor_row,
            cursor_column,
            cursor_visible: scrollback_offset == 0 && !screen.hide_cursor(),
            cursor_style: screen.cursor_style(),
            application_cursor: screen.application_cursor(),
            bracketed_paste: screen.bracketed_paste(),
            mouse_protocol_mode: screen.mouse_protocol_mode(),
            mouse_protocol_encoding: screen.mouse_protocol_encoding(),
            // OSC is filtered, so this will remain empty unless a future
            // parser API supplies a title from another safe source.
            title: screen.title().to_owned(),
            images: Vec::new(),
        }
    }
}

pub fn to_ui_snapshot(snapshot: &TerminalSnapshot) -> ui::CommandLineTerminalSnapshot {
    let mut cells = Vec::with_capacity(usize::from(snapshot.rows) * usize::from(snapshot.columns));
    for row in 0..snapshot.rows {
        for column in 0..snapshot.columns {
            let Some(cell) = snapshot
                .cells
                .get(usize::from(row))
                .and_then(|cells| cells.get(usize::from(column)))
                .filter(|cell| !cell.wide_continuation)
            else {
                cells.push(ui::CommandLineCell::default());
                continue;
            };
            cells.push(ui::CommandLineCell {
                symbol: cell.text.clone(),
                style: ui::CommandLineCellStyle {
                    foreground: to_ui_color(&cell.foreground),
                    background: to_ui_color(&cell.background),
                    bold: cell.bold,
                    italic: cell.italic,
                    underline: cell.underline,
                    inverse: cell.inverse,
                },
                command_status: to_ui_command_status(cell.command_status),
                cursor: snapshot.cursor_visible
                    && snapshot.cursor_row == row
                    && snapshot.cursor_column == column,
            });
        }
    }
    ui::CommandLineTerminalSnapshot {
        columns: snapshot.columns,
        rows: snapshot.rows,
        cursor_style: to_ui_cursor_style(snapshot.cursor_style),
        scrollback_rows: snapshot.scrollback_rows,
        scrollback_offset: snapshot.scrollback_offset,
        cells,
        images: snapshot.images.clone(),
    }
}

/// Builds the render snapshot without first allocating an intermediate copy
/// of every terminal cell and its text.
pub fn ui_snapshot_from_parser(parser: &mut vt100::Parser) -> ui::CommandLineTerminalSnapshot {
    let scrollback_offset = parser.screen().scrollback();
    parser.set_scrollback(usize::MAX);
    let scrollback_rows = parser.screen().scrollback();
    parser.set_scrollback(scrollback_offset);
    let screen = parser.screen();
    let (rows, columns) = screen.size();
    let (cursor_row, cursor_column) = screen.cursor_position();
    let cursor_visible = scrollback_offset == 0 && !screen.hide_cursor();
    let mut cells = Vec::with_capacity(usize::from(rows) * usize::from(columns));
    for row in 0..rows {
        for column in 0..columns {
            let cell = screen
                .cell(row, column)
                .filter(|cell| !cell.is_wide_continuation());
            cells.push(match cell {
                Some(cell) => ui::CommandLineCell {
                    symbol: cell.contents(),
                    style: ui::CommandLineCellStyle {
                        foreground: to_ui_color(&cell.fgcolor().into()),
                        background: to_ui_color(&cell.bgcolor().into()),
                        bold: cell.bold(),
                        italic: cell.italic(),
                        underline: cell.underline(),
                        inverse: cell.inverse(),
                    },
                    cursor: cursor_visible && cursor_row == row && cursor_column == column,
                    command_status: to_ui_command_status(cell.command_status()),
                },
                None => ui::CommandLineCell::default(),
            });
        }
    }
    ui::CommandLineTerminalSnapshot {
        columns,
        rows,
        cursor_style: to_ui_cursor_style(screen.cursor_style()),
        scrollback_rows,
        scrollback_offset,
        cells,
        images: Vec::new(),
    }
}

pub(crate) fn to_ui_cursor_style(style: TerminalCursorStyle) -> ui::CommandLineCursorStyle {
    match style {
        TerminalCursorStyle::Default => ui::CommandLineCursorStyle::Default,
        TerminalCursorStyle::BlinkingBlock => ui::CommandLineCursorStyle::BlinkingBlock,
        TerminalCursorStyle::SteadyBlock => ui::CommandLineCursorStyle::SteadyBlock,
        TerminalCursorStyle::BlinkingUnderline => ui::CommandLineCursorStyle::BlinkingUnderline,
        TerminalCursorStyle::SteadyUnderline => ui::CommandLineCursorStyle::SteadyUnderline,
        TerminalCursorStyle::BlinkingBar => ui::CommandLineCursorStyle::BlinkingBar,
        TerminalCursorStyle::SteadyBar => ui::CommandLineCursorStyle::SteadyBar,
    }
}

pub(crate) fn to_ui_command_status(
    status: Option<Option<bool>>,
) -> Option<ui::components::CommandStatus> {
    status.map(|status| match status {
        None => ui::components::CommandStatus::Pending,
        Some(true) => ui::components::CommandStatus::Succeeded,
        Some(false) => ui::components::CommandStatus::Failed,
    })
}

pub(crate) fn to_ui_color(color: &TerminalColor) -> ui::CommandLineColor {
    match *color {
        TerminalColor::Default => ui::CommandLineColor::Default,
        TerminalColor::Indexed(index) => ui::CommandLineColor::Indexed(index),
        TerminalColor::Rgb(red, green, blue) => ui::CommandLineColor::Rgb(red, green, blue),
    }
}

#[cfg(test)]
#[path = "../tests/unit/cursor_style.rs"]
mod cursor_style_tests;
