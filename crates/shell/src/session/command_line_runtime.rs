//! Connects the Command Line page to the independent terminal runtime.
use crate::{InputEvent, InputKey, PageInstanceId};
use platform::Platform;
use ratatui::layout::Rect;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
pub(super) use terminal_runtime::pty::*;
use watchdog::{AppWatchdog, ComponentId, ManagedTaskGroup};
const SCROLL_LINES_PER_NOTCH: usize = 3;
const EMBEDDED_RESET_EXIT_CODE: u32 = 75;
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
    Running(PtyProcess),
    Exited { code: i32 },
    Failed { message: String },
}

/// Owns one page instance's PTY, including while another page covers it. The ordinary
/// Shell state remains cloneable and deterministic for controller tests.
pub struct CommandLineHost {
    state: CommandLineHostState,
    snapshot: TerminalSnapshot,
    snapshot_revision: u64,
    ui_snapshot: Arc<ui::CommandLineTerminalSnapshot>,
    scrollbar_drag_offset: Option<u16>,
    reader_tasks: ManagedTaskGroup,
    history: Option<platform::task_history::TaskRecorder>,
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
            history: None,
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
        self.ensure_started_recorded(platform, username, accent, directory, None);
    }

    fn ensure_started_recorded(
        &mut self,
        platform: &dyn Platform,
        username: &str,
        accent: ratatui::style::Color,
        directory: Option<&Path>,
        history: Option<platform::task_history::TaskRecorder>,
    ) {
        if !matches!(self.state, CommandLineHostState::Inactive) {
            return;
        }
        self.history = history;

        let result = resolve_tundra_cli_program().and_then(|program| {
            let mut config = PtyConfig::new(program);
            config.args = vec!["repl".into(), "--embedded".into()];
            config.env = vec![
                ("TUNDRA_COMMAND_LINE_USERNAME".into(), username.into()),
                (
                    "TUNDRA_COMMAND_LINE_ACCENT".into(),
                    terminal_runtime::ansi_foreground(accent).into(),
                ),
            ];
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
            PtyProcess::spawn_recorded(config, &self.reader_tasks, self.history.clone())
        });
        match result {
            Ok(pty) => {
                let (snapshot, revision) = pty.snapshot_with_revision();
                self.install_snapshot(snapshot, revision);
                self.state = CommandLineHostState::Running(pty);
            }
            Err(error) => {
                if let Some(history) = &self.history {
                    history.finish(
                        platform::task_history::TaskStatus::Failed,
                        error.to_string(),
                    );
                }
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
            if let Some(history) = &self.history {
                history.finish(
                    platform::task_history::TaskStatus::Succeeded,
                    i18n::tr!("task-exit-code", code = status.code),
                );
            }
            return CommandLineHostEvent::ResetRequested;
        }
        if status.code == crate::COMMAND_LINE_PANIC_EXIT_CODE {
            if let Some(history) = &self.history {
                history.finish(
                    platform::task_history::TaskStatus::Failed,
                    i18n::tr!("task-exit-code", code = status.code),
                );
            }
            return CommandLineHostEvent::PanicRequested;
        }
        if status.success {
            if let Some(history) = &self.history {
                history.finish(
                    platform::task_history::TaskStatus::Succeeded,
                    i18n::tr!("task-exit-code", code = status.code),
                );
            }
            self.install_blank_snapshot();
            return CommandLineHostEvent::ExitToCaller;
        }

        self.state = CommandLineHostState::Exited {
            code: i32::try_from(status.code).unwrap_or(i32::MAX),
        };
        if let Some(history) = &self.history {
            history.finish(
                platform::task_history::TaskStatus::Failed,
                i18n::tr!("task-exit-code", code = status.code),
            );
        }
        CommandLineHostEvent::None
    }

    pub fn handle_input(
        &mut self,
        input: &InputEvent,
        terminal_area: Option<Rect>,
    ) -> CommandLineHostEvent {
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
                pty.send_key(key)
            }
            (CommandLineHostState::Running(pty), InputEvent::Paste(text)) => pty.send_paste(text),
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

    pub fn is_running(&self) -> bool {
        matches!(self.state, CommandLineHostState::Running(_))
    }

    /// A confirmed close must report a failure instead of hiding a live child.
    pub fn try_terminate(&mut self) -> io::Result<()> {
        if let CommandLineHostState::Running(pty) = &self.state {
            pty.force_terminate()?;
            if let Some(history) = &self.history {
                history.finish(
                    platform::task_history::TaskStatus::Cancelled,
                    i18n::tr!("task-status-cancelled"),
                );
            }
        }
        let previous = std::mem::replace(&mut self.state, CommandLineHostState::Inactive);
        drop(previous);
        self.install_blank_snapshot();
        Ok(())
    }

    pub fn terminate(&mut self) {
        let _ = self.try_terminate();
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

/// Runtime resources follow page ids, rather than whichever page happens to be
/// visible. ShellSession can therefore save ordinary page state without cloning
/// a PTY, and covering a terminal never stops its process or output reader.
pub(super) struct CommandLineHosts {
    hosts: HashMap<PageInstanceId, CommandLineHost>,
    tasks: HashMap<PageInstanceId, CommandLineTask>,
    empty: CommandLineHost,
    watchdog: AppWatchdog,
}

struct CommandLineTask {
    request: u64,
    cancelled: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl Drop for CommandLineHosts {
    fn drop(&mut self) {
        let _ = self.terminate_all();
    }
}

impl CommandLineHosts {
    pub(super) fn new(watchdog: AppWatchdog) -> Self {
        Self {
            hosts: HashMap::new(),
            tasks: HashMap::new(),
            empty: CommandLineHost::new(watchdog.clone()),
            watchdog,
        }
    }

    pub(super) fn visible_host(&self, state: &super::ShellSession) -> &CommandLineHost {
        self.hosts
            .get(&state.content_page_id())
            .unwrap_or(&self.empty)
    }

    pub(super) fn has_running_processes(&self) -> bool {
        self.hosts.values().any(CommandLineHost::is_running)
    }

    pub(super) fn terminate_all(&mut self) -> io::Result<()> {
        let mut first_error = None;
        for host in self.hosts.values_mut() {
            if let Err(error) = host.try_terminate() {
                first_error.get_or_insert(error);
            }
        }
        // Drop retries failed termination and releases each host's PTY handles. The
        // caller must propagate the error before allowing another session.
        self.hosts.clear();
        for task in self.tasks.values() {
            task.finished.store(true, Ordering::Relaxed);
        }
        first_error.map_or(Ok(()), Err)
    }

    fn sync_tasks(&mut self, state: &mut super::ShellSession) {
        use super::{TaskClass, TaskControl, TaskDomain, TaskOwner};
        self.tasks.retain(|id, task| {
            if self.hosts.get(id).is_some_and(CommandLineHost::is_running) {
                return true;
            }
            task.finished.store(true, Ordering::Relaxed);
            state.tasks.complete(TaskDomain::CommandLine, task.request);
            false
        });
        for (id, host) in &self.hosts {
            if !host.is_running() || self.tasks.contains_key(id) {
                continue;
            }
            let request = PageInstanceId::new().get();
            let cancelled = Arc::new(AtomicBool::new(false));
            let finished = Arc::new(AtomicBool::new(false));
            let cancel_signal = cancelled.clone();
            let finished_signal = finished.clone();
            state.tasks.register_recorded(
                TaskDomain::CommandLine,
                request,
                TaskOwner::Page(*id),
                TaskClass::Operation,
                TaskControl::new(
                    move || cancel_signal.store(true, Ordering::Relaxed),
                    move || finished_signal.load(Ordering::Relaxed),
                ),
                host.history.clone(),
            );
            self.tasks.insert(
                *id,
                CommandLineTask {
                    request,
                    cancelled,
                    finished,
                },
            );
        }
    }

    pub(super) fn poll(
        &mut self,
        state: &mut super::ShellSession,
        platform: &dyn Platform,
        accent: ratatui::style::Color,
    ) -> (bool, CommandLineHostEvent) {
        let visible_before = (state.content_screen() == crate::ShellScreen::CommandLine)
            .then(|| self.visible_host(state).view_model());
        let cancelled = self
            .tasks
            .iter()
            .filter(|(_, task)| task.cancelled.swap(false, Ordering::Relaxed))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in cancelled {
            if let Some(host) = self.hosts.get_mut(&id) {
                match host.try_terminate() {
                    Ok(()) => state.close_page_instance(id),
                    Err(error) => state.notify_alert_with_tone(
                        i18n::msg!(
                            "shell-command-line-close-failed",
                            reason = error.to_string()
                        ),
                        ui::NotificationTone::Error,
                    ),
                }
            }
        }
        let live_ids = state.live_page_ids();
        self.hosts.retain(|id, _| live_ids.contains(id));

        if state.content_screen() == crate::ShellScreen::CommandLine {
            let id = state.content_page_id();
            let history = if self
                .hosts
                .get(&id)
                .is_none_or(|host| matches!(host.state, CommandLineHostState::Inactive))
            {
                state.tasks.new_recorder(
                    "command-line",
                    i18n::tr!("task-source-command-line"),
                    "command-line",
                )
            } else {
                None
            };
            let host = self
                .hosts
                .entry(id)
                .or_insert_with(|| CommandLineHost::new(self.watchdog.clone()));
            host.ensure_started_recorded(
                platform,
                state.current_home_username().unwrap_or("tundra"),
                accent,
                state.command_line_start_directory_for(id),
                history,
            );
        }

        let mut completed = Vec::new();
        let mut shell_event = CommandLineHostEvent::None;
        for (id, host) in &mut self.hosts {
            match host.poll() {
                CommandLineHostEvent::None => {}
                CommandLineHostEvent::ExitToCaller => completed.push(*id),
                CommandLineHostEvent::PanicRequested => {
                    shell_event = CommandLineHostEvent::PanicRequested;
                }
                CommandLineHostEvent::ResetRequested
                    if shell_event != CommandLineHostEvent::PanicRequested =>
                {
                    shell_event = CommandLineHostEvent::ResetRequested;
                }
                CommandLineHostEvent::ResetRequested => {}
            }
        }
        for id in completed {
            self.hosts.remove(&id);
            state.close_page_instance(id);
        }

        self.process_close_request(state);
        self.sync_tasks(state);
        if state.content_screen() == crate::ShellScreen::CommandLine {
            if let Some(host) = self.hosts.get_mut(&state.content_page_id())
                && let Some(area) = state.command_line_input_area()
            {
                host.resize_to_area(area);
            }
        }
        let visible_after = (state.content_screen() == crate::ShellScreen::CommandLine)
            .then(|| self.visible_host(state).view_model());
        (visible_before != visible_after, shell_event)
    }

    fn process_close_request(&mut self, state: &mut super::ShellSession) {
        let Some((id, confirmed)) = state.command_line_close_request.take() else {
            return;
        };
        if state.active_page_id() != id
            || state.active_screen() != crate::ShellScreen::CommandLine
            || (!confirmed && !state.interactive_overlays().is_empty())
        {
            return;
        }
        let running = self.hosts.get(&id).is_some_and(CommandLineHost::is_running);
        if running && !confirmed {
            state.show_command_line_close_confirmation(id);
            return;
        }
        if let Some(host) = self.hosts.get_mut(&id)
            && let Err(error) = host.try_terminate()
        {
            state.notify_alert_with_tone(
                i18n::msg!(
                    "shell-command-line-close-failed",
                    reason = error.to_string()
                ),
                ui::NotificationTone::Error,
            );
            return;
        }
        self.hosts.remove(&id);
        state.close_page_instance(id);
    }

    pub(super) fn handle_input(
        &mut self,
        state: &super::ShellSession,
        input: &InputEvent,
        terminal_area: Option<Rect>,
    ) -> CommandLineHostEvent {
        if state.active_screen() != crate::ShellScreen::CommandLine
            || !state.interactive_overlays().is_empty()
        {
            return CommandLineHostEvent::None;
        }
        self.hosts
            .get_mut(&state.active_page_id())
            .map_or(CommandLineHostEvent::None, |host| {
                host.handle_input(input, terminal_area)
            })
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
