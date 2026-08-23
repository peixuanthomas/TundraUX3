//! Rendering primitives for the embedded Command Line application.
//!
//! This module intentionally does not know about PTYs or `vt100`. The Shell
//! adapter translates its terminal parser into [`CommandLineTerminalSnapshot`],
//! which keeps this crate small and makes the renderer straightforward to test.

use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use std::sync::Arc;

use crate::components::{Scrollbar, Surface};
use crate::screens::shell::{
    ShellLayout, compute_shell_layout, render_compact_home, render_status, render_top,
};
use crate::{RenderContext, ShellChromeViewModel, TundraTheme};

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
    /// Number of retained rows above the live terminal screen.
    pub scrollback_rows: usize,
    /// Current distance from the live screen. Zero means the newest output is
    /// visible; larger values move toward the oldest retained row.
    pub scrollback_offset: usize,
    /// Row-major cells. Missing cells intentionally render as blanks, making a
    /// partially read parser snapshot safe to display.
    pub cells: Vec<CommandLineCell>,
}

impl CommandLineTerminalSnapshot {
    pub fn blank(columns: u16, rows: u16) -> Self {
        Self {
            columns,
            rows,
            scrollback_rows: 0,
            scrollback_offset: 0,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandLineScrollbarLayout {
    pub track: Rect,
    pub thumb: Rect,
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

/// Returns the region available to the child PTY and its optional scrollbar.
///
/// Command Line uses the normal Shell title and status bars. The terminal
/// region is the inner rectangle of the central Command Line panel, so neither
/// the PTY nor its scrollbar can draw over global navigation, status, or the
/// clock control. Use [`command_line_content_area`] for the exact PTY viewport.
pub fn command_line_terminal_area(area: Rect) -> Option<Rect> {
    (area.width >= MIN_COMMAND_LINE_TERMINAL_WIDTH
        && area.height >= MIN_COMMAND_LINE_TERMINAL_HEIGHT)
        .then(|| match compute_shell_layout(area) {
            ShellLayout::Full { main, .. } => Some(panel_inner_area(main)),
            ShellLayout::Compact(_) => None,
        })
        .flatten()
}

/// Returns the PTY viewport within the Command Line panel. A single right-hand
/// cell is reserved once retained history exists so the ASCII scrollbar never
/// covers terminal output.
pub fn command_line_content_area(
    terminal_area: Rect,
    snapshot: &CommandLineTerminalSnapshot,
) -> Rect {
    let scrollbar_width =
        u16::from(command_line_scrollbar_layout(terminal_area, snapshot).is_some());
    Rect::new(
        terminal_area.x,
        terminal_area.y,
        terminal_area.width.saturating_sub(scrollbar_width),
        terminal_area.height,
    )
}

pub fn command_line_scrollbar_layout(
    terminal_area: Rect,
    snapshot: &CommandLineTerminalSnapshot,
) -> Option<CommandLineScrollbarLayout> {
    if snapshot.scrollback_rows == 0 || terminal_area.width < 2 || terminal_area.height == 0 {
        return None;
    }

    let track = Rect::new(
        terminal_area.right().saturating_sub(1),
        terminal_area.y,
        1,
        terminal_area.height,
    );
    let track_height = usize::from(track.height);
    let visible_rows = usize::from(snapshot.rows.min(terminal_area.height)).max(1);
    let total_rows = snapshot.scrollback_rows.saturating_add(visible_rows);
    let thumb_height = track_height
        .saturating_mul(visible_rows)
        .saturating_add(total_rows / 2)
        .checked_div(total_rows)
        .unwrap_or_default()
        .clamp(1, track_height);
    let thumb_travel = track_height.saturating_sub(thumb_height);
    let visible_start = snapshot
        .scrollback_rows
        .saturating_sub(snapshot.scrollback_offset.min(snapshot.scrollback_rows));
    let thumb_start = thumb_travel
        .saturating_mul(visible_start)
        .saturating_add(snapshot.scrollback_rows / 2)
        / snapshot.scrollback_rows;

    Some(CommandLineScrollbarLayout {
        track,
        thumb: Rect::new(
            track.x,
            track.y.saturating_add(usize_to_u16(thumb_start)),
            1,
            usize_to_u16(thumb_height),
        ),
    })
}

pub fn render_command_line(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &CommandLineViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_command_line_contextual(frame, area, chrome, model, &context);
}

pub fn render_command_line_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &CommandLineViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_command_line_main(frame, main, area, model, theme, context);
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
    context: &RenderContext,
) {
    Surface::new()
        .titled("Command Line")
        .bordered(true)
        .render_frame(frame, main, context);

    let Some(terminal_area) = command_line_terminal_area(outer_area) else {
        render_size_blocker(frame, panel_inner_area(main), theme);
        return;
    };

    let content_area = command_line_content_area(terminal_area, model.terminal.as_ref());
    render_terminal_snapshot(
        frame,
        content_area,
        model.terminal.as_ref(),
        model.prompt_label.as_deref(),
        theme.accent_color,
    );
    render_command_line_scrollbar(frame, terminal_area, model.terminal.as_ref(), context);
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
            .alignment(HorizontalAlignment::Center)
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
            .alignment(HorizontalAlignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_command_line_scrollbar(
    frame: &mut Frame<'_>,
    terminal_area: Rect,
    snapshot: &CommandLineTerminalSnapshot,
    context: &RenderContext,
) {
    let Some(scrollbar) = command_line_scrollbar_layout(terminal_area, snapshot) else {
        return;
    };
    let viewport = usize::from(snapshot.rows.min(terminal_area.height)).max(1);
    let content = snapshot.scrollback_rows.saturating_add(viewport);
    let offset = snapshot
        .scrollback_rows
        .saturating_sub(snapshot.scrollback_offset.min(snapshot.scrollback_rows));
    Scrollbar::new(content, viewport, offset).render_frame(frame, scrollbar.track, context);
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
    let lines = (0..snapshot.rows)
        .map(|row| terminal_snapshot_line(snapshot, row, prompt_label, accent_color))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Left)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn terminal_snapshot_line(
    snapshot: &CommandLineTerminalSnapshot,
    row: u16,
    prompt_label: Option<&str>,
    accent_color: Color,
) -> Line<'static> {
    let row_end = terminal_row_end(snapshot, row);
    if row_end == 0 {
        return Line::default();
    }

    let prompt_width = prompt_label
        .filter(|prompt| row_starts_with(snapshot, row, prompt))
        .map_or(0, str::len);
    let mut spans = Vec::new();
    let mut text = String::new();
    let mut text_style = None;
    let mut column = 0_u16;
    while column < row_end {
        let default_cell = CommandLineCell::default();
        let cell = snapshot.cell(column, row).unwrap_or(&default_cell);
        let symbol = visible_symbol(&cell.symbol);
        let mut style = cell_style(cell);
        if usize::from(column) < prompt_width {
            style = style.fg(accent_color);
        }
        if text_style.is_some_and(|current| current != style) {
            spans.push(Span::styled(
                std::mem::take(&mut text),
                text_style.take().unwrap(),
            ));
        }
        text_style = Some(style);
        text.push_str(symbol);

        // The terminal snapshot stores a wide grapheme in its leading cell
        // and leaves its continuation cells blank. Ratatui advances by the
        // grapheme's full display width, so those placeholders must be skipped.
        let symbol_width = u16::try_from(Line::from(symbol).width())
            .unwrap_or(u16::MAX)
            .max(1);
        column = column.saturating_add(symbol_width);
    }
    if let Some(style) = text_style {
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

fn terminal_row_end(snapshot: &CommandLineTerminalSnapshot, row: u16) -> u16 {
    (0..snapshot.columns)
        .rev()
        .find(|column| {
            snapshot.cell(*column, row).is_some_and(|cell| {
                cell.cursor
                    || cell.style != CommandLineCellStyle::default()
                    || visible_symbol(&cell.symbol) != " "
            })
        })
        .map_or(0, |column| column.saturating_add(1))
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

fn usize_to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
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
            scrollback_rows: 0,
            scrollback_offset: 0,
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

    #[test]
    fn scrollbar_reserves_the_right_column_and_tracks_history_position() {
        let area = Rect::new(3, 5, 10, 4);
        let mut snapshot = CommandLineTerminalSnapshot::blank(9, 4);
        snapshot.scrollback_rows = 4;

        let bottom = command_line_scrollbar_layout(area, &snapshot).expect("scrollbar");
        assert_eq!(bottom.track, Rect::new(12, 5, 1, 4));
        assert_eq!(bottom.thumb, Rect::new(12, 7, 1, 2));
        assert_eq!(
            command_line_content_area(area, &snapshot),
            Rect::new(3, 5, 9, 4)
        );

        snapshot.scrollback_offset = snapshot.scrollback_rows;
        let top = command_line_scrollbar_layout(area, &snapshot).expect("scrollbar");
        assert_eq!(top.thumb, Rect::new(12, 5, 1, 2));
    }

    #[test]
    fn live_terminal_without_history_uses_the_full_width() {
        let area = Rect::new(3, 5, 10, 4);
        let snapshot = CommandLineTerminalSnapshot::blank(10, 4);
        assert!(command_line_scrollbar_layout(area, &snapshot).is_none());
        assert_eq!(command_line_content_area(area, &snapshot), area);
    }
}
