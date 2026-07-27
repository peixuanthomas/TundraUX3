//! Rendering primitives for the embedded Command Line application.
//!
//! This module intentionally does not know about PTYs or `vt100`. The Shell
//! adapter translates its terminal parser into [`CommandLineTerminalSnapshot`],
//! which keeps this crate small and makes the renderer straightforward to test.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Borders, Clear, Paragraph, Wrap};
use std::sync::Arc;

use crate::screens::shell::{
    ShellLayout, compute_shell_layout, render_compact_home, render_status, render_top,
};
use crate::{ShellChromeViewModel, TundraTheme};

/// Smallest outer terminal accepted by the embedded Command Line application.
pub const MIN_COMMAND_LINE_TERMINAL_WIDTH: u16 = 108;
pub const MIN_COMMAND_LINE_TERMINAL_HEIGHT: u16 = 22;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommandLineColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl CommandLineColor {
    fn into_ratatui(self) -> Option<Color> {
        match self {
            Self::Default => None,
            Self::Indexed(index) => Some(Color::Indexed(index)),
            Self::Rgb(red, green, blue) => Some(Color::Rgb(red, green, blue)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandLineCellStyle {
    pub foreground: CommandLineColor,
    pub background: CommandLineColor,
    pub bold: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineCell {
    /// One visible terminal cell. Empty values render as a space.
    pub symbol: String,
    pub style: CommandLineCellStyle,
    pub cursor: bool,
}

impl Default for CommandLineCell {
    fn default() -> Self {
        Self {
            symbol: " ".to_string(),
            style: CommandLineCellStyle::default(),
            cursor: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineTerminalSnapshot {
    pub columns: u16,
    pub rows: u16,
    /// Row-major cells. Missing cells intentionally render as blanks, making a
    /// partially read parser snapshot safe to display.
    pub cells: Vec<CommandLineCell>,
}

impl CommandLineTerminalSnapshot {
    pub fn blank(columns: u16, rows: u16) -> Self {
        Self {
            columns,
            rows,
            cells: vec![CommandLineCell::default(); usize::from(columns) * usize::from(rows)],
        }
    }

    pub fn cell(&self, column: u16, row: u16) -> Option<&CommandLineCell> {
        if column >= self.columns || row >= self.rows {
            return None;
        }
        self.cells
            .get(usize::from(row) * usize::from(self.columns) + usize::from(column))
    }

    pub fn set_cell(&mut self, column: u16, row: u16, cell: CommandLineCell) {
        if column >= self.columns || row >= self.rows {
            return;
        }
        let index = usize::from(row) * usize::from(self.columns) + usize::from(column);
        if let Some(slot) = self.cells.get_mut(index) {
            *slot = cell;
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CommandLineProcessState {
    #[default]
    Running,
    Exited {
        code: i32,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineViewModel {
    /// Immutable frame data shared with the Shell host. Terminal frames can
    /// contain thousands of owned graphemes, so cloning a view model must not
    /// duplicate every cell.
    pub terminal: Arc<CommandLineTerminalSnapshot>,
    pub process_state: CommandLineProcessState,
    /// A host-side message such as a spawn error or a restart hint.
    pub message: Option<String>,
    /// Plain-text prefix emitted by the embedded REPL. The renderer uses this
    /// semantic hint to apply the live application accent without injecting
    /// ANSI sequences that can break line-editor cursor calculations.
    pub prompt_label: Option<String>,
}

impl CommandLineViewModel {
    pub fn new(terminal: CommandLineTerminalSnapshot) -> Self {
        Self {
            terminal: Arc::new(terminal),
            process_state: CommandLineProcessState::Running,
            message: None,
            prompt_label: None,
        }
    }

    pub fn with_prompt_username(mut self, username: &str) -> Self {
        self.prompt_label = Some(format!("{username} >>"));
        self
    }
}

/// Returns the exact region represented by the child PTY.
///
/// Command Line uses the normal Shell title and status bars.  The PTY is the
/// inner rectangle of the central Command Line panel, so it can never draw
/// over global navigation, status, or the clock control.
pub fn command_line_terminal_area(area: Rect) -> Option<Rect> {
    (area.width >= MIN_COMMAND_LINE_TERMINAL_WIDTH
        && area.height >= MIN_COMMAND_LINE_TERMINAL_HEIGHT)
        .then(|| match compute_shell_layout(area) {
            ShellLayout::Full { main, .. } => Some(panel_inner_area(main)),
            ShellLayout::Compact(_) => None,
        })
        .flatten()
}

pub fn render_command_line(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &CommandLineViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_command_line_main(frame, main, area, model, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_command_line_main(
    frame: &mut Frame<'_>,
    main: Rect,
    outer_area: Rect,
    model: &CommandLineViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(
        theme
            .block()
            .title("Command Line")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        main,
    );

    let Some(terminal_area) = command_line_terminal_area(outer_area) else {
        render_size_blocker(frame, panel_inner_area(main), theme);
        return;
    };

    render_terminal_snapshot(
        frame,
        terminal_area,
        model.terminal.as_ref(),
        model.prompt_label.as_deref(),
        theme.accent_color,
    );
    if let Some((message, style)) = command_line_process_message(model, theme) {
        render_process_message(frame, terminal_area, &message, style);
    }
}

fn render_size_blocker(frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
    let message = format!(
        "Command Line needs at least {MIN_COMMAND_LINE_TERMINAL_WIDTH}x{MIN_COMMAND_LINE_TERMINAL_HEIGHT} terminal cells. Resize to continue."
    );
    frame.render_widget(
        Paragraph::new(message)
            .style(theme.error_style())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn command_line_process_message(
    model: &CommandLineViewModel,
    theme: &TundraTheme,
) -> Option<(String, Style)> {
    if let Some(message) = &model.message {
        return Some((message.clone(), theme.muted_style()));
    }
    match &model.process_state {
        CommandLineProcessState::Running => None,
        CommandLineProcessState::Exited { code } => Some((
            format!("CLI exited ({code}); Enter restart · Esc Launcher"),
            theme.muted_style(),
        )),
        CommandLineProcessState::Failed { message } => Some((
            format!("{message} · Enter restart · Esc Launcher"),
            theme.error_style(),
        )),
    }
}

fn render_process_message(frame: &mut Frame<'_>, area: Rect, message: &str, style: Style) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(message.to_string())
            .style(style)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn panel_inner_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

fn render_terminal_snapshot(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &CommandLineTerminalSnapshot,
    prompt_label: Option<&str>,
    accent_color: Color,
) {
    let buffer = frame.buffer_mut();
    for row in 0..area.height {
        let prompt_width = prompt_label
            .filter(|prompt| row_starts_with(snapshot, row, prompt))
            .map_or(0, str::len);
        for column in 0..area.width {
            let default_cell = CommandLineCell::default();
            let cell = snapshot.cell(column, row).unwrap_or(&default_cell);
            let position = (area.x.saturating_add(column), area.y.saturating_add(row));
            if let Some(target) = buffer.cell_mut(position) {
                target.reset();
                target
                    .set_symbol(visible_symbol(&cell.symbol))
                    .set_style(cell_style(cell));
                if usize::from(column) < prompt_width {
                    target.set_fg(accent_color);
                }
            }
        }
    }
}

fn row_starts_with(snapshot: &CommandLineTerminalSnapshot, row: u16, prefix: &str) -> bool {
    if prefix.is_empty() || prefix.len() > usize::from(snapshot.columns) {
        return false;
    }
    prefix.bytes().enumerate().all(|(column, expected)| {
        let Ok(column) = u16::try_from(column) else {
            return false;
        };
        snapshot
            .cell(column, row)
            .is_some_and(|cell| cell.symbol.as_bytes() == [expected])
    })
}

fn cell_style(cell: &CommandLineCell) -> Style {
    let mut foreground = cell.style.foreground;
    let mut background = cell.style.background;
    if cell.style.inverse {
        std::mem::swap(&mut foreground, &mut background);
    }
    let mut style = Style::default();
    if let Some(color) = foreground.into_ratatui() {
        style = style.fg(color);
    }
    if let Some(color) = background.into_ratatui() {
        style = style.bg(color);
    }
    if cell.style.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.style.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.cursor {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

fn visible_symbol(symbol: &str) -> &str {
    if symbol.is_empty() || symbol.chars().any(char::is_control) {
        " "
    } else {
        symbol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_outer_size_uses_the_standard_shell_main_panel() {
        assert_eq!(
            command_line_terminal_area(Rect::new(0, 0, 108, 22)),
            Some(Rect::new(1, 4, 106, 14))
        );
        assert!(command_line_terminal_area(Rect::new(0, 0, 107, 22)).is_none());
        assert!(command_line_terminal_area(Rect::new(0, 0, 108, 21)).is_none());
    }

    #[test]
    fn snapshot_handles_partial_cell_vectors_without_panicking() {
        let snapshot = CommandLineTerminalSnapshot {
            columns: 2,
            rows: 1,
            cells: vec![CommandLineCell {
                symbol: "A".to_string(),
                ..CommandLineCell::default()
            }],
        };
        assert_eq!(snapshot.cell(0, 0).unwrap().symbol, "A");
        assert!(snapshot.cell(1, 0).is_none());
    }

    #[test]
    fn visible_symbols_preserve_wide_and_combining_graphemes() {
        assert_eq!(visible_symbol("界"), "界");
        assert_eq!(visible_symbol("e\u{301}"), "e\u{301}");
        assert_eq!(visible_symbol("\u{1b}"), " ");
    }
}
