//! Owns script processes independently of the cloneable page/UI state.
use super::*;
use app::script::{ScriptEvent, ScriptEventKind as Event, ScriptStatus};
use script_runtime::{RuntimeCommand, RuntimeEvent, ScriptSession};
use std::sync::atomic::{AtomicBool, Ordering};

struct RunningScript {
    session: ScriptSession,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    finished: bool,
    request: u64,
    revision: u64,
    fingerprints: HashMap<PathBuf, DocumentFingerprint>,
    last_check: Instant,
    history_result: Option<(platform::task_history::TaskStatus, String)>,
}

struct CompletedScript {
    output: terminal_runtime::pty::TerminalOutput,
    view: Option<(u16, u16, usize)>,
}

pub(super) struct ScriptHosts {
    hosts: HashMap<PageInstanceId, RunningScript>,
    completed: HashMap<PageInstanceId, CompletedScript>,
    tasks: ManagedTaskGroup,
    next_request: u64,
    window_focused: bool,
    cell_size: (u16, u16),
}

impl ScriptHosts {
    pub(super) fn new(watchdog: AppWatchdog) -> Self {
        Self {
            hosts: HashMap::new(),
            completed: HashMap::new(),
            tasks: watchdog.task_group("script-editor"),
            next_request: 0,
            window_focused: true,
            cell_size: (8, 16),
        }
    }

    pub(super) fn poll(&mut self, state: &mut ShellSession, platform: &dyn Platform) -> bool {
        let cell_size = self.cell_size;
        let mut changed = false;
        for id in state.pages.editor.ids() {
            state.with_page_context(id, |state| {
                if !state.pages.editor.script.enabled {
                    return;
                }
                if !state.pages.editor.script.process_running
                    && !state.pages.editor.script.state.status.is_active()
                    && state.editor_vim_terminal.is_none()
                {
                    state.pages.editor.script.vim_suspended = false;
                    state.pages.editor.script.vim_pending = false;
                }
                let changes = state.app.take_editor_source_line_changes();
                if let Some(path) = state
                    .app
                    .editor_state()
                    .and_then(|editor| editor.document.path.clone())
                {
                    state
                        .pages
                        .editor
                        .script
                        .state
                        .remap_breakpoints(&path, &changes);
                }
                state.poll_script_launch(platform);
            });
        }
        while let Some((page, command)) = state.script_requests.pop_front() {
            changed = true;
            if let RuntimeCommand::Start {
                spec,
                debug,
                session_id,
            } = command
            {
                if state.page_screen(page) != Some(ShellScreen::Editor)
                    || self.hosts.contains_key(&page)
                {
                    continue;
                }
                self.completed.remove(&page);
                let (columns, rows) = state
                    .with_page_context(page, |state| {
                        state.pages.editor.script.scrolls[ui::ScriptPane::Output.index()] = 0;
                        let area = state.script_layout().output_area;
                        (area.width.max(1), area.height.max(1))
                    })
                    .unwrap_or((80, 10));
                let history =
                    state
                        .tasks
                        .new_recorder("script", spec.path.display().to_string(), "script");
                match ScriptSession::spawn_recorded(
                    &spec,
                    debug,
                    session_id,
                    &self.tasks,
                    columns,
                    rows,
                    history.clone(),
                ) {
                    Ok(mut session) => {
                        let _ = session.terminal().set_cell_size(cell_size.0, cell_size.1);
                        let breakpoints = state
                            .with_page_context(page, |state| {
                                state.pages.editor.script.state.breakpoints.clone()
                            })
                            .unwrap_or_default();
                        if debug {
                            if let Err(error) =
                                session.command(RuntimeCommand::SetBreakpoints(breakpoints))
                            {
                                if let Some(history) = &history {
                                    history.finish(
                                        platform::task_history::TaskStatus::Failed,
                                        error.to_string(),
                                    );
                                }
                                fail(state, page, error);
                                let _ = session.stop();
                                continue;
                            }
                        }
                        state.with_page_context(page, |state| {
                            state.pages.editor.script.process_running = true
                        });
                        let cancel = Arc::new(AtomicBool::new(false));
                        let done = Arc::new(AtomicBool::new(false));
                        let cancel_task = cancel.clone();
                        let finished = done.clone();
                        self.next_request = self.next_request.wrapping_add(1).max(1);
                        let request = self.next_request;
                        state.tasks.register_recorded(
                            TaskDomain::Script,
                            request,
                            TaskOwner::Page(page),
                            TaskClass::Operation,
                            TaskControl::new(
                                move || cancel_task.store(true, Ordering::Release),
                                move || finished.load(Ordering::Acquire),
                            ),
                            history,
                        );
                        let mut fingerprints = HashMap::new();
                        if let Ok(value) = platform::document_fingerprint(&spec.path) {
                            fingerprints.insert(spec.path, value);
                        }
                        self.hosts.insert(
                            page,
                            RunningScript {
                                session,
                                cancel,
                                done,
                                finished: false,
                                request,
                                revision: u64::MAX,
                                fingerprints,
                                last_check: Instant::now(),
                                history_result: None,
                            },
                        );
                    }
                    Err(error) => {
                        if let Some(history) = &history {
                            history.finish(
                                platform::task_history::TaskStatus::Failed,
                                error.to_string(),
                            );
                        }
                        fail(state, page, error);
                    }
                }
            } else if let Some(host) = self.hosts.get_mut(&page) {
                if command == RuntimeCommand::Stop {
                    host.cancel.store(true, Ordering::Release);
                } else if let Err(error) = host.session.command(command) {
                    fail(state, page, error);
                    host.cancel.store(true, Ordering::Release);
                }
            } else if command == RuntimeCommand::Stop {
                state.with_page_context(page, |state| {
                    state.pages.editor.script.state.status = ScriptStatus::Finished;
                    state.pages.editor.script.vim_suspended = false;
                    state.app.dispatch_at(
                        app::AppCommand::Editor(app::editor::EditorCommand::SetExecutionReadOnly(
                            false,
                        )),
                        Instant::now(),
                    );
                });
            }
        }
        let ids = self.hosts.keys().copied().collect::<Vec<_>>();
        for page in ids {
            let host = self.hosts.get_mut(&page).unwrap();
            if state.page_screen(page) != Some(ShellScreen::Editor) {
                host.cancel.store(true, Ordering::Release);
            }
            if state
                .pages
                .editor
                .get(page)
                .is_some_and(|page| page.script.state.debugging)
                && host.last_check.elapsed() >= Duration::from_millis(250)
            {
                host.last_check = Instant::now();
                if host.fingerprints.iter().any(|(path, expected)| {
                    platform::document_fingerprint(path).ok().as_ref() != Some(expected)
                }) {
                    state.with_page_context(page, |state| {
                        state.fail_script(i18n::msg!("script-external-change").into())
                    });
                    host.cancel.store(true, Ordering::Release);
                }
            }
            if host.cancel.load(Ordering::Acquire) {
                match host.session.stop() {
                    Ok(()) => {
                        state.with_page_context(page, |state| {
                            if state.pages.editor.script.state.status != ScriptStatus::Failed {
                                state.pages.editor.script.state.status = ScriptStatus::Finished;
                            }
                        });
                    }
                    Err(error) => {
                        fail(state, page, error);
                        continue;
                    }
                }
                host.finished = true;
            }
            let events = if host.finished {
                Vec::new()
            } else {
                match host.session.poll() {
                    Ok(events) => events,
                    Err(error) => {
                        fail(state, page, error);
                        host.cancel.store(true, Ordering::Release);
                        Vec::new()
                    }
                }
            };
            for event in events {
                changed = true;
                let session_id = host.session.session_id();
                if let RuntimeEvent::Stopped {
                    ref stack,
                    ref location,
                    ..
                } = event
                {
                    for path in stack
                        .iter()
                        .map(|frame| &frame.path)
                        .chain(std::iter::once(&location.path))
                    {
                        if !host.fingerprints.contains_key(path) {
                            if let Ok(value) = platform::document_fingerprint(path) {
                                host.fingerprints.insert(path.clone(), value);
                            }
                        }
                    }
                }
                let finished =
                    matches!(event, RuntimeEvent::Exited { .. } | RuntimeEvent::Failed(_));
                let stopped = matches!(event, RuntimeEvent::Stopped { .. });
                match &event {
                    RuntimeEvent::Exited { code } => {
                        host.history_result = Some((
                            if *code == 0 {
                                platform::task_history::TaskStatus::Succeeded
                            } else {
                                platform::task_history::TaskStatus::Failed
                            },
                            i18n::tr!("task-exit-code", code = *code),
                        ))
                    }
                    RuntimeEvent::Failed(reason) => {
                        host.history_result =
                            Some((platform::task_history::TaskStatus::Failed, reason.clone()))
                    }
                    _ => {}
                }
                state.with_page_context(page, |state| {
                    let script = &mut state.pages.editor.script.state;
                    if script.session_id != session_id {
                        return;
                    }
                    let (stop_id, kind) = match event {
                        RuntimeEvent::Started => (script.stop_id, Event::Started),
                        RuntimeEvent::Running => (script.stop_id, Event::Continued),
                        RuntimeEvent::Stopped {
                            stop_id,
                            location,
                            variables,
                            stack,
                            breakpoints,
                        } => {
                            let navigate = script.stop_id != stop_id
                                || script.location.as_ref() != Some(&location);
                            if !script.apply(ScriptEvent {
                                session_id,
                                stop_id,
                                kind: Event::Stopped(location.clone()),
                            }) {
                                return;
                            }
                            script.breakpoints = breakpoints;
                            script.apply(ScriptEvent {
                                session_id,
                                stop_id,
                                kind: Event::Inspected { variables, stack },
                            });
                            if navigate {
                                state.locate_script_source(&location.path, location.line);
                            }
                            return;
                        }
                        RuntimeEvent::Evaluation {
                            stop_id,
                            input,
                            output,
                        } => {
                            if script.stop_id == stop_id {
                                script.status = ScriptStatus::Paused;
                            }
                            (stop_id, Event::Expression(format!("> {input}\n{output}")))
                        }
                        RuntimeEvent::Exited { code } => (script.stop_id, Event::Finished(code)),
                        RuntimeEvent::Failed(reason) => (
                            script.stop_id,
                            Event::Failed(
                                i18n::msg!("script-runtime-error", reason = reason).into(),
                            ),
                        ),
                    };
                    script.apply(ScriptEvent {
                        session_id,
                        stop_id,
                        kind,
                    });
                });
                if stopped && state.active_page_id() != page {
                    state.notify_modal(
                        i18n::msg!("script-paused-title"),
                        i18n::msg!("script-paused-background"),
                        ui::NotificationTone::Info,
                        vec![
                            ShellNotificationAction::new(
                                "return-script",
                                i18n::msg!("script-return"),
                            )
                            .with_follow_up(ShellCommand::ScriptReturn(page)),
                            ShellNotificationAction::new("close", i18n::msg!("shell-close"))
                                .cancel(),
                        ],
                    );
                }
                if finished {
                    if let Err(error) = host.session.stop() {
                        fail(state, page, error);
                        host.cancel.store(true, Ordering::Release);
                    } else {
                        host.finished = true;
                    }
                }
            }
            state.with_page_context(page, |state| {
                let area = state.script_layout().output_area;
                if area.width > 0 && area.height > 0 {
                    let _ = host
                        .session
                        .terminal()
                        .set_cell_size(cell_size.0, cell_size.1);
                    let _ = host.session.resize(area.width, area.height);
                }
                let offset = state.pages.editor.script.scrolls[ui::ScriptPane::Output.index()];
                let _ = host.session.terminal().set_scrollback(offset);
                if let Some((snapshot, revision)) = host
                    .session
                    .terminal()
                    .ui_snapshot_if_changed(host.revision)
                {
                    host.revision = revision;
                    state.pages.editor.script.output = Some(Arc::new(snapshot));
                    changed = true;
                }
            });
            if host.finished {
                let request = host.request;
                let host = self.hosts.remove(&page).unwrap();
                let done = host.done.clone();
                let cancelled = host.cancel.load(Ordering::Acquire);
                let history_result = host.history_result;
                state.with_page_context(page, |state| {
                    state.pages.editor.script.process_running = false;
                    state.pages.editor.script.source_view = None;
                    state.pages.editor.script.vim_suspended = false;
                    state.app.dispatch_at(
                        app::AppCommand::Editor(app::editor::EditorCommand::SetExecutionReadOnly(
                            false,
                        )),
                        Instant::now(),
                    );
                });
                match host.session.into_output() {
                    Ok(output) => {
                        let snapshot = output.ui_snapshot();
                        state.with_page_context(page, |state| {
                            state.pages.editor.script.output = Some(Arc::new(snapshot));
                        });
                        self.completed
                            .insert(page, CompletedScript { output, view: None });
                    }
                    Err(error) => fail(state, page, error),
                }
                let failure = state
                    .pages
                    .editor
                    .get(page)
                    .and_then(|page| page.script.state.error.clone());
                if let Some(failure) = failure {
                    state.tasks.finish_history(
                        TaskDomain::Script,
                        request,
                        platform::task_history::TaskStatus::Failed,
                        failure.render_current(),
                    );
                } else if cancelled {
                    state.tasks.finish_history(
                        TaskDomain::Script,
                        request,
                        platform::task_history::TaskStatus::Cancelled,
                        i18n::tr!("task-status-cancelled"),
                    );
                } else if let Some((status, result)) = history_result {
                    state
                        .tasks
                        .finish_history(TaskDomain::Script, request, status, result);
                }
                done.store(true, Ordering::Release);
                state.tasks.complete(TaskDomain::Script, request);
                changed = true;
            }
        }
        self.completed
            .retain(|page, _| state.page_screen(*page) == Some(ShellScreen::Editor));
        for (page, completed) in &mut self.completed {
            state.with_page_context(*page, |state| {
                let area = state.script_layout().output_area;
                if area.width == 0 || area.height == 0 {
                    return;
                }
                let offset = state.pages.editor.script.scrolls[ui::ScriptPane::Output.index()];
                let view = (area.width, area.height, offset);
                if completed.view == Some(view) {
                    return;
                }
                let _ = completed.output.resize(area.width, area.height);
                let _ = completed.output.set_scrollback(offset);
                state.pages.editor.script.output = Some(Arc::new(completed.output.ui_snapshot()));
                completed.view = Some(view);
                changed = true;
            });
        }
        self.sync_focus(state);
        changed
    }

    pub(super) fn observe_window_focus(&mut self, state: &ShellSession, focused: bool) {
        self.window_focused = focused;
        self.sync_focus(state);
    }

    pub(super) fn set_cell_size(&mut self, width: u16, height: u16) {
        self.cell_size = (width.max(1), height.max(1));
    }

    fn sync_focus(&self, state: &ShellSession) {
        let visible = (self.window_focused
            && state.active_screen() == ShellScreen::Editor
            && state.pages.editor.script.enabled
            && state.pages.editor.script.focus == ui::ScriptPane::Output
            && state.interactive_overlays().is_empty()
            && !state.auto_admin_visible())
        .then(|| state.active_page_id());
        for (page, host) in &self.hosts {
            let _ = host.session.terminal().send_focus(visible == Some(*page));
        }
    }

    pub(super) fn handle_input(&mut self, state: &ShellSession, input: &InputEvent) -> bool {
        if state.active_screen() != ShellScreen::Editor
            || !state.pages.editor.script.enabled
            || state.pages.editor.script.focus != ui::ScriptPane::Output
            || !state.interactive_overlays().is_empty()
        {
            return false;
        }
        let Some(host) = self.hosts.get(&state.content_page_id()) else {
            return false;
        };
        let pty = host.session.terminal();
        match input {
            InputEvent::Key(key)
                if !state.script_host_key(key) && !matches!(key.key, InputKey::F(1 | 8 | 10)) =>
            {
                let _ = pty.send_key(key);
                true
            }
            InputEvent::Paste(text) => {
                let _ = pty.send_paste(text);
                true
            }
            InputEvent::Mouse(mouse) => pty
                .send_mouse(mouse, state.script_layout().output_area)
                .unwrap_or(false),
            _ => false,
        }
    }
    pub(super) fn running(&self) -> bool {
        !self.hosts.is_empty()
    }
    pub(super) fn stop_all(&mut self) {
        for (_, host) in self.hosts.drain() {
            let done = host.done.clone();
            let _ = host.session.into_final_snapshot();
            done.store(true, Ordering::Release);
        }
    }
}

fn fail(state: &mut ShellSession, page: PageInstanceId, error: io::Error) {
    state.with_page_context(page, |state| {
        state.fail_script(i18n::msg!("script-runtime-error", reason = error.to_string()).into())
    });
}
