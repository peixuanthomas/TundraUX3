//! Connects the Command Line page to the independent terminal runtime.
use crate::{InputEvent, InputKey, KeyInput};
use platform::Platform;
use ratatui::layout::Rect;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
pub(super) use terminal_runtime::pty::*;
use watchdog::{AppWatchdog, ComponentId, ManagedTaskGroup};
const SCROLL_LINES_PER_NOTCH: usize = 3;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineHostEvent {
    None,
    ExitToCaller,
    ResetRequested,
    PanicRequested,
}

#[derive(Debug)]
enum CommandLineHostState {
    Inactive,
    Running(CommandLinePty),
    Exited { code: i32 },
    Failed { message: String },
}

/// Owns the PTY only while the Command Line screen is active. The ordinary
/// Shell state remains cloneable and deterministic for controller tests.
pub struct CommandLineHost {
    state: CommandLineHostState,
    snapshot: TerminalSnapshot,
    snapshot_revision: u64,
    ui_snapshot: Arc<ui::CommandLineTerminalSnapshot>,
    scrollbar_drag_offset: Option<u16>,
    reader_tasks: ManagedTaskGroup,
}

impl CommandLineHost {
    pub fn new(watchdog: AppWatchdog) -> Self {
        let snapshot = blank_terminal_snapshot();
        let ui_snapshot = Arc::new(to_ui_snapshot(&snapshot));
        Self {
            state: CommandLineHostState::Inactive,
            snapshot,
            snapshot_revision: 0,
            ui_snapshot,
            scrollbar_drag_offset: None,
            reader_tasks: watchdog
                .child_component(ComponentId::from_static("command-line"))
                .task_group("pty-reader"),
        }
    }

    pub fn ensure_started(
        &mut self,
        platform: &dyn Platform,
        username: &str,
        accent: ratatui::style::Color,
        directory: Option<&Path>,
    ) {
        if !matches!(self.state, CommandLineHostState::Inactive) {
            return;
        }

        let result = resolve_tundra_cli_program().and_then(|program| {
            let mut config = CommandLinePtyConfig::tundra_cli(program)
                .with_username(username)
                .with_accent_color(accent);
            if let Some(directory) = directory {
                config.cwd = Some(directory.to_path_buf());
            } else {
                let directories = platform
                    .user_dirs_for_user(username)
                    .map_err(io::Error::other)?;
                let documents = directories.documents();
                if documents.is_dir() {
                    config.cwd = Some(documents.to_path_buf());
                }
            }
            CommandLinePty::spawn(config, &self.reader_tasks)
        });
        match result {
            Ok(pty) => {
                let (snapshot, revision) = pty.snapshot_with_revision();
                self.install_snapshot(snapshot, revision);
                self.state = CommandLineHostState::Running(pty);
            }
            Err(error) => {
                self.state = CommandLineHostState::Failed {
                    message: format!("Could not start tundra-cli: {error}"),
                };
            }
        }
    }

    pub fn resize_terminal(&mut self, columns: u16, rows: u16) {
        // The caller has already validated the outer Shell layout.  These are
        // the dimensions of the inner, bordered terminal panel, so comparing
        // them with the larger outer-terminal minimum would reject every
        // valid Command Line window (108x22 maps to 106x14).
        if columns == 0 || rows == 0 {
            return;
        }
        let needs_resize = self.snapshot.columns != columns || self.snapshot.rows != rows;
        if !needs_resize {
            return;
        }
        self.scrollbar_drag_offset = None;
        let result = match &self.state {
            CommandLineHostState::Running(pty) => pty.resize(columns, rows),
            _ => return,
        };
        match result {
            Ok(()) => {
                self.refresh_snapshot();
            }
            Err(error) => {
                self.fail_running(format!("Could not resize CLI terminal: {error}"));
            }
        }
    }

    pub fn resize_to_area(&mut self, terminal_area: Rect) {
        let content_area = ui::command_line_content_area(terminal_area, self.ui_snapshot.as_ref());
        self.resize_terminal(content_area.width, content_area.height);
    }

    pub fn poll(&mut self) -> CommandLineHostEvent {
        let status = match &self.state {
            CommandLineHostState::Running(pty) => match pty.try_wait() {
                Ok(status) => status,
                Err(error) => {
                    self.fail_running(format!("Could not read CLI process status: {error}"));
                    return CommandLineHostEvent::None;
                }
            },
            _ => return CommandLineHostEvent::None,
        };

        let Some(status) = status else {
            self.refresh_snapshot();
            return CommandLineHostEvent::None;
        };

        let previous = std::mem::replace(&mut self.state, CommandLineHostState::Inactive);
        if let CommandLineHostState::Running(pty) = previous {
            let snapshot = pty.snapshot_after_exit();
            self.install_snapshot(snapshot, self.snapshot_revision);
        }

        if status.code == EMBEDDED_RESET_EXIT_CODE {
            return CommandLineHostEvent::ResetRequested;
        }
        if status.code == crate::COMMAND_LINE_PANIC_EXIT_CODE {
            return CommandLineHostEvent::PanicRequested;
        }
        if status.success {
            self.install_blank_snapshot();
            return CommandLineHostEvent::ExitToCaller;
        }

        self.state = CommandLineHostState::Exited {
            code: i32::try_from(status.code).unwrap_or(i32::MAX),
        };
        CommandLineHostEvent::None
    }

    pub fn handle_input(
        &mut self,
        input: &InputEvent,
        terminal_area: Option<Rect>,
    ) -> CommandLineHostEvent {
        if let InputEvent::Key(key) = input
            && key.phase.is_press_like()
            && is_emergency_termination(key)
        {
            self.terminate();
            return CommandLineHostEvent::ExitToCaller;
        }

        match &self.state {
            CommandLineHostState::Exited { .. } | CommandLineHostState::Failed { .. } => {
                if let InputEvent::Key(key) = input
                    && key.phase.is_press_like()
                {
                    match key.key {
                        InputKey::Enter => {
                            self.install_blank_snapshot();
                            self.state = CommandLineHostState::Inactive;
                        }
                        InputKey::Escape => return CommandLineHostEvent::ExitToCaller,
                        _ => {}
                    }
                }
                return CommandLineHostEvent::None;
            }
            CommandLineHostState::Inactive | CommandLineHostState::Running(_) => {}
        }

        let Some(terminal_area) = terminal_area else {
            return CommandLineHostEvent::None;
        };

        if let InputEvent::Mouse(mouse) = input {
            self.handle_mouse(*mouse, terminal_area);
            return CommandLineHostEvent::None;
        }

        let returns_to_live_output = match input {
            InputEvent::Key(key) => key.phase.is_press_like(),
            InputEvent::Paste(_) => true,
            _ => false,
        };
        if returns_to_live_output && self.snapshot.scrollback_offset > 0 {
            self.set_scrollback(0);
        }

        let write_result = match (&self.state, input) {
            (CommandLineHostState::Running(pty), InputEvent::Key(key))
                if key.phase.is_press_like() =>
            {
                key_event_bytes(key, self.snapshot.application_cursor)
                    .map_or(Ok(()), |bytes| pty.write(&bytes))
            }
            (CommandLineHostState::Running(pty), InputEvent::Paste(text)) => {
                pty.write(&paste_bytes(text, self.snapshot.bracketed_paste))
            }
            _ => Ok(()),
        };
        if let Err(error) = write_result {
            self.fail_running(format!("Could not write to CLI process: {error}"));
        }

        CommandLineHostEvent::None
    }

    pub fn view_model(&self) -> ui::CommandLineViewModel {
        let process_state = match &self.state {
            CommandLineHostState::Inactive | CommandLineHostState::Running(_) => {
                ui::CommandLineProcessState::Running
            }
            CommandLineHostState::Exited { code } => {
                ui::CommandLineProcessState::Exited { code: *code }
            }
            CommandLineHostState::Failed { message } => ui::CommandLineProcessState::Failed {
                message: message.clone(),
            },
        };
        ui::CommandLineViewModel {
            terminal: Arc::clone(&self.ui_snapshot),
            process_state,
            message: None,
        }
    }

    pub fn terminate(&mut self) {
        let previous = std::mem::replace(&mut self.state, CommandLineHostState::Inactive);
        if let CommandLineHostState::Running(pty) = previous {
            let _ = pty.force_terminate();
            drop(pty);
        }
        self.install_blank_snapshot();
    }

    fn handle_mouse(&mut self, mouse: ui::MouseEvent, terminal_area: Rect) {
        if matches!(mouse.kind, ui::MouseEventKind::Up(ui::MouseButton::Left)) {
            self.scrollbar_drag_offset = None;
            return;
        }

        let scrollbar = ui::command_line_scrollbar_layout(terminal_area, self.ui_snapshot.as_ref());
        if let (
            Some(grab_offset),
            ui::MouseEventKind::Drag(ui::MouseButton::Left),
            Some(scrollbar),
        ) = (self.scrollbar_drag_offset, mouse.kind, scrollbar)
        {
            let offset = command_line_scrollback_offset_for_thumb(
                self.snapshot.scrollback_rows,
                scrollbar,
                mouse.row(),
                grab_offset,
            );
            self.set_scrollback(offset);
            return;
        }

        if !rect_contains(terminal_area, mouse.column(), mouse.row()) {
            return;
        }

        match (mouse.kind, scrollbar) {
            (ui::MouseEventKind::Scroll(ui::ScrollDirection::Up), Some(_)) => {
                self.set_scrollback(
                    self.snapshot
                        .scrollback_offset
                        .saturating_add(SCROLL_LINES_PER_NOTCH),
                );
            }
            (ui::MouseEventKind::Scroll(ui::ScrollDirection::Down), Some(_)) => {
                self.set_scrollback(
                    self.snapshot
                        .scrollback_offset
                        .saturating_sub(SCROLL_LINES_PER_NOTCH),
                );
            }
            (ui::MouseEventKind::Down(ui::MouseButton::Left), Some(scrollbar))
                if rect_contains(scrollbar.thumb, mouse.column(), mouse.row()) =>
            {
                self.scrollbar_drag_offset = Some(mouse.row().saturating_sub(scrollbar.thumb.y));
            }
            _ => {}
        }
    }

    fn set_scrollback(&mut self, offset: usize) {
        let result = match &self.state {
            CommandLineHostState::Running(pty) => pty.set_scrollback(offset),
            _ => return,
        };
        match result {
            Ok(true) => self.refresh_snapshot(),
            Ok(false) => {}
            Err(error) => {
                self.fail_running(format!("Could not scroll CLI terminal: {error}"));
            }
        }
    }

    fn fail_running(&mut self, message: String) {
        let previous = std::mem::replace(&mut self.state, CommandLineHostState::Failed { message });
        if let CommandLineHostState::Running(pty) = previous {
            let _ = pty.force_terminate();
        }
    }

    fn refresh_snapshot(&mut self) {
        let update = match &self.state {
            CommandLineHostState::Running(pty) => pty.snapshot_if_changed(self.snapshot_revision),
            _ => None,
        };
        if let Some((snapshot, revision)) = update {
            self.install_snapshot(snapshot, revision);
        }
    }

    fn install_snapshot(&mut self, snapshot: TerminalSnapshot, revision: u64) {
        if snapshot.scrollback_rows == 0 {
            self.scrollbar_drag_offset = None;
        }
        self.ui_snapshot = Arc::new(to_ui_snapshot(&snapshot));
        self.snapshot = snapshot;
        self.snapshot_revision = revision;
    }

    fn install_blank_snapshot(&mut self) {
        self.install_snapshot(blank_terminal_snapshot(), 0);
    }
}

impl Drop for CommandLineHost {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn resolve_tundra_cli_program() -> io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    let parent = current.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("current executable has no parent: {}", current.display()),
        )
    })?;
    let program = parent.join(format!("tundra-cli{}", std::env::consts::EXE_SUFFIX));
    let metadata = std::fs::metadata(&program).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{} is unavailable: {error}", program.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a file", program.display()),
        ));
    }
    Ok(program)
}

fn is_emergency_termination(key: &KeyInput) -> bool {
    matches!(key.key, InputKey::Char('x' | 'X'))
        && key.modifiers.is_control()
        && key.modifiers.shift
        && !key.modifiers.alt
        && !key.modifiers.super_key
        && !key.modifiers.hyper
        && !key.modifiers.meta
}

fn blank_terminal_snapshot() -> TerminalSnapshot {
    TerminalSnapshot::from_parser(&mut vt100::Parser::new(DEFAULT_ROWS, DEFAULT_COLUMNS, 0))
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

fn command_line_scrollback_offset_for_thumb(
    scrollback_rows: usize,
    scrollbar: ui::CommandLineScrollbarLayout,
    pointer_row: u16,
    grab_offset: u16,
) -> usize {
    let travel = usize::from(
        scrollbar
            .track
            .height
            .saturating_sub(scrollbar.thumb.height),
    );
    if travel == 0 || scrollback_rows == 0 {
        return 0;
    }
    let pointer_offset = usize::from(pointer_row.saturating_sub(scrollbar.track.y));
    let thumb_start = pointer_offset
        .saturating_sub(usize::from(grab_offset))
        .min(travel);
    let visible_start = scrollback_rows
        .saturating_mul(thumb_start)
        .saturating_add(travel / 2)
        / travel;
    scrollback_rows.saturating_sub(visible_start)
}

#[cfg(test)]
#[path = "../../tests/unit/session/command_line_runtime/tests.rs"]
mod tests;
