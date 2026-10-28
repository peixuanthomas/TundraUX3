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

use crate::components::{CommandStatus, Scrollbar, Surface};
use crate::{EditorImagePicker, RenderContext, TerminalImageCache, TundraTheme};

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
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommandLineCursorStyle {
    #[default]
    Default,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineCell {
    /// One visible terminal cell. Empty values render as a space.
    pub symbol: String,
    pub style: CommandLineCellStyle,
    pub cursor: bool,
    pub command_status: Option<CommandStatus>,
}

impl Default for CommandLineCell {
    fn default() -> Self {
        Self {
            symbol: " ".to_string(),
            style: CommandLineCellStyle::default(),
            cursor: false,
            command_status: None,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct TerminalImageFragment {
    pub image_id: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
    /// Source rectangle in image pixels: x, y, width, height.
    pub source: [u32; 4],
    /// Transparent left, top, right and bottom margins in child-terminal pixels.
    pub padding: [u16; 4],
    /// Pixel dimensions of one child-terminal cell.
    pub cell_size: [u16; 2],
    /// Destination in terminal content cells, before the host offset is applied.
    pub area: Rect,
    pub z_index: i32,
}

impl std::fmt::Debug for TerminalImageFragment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalImageFragment")
            .field("image_id", &self.image_id)
            .field("size", &(self.width, self.height))
            .field("rgba_bytes", &self.rgba.len())
            .field("source", &self.source)
            .field("padding", &self.padding)
            .field("cell_size", &self.cell_size)
            .field("area", &self.area)
            .field("z_index", &self.z_index)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineTerminalSnapshot {
    pub columns: u16,
    pub rows: u16,
    pub cursor_style: CommandLineCursorStyle,
    /// Number of retained rows above the live terminal screen.
    pub scrollback_rows: usize,
    /// Current distance from the live screen. Zero means the newest output is
    /// visible; larger values move toward the oldest retained row.
    pub scrollback_offset: usize,
    /// Row-major cells. Missing cells intentionally render as blanks, making a
    /// partially read parser snapshot safe to display.
    pub cells: Vec<CommandLineCell>,
    pub images: Vec<TerminalImageFragment>,
}

impl CommandLineTerminalSnapshot {
    pub fn blank(columns: u16, rows: u16) -> Self {
        Self {
            columns,
            rows,
            cursor_style: CommandLineCursorStyle::Default,
            scrollback_rows: 0,
            scrollback_offset: 0,
            cells: vec![CommandLineCell::default(); usize::from(columns) * usize::from(rows)],
            images: Vec::new(),
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
    pub title: Option<String>,
}

impl CommandLineViewModel {
    pub fn new(terminal: CommandLineTerminalSnapshot) -> Self {
        Self {
            terminal: Arc::new(terminal),
            process_state: CommandLineProcessState::Running,
            message: None,
            title: None,
        }
    }

    /// Reserves the process message row for drawing, resizing and pointer input.
    pub fn output_area(&self, mut terminal_area: Rect) -> Rect {
        if self.message.is_some() || !matches!(self.process_state, CommandLineProcessState::Running)
        {
            terminal_area.height = terminal_area.height.saturating_sub(1);
        }
        terminal_area
    }
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
    let visible_rows = usize::from(snapshot.rows.min(terminal_area.height)).max(1);
    let total_rows = snapshot.scrollback_rows.saturating_add(visible_rows);
    let visible_start = snapshot
        .scrollback_rows
        .saturating_sub(snapshot.scrollback_offset.min(snapshot.scrollback_rows));
    let (thumb_start, thumb_height) =
        Scrollbar::new(total_rows, visible_rows, visible_start).thumb_range(track);

    Some(CommandLineScrollbarLayout {
        track,
        thumb: Rect::new(
            track.x,
            track.y.saturating_add(thumb_start),
            1,
            thumb_height,
        ),
    })
}

pub fn render_command_line_content(
    frame: &mut Frame<'_>,
    main: Rect,
    terminal_area: Option<Rect>,
    model: &CommandLineViewModel,
    context: &RenderContext,
) {
    render_command_line_content_with_images(
        frame,
        main,
        terminal_area,
        model,
        context,
        &mut TerminalImageCache::default(),
        None,
    );
}

pub fn render_command_line_content_with_images(
    frame: &mut Frame<'_>,
    main: Rect,
    terminal_area: Option<Rect>,
    model: &CommandLineViewModel,
    context: &RenderContext,
    images: &mut TerminalImageCache,
    picker: Option<&EditorImagePicker>,
) {
    let theme = &context.compatibility_theme();
    Surface::new()
        .titled(
            model
                .title
                .clone()
                .unwrap_or_else(|| i18n::tr!("ui-command-line-command-line")),
        )
        .bordered(true)
        .render_frame(frame, main, context);

    let Some(terminal_area) = terminal_area else {
        images.clear();
        render_size_blocker(frame, panel_inner_area(main), theme);
        return;
    };

    let output_area = model.output_area(terminal_area);
    let content_area = command_line_content_area(output_area, model.terminal.as_ref());
    let message = command_line_process_message(model, theme);
    render_terminal_snapshot_cells(frame, content_area, model.terminal.as_ref(), theme);
    images.render(frame, content_area, model.terminal.as_ref(), picker);
    render_command_line_scrollbar(frame, output_area, model.terminal.as_ref(), context);
    if let Some((message, style)) = message {
        render_process_message(
            frame,
            Rect::new(
                terminal_area.x,
                terminal_area.bottom().saturating_sub(1),
                terminal_area.width,
                1,
            ),
            &message,
            style,
        );
    }
}

fn render_size_blocker(frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
    let message = i18n::tr!(
        "ui-command-line-minimum-size",
        min_command_line_terminal_width = MIN_COMMAND_LINE_TERMINAL_WIDTH,
        min_command_line_terminal_height = MIN_COMMAND_LINE_TERMINAL_HEIGHT
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
            i18n::tr!(
                if model.title.is_some() {
                    "ui-terminal-application-exited"
                } else {
                    "ui-command-line-exited"
                },
                code = *code
            ),
            theme.muted_style(),
        )),
        CommandLineProcessState::Failed { message } => Some((
            i18n::tr!(
                if model.title.is_some() {
                    "ui-terminal-application-failed"
                } else {
                    "ui-command-line-failed"
                },
                message = message
            ),
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

pub(crate) fn render_terminal_snapshot(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &CommandLineTerminalSnapshot,
    theme: &TundraTheme,
) {
    render_terminal_snapshot_cells(frame, area, snapshot, theme);
    TerminalImageCache::default().render(frame, area, snapshot, None);
}

fn render_terminal_snapshot_cells(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &CommandLineTerminalSnapshot,
    theme: &TundraTheme,
) {
    crate::automation::control(
        "terminal",
        "terminal",
        "",
        area,
        crate::components::ComponentState {
            focused: true,
            ..Default::default()
        },
        &["click", "text", "key", "scroll"],
    );
    let lines = (0..snapshot.rows)
        .map(|row| terminal_snapshot_line(snapshot, row, theme, true))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Left)
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn render_native_terminal_snapshot_with_images(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &CommandLineTerminalSnapshot,
    theme: &TundraTheme,
    focused: bool,
    images: &mut TerminalImageCache,
    picker: Option<&EditorImagePicker>,
) {
    crate::automation::control(
        "terminal",
        "terminal",
        "",
        area,
        crate::components::ComponentState {
            focused,
            ..Default::default()
        },
        &["click", "text", "key", "scroll"],
    );
    let lines = (0..snapshot.rows.min(area.height))
        .map(|row| terminal_snapshot_line(snapshot, row, theme, false))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
    images.render(frame, area, snapshot, picker);
    if focused
        && snapshot.scrollback_offset == 0
        && let Some((column, row)) = (0..snapshot.rows.min(area.height)).find_map(|row| {
            (0..snapshot.columns.min(area.width))
                .find(|column| snapshot.cell(*column, row).is_some_and(|cell| cell.cursor))
                .map(|column| (column, row))
        })
    {
        frame.set_cursor_position((area.x + column, area.y + row));
    }
}

fn terminal_snapshot_line(
    snapshot: &CommandLineTerminalSnapshot,
    row: u16,
    theme: &TundraTheme,
    software_cursor: bool,
) -> Line<'static> {
    let row_end = terminal_row_end(snapshot, row);
    if row_end == 0 {
        return Line::default();
    }

    let mut spans = Vec::new();
    let mut text = String::with_capacity(usize::from(row_end));
    let mut text_style = None;
    let mut column = 0_u16;
    while column < row_end {
        let Some(cell) = snapshot.cell(column, row) else {
            break;
        };
        let marker = cell.command_status.map(|status| status.span(theme));
        let symbol = marker.as_ref().map_or_else(
            || visible_symbol(&cell.symbol),
            |span| span.content.as_ref(),
        );
        let style = marker
            .as_ref()
            .map_or_else(|| cell_style(cell, software_cursor), |span| span.style);
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
        let symbol_width = u16::try_from(Span::raw(symbol).width())
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

fn cell_style(cell: &CommandLineCell, software_cursor: bool) -> Style {
    let mut style = Style::default();
    if let Some(color) = cell.style.foreground.into_ratatui() {
        style = style.fg(color);
    }
    if let Some(color) = cell.style.background.into_ratatui() {
        style = style.bg(color);
    }
    if cell.style.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.style.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.style.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.style.inverse ^ (software_cursor && cell.cursor) {
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
#[path = "../../tests/unit/screens/command_line/tests.rs"]
mod tests;
