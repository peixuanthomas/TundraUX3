//! A child terminal model; its writer is connected only to that child's PTY.
use crate::input::{MouseProtocolEncoding, MouseProtocolMode};
use crate::snapshot::{TerminalCell, TerminalColor, TerminalCursorStyle, TerminalSnapshot};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use wezterm_cell::color::ColorAttribute;
use wezterm_escape_parser::Action;
use wezterm_escape_parser::csi::{CSI, DecPrivateMode, DecPrivateModeCode, Keyboard, Mode};
use wezterm_escape_parser::osc::{ITermProprietary, OperatingSystemCommand};
use wezterm_escape_parser::parser::Parser;
use wezterm_surface::{CursorShape, CursorVisibility};
use wezterm_term::{
    CellAttributes, Hyperlink, Intensity, Terminal, TerminalConfiguration, TerminalSize, Underline,
};

mod images;
use images::DecodedImage;

pub(crate) type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type WriterRef = Weak<Mutex<Box<dyn Write + Send>>>;
const SYNC_TIMEOUT: Duration = Duration::from_millis(200);
const MARKER_PREFIX: &str = "tundra-command-status:";

#[derive(Clone)]
struct ChildWriter(WriterRef);

impl Write for ChildWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .upgrade()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "terminal PTY is closed"))?
            .lock()
            .map_err(|_| io::Error::other("terminal writer lock was poisoned"))?
            .write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .upgrade()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "terminal PTY is closed"))?
            .lock()
            .map_err(|_| io::Error::other("terminal writer lock was poisoned"))?
            .flush()
    }
}

#[derive(Debug)]
struct Configuration {
    scrollback: usize,
}

impl TerminalConfiguration for Configuration {
    fn use_threaded_writer(&self) -> bool {
        false
    }

    fn canonicalize_pasted_newlines(&self) -> wezterm_term::config::NewlineCanon {
        wezterm_term::config::NewlineCanon::None
    }

    fn scrollback_size(&self) -> usize {
        self.scrollback
    }
    fn color_palette(&self) -> wezterm_term::color::ColorPalette {
        let mut palette = wezterm_term::color::ColorPalette::default();
        let colors = platform::terminal_colors();
        let rgb = |[r, g, b]: [u8; 3]| {
            wezterm_cell::color::SrgbaTuple(
                f32::from(r) / 255.,
                f32::from(g) / 255.,
                f32::from(b) / 255.,
                1.,
            )
        };
        for (index, color) in colors.palette.into_iter().enumerate() {
            if let Some(color) = color {
                palette.colors.0[index] = rgb(color);
            }
        }
        if let Some(color) = colors.foreground {
            palette.foreground = rgb(color);
        }
        if let Some(color) = colors.background {
            palette.background = rgb(color);
        }
        palette
    }
    fn enable_kitty_graphics(&self) -> bool {
        true
    }
    fn enable_kitty_keyboard(&self) -> bool {
        true
    }

    fn unicode_version(&self) -> wezterm_cell::UnicodeVersion {
        wezterm_cell::UnicodeVersion::new(14)
    }
}

struct CommandMarker {
    status: Option<bool>,
    alternate: bool,
}

pub(crate) struct Emulator {
    terminal: Terminal,
    parser: Parser,
    writer: WriterRef,
    scrollback: usize,
    cell_size: (u16, u16),
    application_cursor: bool,
    mouse_mode: MouseProtocolMode,
    mouse_encoding: MouseProtocolEncoding,
    synchronized: Option<Instant>,
    frozen: Option<TerminalSnapshot>,
    markers: HashMap<u64, CommandMarker>,
    scrollback_limit: usize,
    next_marker: u64,
    pending_marker: Option<u64>,
    images: HashMap<[u8; 32], DecodedImage>,
    image_bytes: usize,
}

impl Emulator {
    pub(crate) fn new(rows: u16, columns: u16, scrollback: usize, writer: SharedWriter) -> Self {
        let writer = Arc::downgrade(&writer);
        let terminal = Terminal::new(
            terminal_size(columns, rows, (8, 16)),
            Arc::new(Configuration { scrollback }),
            "TundraUX3",
            env!("CARGO_PKG_VERSION"),
            Box::new(ChildWriter(writer.clone())),
        );
        Self {
            terminal,
            parser: Parser::new(),
            writer,
            scrollback: 0,
            cell_size: (8, 16),
            application_cursor: false,
            mouse_mode: MouseProtocolMode::None,
            mouse_encoding: MouseProtocolEncoding::Default,
            synchronized: None,
            frozen: None,
            markers: HashMap::new(),
            scrollback_limit: scrollback,
            next_marker: 0,
            pending_marker: None,
            images: HashMap::new(),
            image_bytes: 0,
        }
    }

    pub(crate) fn size(&self) -> (u16, u16) {
        let size = self.terminal.get_size();
        (size.rows as u16, size.cols as u16)
    }

    pub(crate) fn cell_size(&self) -> (u16, u16) {
        self.cell_size
    }

    pub(crate) fn set_cell_size(&mut self, width: u16, height: u16) {
        self.cell_size = (width.max(1), height.max(1));
        let (rows, columns) = self.size();
        self.resize(columns, rows);
    }

    pub(crate) fn resize(&mut self, columns: u16, rows: u16) {
        self.release_sync();
        self.terminal
            .resize(terminal_size(columns, rows, self.cell_size));
        self.scrollback = self.scrollback.min(self.scrollback_rows());
    }

    pub(crate) fn input_modes(&self) -> (bool, bool) {
        (
            self.application_cursor,
            self.terminal.bracketed_paste_enabled(),
        )
    }

    pub(crate) fn send_key(&mut self, key: &ui::KeyEvent) -> io::Result<()> {
        let bytes = match self.terminal.get_keyboard_encoding() {
            termwiz::input::KeyboardEncoding::Kitty(flags)
                if flags.bits() & crate::input::SUPPORTED_KITTY_KEYBOARD_FLAGS != 0 =>
            {
                crate::input::kitty_key_event_bytes(key, flags.bits())
            }
            _ => crate::input::key_event_bytes(key, self.application_cursor),
        };
        if let Some(bytes) = bytes {
            self.write(&bytes)?;
        }
        Ok(())
    }

    pub(crate) fn send_focus(&mut self, focused: bool) {
        self.terminal.focus_changed(focused);
    }

    pub(crate) fn send_paste(&mut self, text: &str) -> io::Result<()> {
        self.terminal.send_paste(text).map_err(io::Error::other)
    }

    pub(crate) fn send_mouse(&mut self, mouse: ui::MouseEvent) -> io::Result<bool> {
        if !self.terminal.is_mouse_grabbed() {
            return Ok(false);
        }
        use ui::{MouseEventKind as Kind, ScrollDirection};
        use wezterm_term::{MouseButton as Button, MouseEventKind as EventKind};
        let button = |button| match button {
            ui::MouseButton::Left => Button::Left,
            ui::MouseButton::Middle => Button::Middle,
            ui::MouseButton::Right => Button::Right,
        };
        let (kind, button) = match mouse.kind {
            Kind::Down(b) => (EventKind::Press, button(b)),
            Kind::Up(b) => (EventKind::Release, button(b)),
            Kind::Drag(b) => (EventKind::Move, button(b)),
            Kind::Moved => (EventKind::Move, Button::None),
            Kind::Scroll(direction) => (
                EventKind::Press,
                match direction {
                    ScrollDirection::Up => Button::WheelUp(1),
                    ScrollDirection::Down => Button::WheelDown(1),
                    ScrollDirection::Left => Button::WheelLeft(1),
                    ScrollDirection::Right => Button::WheelRight(1),
                },
            ),
            Kind::Click(_) | Kind::DoubleClick(_) => return Ok(true),
        };
        let mut modifiers = wezterm_term::KeyModifiers::NONE;
        if mouse.modifiers.shift {
            modifiers |= wezterm_term::KeyModifiers::SHIFT;
        }
        if mouse.modifiers.alt {
            modifiers |= wezterm_term::KeyModifiers::ALT;
        }
        if mouse.modifiers.is_control() {
            modifiers |= wezterm_term::KeyModifiers::CTRL;
        }
        self.terminal
            .mouse_event(wezterm_term::MouseEvent {
                kind,
                button,
                modifiers,
                x: usize::from(mouse.column()),
                y: i64::from(mouse.row()),
                x_pixel_offset: 0,
                y_pixel_offset: 0,
            })
            .map_err(io::Error::other)?;
        Ok(true)
    }

    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let writer = self
            .writer
            .upgrade()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "terminal PTY is closed"))?;
        let mut writer = writer
            .lock()
            .map_err(|_| io::Error::other("terminal writer lock was poisoned"))?;
        writer.write_all(bytes)?;
        writer.flush()
    }

    pub(crate) fn process(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut actions = Vec::new();
        self.parser.parse(bytes, |action| actions.push(action));
        let mut history = Vec::new();
        let mut pending = Vec::new();
        for mut action in actions {
            if !allowed_action(&action) {
                continue;
            }
            let image = matches!(
                &action,
                Action::Sixel(_) | Action::KittyImage(_) | Action::OperatingSystemCommand(_)
            );
            if !image {
                history.extend_from_slice(action.to_string().as_bytes());
            }
            if let Action::CSI(CSI::Unspecified(sequence)) = &action {
                if sequence.control == 'z' {
                    match sequence.to_string().as_str() {
                        "777;0z" => {
                            self.flush_actions(&mut pending);
                            self.start_marker();
                            continue;
                        }
                        "777;1;0z" => {
                            self.flush_actions(&mut pending);
                            self.finish_marker(true);
                            continue;
                        }
                        "777;1;1z" => {
                            self.flush_actions(&mut pending);
                            self.finish_marker(false);
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            if let Action::CSI(CSI::Keyboard(
                Keyboard::SetKittyState { flags, .. } | Keyboard::PushKittyState { flags, .. },
            )) = &mut action
            {
                *flags = wezterm_escape_parser::csi::KittyKeyboardFlags::from_bits_truncate(
                    flags.bits() & crate::input::SUPPORTED_KITTY_KEYBOARD_FLAGS,
                );
            }
            if let Action::CSI(CSI::Mode(mode)) = &action {
                match mode {
                    Mode::SetDecPrivateMode(DecPrivateMode::Code(
                        DecPrivateModeCode::SynchronizedOutput,
                    )) => {
                        self.flush_actions(&mut pending);
                        if self.synchronized.is_none() {
                            self.frozen = Some(self.snapshot_live());
                            self.synchronized = Some(Instant::now());
                        }
                        continue;
                    }
                    Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                        DecPrivateModeCode::SynchronizedOutput,
                    )) => {
                        self.flush_actions(&mut pending);
                        self.release_sync();
                        continue;
                    }
                    Mode::QueryDecPrivateMode(DecPrivateMode::Code(
                        DecPrivateModeCode::SynchronizedOutput,
                    )) => {
                        let state = if self.synchronized.is_some() { 1 } else { 2 };
                        let _ = self.write(format!("\x1b[?2026;{state}$y").as_bytes());
                        continue;
                    }
                    _ => self.track_mode(mode),
                }
            }
            if matches!(
                &action,
                Action::Esc(wezterm_escape_parser::Esc::Code(
                    wezterm_escape_parser::EscCode::FullReset
                ))
            ) {
                self.application_cursor = false;
                self.mouse_mode = MouseProtocolMode::None;
                self.mouse_encoding = MouseProtocolEncoding::Default;
                self.release_sync();
                self.markers.clear();
                self.pending_marker = None;
            }
            pending.push(action);
        }
        self.flush_actions(&mut pending);
        self.scrollback = self.scrollback.min(self.scrollback_rows());
        self.prune_markers();
        history
    }

    fn flush_actions(&mut self, actions: &mut Vec<Action>) {
        if !actions.is_empty() {
            self.terminal.perform_actions(std::mem::take(actions));
        }
    }

    fn track_mode(&mut self, mode: &Mode) {
        let (mode, enabled) = match mode {
            Mode::SetDecPrivateMode(DecPrivateMode::Code(mode)) => (mode, true),
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(mode)) => (mode, false),
            _ => return,
        };
        match mode {
            DecPrivateModeCode::ApplicationCursorKeys => self.application_cursor = enabled,
            DecPrivateModeCode::MouseTracking => {
                self.mouse_mode = if enabled {
                    MouseProtocolMode::PressRelease
                } else {
                    MouseProtocolMode::None
                }
            }
            DecPrivateModeCode::ButtonEventMouse => {
                self.mouse_mode = if enabled {
                    MouseProtocolMode::ButtonMotion
                } else {
                    MouseProtocolMode::None
                }
            }
            DecPrivateModeCode::AnyEventMouse => {
                self.mouse_mode = if enabled {
                    MouseProtocolMode::AnyMotion
                } else {
                    MouseProtocolMode::None
                }
            }
            DecPrivateModeCode::SGRMouse => {
                self.mouse_encoding = if enabled {
                    MouseProtocolEncoding::Sgr
                } else {
                    MouseProtocolEncoding::Default
                }
            }
            DecPrivateModeCode::Utf8Mouse => {
                self.mouse_encoding = if enabled {
                    MouseProtocolEncoding::Utf8
                } else {
                    MouseProtocolEncoding::Default
                }
            }
            _ => {}
        }
    }

    fn start_marker(&mut self) {
        let cursor = self.terminal.cursor_pos();
        let alternate = self.terminal.is_alt_screen_active();
        for x in [cursor.x.saturating_sub(1), cursor.x] {
            if let Some(cell) = self.terminal.screen_mut().cell_mut(x, cursor.y) {
                if cell.str() != "○" {
                    continue;
                }
                if let Some(id) = marker_id(cell.attrs()) {
                    self.markers.remove(&id);
                }
                self.next_marker += 1;
                let id = self.next_marker;
                // Cell metadata follows reflow and scrollback; it is never a host hyperlink.
                cell.attrs_mut()
                    .set_hyperlink(Some(Arc::new(Hyperlink::new(format!(
                        "{MARKER_PREFIX}{id}"
                    )))));
                self.markers.insert(
                    id,
                    CommandMarker {
                        status: None,
                        alternate,
                    },
                );
                self.pending_marker = Some(id);
                break;
            }
        }
    }

    fn finish_marker(&mut self, success: bool) {
        if let Some(id) = self.pending_marker.take() {
            if let Some(status) = self.markers.get_mut(&id) {
                status.status = Some(success);
            }
        }
    }

    fn prune_markers(&mut self) {
        let (rows, columns) = self.size();
        let limit = (self.scrollback_limit + usize::from(rows) * 2)
            .saturating_mul(usize::from(columns))
            .max(64);
        if self.markers.len() <= limit {
            return;
        }
        let alternate = self.terminal.is_alt_screen_active();
        let mut retained = HashSet::new();
        self.terminal.screen().for_each_phys_line(|_, line| {
            for cell in line.visible_cells() {
                if let Some(id) = marker_id(cell.attrs()) {
                    retained.insert(id);
                }
            }
        });
        self.markers
            .retain(|id, marker| marker.alternate != alternate || retained.contains(id));
    }

    pub(crate) fn scrollback_rows(&self) -> usize {
        if self.terminal.is_alt_screen_active() {
            return 0;
        }
        self.terminal
            .screen()
            .scrollback_rows()
            .saturating_sub(self.terminal.get_size().rows)
    }

    pub(crate) fn set_scrollback(&mut self, rows: usize) -> bool {
        let next = rows.min(self.scrollback_rows());
        let changed = self.scrollback != next;
        self.scrollback = next;
        changed
    }

    fn release_sync(&mut self) {
        self.synchronized = None;
        self.frozen = None;
    }

    pub(crate) fn finish_output(&mut self) -> bool {
        let synchronized = self.synchronized.is_some();
        self.release_sync();
        synchronized
    }

    pub(crate) fn refresh_sync(&mut self) -> bool {
        if self
            .synchronized
            .is_some_and(|start| start.elapsed() >= SYNC_TIMEOUT)
        {
            self.release_sync();
            return true;
        }
        false
    }

    pub(crate) fn snapshot(&mut self) -> TerminalSnapshot {
        self.refresh_sync();
        if let Some(snapshot) = &self.frozen {
            return snapshot.clone();
        }
        self.snapshot_live()
    }

    pub(crate) fn ui_snapshot(&mut self) -> ui::CommandLineTerminalSnapshot {
        self.refresh_sync();
        if let Some(snapshot) = &self.frozen {
            return crate::snapshot::to_ui_snapshot(snapshot);
        }
        let (rows, columns) = self.size();
        let cursor = self.terminal.cursor_pos();
        let blank = ui::CommandLineCell {
            symbol: String::new(),
            style: Default::default(),
            cursor: false,
            command_status: None,
        };
        let mut snapshot = ui::CommandLineTerminalSnapshot {
            columns,
            rows,
            cursor_style: crate::snapshot::to_ui_cursor_style(cursor_style(cursor.shape)),
            scrollback_rows: self.scrollback_rows(),
            scrollback_offset: self.scrollback,
            cells: vec![blank; usize::from(columns) * usize::from(rows)],
            images: Vec::new(),
        };
        let mut cursor_on_continuation = false;
        let start = self
            .terminal
            .screen()
            .scrollback_rows()
            .saturating_sub(usize::from(rows))
            .saturating_sub(self.scrollback);
        self.terminal
            .screen()
            .with_phys_lines(start..start + usize::from(rows), |lines| {
                for (y, line) in lines.iter().enumerate() {
                    for cell in line.visible_cells() {
                        let x = cell.cell_index();
                        if x >= usize::from(columns) {
                            break;
                        }
                        let converted =
                            cell_snapshot(cell.str(), cell.width(), cell.attrs(), &self.markers);
                        snapshot.cells[y * usize::from(columns) + x] = ui::CommandLineCell {
                            symbol: converted.text,
                            style: ui::CommandLineCellStyle {
                                foreground: crate::snapshot::to_ui_color(&converted.foreground),
                                background: crate::snapshot::to_ui_color(&converted.background),
                                bold: converted.bold,
                                italic: converted.italic,
                                underline: converted.underline,
                                inverse: converted.inverse,
                            },
                            cursor: self.scrollback == 0
                                && cursor.visibility == CursorVisibility::Visible
                                && cursor.x == x
                                && cursor.y == y as i64,
                            command_status: crate::snapshot::to_ui_command_status(
                                converted.command_status,
                            ),
                        };
                        for continuation in x + 1..(x + cell.width()).min(usize::from(columns)) {
                            snapshot.cells[y * usize::from(columns) + continuation] =
                                ui::CommandLineCell::default();
                            cursor_on_continuation |=
                                cursor.y == y as i64 && cursor.x == continuation;
                        }
                    }
                }
            });
        if !cursor_on_continuation
            && self.scrollback == 0
            && cursor.visibility == CursorVisibility::Visible
            && cursor.x < usize::from(columns)
            && (0..i64::from(rows)).contains(&cursor.y)
        {
            snapshot.cells[cursor.y as usize * usize::from(columns) + cursor.x].cursor = true;
        }
        snapshot.images = self.snapshot_images();
        snapshot
    }

    fn snapshot_live(&mut self) -> TerminalSnapshot {
        let (rows, columns) = self.size();
        let cursor = self.terminal.cursor_pos();
        let blank = cell_snapshot("", 1, &CellAttributes::default(), &self.markers);
        let mut cells = vec![vec![blank; usize::from(columns)]; usize::from(rows)];
        let start = self
            .terminal
            .screen()
            .scrollback_rows()
            .saturating_sub(usize::from(rows))
            .saturating_sub(self.scrollback);
        self.terminal
            .screen()
            .with_phys_lines(start..start + usize::from(rows), |lines| {
                for (y, line) in lines.iter().enumerate() {
                    for cell in line.visible_cells() {
                        let x = cell.cell_index();
                        if x >= usize::from(columns) {
                            break;
                        }
                        cells[y][x] =
                            cell_snapshot(cell.str(), cell.width(), cell.attrs(), &self.markers);
                        for continuation in x + 1..(x + cell.width()).min(usize::from(columns)) {
                            cells[y][continuation].wide_continuation = true;
                        }
                    }
                }
            });
        TerminalSnapshot {
            rows,
            columns,
            cells,
            scrollback_rows: self.scrollback_rows(),
            scrollback_offset: self.scrollback,
            cursor_row: cursor.y.max(0) as u16,
            cursor_column: cursor.x as u16,
            cursor_visible: self.scrollback == 0 && cursor.visibility == CursorVisibility::Visible,
            cursor_style: cursor_style(cursor.shape),
            application_cursor: self.application_cursor,
            bracketed_paste: self.terminal.bracketed_paste_enabled(),
            mouse_protocol_mode: self.mouse_mode,
            mouse_protocol_encoding: self.mouse_encoding,
            title: String::new(),
            images: self.snapshot_images(),
        }
    }
}

fn terminal_size(columns: u16, rows: u16, cell: (u16, u16)) -> TerminalSize {
    TerminalSize {
        rows: usize::from(rows.max(1)),
        cols: usize::from(columns.max(1)),
        pixel_width: usize::from(columns.max(1)) * usize::from(cell.0),
        pixel_height: usize::from(rows.max(1)) * usize::from(cell.1),
        dpi: 0,
    }
}

fn allowed_action(action: &Action) -> bool {
    match action {
        Action::Esc(wezterm_escape_parser::Esc::Code(
            wezterm_escape_parser::EscCode::StringTerminator,
        )) => false,
        Action::OperatingSystemCommand(osc) => matches!(
            osc.as_ref(),
            OperatingSystemCommand::ChangeColorNumber(_)
                | OperatingSystemCommand::ChangeDynamicColors(_, _)
                | OperatingSystemCommand::ResetDynamicColor(_)
                | OperatingSystemCommand::ResetColors(_)
                | OperatingSystemCommand::ITermProprietary(
                    ITermProprietary::File(_) | ITermProprietary::RequestCellSize
                )
        ),
        _ => true,
    }
}

fn marker_id(attrs: &CellAttributes) -> Option<u64> {
    attrs
        .hyperlink()?
        .uri()
        .strip_prefix(MARKER_PREFIX)?
        .parse()
        .ok()
}

fn cell_snapshot(
    text: &str,
    width: usize,
    attrs: &CellAttributes,
    markers: &HashMap<u64, CommandMarker>,
) -> TerminalCell {
    let command_status =
        marker_id(attrs).and_then(|id| markers.get(&id).map(|marker| marker.status));
    TerminalCell {
        text: text.to_owned(),
        foreground: color(attrs.foreground()),
        background: color(attrs.background()),
        bold: attrs.intensity() == Intensity::Bold,
        italic: attrs.italic(),
        underline: attrs.underline() != Underline::None,
        inverse: attrs.reverse(),
        wide: width > 1,
        wide_continuation: false,
        command_status,
    }
}

fn color(color: ColorAttribute) -> TerminalColor {
    match color {
        ColorAttribute::Default => TerminalColor::Default,
        ColorAttribute::PaletteIndex(index) => TerminalColor::Indexed(index),
        ColorAttribute::TrueColorWithDefaultFallback(rgb)
        | ColorAttribute::TrueColorWithPaletteFallback(rgb, _) => {
            let (r, g, b, _) = rgb.to_srgb_u8();
            TerminalColor::Rgb(r, g, b)
        }
    }
}

fn cursor_style(style: CursorShape) -> TerminalCursorStyle {
    match style {
        CursorShape::Default => TerminalCursorStyle::Default,
        CursorShape::BlinkingBlock => TerminalCursorStyle::BlinkingBlock,
        CursorShape::SteadyBlock => TerminalCursorStyle::SteadyBlock,
        CursorShape::BlinkingUnderline => TerminalCursorStyle::BlinkingUnderline,
        CursorShape::SteadyUnderline => TerminalCursorStyle::SteadyUnderline,
        CursorShape::BlinkingBar => TerminalCursorStyle::BlinkingBar,
        CursorShape::SteadyBar => TerminalCursorStyle::SteadyBar,
    }
}

#[cfg(test)]
#[path = "../tests/unit/emulator.rs"]
mod tests;
