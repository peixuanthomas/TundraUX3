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
}

impl ScriptHosts {
    pub(super) fn new(watchdog: AppWatchdog) -> Self {
        Self {
            hosts: HashMap::new(),
            completed: HashMap::new(),
            tasks: watchdog.task_group("script-editor"),
            next_request: 0,
        }
    }

    pub(super) fn poll(&mut self, state: &mut ShellSession, platform: &dyn Platform) -> bool {
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
                match ScriptSession::spawn(&spec, debug, session_id, &self.tasks, columns, rows) {
                    Ok(mut session) => {
                        let breakpoints = state
                            .with_page_context(page, |state| {
                                state.pages.editor.script.state.breakpoints.clone()
                            })
                            .unwrap_or_default();
                        if debug {
                            if let Err(error) =
                                session.command(RuntimeCommand::SetBreakpoints(breakpoints))
                            {
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
                        state.tasks.register(
                            TaskDomain::Script,
                            request,
                            TaskOwner::Page(page),
                            TaskClass::Operation,
                            TaskControl::new(
                                move || cancel_task.store(true, Ordering::Release),
                                move || finished.load(Ordering::Acquire),
                            ),
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
                            },
                        );
                    }
                    Err(error) => fail(state, page, error),
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
                    let _ = host.session.resize(area.width, area.height);
                }
                let offset = state.pages.editor.script.scrolls[ui::ScriptPane::Output.index()];
                let _ = host.session.terminal().set_scrollback(offset);
                if let Some((snapshot, revision)) =
                    host.session.terminal().snapshot_if_changed(host.revision)
                {
                    host.revision = revision;
                    state.pages.editor.script.output = Some(Arc::new(
                        terminal_runtime::snapshot::to_ui_snapshot(&snapshot),
                    ));
                    changed = true;
                }
            });
            if host.finished {
                let request = host.request;
                let host = self.hosts.remove(&page).unwrap();
                let done = host.done.clone();
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
                        let snapshot = output.snapshot();
                        state.with_page_context(page, |state| {
                            state.pages.editor.script.output = Some(Arc::new(
                                terminal_runtime::snapshot::to_ui_snapshot(&snapshot),
                            ));
                        });
                        self.completed
                            .insert(page, CompletedScript { output, view: None });
                    }
                    Err(error) => fail(state, page, error),
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
                state.pages.editor.script.output = Some(Arc::new(
                    terminal_runtime::snapshot::to_ui_snapshot(&completed.output.snapshot()),
                ));
                completed.view = Some(view);
                changed = true;
            });
        }
        changed
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
                if !state.script_host_key(key)
                    && !matches!(key.key, InputKey::F(1 | 8 | 10))
                    && key.phase.is_press_like() =>
            {
                let (application_cursor, _) = pty.input_modes();
                if let Some(bytes) =
                    terminal_runtime::input::key_event_bytes(key, application_cursor)
                {
                    let _ = pty.write(&bytes);
                }
                true
            }
            InputEvent::Paste(text) => {
                let (_, bracketed) = pty.input_modes();
                let _ = pty.write(&terminal_runtime::input::paste_bytes(text, bracketed));
                true
            }
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
