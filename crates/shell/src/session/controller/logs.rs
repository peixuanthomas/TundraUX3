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
    detail_scroll: usize,
    detail_scrollbar_grab: Option<u16>,
    feedback: Option<i18n::LocalizedText>,
    job: Option<LogsJob>,
    revision: u64,
    time_filter: u8,
    known_modules: Vec<String>,
    pub(super) scrollbar_grab: Option<u16>,
    last_document: Option<LogDocumentSelection>,
    editor_snapshot: Option<PathBuf>,
    refreshing_editor: bool,
    return_component: Option<ShellComponent>,
    paused: bool,
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
            self.logs_state.return_component = Some(self.focused_component);
            self.screen_stack.push(ShellScreen::Logs);
        }
        self.focused_component = ShellComponent::Logs;
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
        self.logs_state.query = query;
        self.logs_state.category = ui::LogsCategory::Linux;
        self.logs_state.section = ui::LogsSection::Events;
        self.logs_state.time_filter = 0;
        self.logs_state.known_modules.clear();
        self.logs_state.selected = 0;
        self.logs_state.scroll = 0;
        self.logs_state.explicit_scroll = false;
        self.logs_state.snapshot = LogsSnapshot::default();
        self.logs_state.paused = false;
        self.logs_state.new_events = 0;
        self.open_logs();
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
            _ => self.logs_state.query.since,
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
            background: self.logs_state.background_refresh,
        });
        self.logs_state.last_refresh = Some(Instant::now());
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
            self.poll_live_logs();
            return;
        };
        let background = self
            .logs_state
            .job
            .as_ref()
            .is_some_and(|job| job.0.background);
        self.logs_state.job = None;
        match result {
            LogsJobResult::Snapshot(mut snapshot) => {
                let selected_id = self.logs_id_at(self.logs_state.selected);
                let top_id = self.logs_id_at(self.logs_state.scroll);
                if background {
                    let old_status = self.logs_state.snapshot.result.file_status.as_ref();
                    let current_status = snapshot.result.file_status.as_ref();
                    if old_status.zip(current_status).is_some_and(|(old, new)| {
                        old.identity != new.identity || old.length > new.length
                    }) {
                        snapshot
                            .result
                            .notices
                            .push(i18n::tr!("ui-logs-file-replaced"));
                        self.logs_state.snapshot.result.events.clear();
                    }
                    let new = merge_live_events(&mut snapshot, &self.logs_state.snapshot);
                    if self.logs_state.paused {
                        self.logs_state.new_events = self.logs_state.new_events.saturating_add(new);
                    }
                }
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
                if background && !self.logs_state.paused {
                    self.logs_state.selected = 0;
                    self.logs_state.scroll = 0;
                    self.logs_state.explicit_scroll = false;
                } else {
                    if let Some(index) = selected_id.and_then(|id| {
                        (0..self.logs_count())
                            .find(|index| self.logs_id_at(*index).as_ref() == Some(&id))
                    }) {
                        self.logs_state.selected = index;
                    }
                    if background
                        && let Some(index) = top_id.and_then(|id| {
                            (0..self.logs_count())
                                .find(|index| self.logs_id_at(*index).as_ref() == Some(&id))
                        })
                    {
                        self.logs_state.scroll = index;
                    }
                }
                self.logs_state.selected = self
                    .logs_state
                    .selected
                    .min(self.logs_count().saturating_sub(1));
                self.logs_state.feedback = Some(
                    format!(
                        "{}{}",
                        log_state_label(self.logs_state.snapshot.result.state),
                        if self.logs_state.snapshot.result.notices.is_empty() {
                            String::new()
                        } else {
                            format!(
                                " — {}",
                                self.logs_state
                                    .snapshot
                                    .result
                                    .notices
                                    .iter()
                                    .map(|notice| log_notice_label(notice))
                                    .collect::<Vec<_>>()
                                    .join("; ")
                            )
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
    pub(super) fn logs_main_area(&self) -> Option<Rect> {
        match self.shell_layout_for(Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1)) {
            ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => Some(main),
        }
    }
    fn logs_move(&mut self, delta: isize) {
        self.logs_state.paused = true;
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
        self.logs_state.paused = false;
        self.logs_state.new_events = 0;
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

    fn poll_live_logs(&mut self) {
        if self.active_screen() == ShellScreen::Logs
            && self.logs_state.job.is_none()
            && (self.logs_state.category == ui::LogsCategory::Linux
                || self.logs_state.section == ui::LogsSection::Events)
            && self
                .logs_state
                .last_refresh
                .is_some_and(|time| time.elapsed() >= Duration::from_secs(1))
        {
            self.logs_state.background_refresh = true;
            self.request_logs_job(None);
            self.logs_state.background_refresh = false;
        }
    }
    fn logs_follow_control(&mut self) {
        self.logs_state.paused = !self.logs_state.paused;
        if !self.logs_state.paused {
            self.logs_state.selected = 0;
            self.logs_state.scroll = 0;
            self.logs_state.explicit_scroll = false;
            self.logs_state.new_events = 0;
        }
    }
    fn logs_open_filter_form(&mut self) {
        let query = &self.logs_state.query;
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
        self.logs_state.more_selected = None;
        self.logs_state.filter_form = Some(ui::ManagementForm {
            identity: "logs.filters".into(),
            title: i18n::tr!("ui-logs-source-filters"),
            fields,
            ..Default::default()
        });
    }
    fn logs_apply_filter_form(&mut self) {
        let Some(form) = self.logs_state.filter_form.clone() else {
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
                if let Some(form) = self.logs_state.filter_form.as_mut() {
                    form.message = i18n::tr!("ui-logs-file-absolute");
                }
                return;
            }
            query.file_path = Some(path);
        } else {
            if !value("unit").is_empty() {
                let Some(service) = service_log_query(value("unit"), value("scope")) else {
                    if let Some(form) = self.logs_state.filter_form.as_mut() {
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
                    if let Some(form) = self.logs_state.filter_form.as_mut() {
                        form.message = i18n::tr!("ui-logs-invalid-boot");
                    }
                    return;
                }
                query.systemd_boot = Some(value("boot").into());
            }
            if !value("invocation").is_empty() {
                if !valid_id(value("invocation")) {
                    if let Some(form) = self.logs_state.filter_form.as_mut() {
                        form.message = i18n::tr!("ui-logs-invalid-invocation");
                    }
                    return;
                }
                query.systemd_invocation = Some(value("invocation").into());
            }
        }
        self.logs_state.query = query;
        self.logs_state.category = ui::LogsCategory::Linux;
        self.logs_state.section = ui::LogsSection::Events;
        self.logs_state.time_filter = 0;
        self.logs_state.filter_form = None;
        self.logs_state.snapshot = LogsSnapshot::default();
        self.logs_state.selected = 0;
        self.logs_state.scroll = 0;
        self.logs_state.paused = false;
        self.logs_state.new_events = 0;
        self.request_logs_job(None);
    }
    fn logs_handle_filter_key(&mut self, key: &KeyInput) {
        if key.phase != InputPhase::Press
            && !matches!(key.key, InputKey::Char(_) | InputKey::Backspace)
        {
            return;
        }
        if key.key == InputKey::Escape {
            self.logs_state.filter_form = None;
            return;
        }
        if key.modifiers.is_control() && key.key == InputKey::Enter {
            self.logs_apply_filter_form();
            return;
        }
        if key.has_non_shift_modifier() {
            return;
        }
        let Some(form) = self.logs_state.filter_form.as_mut() else {
            return;
        };
        let count = form.fields.len();
        match key.key {
            InputKey::Tab if key.modifiers.shift => {
                form.selected = if form.selected == 0 {
                    count
                } else {
                    form.selected - 1
                }
            }
            InputKey::Tab | InputKey::Down => form.selected = (form.selected + 1) % (count + 1),
            InputKey::BackTab | InputKey::Up => {
                form.selected = if form.selected == 0 {
                    count
                } else {
                    form.selected - 1
                }
            }
            InputKey::Enter if form.selected == count => self.logs_apply_filter_form(),
            InputKey::Enter | InputKey::Left | InputKey::Right => {
                if let Some(field) = form.fields.get_mut(form.selected)
                    && !field.choices.is_empty()
                {
                    let index = field
                        .choices
                        .iter()
                        .position(|choice| choice == &field.value)
                        .unwrap_or(0);
                    field.value = field.choices[(index + 1) % field.choices.len()].clone();
                }
            }
            InputKey::Backspace => {
                if let Some(field) = form.fields.get_mut(form.selected) {
                    field.value.pop();
                }
            }
            InputKey::Space => {
                if let Some(field) = form.fields.get_mut(form.selected) {
                    field.value.push(' ');
                }
            }
            InputKey::Char(c) if !c.is_control() => {
                if let Some(field) = form.fields.get_mut(form.selected)
                    && field.choices.is_empty()
                {
                    field.value.push(c);
                }
            }
            _ => {}
        }
    }

    pub(in crate::session) fn handle_logs_key(&mut self, key: &KeyInput) {
        if !key.phase.is_press_like() {
            return;
        }
        if self.logs_state.filter_form.is_some() {
            self.logs_handle_filter_key(key);
            return;
        }
        if let Some(selected) = self.logs_state.more_selected {
            if key.has_non_shift_modifier() {
                return;
            }
            let controls = ui::logs_more_controls();
            match key.key {
                InputKey::Escape | InputKey::F(10) if key.phase == InputPhase::Press => {
                    self.logs_state.more_selected = None
                }
                InputKey::Up | InputKey::BackTab => self.logs_move_more_selection(-1, true),
                InputKey::Down | InputKey::Tab => self.logs_move_more_selection(1, true),
                InputKey::Home => {
                    self.logs_state.more_selected = Some(controls.len().saturating_sub(1));
                    self.logs_move_more_selection(1, true);
                }
                InputKey::End => {
                    self.logs_state.more_selected = Some(0);
                    self.logs_move_more_selection(-1, true);
                }
                InputKey::Enter | InputKey::Space | InputKey::Char(' ')
                    if key.phase == InputPhase::Press =>
                {
                    if let Some((target, _)) = controls.get(selected)
                        && ui::logs_control_enabled(&self.to_logs_view_model(), *target)
                    {
                        let target = *target;
                        self.logs_state.more_selected = None;
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
            _ => None,
        };
        if control.is_some_and(|control| {
            key.phase != InputPhase::Press
                || !ui::logs_control_enabled(&self.to_logs_view_model(), control)
        }) {
            return;
        }
        if key.phase != InputPhase::Press && matches!(key.key, InputKey::Escape | InputKey::Tab) {
            return;
        }
        if key.key == InputKey::Escape {
            self.cancel_logs_pointer_gesture();
        }
        if detail_navigation {
            self.logs_state.detail_scroll = self
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
            self.logs_state.detail_scroll = 0;
        }
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
                    self.logs_state
                        .return_component
                        .take()
                        .unwrap_or(ShellComponent::Home)
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
                self.logs_state.paused = false;
                self.logs_state.new_events = 0;
            }
            InputKey::End => {
                self.logs_state.selected = self.logs_count().saturating_sub(1);
                self.logs_state.explicit_scroll = false;
                self.logs_state.paused = true;
            }
            InputKey::Enter | InputKey::Char('o' | 'O') => self.logs_open_selected(),
            InputKey::Char('r' | 'R') | InputKey::F(5) => self.request_logs_job(None),
            InputKey::Char('l' | 'L') => self.logs_filter_level(),
            InputKey::Char('m' | 'M') => self.logs_filter_module(),
            InputKey::Char('t' | 'T') => self.logs_filter_time(),
            InputKey::Char('p' | 'P') => self.logs_follow_control(),
            InputKey::F(10) => self.logs_state.more_selected = Some(0),
            InputKey::Char('f' | 'F') => self.logs_open_filter_form(),
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
    pub(in crate::session) fn to_logs_view_model(&self) -> ui::LogsViewModel {
        let _language = i18n::enter_snapshot(self.language.clone());
        let state = &self.logs_state;
        let system = matches!(
            self.logs_access(),
            Some(LogAccess::Admin | LogAccess::OsUser)
        );
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
            detail_scroll: state.detail_scroll,
            linux_available: cfg!(target_os = "linux"),
            can_view_system: system,
            loading: state.job.as_ref().is_some_and(|job| !job.0.background),
            following: !state.paused,
            new_events: state.new_events,
            more_selected: state.more_selected,
            filter_form: state.filter_form.clone(),
            selected_file: state.query.file_path.is_some(),
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
                arg4 = if let Some(file) = &state.query.file_path {
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
#[path = "../../../tests/unit/session/controller/logs/tests.rs"]
mod tests;

#[path = "logs_touch.rs"]
mod touch;
