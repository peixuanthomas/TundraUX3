use crate::session::*;
use app::runtime_logs::{LogAccess, LogDocumentSelection, LogsSnapshot};
use runtime_log::{LogLevel, LogQuery, LogSource};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::session) struct LogsUiState {
    pub(super) category: ui::LogsCategory,
    query: LogQuery,
    pub(super) snapshot: LogsSnapshot,
    pub(super) selected: usize,
    pub(super) scroll: usize,
    pub(super) explicit_scroll: bool,
    detail_scroll: usize,
    detail_scrollbar_grab: Option<u16>,
    feedback: Option<i18n::LocalizedText>,
    job: Option<LogsJob>,
    pub(super) revision: u64,
    time_filter: u8,
    known_modules: Vec<String>,
    pub(super) scrollbar_grab: Option<u16>,
    last_document: Option<LogDocumentSelection>,
    editor_snapshot: Option<PathBuf>,
    pending_document: Option<PathBuf>,
    refreshing_editor: bool,
    pub(super) paused: bool,
    new_events: usize,
    last_refresh: Option<Instant>,
    background_refresh: bool,
    more_selected: Option<usize>,
    more_scrollbar_grab: Option<u16>,
    filter_form: Option<ui::ManagementForm>,
}

#[derive(Clone)]
struct LogsJob(Arc<LogsJobShared>);
struct LogsJobShared {
    id: u64,
    cancelled: Arc<AtomicBool>,
    result: Mutex<Option<LogsJobResult>>,
    worker: Mutex<Option<ManagedThreadHandle<()>>>,
    background: bool,
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
impl LogsJob {
    fn finished(&self) -> bool {
        self.0
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_none_or(ManagedThreadHandle::is_finished)
    }
}
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
        self.enter_new_page(ShellScreen::Logs);
        self.request_logs_job(None);
        self.notify_status(i18n::LocalizedText::from(i18n::msg!("shell-logs")));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn open_service_logs(&mut self, unit: &str, scope: &str) {
        self.open_related_logs(unit, scope, None, None, None);
    }

    pub(in crate::session) fn open_related_logs(
        &mut self,
        unit: &str,
        scope: &str,
        boot_id: Option<&str>,
        invocation_id: Option<&str>,
        since_usec: Option<u64>,
    ) {
        if self
            .app
            .auth_session()
            .is_none_or(|session| session.role == UserRole::Guest)
        {
            return;
        }
        let Some(mut query) = service_log_query(unit, scope) else {
            return;
        };
        query.systemd_boot = Some(boot_id.unwrap_or("0").into());
        query.systemd_invocation = invocation_id.map(String::from);
        query.since = since_usec
            .and_then(|micros| i64::try_from(micros).ok())
            .and_then(chrono::DateTime::from_timestamp_micros);
        self.enter_new_page(ShellScreen::Logs);
        self.pages.logs.logs_state.query = query;
        self.pages.logs.logs_state.category = ui::LogsCategory::Linux;
        self.pages.logs.logs_state.time_filter = 0;
        self.pages.logs.logs_state.known_modules.clear();
        self.pages.logs.logs_state.selected = 0;
        self.pages.logs.logs_state.scroll = 0;
        self.pages.logs.logs_state.explicit_scroll = false;
        self.pages.logs.logs_state.snapshot = LogsSnapshot::default();
        self.pages.logs.logs_state.paused = false;
        self.pages.logs.logs_state.new_events = 0;
        self.request_logs_job(None);
        self.notify_status(i18n::LocalizedText::from(i18n::msg!("shell-logs")));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn logs_select_legacy_section(&mut self, incidents: bool) {
        self.pages.logs.logs_state.query.min_level = incidents.then_some(LogLevel::Critical);
        self.request_logs_job(None);
    }

    fn logs_access(&self) -> Option<LogAccess> {
        let session = self.app.auth_session()?;
        if session.source == identity::IdentitySource::LinuxCurrentProcess {
            return Some(LogAccess::OsUser);
        }
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
            self.pages.logs.logs_state.feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-log-storage-unavailable"
            )));
            return;
        };
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            self.pages.logs.logs_state.feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-logs-worker-unavailable"
            )));
            return;
        };
        if let Some(previous) = self.pages.logs.logs_state.job.take() {
            previous.0.cancelled.store(true, AtomicOrdering::Relaxed);
            self.tasks.cancel(TaskDomain::Logs, previous.0.id);
            self.tasks.complete(TaskDomain::Logs, previous.0.id);
        }
        let mut query = self.pages.logs.logs_state.query.clone();
        query.source = if self.pages.logs.logs_state.category == ui::LogsCategory::Linux {
            LogSource::Linux
        } else {
            LogSource::Ux
        };
        query.since = match self.pages.logs.logs_state.time_filter {
            1 => Some(Utc::now() - chrono::Duration::hours(1)),
            2 => Some(Utc::now() - chrono::Duration::days(1)),
            3 => Some(Utc::now() - chrono::Duration::days(7)),
            _ => self.pages.logs.logs_state.query.since,
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
            id: PageInstanceId::new().get(),
            cancelled: Arc::new(AtomicBool::new(false)),
            result: Mutex::new(None),
            worker: Mutex::new(None),
            background: self.pages.logs.logs_state.background_refresh,
        });
        self.pages.logs.logs_state.last_refresh = Some(Instant::now());
        let output = Arc::downgrade(&shared);
        let cancelled = shared.cancelled.clone();
        self.pages.logs.logs_state.revision = self.pages.logs.logs_state.revision.wrapping_add(1);
        let task = TaskId::new(format!("logs-query-{}", shared.id)).expect("unique logs task id");
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
                let job = LogsJob(shared);
                self.register_logs_job(&job);
                self.pages.logs.logs_state.job = Some(job);
                self.pages.logs.logs_state.feedback = None;
            }
            Err(error) => {
                self.pages.logs.logs_state.feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-cannot-start-logs-query-error",
                    error = error.to_string()
                )))
            }
        }
    }

    pub(in crate::session) fn poll_logs_tasks(&mut self) {
        for id in self.pages.logs.ids() {
            self.with_page_context(id, |session| session.poll_logs_page());
        }
    }

    pub(in crate::session) fn drain_logs_for_shutdown(&mut self) {
        for id in self.pages.logs.ids() {
            if let Some(page) = self.pages.logs.get(id)
                && let Some(job) = &page.logs_state.job
            {
                job.0
                    .result
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
            }
        }
    }

    fn register_logs_job(&mut self, job: &LogsJob) {
        let cancel = job.clone();
        let finished = job.clone();
        self.register_page_task(
            TaskDomain::Logs,
            job.0.id,
            TaskClass::Query,
            TaskControl::new(
                move || cancel.0.cancelled.store(true, AtomicOrdering::Relaxed),
                move || finished.finished(),
            ),
        );
    }

    fn poll_logs_page(&mut self) {
        let visible = self.pages.logs.selected_id() == Some(self.active_page_id());
        if visible && let Some(path) = self.pages.logs.logs_state.pending_document.take() {
            self.show_logs_document(path);
            return;
        }
        if self.pages.logs.logs_state.job.as_ref().is_some_and(|job| {
            self.tasks.owner(TaskDomain::Logs, job.0.id)
                != self.pages.logs.selected_id().map(TaskOwner::Page)
        }) {
            self.pages.logs.logs_state.job = None;
            return;
        }
        let stopped = self.pages.logs.logs_state.job.as_ref().is_some_and(|job| {
            job.0.worker.lock().ok().is_some_and(|worker| {
                worker
                    .as_ref()
                    .is_some_and(ManagedThreadHandle::is_finished)
            })
        });
        let result = self
            .pages
            .logs
            .logs_state
            .job
            .as_ref()
            .and_then(|job| job.0.result.lock().ok()?.take());
        let Some(result) = result else {
            if stopped {
                if let Some(job) = self.pages.logs.logs_state.job.take() {
                    self.tasks.complete(TaskDomain::Logs, job.0.id);
                }
                self.pages.logs.logs_state.feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-logs-worker-stopped-before-completing-the-request"
                )));
            }
            self.poll_live_logs();
            return;
        };
        let background = self
            .pages
            .logs
            .logs_state
            .job
            .as_ref()
            .is_some_and(|job| job.0.background);
        if let Some(job) = self.pages.logs.logs_state.job.take() {
            self.tasks.complete(TaskDomain::Logs, job.0.id);
        }
        match result {
            LogsJobResult::Snapshot(mut snapshot) => {
                let selected_id = self.logs_id_at(self.pages.logs.logs_state.selected);
                let top_id = self.logs_id_at(self.pages.logs.logs_state.scroll);
                if background {
                    let old_status = self
                        .pages
                        .logs
                        .logs_state
                        .snapshot
                        .result
                        .file_status
                        .as_ref();
                    let current_status = snapshot.result.file_status.as_ref();
                    if old_status.zip(current_status).is_some_and(|(old, new)| {
                        old.identity != new.identity || old.length > new.length
                    }) {
                        snapshot
                            .result
                            .notices
                            .push(i18n::tr!("ui-logs-file-replaced"));
                        self.pages.logs.logs_state.snapshot.result.events.clear();
                    }
                    let new =
                        merge_live_events(&mut snapshot, &self.pages.logs.logs_state.snapshot);
                    if self.pages.logs.logs_state.paused {
                        self.pages.logs.logs_state.new_events =
                            self.pages.logs.logs_state.new_events.saturating_add(new);
                    }
                }
                for event in &snapshot.result.events {
                    if self.pages.logs.logs_state.known_modules.len() < 128
                        && !self
                            .pages
                            .logs
                            .logs_state
                            .known_modules
                            .contains(&event.context.module)
                    {
                        self.pages
                            .logs
                            .logs_state
                            .known_modules
                            .push(event.context.module.clone());
                    }
                }
                self.pages.logs.logs_state.known_modules.sort();
                self.pages.logs.logs_state.snapshot = snapshot;
                self.apply_logs_sort();
                if background && !self.pages.logs.logs_state.paused {
                    self.pages.logs.logs_state.selected = 0;
                    self.pages.logs.logs_state.scroll = 0;
                    self.pages.logs.logs_state.explicit_scroll = false;
                } else {
                    if let Some(index) = selected_id.and_then(|id| {
                        (0..self.logs_count())
                            .find(|index| self.logs_id_at(*index).as_ref() == Some(&id))
                    }) {
                        self.pages.logs.logs_state.selected = index;
                    }
                    if background
                        && let Some(index) = top_id.and_then(|id| {
                            (0..self.logs_count())
                                .find(|index| self.logs_id_at(*index).as_ref() == Some(&id))
                        })
                    {
                        self.pages.logs.logs_state.scroll = index;
                    }
                }
                self.pages.logs.logs_state.selected = self
                    .pages
                    .logs
                    .logs_state
                    .selected
                    .min(self.logs_count().saturating_sub(1));
                let result = &self.pages.logs.logs_state.snapshot.result;
                let mut feedback = Vec::new();
                if result.state != runtime_log::LogSourceState::Ready {
                    feedback.push(log_state_label(result.state));
                }
                if !result.notices.is_empty() {
                    feedback.push(
                        result
                            .notices
                            .iter()
                            .map(|notice| log_notice_label(notice))
                            .collect::<Vec<_>>()
                            .join("; "),
                    );
                }
                self.pages.logs.logs_state.feedback =
                    (!feedback.is_empty()).then(|| feedback.join(" — ").into());
            }
            LogsJobResult::Document(Ok(path)) => {
                if visible {
                    self.show_logs_document(path);
                } else {
                    self.pages.logs.logs_state.pending_document = Some(path);
                }
            }
            LogsJobResult::Document(Err(error)) => {
                self.pages.logs.logs_state.refreshing_editor = false;
                self.pages.logs.logs_state.feedback = Some(error.clone().into());
                if visible && self.active_screen() == ShellScreen::Editor {
                    self.report_editor_error(error);
                }
            }
        }
    }

    fn show_logs_document(&mut self, path: PathBuf) {
        if self.active_screen() == ShellScreen::Logs {
            let context = LogsUiState {
                category: self.pages.logs.logs_state.category,
                query: self.pages.logs.logs_state.query.clone(),
                time_filter: self.pages.logs.logs_state.time_filter,
                last_document: self.pages.logs.logs_state.last_document.clone(),
                editor_snapshot: Some(path.clone()),
                ..LogsUiState::default()
            };
            match self.open_diagnostics_editor(EditorReloadPolicy::Log { path }) {
                Ok(()) => self.pages.logs.logs_state = context,
                Err(error) => self.pages.logs.logs_state.feedback = Some(error),
            }
        } else if self.active_screen() == ShellScreen::Editor
            && self.pages.logs.logs_state.refreshing_editor
        {
            if let Some(session) = self.pages.editor.editor_read_session.as_mut() {
                session.reload = EditorReloadPolicy::Log { path: path.clone() };
            }
            self.pages.logs.logs_state.editor_snapshot = Some(path);
            self.pages.logs.logs_state.refreshing_editor = false;
            self.reload_log_editor_file();
        }
    }

    fn logs_id_at(&self, index: usize) -> Option<String> {
        self.pages
            .logs
            .logs_state
            .snapshot
            .result
            .events
            .get(index)
            .map(|event| event.event_id.clone())
    }
    fn logs_count(&self) -> usize {
        self.pages.logs.logs_state.snapshot.result.events.len()
    }
    pub(super) fn logs_main_area(&self) -> Option<Rect> {
        match self.shell_layout_for(Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1)) {
            ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => Some(main),
        }
    }
    fn logs_move(&mut self, delta: isize) {
        self.pages.logs.logs_state.paused = true;
        self.pages.logs.logs_state.selected = self
            .pages
            .logs
            .logs_state
            .selected
            .saturating_add_signed(delta)
            .min(self.logs_count().saturating_sub(1));
        self.pages.logs.logs_state.explicit_scroll = false;
    }
    fn logs_open_selected(&mut self) {
        if self.logs_count() == 0 {
            return;
        }
        let selection = LogDocumentSelection::Events;
        self.pages.logs.logs_state.last_document = Some(selection.clone());
        self.request_logs_job(Some(selection));
    }
    fn logs_open_folder(&mut self) {
        let platform = self
            .settings_task_runtime
            .shared
            .platform
            .clone()
            .unwrap_or_else(|| Arc::from(platform::native_platform()));
        self.open_diagnostics_logs_in_explorer(platform.as_ref());
    }
    pub(in crate::session) fn refresh_logs_editor_snapshot(&mut self) -> bool {
        let matches = self
            .pages
            .editor
            .editor_read_session
            .as_ref()
            .is_some_and(|session| {
                self.pages.logs.logs_state.editor_snapshot.as_deref() == Some(session.reload.path())
            });
        if !matches {
            return false;
        }
        let Some(selection) = self.pages.logs.logs_state.last_document.clone() else {
            return false;
        };
        if self.pages.logs.logs_state.job.is_none() {
            self.pages.logs.logs_state.refreshing_editor = true;
            self.request_logs_job(Some(selection));
            self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-refreshing-log-query"
            )));
        }
        true
    }
    fn logs_set_category(&mut self, category: ui::LogsCategory) {
        self.pages.logs.logs_state.category = category;
        self.pages.logs.logs_state.query = LogQuery::default();
        self.pages.logs.logs_state.known_modules.clear();
        self.pages.logs.logs_state.selected = 0;
        self.pages.logs.logs_state.scroll = 0;
        self.pages.logs.logs_state.snapshot = LogsSnapshot::default();
        self.pages.logs.logs_state.paused = false;
        self.pages.logs.logs_state.new_events = 0;
        self.request_logs_job(None);
    }
    fn logs_filter_level(&mut self) {
        self.pages.logs.logs_state.query.min_level =
            match self.pages.logs.logs_state.query.min_level {
                None => Some(LogLevel::Trace),
                Some(LogLevel::Trace) => Some(LogLevel::Debug),
                Some(LogLevel::Debug) => Some(LogLevel::Info),
                Some(LogLevel::Info) => Some(LogLevel::Warning),
                Some(LogLevel::Warning) => Some(LogLevel::Error),
                Some(LogLevel::Error) => Some(LogLevel::Critical),
                Some(LogLevel::Critical) => None,
            };
        self.pages.logs.logs_state.selected = 0;
        self.request_logs_job(None);
    }
    fn logs_filter_module(&mut self) {
        let modules = &self.pages.logs.logs_state.known_modules;
        self.pages.logs.logs_state.query.module =
            match self.pages.logs.logs_state.query.module.as_ref() {
                None => modules.first().cloned(),
                Some(current) => modules
                    .iter()
                    .position(|module| module == current)
                    .and_then(|index| modules.get(index + 1))
                    .cloned(),
            };
        self.pages.logs.logs_state.selected = 0;
        self.request_logs_job(None);
    }
    fn logs_filter_time(&mut self) {
        self.pages.logs.logs_state.time_filter = (self.pages.logs.logs_state.time_filter + 1) % 4;
        self.pages.logs.logs_state.selected = 0;
        self.request_logs_job(None);
    }

    fn poll_live_logs(&mut self) {
        if self.active_screen() == ShellScreen::Logs
            && self.pages.logs.selected_id() == Some(self.active_page_id())
            && self.pages.logs.logs_state.job.is_none()
            && self
                .pages
                .logs
                .logs_state
                .last_refresh
                .is_some_and(|time| time.elapsed() >= Duration::from_secs(1))
        {
            self.pages.logs.logs_state.background_refresh = true;
            self.request_logs_job(None);
            self.pages.logs.logs_state.background_refresh = false;
        }
    }
    fn logs_restore_event_order(&mut self) {
        let sort_key = self.logs_sort_key();
        self.pages.common.table_sorts.remove(&sort_key);
        self.pages
            .logs
            .logs_state
            .snapshot
            .result
            .events
            .sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    }
    fn logs_follow_control(&mut self) {
        self.pages.logs.logs_state.paused = !self.pages.logs.logs_state.paused;
        if !self.pages.logs.logs_state.paused {
            self.logs_restore_event_order();
            self.pages.logs.logs_state.selected = 0;
            self.pages.logs.logs_state.scroll = 0;
            self.pages.logs.logs_state.explicit_scroll = false;
            self.pages.logs.logs_state.new_events = 0;
        }
    }
    fn logs_open_filter_form(&mut self) {
        let query = &self.pages.logs.logs_state.query;
        let fields = [
            (
                "unit",
                "ui-logs-unit",
                query.systemd_unit.clone().unwrap_or_default(),
            ),
            (
                "scope",
                "ui-logs-scope",
                query
                    .systemd_scope
                    .clone()
                    .unwrap_or_else(|| "system".into()),
            ),
            (
                "boot",
                "ui-logs-boot",
                query.systemd_boot.clone().unwrap_or_else(|| "0".into()),
            ),
            (
                "invocation",
                "ui-logs-invocation",
                query.systemd_invocation.clone().unwrap_or_default(),
            ),
            (
                "file",
                "ui-logs-file",
                query
                    .file_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
        ]
        .into_iter()
        .map(|(id, label, value)| ui::ManagementFormField {
            id: id.into(),
            label: i18n::tr!(label),
            value,
            choices: if id == "scope" {
                vec!["system".into(), "user".into()]
            } else {
                vec![]
            },
            ..Default::default()
        })
        .collect();
        self.pages.logs.logs_state.more_selected = None;
        self.pages.logs.logs_state.filter_form = Some(ui::ManagementForm {
            identity: "logs.filters".into(),
            title: i18n::tr!("ui-logs-source-filters"),
            fields,
            ..Default::default()
        });
    }
    fn logs_apply_filter_form(&mut self) {
        let Some(form) = self.pages.logs.logs_state.filter_form.clone() else {
            return;
        };
        let value = |id: &str| {
            form.fields
                .iter()
                .find(|field| field.id == id)
                .map(|field| field.value.trim())
                .unwrap_or("")
        };
        let mut query = LogQuery {
            source: LogSource::Linux,
            ..Default::default()
        };
        if !value("file").is_empty() {
            let path = PathBuf::from(value("file"));
            if !path.is_absolute() {
                if let Some(form) = self.pages.logs.logs_state.filter_form.as_mut() {
                    form.message = i18n::tr!("ui-logs-file-absolute");
                }
                return;
            }
            query.file_path = Some(path);
        } else {
            if !value("unit").is_empty() {
                let Some(service) = service_log_query(value("unit"), value("scope")) else {
                    if let Some(form) = self.pages.logs.logs_state.filter_form.as_mut() {
                        form.message = i18n::tr!("ui-logs-invalid-service");
                    }
                    return;
                };
                query = service;
            } else if !matches!(value("scope"), "system" | "user") {
                return;
            }
            query.systemd_scope = Some(value("scope").into());
            let valid_id =
                |id: &str| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit());
            if !value("boot").is_empty() {
                if !valid_id(value("boot"))
                    && !value("boot").parse::<i32>().is_ok_and(|number| {
                        (-10_000..=0).contains(&number) && number.to_string() == value("boot")
                    })
                {
                    if let Some(form) = self.pages.logs.logs_state.filter_form.as_mut() {
                        form.message = i18n::tr!("ui-logs-invalid-boot");
                    }
                    return;
                }
                query.systemd_boot = Some(value("boot").into());
            }
            if !value("invocation").is_empty() {
                if !valid_id(value("invocation")) {
                    if let Some(form) = self.pages.logs.logs_state.filter_form.as_mut() {
                        form.message = i18n::tr!("ui-logs-invalid-invocation");
                    }
                    return;
                }
                query.systemd_invocation = Some(value("invocation").into());
            }
        }
        self.pages.logs.logs_state.query = query;
        self.pages.logs.logs_state.category = ui::LogsCategory::Linux;
        self.pages.logs.logs_state.time_filter = 0;
        self.pages.logs.logs_state.filter_form = None;
        self.pages.logs.logs_state.snapshot = LogsSnapshot::default();
        self.pages.logs.logs_state.selected = 0;
        self.pages.logs.logs_state.scroll = 0;
        self.pages.logs.logs_state.paused = false;
        self.pages.logs.logs_state.new_events = 0;
        self.request_logs_job(None);
    }
    fn logs_handle_filter_key(&mut self, key: &KeyInput) {
        self.handle_logs_form_input(InputEvent::Key(key.clone()));
    }

    pub(in crate::session) fn handle_logs_form_input(&mut self, event: InputEvent) {
        let main = self.touch_main();
        let Some(form) = self.pages.logs.logs_state.filter_form.as_mut() else {
            return;
        };
        let event = form.handle_event(event, main);
        self.logs_filter_event(event);
    }

    fn logs_filter_event(&mut self, event: ui::components::FormEvent) {
        match event {
            ui::components::FormEvent::Action(ui::components::FormAction::Submit) => {
                self.logs_apply_filter_form()
            }
            ui::components::FormEvent::Action(ui::components::FormAction::Cancel) => {
                self.pages.logs.logs_state.filter_form = None
            }
            _ => {}
        }
    }

    pub(in crate::session) fn handle_logs_key(&mut self, key: &KeyInput) {
        if !key.phase.is_press_like() {
            return;
        }
        if self.pages.logs.logs_state.filter_form.is_some() {
            self.logs_handle_filter_key(key);
            return;
        }
        if let Some(selected) = self.pages.logs.logs_state.more_selected {
            if key.has_non_shift_modifier() {
                return;
            }
            let controls = ui::logs_more_controls();
            match key.key {
                InputKey::Escape | InputKey::F(10) if key.phase == InputPhase::Press => {
                    self.pages.logs.logs_state.more_selected = None
                }
                InputKey::Up | InputKey::BackTab => self.logs_move_more_selection(-1, true),
                InputKey::Down | InputKey::Tab => self.logs_move_more_selection(1, true),
                InputKey::Home => {
                    self.pages.logs.logs_state.more_selected =
                        Some(controls.len().saturating_sub(1));
                    self.logs_move_more_selection(1, true);
                }
                InputKey::End => {
                    self.pages.logs.logs_state.more_selected = Some(0);
                    self.logs_move_more_selection(-1, true);
                }
                InputKey::Enter | InputKey::Space | InputKey::Char(' ')
                    if key.phase == InputPhase::Press =>
                {
                    if let Some((target, _)) = controls.get(selected)
                        && ui::logs_control_enabled(&self.to_logs_view_model(), *target)
                    {
                        let target = *target;
                        self.pages.logs.logs_state.more_selected = None;
                        self.logs_touch_action(target);
                    }
                }
                _ => {}
            }
            return;
        }
        let detail_navigation = (key.modifiers.shift || key.modifiers.is_control())
            && !key.modifiers.alt
            && !key.modifiers.super_key
            && !key.modifiers.hyper
            && !key.modifiers.meta
            && matches!(key.key, InputKey::PageUp | InputKey::PageDown);
        if key.has_non_shift_modifier() && !detail_navigation {
            return;
        }
        let control = match key.key {
            InputKey::Enter | InputKey::Char('o' | 'O') => Some(ui::LogsHitTarget::Open),
            InputKey::Char('r' | 'R') | InputKey::F(5) => Some(ui::LogsHitTarget::Refresh),
            InputKey::Char('l' | 'L') => Some(ui::LogsHitTarget::FilterLevel),
            InputKey::Char('m' | 'M') => Some(ui::LogsHitTarget::FilterModule),
            InputKey::Char('t' | 'T') => Some(ui::LogsHitTarget::FilterTime),
            InputKey::Char('c' | 'C') => Some(ui::LogsHitTarget::ClearFilters),
            InputKey::Char('i' | 'I') => Some(ui::LogsHitTarget::RelatedIncident),
            InputKey::Char('e' | 'E') => Some(ui::LogsHitTarget::RelatedEvents),
            InputKey::Char('p' | 'P') => Some(ui::LogsHitTarget::Follow),
            InputKey::F(10) => Some(ui::LogsHitTarget::More),
            InputKey::Char('f' | 'F') => Some(ui::LogsHitTarget::Filters),
            InputKey::Char('g' | 'G') => Some(ui::LogsHitTarget::OpenFolder),
            _ => None,
        };
        if control.is_some_and(|control| {
            key.phase != InputPhase::Press
                || !ui::logs_control_enabled(&self.to_logs_view_model(), control)
        }) {
            return;
        }
        if key.phase != InputPhase::Press && key.key == InputKey::Escape {
            return;
        }
        if key.key == InputKey::Escape {
            self.cancel_logs_pointer_gesture();
        }
        if detail_navigation {
            self.pages.logs.logs_state.detail_scroll = self
                .pages
                .logs
                .logs_state
                .detail_scroll
                .saturating_add_signed(if key.key == InputKey::PageUp { -5 } else { 5 });
            return;
        }
        if matches!(
            key.key,
            InputKey::Up
                | InputKey::Down
                | InputKey::PageUp
                | InputKey::PageDown
                | InputKey::Home
                | InputKey::End
        ) {
            self.pages.logs.logs_state.detail_scroll = 0;
        }
        match key.key {
            InputKey::Escape => {
                self.pages.logs.logs_state.job = None;
                self.pages.logs.logs_state.scrollbar_grab = None;
                self.return_from_screen(ShellScreen::Logs);
            }
            InputKey::Left | InputKey::Right => self.logs_set_category(
                if self.pages.logs.logs_state.category == ui::LogsCategory::Ux {
                    ui::LogsCategory::Linux
                } else {
                    ui::LogsCategory::Ux
                },
            ),
            InputKey::Up => self.logs_move(-1),
            InputKey::Down => self.logs_move(1),
            InputKey::PageUp => self.logs_move(-10),
            InputKey::PageDown => self.logs_move(10),
            InputKey::Home => {
                self.logs_restore_event_order();
                self.pages.logs.logs_state.selected = 0;
                self.pages.logs.logs_state.explicit_scroll = false;
                self.pages.logs.logs_state.paused = false;
                self.pages.logs.logs_state.new_events = 0;
            }
            InputKey::End => {
                self.pages.logs.logs_state.selected = self.logs_count().saturating_sub(1);
                self.pages.logs.logs_state.explicit_scroll = false;
                self.pages.logs.logs_state.paused = true;
            }
            InputKey::Enter | InputKey::Char('o' | 'O') => self.logs_open_selected(),
            InputKey::Char('r' | 'R') | InputKey::F(5) => self.request_logs_job(None),
            InputKey::Char('l' | 'L') => self.logs_filter_level(),
            InputKey::Char('m' | 'M') => self.logs_filter_module(),
            InputKey::Char('t' | 'T') => self.logs_filter_time(),
            InputKey::Char('p' | 'P') => self.logs_follow_control(),
            InputKey::F(10) => self.pages.logs.logs_state.more_selected = Some(0),
            InputKey::Char('f' | 'F') => self.logs_open_filter_form(),
            InputKey::Char('g' | 'G') => self.logs_open_folder(),
            InputKey::Char('i' | 'I') => {
                if let Some(id) = self
                    .pages
                    .logs
                    .logs_state
                    .snapshot
                    .result
                    .events
                    .get(self.pages.logs.logs_state.selected)
                    .and_then(|event| event.incident_id.clone())
                {
                    let selection = LogDocumentSelection::Incident(id);
                    self.pages.logs.logs_state.last_document = Some(selection.clone());
                    self.request_logs_job(Some(selection));
                }
            }
            InputKey::Char('e' | 'E') => {
                let id = self
                    .pages
                    .logs
                    .logs_state
                    .snapshot
                    .result
                    .events
                    .get(self.pages.logs.logs_state.selected)
                    .and_then(|event| event.incident_id.clone());
                self.pages.logs.logs_state.query = LogQuery {
                    incident_id: id,
                    ..Default::default()
                };
                self.pages.logs.logs_state.time_filter = 0;
                self.pages.logs.logs_state.selected = 0;
                self.pages.logs.logs_state.scroll = 0;
                self.request_logs_job(None);
            }
            InputKey::Char('c' | 'C') => {
                self.pages.logs.logs_state.query = LogQuery::default();
                self.pages.logs.logs_state.time_filter = 0;
                self.request_logs_job(None);
            }
            _ => {}
        }
    }
    pub(in crate::session) fn to_logs_view_model(&self) -> ui::LogsViewModel {
        let _language = i18n::enter_snapshot(self.language.clone());
        let timezone = self
            .app
            .snapshot()
            .clock_timezone_id
            .and_then(|timezone| timezone.parse::<chrono_tz::Tz>().ok());
        let state = &self.pages.logs.logs_state;
        let system = matches!(
            self.logs_access(),
            Some(LogAccess::Admin | LogAccess::OsUser)
        );
        let diagnostics = ui::DiagnosticsViewModel {
            list_window_start: state.scroll,
            list_window_is_explicit: state.explicit_scroll,
            can_view_details: self.logs_access().is_some(),
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
            sort: self
                .pages
                .common
                .table_sorts
                .get(&self.logs_sort_key())
                .copied(),
            diagnostics,
            events: state
                .snapshot
                .result
                .events
                .iter()
                .map(|event| ui::LogsEventViewModel {
                    id: event.event_id.clone(),
                    timestamp: event.timestamp.to_rfc3339(),
                    time_label: timezone
                        .map(|timezone| {
                            event
                                .timestamp
                                .with_timezone(&timezone)
                                .format("%H:%M:%S")
                                .to_string()
                        })
                        .unwrap_or_else(|| {
                            event
                                .timestamp
                                .with_timezone(&chrono::Local)
                                .format("%H:%M:%S")
                                .to_string()
                        }),
                    level: log_level_label(event.level),
                    status: match event.level {
                        LogLevel::Critical | LogLevel::Error => ui::DiagnosticsStatus::Fail,
                        LogLevel::Warning => ui::DiagnosticsStatus::Warning,
                        _ => ui::DiagnosticsStatus::Pass,
                    },
                    module: event.context.module.clone(),
                    operation: event.context.operation.clone(),
                    summary: event.message.clone(),
                    detail: serde_event_detail(event),
                    incident_id: event.incident_id.clone(),
                })
                .collect(),
            selected_event: state.selected,
            scroll_offset: state.scroll,
            detail_scroll: state.detail_scroll,
            linux_available: cfg!(target_os = "linux"),
            can_view_system: system,
            loading: state.job.as_ref().is_some_and(|job| !job.0.background),
            following: !state.paused,
            new_events: state.new_events,
            more_selected: state.more_selected,
            filter_form: state.filter_form.clone(),
            selected_file: state.query.file_path.is_some(),
            level_filter: state
                .query
                .min_level
                .map(|level| {
                    if level == LogLevel::Critical {
                        log_level_label(level)
                    } else {
                        i18n::tr!("ui-logs-level-or-higher", level = log_level_label(level))
                    }
                })
                .unwrap_or_else(|| i18n::tr!("shell-all")),
            filter_summary: i18n::tr!(
                "ui-logs-filter-summary",
                module = state
                    .query
                    .module
                    .clone()
                    .unwrap_or_else(|| i18n::tr!("shell-all")),
                time = [
                    i18n::tr!("shell-all"),
                    i18n::tr!("shell-1-hour"),
                    i18n::tr!("shell-24-hours"),
                    i18n::tr!("shell-7-days")
                ][usize::from(state.time_filter)]
                .clone(),
                context = if let Some(file) = &state.query.file_path {
                    file.display().to_string()
                } else if let Some(unit) = &state.query.systemd_unit {
                    format!(
                        "{}: {} · boot={}{}",
                        state.query.systemd_scope.as_deref().unwrap_or("system"),
                        unit,
                        state.query.systemd_boot.as_deref().unwrap_or("all"),
                        state
                            .query
                            .systemd_invocation
                            .as_ref()
                            .map(|id| format!(" · run={}", &id[..id.len().min(8)]))
                            .unwrap_or_default()
                    )
                } else if let Some(boot) = &state.query.systemd_boot {
                    format!("boot={boot}")
                } else {
                    state
                        .query
                        .incident_id
                        .as_ref()
                        .map(|id| i18n::tr!("shell-incident-id", id = id))
                        .unwrap_or_else(|| i18n::tr!("shell-c-clear-filters"))
                }
            )
            .into(),
            feedback,
        }
    }
}

fn log_level_label(level: LogLevel) -> String {
    i18n::tr!(match level {
        LogLevel::Trace => "ui-logs-level-trace",
        LogLevel::Debug => "ui-logs-level-debug",
        LogLevel::Info => "ui-logs-level-info",
        LogLevel::Warning => "ui-logs-level-warning",
        LogLevel::Error => "ui-logs-level-error",
        LogLevel::Critical => "ui-logs-level-critical",
    })
}

fn log_state_label(state: runtime_log::LogSourceState) -> String {
    i18n::tr!(match state {
        runtime_log::LogSourceState::Ready => "ui-logs-state-ready",
        runtime_log::LogSourceState::Partial => "ui-logs-state-partial",
        runtime_log::LogSourceState::PermissionDenied => "ui-logs-state-permission",
        runtime_log::LogSourceState::Unavailable => "ui-logs-state-unavailable",
        runtime_log::LogSourceState::Unsupported => "ui-logs-state-unsupported",
        runtime_log::LogSourceState::Cancelled => "ui-logs-state-cancelled",
    })
}
fn log_notice_label(notice: &str) -> String {
    let key = match notice {
        "The log file is missing. Check the path or wait for rotation to finish." => {
            "ui-logs-file-missing"
        }
        "Log access denied. Check the file permissions." => "ui-logs-file-denied",
        "The log file could not be read. Check the file and refresh." => "ui-logs-file-unreadable",
        "Showing a bounded tail of the file. Earlier records remain in the log file." => {
            "ui-logs-file-tail"
        }
        "The log file was replaced. Refresh to read the new file." => "ui-logs-file-replaced",
        "Raw file records have no parsed event time or level. Clear event filters to inspect the file." => {
            "ui-logs-file-unparsed"
        }
        "Permission denied reading the system journal" => "ui-logs-journal-denied",
        _ => return notice.to_string(),
    };
    i18n::tr!(key)
}

fn merge_live_events(new: &mut LogsSnapshot, old: &LogsSnapshot) -> usize {
    use std::collections::HashSet;
    const BUFFER: usize = 1000;
    let existing: HashSet<_> = old
        .result
        .events
        .iter()
        .map(|event| event.event_id.as_str())
        .collect();
    let added = new
        .result
        .events
        .iter()
        .filter(|event| !existing.contains(event.event_id.as_str()))
        .count();
    let mut seen: HashSet<_> = new
        .result
        .events
        .iter()
        .map(|event| event.event_id.clone())
        .collect();
    let offsets: HashSet<_> = new
        .result
        .events
        .iter()
        .filter(|event| event.event_id.starts_with("file:"))
        .filter_map(|event| {
            event
                .event_id
                .rsplit_once(':')
                .map(|(offset, _)| offset.to_string())
        })
        .collect();
    for event in &old.result.events {
        if event.event_id.starts_with("file:")
            && event
                .event_id
                .rsplit_once(':')
                .is_some_and(|(offset, _)| offsets.contains(offset))
        {
            continue;
        }
        if seen.insert(event.event_id.clone()) {
            new.result.events.push(event.clone());
        }
    }
    new.result
        .events
        .sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    if new.result.events.len() > BUFFER {
        new.result.events.truncate(BUFFER);
        new.result.truncated = true;
        new.result.notices.push(i18n::tr!("ui-logs-buffer-limited"));
    }
    added
}

fn service_log_query(unit: &str, scope: &str) -> Option<LogQuery> {
    if !matches!(scope, "system" | "user")
        || !unit.ends_with(".service")
        || unit.len() <= ".service".len()
        || unit.len() > 255
        || unit.starts_with('-')
        || !unit.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'.' | b'@' | b'-' | b'\\')
        })
    {
        return None;
    }
    Some(LogQuery {
        source: LogSource::Linux,
        systemd_unit: Some(unit.into()),
        systemd_scope: Some(scope.into()),
        ..Default::default()
    })
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
#[path = "../../../../tests/unit/session/controller/logs/tests.rs"]
mod tests;

mod touch;
