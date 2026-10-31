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
use ui::components::ScrollbarOrientation;
use watchdog::{AppWatchdog, ComponentId, ManagedTaskGroup};
const SCROLL_LINES_PER_NOTCH: usize = 3;
const EMBEDDED_RESET_EXIT_CODE: u32 = 75;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExternalTerminalLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub wait_for_enter: Option<bool>,
}

impl ExternalTerminalLaunch {
    pub(super) fn title(&self) -> String {
        self.program
            .file_name()
            .unwrap_or(self.program.as_os_str())
            .to_string_lossy()
            .chars()
            .filter(|character| !character.is_control())
            .collect()
    }
}
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
    launch: Option<ExternalTerminalLaunch>,
    output: Option<TerminalOutput>,
    focused: bool,
    snapshot_revision: u64,
    ui_snapshot: Arc<ui::CommandLineTerminalSnapshot>,
    scrollbar_drag_offset: Option<u16>,
    child_mouse_buttons: Vec<ui::MouseButton>,
    reader_tasks: ManagedTaskGroup,
    history: Option<platform::task_history::TaskRecorder>,
}

impl CommandLineHost {
    pub fn new(watchdog: AppWatchdog) -> Self {
        let ui_snapshot = Arc::new(blank_terminal_snapshot());
        Self {
            state: CommandLineHostState::Inactive,
            launch: None,
            output: None,
            focused: false,
            snapshot_revision: 0,
            ui_snapshot,
            scrollbar_drag_offset: None,
            child_mouse_buttons: Vec::new(),
            history: None,
            reader_tasks: watchdog
                .child_component(ComponentId::from_static("command-line"))
                .task_group("pty-reader"),
        }
    }

    fn ensure_started_recorded(
        &mut self,
        platform: &dyn Platform,
        username: &str,
        accent: ratatui::style::Color,
        directory: Option<&Path>,
        external: Option<&ExternalTerminalLaunch>,
        history: Option<platform::task_history::TaskRecorder>,
    ) {
        if !matches!(self.state, CommandLineHostState::Inactive) {
            return;
        }
        self.history = history;
        self.launch = external.cloned();
        self.output = None;
        self.child_mouse_buttons.clear();
        self.focused = false;

        let program = external.map_or_else(resolve_tundra_cli_program, |launch| {
            Ok(launch.program.clone())
        });
        let result = program.and_then(|program| {
            let mut config = PtyConfig::new(program);
            config.columns = self.ui_snapshot.columns;
            config.rows = self.ui_snapshot.rows;
            if let Some(external) = external {
                config.args = external.args.iter().map(Into::into).collect();
            } else {
                config.args = vec!["repl".into(), "--embedded".into()];
                config.env = vec![
                    ("TUNDRA_COMMAND_LINE_USERNAME".into(), username.into()),
                    (
                        "TUNDRA_COMMAND_LINE_ACCENT".into(),
                        terminal_runtime::ansi_foreground(accent).into(),
                    ),
                ];
            }
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
                let (snapshot, revision) = pty.ui_snapshot_with_revision();
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
                    message: i18n::tr!(
                        "shell-terminal-start-failed",
                        program = self.launch.as_ref().map_or_else(
                            || "tundra-cli".to_string(),
                            ExternalTerminalLaunch::title,
                        ),
                        reason = error.to_string()
                    ),
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
        let needs_resize = self.ui_snapshot.columns != columns || self.ui_snapshot.rows != rows;
        if !needs_resize {
            return;
        }
        self.scrollbar_drag_offset = None;
        let result = match &self.state {
            CommandLineHostState::Running(pty) => pty.resize(columns, rows),
            _ => match &self.output {
                Some(output) => output.resize(columns, rows),
                None => return,
            },
        };
        match result {
            Ok(()) => {
                self.refresh_snapshot();
            }
            Err(error) => {
                self.fail_running(i18n::tr!(
                    "shell-terminal-resize-failed",
                    reason = error.to_string()
                ));
            }
        }
    }

    pub fn resize_to_area(&mut self, terminal_area: Rect) {
        let terminal_area = self.view_model().output_area(terminal_area);
        let content_area = ui::command_line_content_area(terminal_area, self.ui_snapshot.as_ref());
        self.resize_terminal(content_area.width, content_area.height);
    }

    pub fn poll(&mut self) -> CommandLineHostEvent {
        let status = match &self.state {
            CommandLineHostState::Running(pty) => match pty.try_wait() {
                Ok(status) => status,
                Err(error) => {
                    self.fail_running(i18n::tr!(
                        "shell-terminal-status-failed",
                        reason = error.to_string()
                    ));
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
            match pty.into_output() {
                Ok(output) => {
                    self.install_snapshot(output.ui_snapshot(), self.snapshot_revision);
                    self.output = Some(output);
                }
                Err(error) => {
                    self.fail_running(i18n::tr!(
                        "shell-terminal-output-failed",
                        reason = error.to_string()
                    ));
                    return CommandLineHostEvent::None;
                }
            }
        }

        if self.launch.is_none() && status.code == EMBEDDED_RESET_EXIT_CODE {
            if let Some(history) = &self.history {
                history.finish(
                    platform::task_history::TaskStatus::Succeeded,
                    i18n::tr!("task-exit-code", code = status.code),
                );
            }
            return CommandLineHostEvent::ResetRequested;
        }
        if self.launch.is_none() && status.code == crate::COMMAND_LINE_PANIC_EXIT_CODE {
            if let Some(history) = &self.history {
                history.finish(
                    platform::task_history::TaskStatus::Failed,
                    i18n::tr!("task-exit-code", code = status.code),
                );
            }
            return CommandLineHostEvent::PanicRequested;
        }
        if let Some(history) = &self.history {
            history.finish(
                if status.success {
                    platform::task_history::TaskStatus::Succeeded
                } else {
                    platform::task_history::TaskStatus::Failed
                },
                i18n::tr!("task-exit-code", code = status.code),
            );
        }
        let wait_for_enter = self
            .launch
            .as_ref()
            .and_then(|launch| launch.wait_for_enter);
        if wait_for_enter == Some(false) || status.success && wait_for_enter != Some(true) {
            self.install_blank_snapshot();
            self.output = None;
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
        if let Some(terminal_area) = terminal_area
            && let InputEvent::Mouse(mouse) = input
        {
            self.handle_mouse(*mouse, terminal_area);
            return CommandLineHostEvent::None;
        }
        match &self.state {
            CommandLineHostState::Exited { .. } | CommandLineHostState::Failed { .. } => {
                if let InputEvent::Key(key) = input
                    && key.phase.is_press_like()
                {
                    match key.key {
                        InputKey::F(12)
                            if self.launch.is_some()
                                && key.phase == ui::InputPhase::Press
                                && key.modifiers == ui::KeyModifiers::NONE =>
                        {
                            return CommandLineHostEvent::ExitToCaller;
                        }
                        InputKey::Enter
                            if matches!(self.state, CommandLineHostState::Exited { .. })
                                && self
                                    .launch
                                    .as_ref()
                                    .and_then(|launch| launch.wait_for_enter)
                                    == Some(true) =>
                        {
                            if key.phase == ui::InputPhase::Press {
                                return CommandLineHostEvent::ExitToCaller;
                            }
                        }
                        InputKey::Enter => {
                            self.install_blank_snapshot();
                            self.output = None;
                            self.state = CommandLineHostState::Inactive;
                        }
                        _ => {}
                    }
                }
                return CommandLineHostEvent::None;
            }
            CommandLineHostState::Inactive | CommandLineHostState::Running(_) => {}
        }

        let Some(_) = terminal_area else {
            return CommandLineHostEvent::None;
        };

        let returns_to_live_output = match input {
            InputEvent::Key(key) => key.phase.is_press_like(),
            InputEvent::Paste(_) => true,
            _ => false,
        };
        if returns_to_live_output && self.ui_snapshot.scrollback_offset > 0 {
            self.set_scrollback(0);
        }

        let write_result = match (&self.state, input) {
            (CommandLineHostState::Running(pty), InputEvent::Key(key)) => pty.send_key(key),
            (CommandLineHostState::Running(pty), InputEvent::Paste(text)) => pty.send_paste(text),
            _ => Ok(()),
        };
        if let Err(error) = write_result {
            self.fail_running(i18n::tr!(
                "shell-terminal-input-failed",
                reason = error.to_string()
            ));
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
            title: self.launch.as_ref().map(ExternalTerminalLaunch::title),
            terminal: Arc::clone(&self.ui_snapshot),
            process_state,
            message: match (
                &self.state,
                self.launch
                    .as_ref()
                    .and_then(|launch| launch.wait_for_enter),
            ) {
                (CommandLineHostState::Exited { code }, Some(true)) => {
                    Some(i18n::tr!("shell-terminal-wait-for-enter", code = *code))
                }
                _ => None,
            },
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
        self.output = None;
        self.child_mouse_buttons.clear();
        self.install_blank_snapshot();
        Ok(())
    }

    pub fn terminate(&mut self) {
        let _ = self.try_terminate();
    }

    fn handle_mouse(&mut self, mouse: ui::MouseEvent, terminal_area: Rect) {
        if matches!(mouse.kind, ui::MouseEventKind::Up(ui::MouseButton::Left))
            && self.scrollbar_drag_offset.is_some()
        {
            self.scrollbar_drag_offset = None;
            return;
        }

        let terminal_area = self.view_model().output_area(terminal_area);
        let scrollbar = ui::command_line_scrollbar_layout(terminal_area, self.ui_snapshot.as_ref());
        if let (
            Some(grab_offset),
            ui::MouseEventKind::Drag(ui::MouseButton::Left),
            Some(scrollbar),
        ) = (self.scrollbar_drag_offset, mouse.kind, scrollbar)
        {
            let offset = command_line_scrollback_offset_for_thumb(
                self.ui_snapshot.scrollback_rows,
                scrollbar,
                mouse.row(),
                grab_offset,
            );
            self.set_scrollback(offset);
            return;
        }

        if let (ui::MouseEventKind::Down(ui::MouseButton::Left), Some(scrollbar)) =
            (mouse.kind, scrollbar)
            && ScrollbarOrientation::VerticalRight.hit_test(scrollbar.thumb, mouse.coordinates())
        {
            self.scrollbar_drag_offset = Some(mouse.row().saturating_sub(scrollbar.thumb.y));
            return;
        }

        let content = ui::command_line_content_area(terminal_area, self.ui_snapshot.as_ref());
        let captured = match mouse.kind {
            ui::MouseEventKind::Drag(button) | ui::MouseEventKind::Up(button) => {
                self.child_mouse_buttons.contains(&button)
            }
            _ => false,
        };
        if !captured && !rect_contains(terminal_area, mouse.column(), mouse.row()) {
            return;
        }

        if let CommandLineHostState::Running(pty) = &self.state {
            let mut forwarded = mouse;
            if captured && !content.is_empty() {
                forwarded.position = ui::Point::new(
                    mouse.column().clamp(content.x, content.right() - 1),
                    mouse.row().clamp(content.y, content.bottom() - 1),
                );
            }
            match pty.send_mouse(&forwarded, content) {
                Ok(consumed) => {
                    match mouse.kind {
                        ui::MouseEventKind::Down(button)
                            if consumed && !self.child_mouse_buttons.contains(&button) =>
                        {
                            self.child_mouse_buttons.push(button);
                        }
                        ui::MouseEventKind::Up(button) => {
                            self.child_mouse_buttons.retain(|held| *held != button);
                        }
                        _ => {}
                    }
                    if consumed {
                        return;
                    }
                }
                Err(error) => {
                    self.fail_running(i18n::tr!(
                        "shell-terminal-input-failed",
                        reason = error.to_string()
                    ));
                    return;
                }
            }
        }

        match (mouse.kind, scrollbar) {
            (ui::MouseEventKind::Scroll(ui::ScrollDirection::Up), Some(_)) => {
                self.set_scrollback(
                    self.ui_snapshot
                        .scrollback_offset
                        .saturating_add(SCROLL_LINES_PER_NOTCH),
                );
            }
            (ui::MouseEventKind::Scroll(ui::ScrollDirection::Down), Some(_)) => {
                self.set_scrollback(
                    self.ui_snapshot
                        .scrollback_offset
                        .saturating_sub(SCROLL_LINES_PER_NOTCH),
                );
            }
            _ => {}
        }
    }

    fn set_scrollback(&mut self, offset: usize) {
        let result = match &self.state {
            CommandLineHostState::Running(pty) => pty.set_scrollback(offset),
            _ => match &self.output {
                Some(output) => output.set_scrollback(offset),
                None => return,
            },
        };
        match result {
            Ok(true) => self.refresh_snapshot(),
            Ok(false) => {}
            Err(error) => {
                self.fail_running(i18n::tr!(
                    "shell-terminal-scroll-failed",
                    reason = error.to_string()
                ));
            }
        }
    }

    fn fail_running(&mut self, message: String) {
        self.child_mouse_buttons.clear();
        let previous = std::mem::replace(&mut self.state, CommandLineHostState::Failed { message });
        if let CommandLineHostState::Running(pty) = previous {
            if let Ok(output) = pty.into_output() {
                self.install_snapshot(output.ui_snapshot(), self.snapshot_revision);
                self.output = Some(output);
            }
        }
        if let Some(history) = &self.history {
            let message = match &self.state {
                CommandLineHostState::Failed { message } => message.clone(),
                _ => unreachable!(),
            };
            history.finish(platform::task_history::TaskStatus::Failed, message);
        }
    }

    fn refresh_snapshot(&mut self) {
        let update = match &self.state {
            CommandLineHostState::Running(pty) => {
                pty.ui_snapshot_if_changed(self.snapshot_revision)
            }
            _ => self
                .output
                .as_ref()
                .map(|output| (output.ui_snapshot(), self.snapshot_revision)),
        };
        if let Some((snapshot, revision)) = update {
            self.install_snapshot(snapshot, revision);
        }
    }

    fn install_snapshot(&mut self, snapshot: ui::CommandLineTerminalSnapshot, revision: u64) {
        if snapshot.scrollback_rows == 0 {
            self.scrollbar_drag_offset = None;
        }
        self.ui_snapshot = Arc::new(snapshot);
        self.snapshot_revision = revision;
    }

    fn install_blank_snapshot(&mut self) {
        self.install_snapshot(blank_terminal_snapshot(), 0);
    }

    fn set_focused(&mut self, focused: bool) {
        if !focused {
            self.child_mouse_buttons.clear();
            self.scrollbar_drag_offset = None;
        }
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        if let CommandLineHostState::Running(pty) = &self.state
            && let Err(error) = pty.send_focus(focused)
        {
            self.fail_running(i18n::tr!(
                "shell-terminal-input-failed",
                reason = error.to_string()
            ));
        }
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
    window_focused: bool,
    cell_size: (u16, u16),
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
            window_focused: true,
            cell_size: (8, 16),
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

    pub(super) fn set_cell_size(&mut self, width: u16, height: u16) {
        self.cell_size = (width.max(1), height.max(1));
    }

    pub(super) fn observe_window_focus(&mut self, focused: bool) {
        self.window_focused = focused;
        if !focused {
            for host in self.hosts.values_mut() {
                host.set_focused(false);
            }
        }
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
            let external = state
                .pages
                .command_line
                .get(id)
                .and_then(|page| page.command_line_external.clone());
            let history = if self
                .hosts
                .get(&id)
                .is_none_or(|host| matches!(host.state, CommandLineHostState::Inactive))
            {
                state.tasks.new_recorder(
                    if external.is_some() {
                        "terminal"
                    } else {
                        "command-line"
                    },
                    external.as_ref().map_or_else(
                        || i18n::tr!("task-source-command-line"),
                        |launch| i18n::tr!("shell-terminal-task-title", program = launch.title()),
                    ),
                    "command-line",
                )
            } else {
                None
            };
            let host = self
                .hosts
                .entry(id)
                .or_insert_with(|| CommandLineHost::new(self.watchdog.clone()));
            if matches!(host.state, CommandLineHostState::Inactive)
                && let Some(area) = state.command_line_input_area()
            {
                host.install_snapshot(
                    ui::CommandLineTerminalSnapshot::blank(area.width, area.height),
                    0,
                );
            }
            host.ensure_started_recorded(
                platform,
                state.current_home_username().unwrap_or("tundra"),
                accent,
                state.command_line_start_directory_for(id),
                external.as_ref(),
                history,
            );
            if let CommandLineHostState::Running(pty) = &host.state
                && let Err(error) = pty.set_cell_size(self.cell_size.0, self.cell_size.1)
            {
                host.fail_running(i18n::tr!(
                    "shell-terminal-resize-failed",
                    reason = error.to_string()
                ));
            }
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

        let focused_id = (self.window_focused
            && !state.screen_saver_active()
            && state.active_screen() == crate::ShellScreen::CommandLine
            && state.interactive_overlays().is_empty())
        .then(|| state.active_page_id());
        for (id, host) in &mut self.hosts {
            host.set_focused(focused_id == Some(*id));
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

fn blank_terminal_snapshot() -> ui::CommandLineTerminalSnapshot {
    terminal_runtime::snapshot::ui_snapshot_from_parser(&mut vt100::Parser::new(
        DEFAULT_ROWS,
        DEFAULT_COLUMNS,
        0,
    ))
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
