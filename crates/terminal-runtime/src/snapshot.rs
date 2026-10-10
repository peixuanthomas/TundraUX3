//! Safe structured output for terminal rendering.
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
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub title: String,
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
            application_cursor: screen.application_cursor(),
            bracketed_paste: screen.bracketed_paste(),
            // OSC is filtered, so this will remain empty unless a future
            // parser API supplies a title from another safe source.
            title: screen.title().to_owned(),
        }
    }
}

pub fn to_ui_snapshot(snapshot: &TerminalSnapshot) -> ui::CommandLineTerminalSnapshot {
    let mut result = ui::CommandLineTerminalSnapshot::blank(snapshot.columns, snapshot.rows);
    result.scrollback_rows = snapshot.scrollback_rows;
    result.scrollback_offset = snapshot.scrollback_offset;
    for (row, cells) in snapshot.cells.iter().enumerate() {
        let Ok(row) = u16::try_from(row) else {
            break;
        };
        for (column, cell) in cells.iter().enumerate() {
            let Ok(column) = u16::try_from(column) else {
                break;
            };
            if cell.wide_continuation {
                continue;
            }
            result.set_cell(
                column,
                row,
                ui::CommandLineCell {
                    symbol: cell.text.clone(),
                    style: ui::CommandLineCellStyle {
                        foreground: to_ui_color(&cell.foreground),
                        background: to_ui_color(&cell.background),
                        bold: cell.bold,
                        underline: cell.underline,
                        inverse: cell.inverse,
                    },
                    command_status: cell.command_status.map(|status| match status {
                        None => ui::components::CommandStatus::Pending,
                        Some(true) => ui::components::CommandStatus::Succeeded,
                        Some(false) => ui::components::CommandStatus::Failed,
                    }),
                    cursor: snapshot.cursor_visible
                        && snapshot.cursor_row == row
                        && snapshot.cursor_column == column,
                },
            );
        }
    }
    result
}

fn to_ui_color(color: &TerminalColor) -> ui::CommandLineColor {
    match *color {
        TerminalColor::Default => ui::CommandLineColor::Default,
        TerminalColor::Indexed(index) => ui::CommandLineColor::Indexed(index),
        TerminalColor::Rgb(red, green, blue) => ui::CommandLineColor::Rgb(red, green, blue),
    }
}
