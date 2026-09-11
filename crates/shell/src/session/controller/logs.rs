use super::super::*;
use app::runtime_logs::{LogAccess, LogDocumentSelection, LogsSnapshot};
use runtime_log::{LogLevel, LogQuery, LogSource};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::session) struct LogsUiState {
    category: ui::LogsCategory,
    section: ui::LogsSection,
    query: LogQuery,
    snapshot: LogsSnapshot,
    selected: usize,
    scroll: usize,
    explicit_scroll: bool,
    feedback: Option<i18n::LocalizedText>,
    job: Option<LogsJob>,
    revision: u64,
    time_filter: u8,
    known_modules: Vec<String>,
    pub(super) scrollbar_grab: Option<u16>,
    last_document: Option<LogDocumentSelection>,
    editor_snapshot: Option<PathBuf>,
    refreshing_editor: bool,
}

#[derive(Clone)]
struct LogsJob(Arc<LogsJobShared>);
struct LogsJobShared {
    cancelled: Arc<AtomicBool>,
    result: Mutex<Option<LogsJobResult>>,
    worker: Mutex<Option<ManagedThreadHandle<()>>>,
}
enum LogsJobResult {
    Snapshot(LogsSnapshot),
    Document(Result<PathBuf, String>),
}
impl std::fmt::Debug for LogsJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LogsJob")
    }
}
impl PartialEq for LogsJob {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for LogsJob {}
impl Drop for LogsJobShared {
    fn drop(&mut self) {
        self.cancelled.store(true, AtomicOrdering::Relaxed);
        if let Ok(Some(worker)) = self.worker.get_mut() {
            worker.cancel();
        }
    }
}

impl ShellSession {
    pub(in crate::session) fn open_logs(&mut self) {
        if self
            .app
            .auth_session()
            .is_none_or(|session| session.role == UserRole::Guest)
        {
            return;
        }
        if self.active_screen() != ShellScreen::Logs {
            self.screen_stack.push(ShellScreen::Logs);
        }
        self.focused_component = ShellComponent::Logs;
        self.request_logs_job(None);
        self.notify_status(i18n::LocalizedText::from(i18n::msg!("shell-logs")));
    }

    pub(in crate::session) fn logs_select_legacy_section(&mut self, incidents: bool) {
        self.logs_set_section(if incidents {
            ui::LogsSection::Incidents
        } else {
            ui::LogsSection::Events
        });
    }

    fn logs_access(&self) -> Option<LogAccess> {
        let session = self.app.auth_session()?;
        match session.role {
            UserRole::Guest => None,
            UserRole::Admin => Some(LogAccess::Admin),
            _ => Some(LogAccess::User(session.user_id.clone())),
        }
    }

    fn request_logs_job(&mut self, document: Option<LogDocumentSelection>) {
        let Some(access) = self.logs_access() else {
            return;
        };
        let Some(storage) = self.storage_manager.clone() else {
            self.logs_state.feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-log-storage-unavailable"
            )));
            return;
        };
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            self.logs_state.feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-logs-worker-unavailable"
            )));
            return;
        };
        if let Some(previous) = self.logs_state.job.take() {
            previous.0.cancelled.store(true, AtomicOrdering::Relaxed);
        }
        let mut query = self.logs_state.query.clone();
        query.source = if self.logs_state.category == ui::LogsCategory::Linux {
            LogSource::Linux
        } else {
            LogSource::Ux
        };
        query.since = match self.logs_state.time_filter {
            1 => Some(Utc::now() - chrono::Duration::hours(1)),
            2 => Some(Utc::now() - chrono::Duration::days(1)),
            3 => Some(Utc::now() - chrono::Duration::days(7)),
            _ => None,
        };
        let job_context = group.new_log_context(
            "ux.logs",
            "query",
            self.app.auth_session().map(|s| s.user_id.clone()),
        );
        let group = group.with_log_context(job_context.owner_id, job_context.operation_id);
        let root = storage.layout().logs_path.clone();
        let platform = self
            .settings_task_runtime
            .shared
            .platform
            .clone()
            .unwrap_or_else(|| Arc::from(platform::native_platform()));
        let shared = Arc::new(LogsJobShared {
            cancelled: Arc::new(AtomicBool::new(false)),
            result: Mutex::new(None),
            worker: Mutex::new(None),
        });
        let output = Arc::downgrade(&shared);
        let cancelled = shared.cancelled.clone();
        self.logs_state.revision = self.logs_state.revision.wrapping_add(1);
        let task = TaskId::new(format!("logs-query-{}", self.logs_state.revision % 64))
            .expect("bounded logs task id");
        match group.spawn_thread(TaskSpec::one_shot(task), move || {
            let result = match &document {
                Some(selection) => {
                    LogsJobResult::Document(app::runtime_logs::prepare_log_document(
                        &root,
                        &query,
                        &access,
                        &selection,
                        platform.as_ref(),
                        &cancelled,
                    ))
                }
                None => LogsJobResult::Snapshot(app::runtime_logs::query_snapshot(
                    &root,
                    &query,
                    &access,
                    platform.as_ref(),
                    &cancelled,
                )),
            };
            if !cancelled.load(AtomicOrdering::Relaxed)
                && let Some(output) = output.upgrade()
                && let Ok(mut slot) = output.result.lock()
            {
                *slot = Some(result);
            }
        }) {
            Ok(worker) => {
                *shared.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.logs_state.job = Some(LogsJob(shared));
                self.logs_state.feedback = None;
            }
            Err(error) => {
                self.logs_state.feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-cannot-start-logs-query-error",
                    error = error.to_string()
                )))
            }
        }
    }

    pub(in crate::session) fn poll_logs_tasks(&mut self) {
        let result = self
            .logs_state
            .job
            .as_ref()
            .and_then(|job| job.0.result.lock().ok()?.take());
        let Some(result) = result else {
            let stopped = self.logs_state.job.as_ref().is_some_and(|job| {
                job.0.worker.lock().ok().is_some_and(|worker| {
                    worker.as_ref().is_some_and(|worker| worker.is_finished())
                })
            });
            if stopped {
                self.logs_state.job = None;
                self.logs_state.feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-logs-worker-stopped-before-completing-the-request"
                )));
            }
            return;
        };
        self.logs_state.job = None;
        match result {
            LogsJobResult::Snapshot(snapshot) => {
                let selected_id = self.logs_id_at(self.logs_state.selected);
                for event in &snapshot.result.events {
                    if self.logs_state.known_modules.len() < 128
                        && !self
                            .logs_state
                            .known_modules
                            .contains(&event.context.module)
                    {
                        self.logs_state
                            .known_modules
                            .push(event.context.module.clone());
                    }
                }
                self.logs_state.known_modules.sort();
                self.logs_state.snapshot = snapshot;
                if let Some(index) = selected_id.and_then(|id| {
                    (0..self.logs_count())
                        .find(|index| self.logs_id_at(*index).as_ref() == Some(&id))
                }) {
                    self.logs_state.selected = index;
                }
                self.logs_state.selected = self
                    .logs_state
                    .selected
                    .min(self.logs_count().saturating_sub(1));
                self.logs_state.feedback = Some(
                    format!(
                        "{:?}{}",
                        self.logs_state.snapshot.result.state,
                        if self.logs_state.snapshot.result.notices.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", self.logs_state.snapshot.result.notices.join("; "))
                        }
                    )
                    .into(),
                );
            }
            LogsJobResult::Document(Ok(path)) => {
                if self.active_screen() == ShellScreen::Logs {
                    self.logs_state.editor_snapshot = Some(path.clone());
                    if let Err(error) =
                        self.open_diagnostics_editor(EditorReloadPolicy::Log { path })
                    {
                        self.logs_state.feedback = Some(error);
                    }
                } else if self.active_screen() == ShellScreen::Editor
                    && self.logs_state.refreshing_editor
                {
                    if let Some(session) = self.editor_read_session.as_mut() {
                        session.reload = EditorReloadPolicy::Log { path: path.clone() };
                    }
                    self.logs_state.editor_snapshot = Some(path);
                    self.logs_state.refreshing_editor = false;
                    self.reload_log_editor_file();
                }
            }
            LogsJobResult::Document(Err(error)) => {
                self.logs_state.refreshing_editor = false;
                self.logs_state.feedback = Some(error.clone().into());
                if self.active_screen() == ShellScreen::Editor {
                    self.report_editor_error(error);
                }
            }
        }
    }

    fn logs_id_at(&self, index: usize) -> Option<String> {
        let state = &self.logs_state;
        if state.category == ui::LogsCategory::Linux || state.section == ui::LogsSection::Events {
            state
                .snapshot
                .result
                .events
                .get(index)
                .map(|e| e.event_id.clone())
        } else if state.section == ui::LogsSection::Files {
            state
                .snapshot
                .files
                .get(index)
                .map(|f| f.path.to_string_lossy().into_owned())
        } else {
            state
                .snapshot
                .incidents
                .get(index)
                .map(|i| i.incident_id.clone())
        }
    }
    fn logs_count(&self) -> usize {
        if self.logs_state.category == ui::LogsCategory::Linux
            || self.logs_state.section == ui::LogsSection::Events
        {
            self.logs_state.snapshot.result.events.len()
        } else if self.logs_state.section == ui::LogsSection::Files {
            self.logs_state.snapshot.files.len()
        } else {
            self.logs_state.snapshot.incidents.len()
        }
    }
    fn logs_main_area(&self) -> Option<Rect> {
        match ui::compute_shell_layout(Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1))
        {
            ui::ShellLayout::Full { main, .. } => Some(main),
            _ => None,
        }
    }
    fn logs_move(&mut self, delta: isize) {
        self.logs_state.selected = self
            .logs_state
            .selected
            .saturating_add_signed(delta)
            .min(self.logs_count().saturating_sub(1));
        self.logs_state.explicit_scroll = false;
    }
    fn logs_open_selected(&mut self) {
        let selection = if self.logs_state.category == ui::LogsCategory::Linux
            || self.logs_state.section == ui::LogsSection::Events
        {
            Some(LogDocumentSelection::Events)
        } else if self.logs_state.section == ui::LogsSection::Files {
            self.logs_state
                .snapshot
                .files
                .get(self.logs_state.selected)
                .map(|f| LogDocumentSelection::File(f.path.clone()))
        } else {
            self.logs_state
                .snapshot
                .incidents
                .get(self.logs_state.selected)
                .map(|i| LogDocumentSelection::Incident(i.incident_id.clone()))
        };
        if let Some(selection) = selection {
            self.logs_state.last_document = Some(selection.clone());
            self.request_logs_job(Some(selection));
        }
    }
    pub(in crate::session) fn refresh_logs_editor_snapshot(&mut self) -> bool {
        let matches = self.editor_read_session.as_ref().is_some_and(|session| {
            self.logs_state.editor_snapshot.as_deref() == Some(session.reload.path())
        });
        if !matches {
            return false;
        }
        let Some(selection) = self.logs_state.last_document.clone() else {
            return false;
        };
        if self.logs_state.job.is_none() {
            self.logs_state.refreshing_editor = true;
            self.request_logs_job(Some(selection));
            self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-refreshing-log-query"
            )));
        }
        true
    }
    fn logs_set_category(&mut self, category: ui::LogsCategory) {
        self.logs_state.category = category;
        self.logs_state.query = LogQuery::default();
        self.logs_state.known_modules.clear();
        self.logs_state.selected = 0;
        self.logs_state.scroll = 0;
        self.logs_state.snapshot = LogsSnapshot::default();
        self.request_logs_job(None);
    }
    fn logs_set_section(&mut self, section: ui::LogsSection) {
        self.logs_state.section = section;
        self.logs_state.selected = 0;
        self.logs_state.scroll = 0;
        self.logs_state.explicit_scroll = false;
    }
    fn logs_filter_level(&mut self) {
        self.logs_state.query.min_level = match self.logs_state.query.min_level {
            None => Some(LogLevel::Warning),
            Some(LogLevel::Warning) => Some(LogLevel::Error),
            _ => None,
        };
        self.logs_state.selected = 0;
        self.request_logs_job(None);
    }
    fn logs_filter_module(&mut self) {
        let modules = &self.logs_state.known_modules;
        self.logs_state.query.module = match self.logs_state.query.module.as_ref() {
            None => modules.first().cloned(),
            Some(current) => modules
                .iter()
                .position(|module| module == current)
                .and_then(|index| modules.get(index + 1))
                .cloned(),
        };
        self.logs_state.selected = 0;
        self.request_logs_job(None);
    }
    fn logs_filter_time(&mut self) {
        self.logs_state.time_filter = (self.logs_state.time_filter + 1) % 4;
        self.logs_state.selected = 0;
        self.request_logs_job(None);
    }

    pub(in crate::session) fn handle_logs_key(&mut self, key: &KeyInput) {
        match key.key {
            InputKey::Escape => {
                self.logs_state.job = None;
                self.logs_state.scrollbar_grab = None;
                if self.active_screen() == ShellScreen::Logs {
                    self.screen_stack.pop();
                }
                self.focused_component = if self.active_screen() == ShellScreen::SystemStatus {
                    ShellComponent::SystemStatus
                } else {
                    ShellComponent::Home
                };
            }
            InputKey::Left | InputKey::Right => {
                self.logs_set_category(if self.logs_state.category == ui::LogsCategory::Ux {
                    ui::LogsCategory::Linux
                } else {
                    ui::LogsCategory::Ux
                })
            }
            InputKey::Tab => self.logs_set_section(match self.logs_state.section {
                ui::LogsSection::Events => ui::LogsSection::Files,
                ui::LogsSection::Files => ui::LogsSection::Incidents,
                ui::LogsSection::Incidents => ui::LogsSection::Events,
            }),
            InputKey::Up => self.logs_move(-1),
            InputKey::Down => self.logs_move(1),
            InputKey::PageUp => self.logs_move(-10),
            InputKey::PageDown => self.logs_move(10),
            InputKey::Home => {
                self.logs_state.selected = 0;
                self.logs_state.explicit_scroll = false;
            }
            InputKey::End => {
                self.logs_state.selected = self.logs_count().saturating_sub(1);
                self.logs_state.explicit_scroll = false;
            }
            InputKey::Enter | InputKey::Char('o' | 'O') => self.logs_open_selected(),
            InputKey::Char('r' | 'R') => self.request_logs_job(None),
            InputKey::Char('l' | 'L') => self.logs_filter_level(),
            InputKey::Char('m' | 'M') => self.logs_filter_module(),
            InputKey::Char('t' | 'T') => self.logs_filter_time(),
            InputKey::Char('i' | 'I') if self.logs_state.category == ui::LogsCategory::Ux => {
                let id = self
                    .logs_state
                    .snapshot
                    .result
                    .events
                    .get(self.logs_state.selected)
                    .and_then(|e| e.incident_id.clone());
                self.logs_state.query = LogQuery {
                    incident_id: id,
                    ..Default::default()
                };
                self.logs_state.time_filter = 0;
                self.logs_set_section(ui::LogsSection::Incidents);
                self.request_logs_job(None);
            }
            InputKey::Char('e' | 'E')
                if self.logs_state.category == ui::LogsCategory::Ux
                    && self.logs_state.section == ui::LogsSection::Incidents =>
            {
                let id = self
                    .logs_state
                    .snapshot
                    .incidents
                    .get(self.logs_state.selected)
                    .map(|e| e.incident_id.clone());
                self.logs_state.query = LogQuery {
                    incident_id: id,
                    ..Default::default()
                };
                self.logs_state.time_filter = 0;
                self.logs_set_section(ui::LogsSection::Events);
                self.request_logs_job(None);
            }
            InputKey::Char('c' | 'C') => {
                self.logs_state.query = LogQuery::default();
                self.logs_state.time_filter = 0;
                self.request_logs_job(None);
            }
            _ => {}
        }
    }
    pub(in crate::session) fn handle_logs_pointer(&mut self, mouse: MouseInput) {
        let Some(main) = self.logs_main_area() else {
            return;
        };
        let model = self.to_logs_view_model();
        let layout = ui::logs_layout(main, &model);
        let point = mouse.coordinates();
        match mouse.kind {
            ui::MouseEventKind::Scroll(direction) => {
                let delta = if direction == ScrollDirection::Up {
                    -3
                } else if direction == ScrollDirection::Down {
                    3
                } else {
                    0
                };
                self.logs_state.scroll = layout
                    .visible_start
                    .saturating_add_signed(delta)
                    .min(self.logs_count().saturating_sub(layout.visible_capacity));
                self.logs_state.explicit_scroll = true;
            }
            ui::MouseEventKind::Up(PointerButton::Left) => {
                self.logs_state.scrollbar_grab = None;
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
                if let Some(target) = ui::logs_hit_test(main, &model, point) {
                    match target {
                        ui::LogsHitTarget::Category(category) => self.logs_set_category(category),
                        ui::LogsHitTarget::Section(section) => self.logs_set_section(section),
                        ui::LogsHitTarget::Event(index)
                        | ui::LogsHitTarget::File(index)
                        | ui::LogsHitTarget::Incident(index) => {
                            let click = self.register_click(
                                Some(ShellComponent::Logs),
                                point,
                                PointerButton::Left,
                                Instant::now(),
                            );
                            self.logs_state.selected = index;
                            if click == ClickKind::Double {
                                self.logs_open_selected();
                            }
                        }
                        ui::LogsHitTarget::Refresh => self.request_logs_job(None),
                        ui::LogsHitTarget::Open => self.logs_open_selected(),
                        ui::LogsHitTarget::FilterLevel => self.logs_filter_level(),
                        ui::LogsHitTarget::FilterModule => self.logs_filter_module(),
                        ui::LogsHitTarget::FilterTime => self.logs_filter_time(),
                        ui::LogsHitTarget::Scrollbar => {
                            self.logs_state.scrollbar_grab = Some(
                                layout
                                    .content
                                    .list_scrollbar
                                    .filter(|bar| bar.thumb.contains(point.into()))
                                    .map(|bar| point.1.saturating_sub(bar.thumb.y))
                                    .unwrap_or(0),
                            );
                            self.drag_logs_scrollbar(point);
                        }
                    }
                }
            }
            ui::MouseEventKind::Drag(PointerButton::Left)
                if self.logs_state.scrollbar_grab.is_some() =>
            {
                self.drag_logs_scrollbar(point)
            }
            _ => {}
        }
    }
    fn drag_logs_scrollbar(&mut self, point: CellPosition) {
        let Some(main) = self.logs_main_area() else {
            return;
        };
        let layout = ui::logs_layout(main, &self.to_logs_view_model());
        let Some(scrollbar_layout) = layout.content.list_scrollbar else {
            return;
        };
        let track = scrollbar_layout.track;
        let count = self.logs_count();
        let scrollbar =
            ui::components::Scrollbar::new(count, layout.visible_capacity, layout.visible_start);
        let (_, thumb_len) = scrollbar.thumb_range(track);
        self.logs_state.scroll = scrollbar_window_start(
            point.1,
            self.logs_state.scrollbar_grab.unwrap_or(0),
            track.y,
            track.height,
            thumb_len,
            count,
            layout.visible_capacity,
        );
        self.logs_state.explicit_scroll = true;
    }

    pub(in crate::session) fn to_logs_view_model(&self) -> ui::LogsViewModel {
        let _language = i18n::enter_snapshot(self.language.clone());
        let state = &self.logs_state;
        let system = matches!(self.logs_access(), Some(LogAccess::Admin));
        let diagnostics = ui::DiagnosticsViewModel {
            tab: if state.section == ui::LogsSection::Incidents {
                ui::DiagnosticsTab::Incidents
            } else {
                ui::DiagnosticsTab::Logs
            },
            logs: state
                .snapshot
                .files
                .iter()
                .map(|file| ui::DiagnosticsLogViewModel {
                    relative_path: file.relative_path.display().to_string(),
                    path: file.path.display().to_string(),
                    modified_at: file.modified_at.to_rfc3339(),
                    size_bytes: file.size_bytes,
                })
                .collect(),
            incidents: state
                .snapshot
                .incidents
                .iter()
                .map(|incident| ui::DiagnosticsIncidentViewModel {
                    id: incident.incident_id.clone(),
                    occurred_at: incident.occurred_at.to_rfc3339(),
                    app: incident
                        .app
                        .as_ref()
                        .map(|app| app.display_name.clone())
                        .unwrap_or_else(|| i18n::tr!("shell-process")),
                    severity: diagnostics_incident_severity_to_ui(incident.severity),
                    recovery: format!("{:?}", incident.recovery),
                    summary: incident.summary.clone(),
                    detail: format!(
                        "{} / {}",
                        incident.boundary,
                        incident.component.as_deref().unwrap_or("process")
                    ),
                    report_path: String::new(),
                    restricted: !system,
                })
                .collect(),
            selected_log: state.selected,
            selected_incident: state.selected,
            list_window_start: state.scroll,
            list_window_is_explicit: state.explicit_scroll,
            can_view_details: true,
            scanning: state.job.is_some(),
            ..Default::default()
        };
        let health = ProcessWatchdog::global().map(|process| process.runtime_log_health());
        let feedback =
            match health.filter(|health| health.dropped_events > 0 || health.write_failures > 0) {
                Some(health) => Some(i18n::tr!(
                    "shell-arg1-writer-arg2-dropped-arg3-failuresarg4",
                    arg1 = state
                        .feedback
                        .as_ref()
                        .map(i18n::LocalizedText::render_current)
                        .unwrap_or_default(),
                    arg2 = health.dropped_events,
                    arg3 = health.write_failures,
                    arg4 = health
                        .last_error
                        .map(|error| format!(" ({error})"))
                        .unwrap_or_default()
                )),
                None => state
                    .feedback
                    .as_ref()
                    .map(i18n::LocalizedText::render_current),
            };
        ui::LogsViewModel {
            category: state.category,
            section: state.section,
            diagnostics,
            events: state
                .snapshot
                .result
                .events
                .iter()
                .map(|event| ui::LogsEventViewModel {
                    id: event.event_id.clone(),
                    timestamp: event.timestamp.to_rfc3339(),
                    level: format!("{:?}", event.level),
                    module: event.context.module.clone(),
                    operation: event.context.operation.clone(),
                    summary: event.message.clone(),
                    detail: serde_event_detail(event),
                    incident_id: event.incident_id.clone(),
                })
                .collect(),
            selected_event: state.selected,
            scroll_offset: state.scroll,
            linux_available: cfg!(target_os = "linux"),
            can_view_system: system,
            loading: state.job.is_some(),
            filter_summary: i18n::tr!(
                "shell-level-arg1-module-arg2-time-arg3-arg4",
                arg1 = state
                    .query
                    .min_level
                    .map(|level| format!("{level:?}+ "))
                    .unwrap_or_else(|| i18n::tr!("shell-all")),
                arg2 = state
                    .query
                    .module
                    .clone()
                    .unwrap_or_else(|| i18n::tr!("shell-all")),
                arg3 = [
                    i18n::tr!("shell-all"),
                    i18n::tr!("shell-1-hour"),
                    i18n::tr!("shell-24-hours"),
                    i18n::tr!("shell-7-days")
                ][usize::from(state.time_filter)]
                .clone(),
                arg4 = state
                    .query
                    .incident_id
                    .as_ref()
                    .map(|id| i18n::tr!("shell-incident-id", id = id))
                    .unwrap_or_else(|| i18n::tr!("shell-c-clear-filters"))
            )
            .into(),
            feedback,
        }
    }
}

fn serde_event_detail(event: &runtime_log::RuntimeLogEvent) -> String {
    format!(
        "Event: {}\nTime: {}\nRun: {}\nModule: {}\nOperation: {} ({})\nTask: {}\nPhase: {:?}\nCode: {} / OS {:?}\nReason chain: {}\nSource: {}\nTarget: {}\nIncident: {}\nCount: {} / retries {}{}",
        event.event_id,
        event.timestamp.to_rfc3339(),
        event.context.run_id.as_deref().unwrap_or("—"),
        event.context.module,
        event.context.operation,
        event.context.operation_id.as_deref().unwrap_or("—"),
        event.context.task_id.as_deref().unwrap_or("—"),
        event.phase,
        event.error_code.as_deref().unwrap_or("—"),
        event.os_error_code,
        event.error_chain.join(" → "),
        event
            .source_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        event
            .target_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        event.incident_id.as_deref().unwrap_or("—"),
        event.repeat_count,
        event.retry_count,
        event
            .timestamp_note
            .as_ref()
            .map(|note| format!("\nTime note: {note}"))
            .unwrap_or_default()
    )
}

impl ShellSession {
    pub(in crate::session) fn operation_log_context(
        &self,
        module: &str,
        operation: &str,
    ) -> runtime_log::LogContext {
        let mut context = ProcessWatchdog::global()
            .map(|process| process.log_context(module, operation))
            .unwrap_or_default();
        context.operation_id = ProcessWatchdog::global().map(|p| p.new_log_operation_id());
        context.app = "shell".into();
        context.module = module.into();
        context.operation = operation.into();
        context.owner_id = self
            .app
            .auth_session()
            .map(|session| session.user_id.clone());
        context
    }
    pub(in crate::session) fn save_settings_config_logged(
        &self,
        storage: &StorageManager,
        config: &storage::StorageConfig,
    ) -> Result<(), storage::StorageError> {
        let started = runtime_log::RuntimeLogEvent::new(
            self.operation_log_context("ux.settings", "save_configuration"),
            runtime_log::LogLevel::Info,
            runtime_log::LogPhase::Started,
            "Saving configuration",
        );
        let context = started.context.clone();
        record_shell_runtime_event(started);
        let result = storage.save_config(config);
        let mut event = runtime_log::RuntimeLogEvent::new(
            context,
            if result.is_ok() {
                runtime_log::LogLevel::Info
            } else {
                runtime_log::LogLevel::Error
            },
            if result.is_ok() {
                runtime_log::LogPhase::Succeeded
            } else {
                runtime_log::LogPhase::Failed
            },
            "Configuration save result",
        );
        if let Err(error) = &result {
            event.error_code = Some("UX_CONFIG_SAVE_FAILED".into());
            watchdog::capture_error(&mut event, error);
        }
        record_shell_runtime_event(event);
        result
    }
}

pub(in crate::session) fn record_shell_runtime_event(event: runtime_log::RuntimeLogEvent) {
    if let Some(process) = ProcessWatchdog::global() {
        if let Ok(app) = process.register_app(shell_watchdog_descriptor()) {
            app.record_log(event);
        } else {
            process.record_log(event);
        }
    } else {
        runtime_log::record(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(role: UserRole) -> ShellSession {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(AuthSession {
                system_user: None,
                session_id: "logs-session".into(),
                user_id: "alice".into(),
                username: "alice".into(),
                role,
                started_at_epoch_ms: 1,
            })),
            Instant::now(),
        );
        state
    }
    fn key(state: &mut ShellSession, value: &str) {
        state.apply_input(InputEvent::from_key_label(value));
    }
    #[test]
    fn logs_home_entry_and_direct_route_enforce_guest_gate() {
        for role in [UserRole::Admin, UserRole::User, UserRole::Guest] {
            let mut state = state(role);
            assert_eq!(
                state.user_home_entries().iter().any(|e| e.label == "Logs"),
                role != UserRole::Guest
            );
            state.open_logs();
            assert_eq!(
                state.active_screen() == ShellScreen::Logs,
                role != UserRole::Guest
            );
            assert_eq!(
                state.to_logs_view_model().can_view_system,
                role == UserRole::Admin
            );
        }
    }
    #[test]
    fn logs_navigation_retains_filters_and_selection_across_close() {
        let mut state = state(UserRole::User);
        state.open_logs();
        key(&mut state, "L");
        key(&mut state, "Tab");
        assert_eq!(state.logs_state.section, ui::LogsSection::Files);
        assert_eq!(state.logs_state.query.min_level, Some(LogLevel::Warning));
        key(&mut state, "Esc");
        assert_eq!(state.active_screen(), ShellScreen::Home);
        state.open_logs();
        assert_eq!(state.logs_state.section, ui::LogsSection::Files);
        assert_eq!(state.logs_state.query.min_level, Some(LogLevel::Warning));
        key(&mut state, "Right");
        assert_eq!(state.logs_state.category, ui::LogsCategory::Linux);
        assert_eq!(state.logs_state.query.min_level, None);
        assert_eq!(
            state.to_logs_view_model().linux_available,
            cfg!(target_os = "linux")
        );
    }
    #[test]
    fn legacy_status_navigation_redirects_to_logs_app() {
        let mut state = state(UserRole::Admin);
        state.screen_stack.push(ShellScreen::SystemStatus);
        state.set_system_status_tab(ui::SystemStatusTab::Incidents);
        assert_eq!(state.active_screen(), ShellScreen::Logs);
        assert_eq!(state.logs_state.section, ui::LogsSection::Incidents);
        key(&mut state, "Esc");
        assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
        assert!(!ui::SystemStatusWidgetKind::ALL.contains(&ui::SystemStatusWidgetKind::Logs));
        assert!(!ui::SystemStatusWidgetKind::ALL.contains(&ui::SystemStatusWidgetKind::Incidents));
    }
    #[test]
    fn refresh_preserves_selected_event_and_scroll() {
        let mut state = state(UserRole::User);
        state.open_logs();
        let event = runtime_log::RuntimeLogEvent::new(
            runtime_log::LogContext::default(),
            LogLevel::Info,
            runtime_log::LogPhase::Succeeded,
            "finished",
        );
        state.logs_state.snapshot.result.events.push(event.clone());
        state.logs_state.scroll = 4;
        state.logs_state.explicit_scroll = true;
        let mut snapshot = LogsSnapshot::default();
        let mut newer = event.clone();
        newer.event_id = "newer".into();
        snapshot.result.events = vec![newer, event];
        state.logs_state.job = Some(LogsJob(Arc::new(LogsJobShared {
            cancelled: Arc::new(AtomicBool::new(false)),
            result: Mutex::new(Some(LogsJobResult::Snapshot(snapshot))),
            worker: Mutex::new(None),
        })));
        state.poll_logs_tasks();
        assert_eq!(state.logs_state.selected, 1);
        assert_eq!(state.logs_state.scroll, 4);
        assert!(state.logs_state.explicit_scroll);
    }
}
