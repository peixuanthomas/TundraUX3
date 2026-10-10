mod actions;
mod input;
use crate::session::*;
use platform::management::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub(in crate::session) struct ManagementJob(Arc<ManagementJobShared>);
struct ManagementJobShared {
    cancelled: Arc<AtomicBool>,
    snapshot: Mutex<Option<Result<ManagementSnapshot, ManagementError>>>,
    draft: Mutex<Option<Result<ConfigDraft, ManagementError>>>,
    events: Mutex<VecDeque<OperationEvent>>,
    responses: mpsc::Sender<OperationInput>,
    worker: Mutex<Option<ManagedThreadHandle<()>>>,
}
impl fmt::Debug for ManagementJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ManagementJob")
    }
}
impl PartialEq for ManagementJob {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for ManagementJob {}
impl Drop for ManagementJobShared {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Ok(Some(worker)) = self.worker.get_mut() {
            worker.cancel();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FormPurpose {
    Action(ManagementAction, Option<ManagementRow>),
    Answer(String),
    Menu(Vec<(ManagementAction, Option<ManagementRow>)>),
    Configuration(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagementEditor {
    title: String,
    message: String,
    message_scroll: u16,
    fields: Vec<ManagementField>,
    selected: usize,
    purpose: FormPurpose,
}

#[derive(Clone)]
struct ManagementTerminal(Arc<Mutex<vt100::Parser>>);
impl fmt::Debug for ManagementTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ManagementTerminal")
    }
}
impl PartialEq for ManagementTerminal {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for ManagementTerminal {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::session) struct ManagementState {
    kind: Option<ManagementKind>,
    query: Option<ManagementQuery>,
    snapshot: ManagementSnapshot,
    selected: usize,
    scroll: usize,
    list_scroll_explicit: bool,
    table_scroll: usize,
    sort: Option<ui::TableSort>,
    sort_columns: Vec<String>,
    action_scroll: Option<usize>,
    form_field_scroll: Option<usize>,
    choice_field: Option<usize>,
    choice_scroll: usize,
    choice_selected: usize,
    choice_columns: usize,
    scrollbar_grab: Option<(ui::ManagementScrollTarget, u16)>,
    selected_action: usize,
    actions_focused: bool,
    filtering: bool,
    filter_input: String,
    shortcut_repeat_guard: Option<InputKey>,
    status: String,
    outcome: Option<String>,
    received_snapshot: bool,
    pending_directory: Option<PathBuf>,
    pending_select: Option<PathBuf>,
    output: String,
    output_scroll: u16,
    details_scroll: u16,
    details_only: bool,
    query_job: Option<ManagementJob>,
    draft_job: Option<ManagementJob>,
    pending_draft: Option<ConfigDraft>,
    operation_job: Option<ManagementJob>,
    auto_admin_job: Option<AutoAdminJob>,
    form: Option<ManagementEditor>,
    parser: Option<ManagementTerminal>,
    terminal_mode: bool,
    revision: u64,
    refreshed: Option<Instant>,
    problem: Option<problem::OperationProblem>,
    configuration_operation: bool,
}

impl ManagementState {
    fn sort_snapshot(&mut self, snapshot: &mut ManagementSnapshot) {
        if self.sort_columns != snapshot.columns {
            self.sort = None;
            self.sort_columns = snapshot.columns.clone();
        }
        if let Some(sort) = self.sort {
            snapshot.rows.sort_by(|a, b| {
                sort.compare(
                    a.cells.get(sort.column).map_or("", String::as_str),
                    b.cells.get(sort.column).map_or("", String::as_str),
                )
            });
        }
    }
}

pub(in crate::session) fn management_title(kind: ManagementKind) -> String {
    match kind {
        ManagementKind::Services => i18n::tr!("management-services"),
        ManagementKind::Processes => i18n::tr!("management-processes"),
        ManagementKind::Packages => i18n::tr!("management-packages"),
        ManagementKind::Network => i18n::tr!("management-network"),
        ManagementKind::Disks => i18n::tr!("management-disks"),
        ManagementKind::Users => i18n::tr!("management-users"),
        ManagementKind::SystemConfig => i18n::tr!("management-system-config"),
    }
}
fn management_label(id: &str, fallback: &str) -> String {
    let key = format!("management-action-{}", id.replace('_', "-"));
    let result = i18n::tr!(key.clone());
    if result.contains(&key) {
        fallback.to_string()
    } else {
        result
    }
}
fn action_label(action: &ManagementAction) -> String {
    if action.id == "set_view" {
        if let Some(scope) = action.values.get("scope") {
            return management_label(&format!("scope-{scope}"), &action.label);
        }
    }
    management_label(&action.id, &action.label)
}

fn management_action_shortcut(
    kind: Option<ManagementKind>,
    id: &str,
) -> Option<(&'static str, InputKey)> {
    let letter = match id {
        "start" | "upgrade_all" | "pacman_upgrade_all" | "scan" => 'a',
        "stop" if kind == Some(ManagementKind::Processes) => 'p',
        "stop" | "remove" => 'x',
        "restart" | "term" => 't',
        "enable" | "refresh" => 'e',
        "disable" | "disconnect" | "wifi-disconnect" => 'd',
        "view_logs" => 'l',
        "set_view" => 'v',
        "kill" => 'k',
        "cont" | "configure" => 'c',
        "nice" | "check" => 'n',
        "install" | "pacman_install" | "inspect_network" => 'i',
        "upgrade" | "pacman_upgrade" | "unmount" => 'u',
        "wifi-connect" => 'w',
        "forget" | "wifi-forget" | "forget_saved_wifi" => 'f',
        "mount" => 'm',
        "open_directory" => 'o',
        "scope_search" => return Some(("F6", InputKey::F(6))),
        "scope_installed" => return Some(("F7", InputKey::F(7))),
        "scope_updates" => return Some(("F8", InputKey::F(8))),
        "recover" => return Some(("F9", InputKey::F(9))),
        "cancel_operation" => return Some(("F10", InputKey::F(10))),
        _ => return None,
    };
    let label = match letter {
        'a' => "A",
        'c' => "C",
        'd' => "D",
        'e' => "E",
        'f' => "F",
        'i' => "I",
        'k' => "K",
        'l' => "L",
        'm' => "M",
        'n' => "N",
        'o' => "O",
        'p' => "P",
        't' => "T",
        'u' => "U",
        'v' => "V",
        'w' => "W",
        'x' => "X",
        _ => unreachable!(),
    };
    Some((label, InputKey::Char(letter)))
}

fn management_control_modifier(key: &KeyInput) -> bool {
    key.modifiers.is_control()
        && !key.modifiers.shift
        && !key.modifiers.alt
        && !key.modifiers.super_key
        && !key.modifiers.hyper
        && !key.modifiers.meta
}
fn management_text(prefix: &str, id: &str, fallback: &str) -> String {
    if id.len() > 200 {
        return fallback.into();
    }
    let mut normalized = String::new();
    for character in id.chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
        } else if !normalized.ends_with('-') {
            normalized.push('-');
        }
    }
    let id = normalized;
    let key = format!("management-{prefix}-{}", id.trim_matches('-'));
    let text = i18n::tr!(key.clone());
    if text.contains(&key) {
        fallback.to_string()
    } else {
        text
    }
}

fn management_detail_value(kind: Option<ManagementKind>, key: &str, value: &str) -> String {
    if kind == Some(ManagementKind::Users) && key == "Source" {
        return management_text("account-source", value, value);
    }
    let account_state = kind == Some(ManagementKind::Users)
        && (matches!(key, "Password locked" | "Account expiry")
            || (key == "Groups"
                && value == "unknown (select the account to query full membership)"));
    if account_state {
        management_text("value", value, value)
    } else {
        value.to_owned()
    }
}

fn management_view_action(action: &ManagementAction) -> bool {
    action.id == "set_view" || action.id.starts_with("scope_")
}

fn query_management_snapshot(
    query: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let package_details = query.kind == ManagementKind::Packages
        && matches!(
            query.scope.as_str(),
            "" | "installed" | "search" | "updates"
        )
        && query.target.is_some();
    if !package_details {
        return platform::management::query(query, cancelled);
    }
    // Package detail queries return one package. Keep the current list visible
    // and replace only the selected row with its richer metadata.
    let mut list_query = query.clone();
    list_query.target = None;
    let mut snapshot = platform::management::query(&list_query, cancelled)?;
    let mut details_query = query.clone();
    details_query.filter.clear();
    let details = platform::management::query(&details_query, cancelled);
    merge_package_details(&mut snapshot, query.target.as_deref().unwrap(), details)?;
    Ok(snapshot)
}

fn merge_package_details(
    snapshot: &mut ManagementSnapshot,
    target: &str,
    details: Result<ManagementSnapshot, ManagementError>,
) -> Result<(), ManagementError> {
    let row = snapshot.rows.iter_mut().find(|row| row.id == target);
    match details {
        Ok(details) => {
            if let (Some(row), Some(detail)) =
                (row, details.rows.into_iter().find(|row| row.id == target))
            {
                *row = detail;
            }
        }
        Err(ManagementError::Cancelled) => return Err(ManagementError::Cancelled),
        Err(error) => {
            // A cache or detail failure must not discard a usable installed
            // list or its removal actions. Show the actual error with the row.
            if let Some(row) = row {
                row.detail.push(("Details".into(), error.to_string()));
            }
        }
    }
    Ok(())
}

impl ShellSession {
    pub(in crate::session) fn open_management(&mut self, kind: ManagementKind) {
        if !cfg!(target_os = "linux") || self.app.auth_session().is_none() || self.is_strict_guest()
        {
            return;
        }
        self.enter_screen(ShellScreen::Management);
        if self.management_state.kind != Some(kind) {
            let next =
                self.management_background
                    .remove(&kind)
                    .unwrap_or_else(|| ManagementState {
                        kind: Some(kind),
                        query: Some(ManagementQuery::new(kind)),
                        ..Default::default()
                    });
            let previous = std::mem::replace(&mut self.management_state, next);
            if let Some(previous_kind) = previous.kind {
                self.management_background.insert(previous_kind, previous);
            }
        }
        self.focused_component = ShellComponent::Management;
        self.resolve_notification_alert(&format!("management.{}", kind.id()));
        if self.management_state.operation_job.is_none() && !self.management_state.received_snapshot
        {
            self.refresh_management();
        }
    }

    fn management_job(&self) -> (ManagementJob, mpsc::Receiver<OperationInput>) {
        let (tx, rx) = mpsc::channel();
        (
            ManagementJob(Arc::new(ManagementJobShared {
                cancelled: Arc::new(AtomicBool::new(false)),
                snapshot: Mutex::new(None),
                draft: Mutex::new(None),
                events: Mutex::new(VecDeque::new()),
                responses: tx,
                worker: Mutex::new(None),
            })),
            rx,
        )
    }
    pub(in crate::session) fn refresh_management(&mut self) {
        let Some(query) = self.management_state.query.clone() else {
            return;
        };
        // A requested refresh replaces the previous query, including when a new
        // worker cannot be started. Its late result must not restore an old filter.
        self.management_state.query_job = None;
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            self.management_state.status = i18n::tr!("management-worker-unavailable");
            return;
        };
        let (job, _) = self.management_job();
        let output = Arc::downgrade(&job.0);
        let cancelled = job.0.cancelled.clone();
        self.management_state.revision = self.management_state.revision.wrapping_add(1);
        let task = TaskId::new(format!(
            "management-query-{}-{}",
            query.kind.id(),
            self.management_state.revision % 64
        ))
        .expect("bounded task identifier");
        match group.spawn_thread(TaskSpec::one_shot(task), move || {
            let result = query_management_snapshot(&query, &cancelled);
            if let Some(output) = output.upgrade() {
                *output.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            }
        }) {
            Ok(worker) => {
                *job.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.management_state.query_job = Some(job);
                self.management_state.status = i18n::tr!("management-loading");
            }
            Err(e) => {
                self.management_state.status = i18n::tr!("management-worker-unavailable");
                self.management_state.output = e.to_string();
            }
        }
    }

    pub(in crate::session) fn poll_management(&mut self) {
        let visible = self.active_screen() == ShellScreen::Management;
        self.poll_management_page(visible);
        let kinds = self
            .management_background
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for kind in kinds {
            if let Some(background) = self.management_background.remove(&kind) {
                let active = std::mem::replace(&mut self.management_state, background);
                self.poll_management_page(false);
                let background = std::mem::replace(&mut self.management_state, active);
                self.management_background.insert(kind, background);
            }
        }
    }
    fn poll_management_page(&mut self, visible: bool) {
        let draft_result = self
            .management_state
            .draft_job
            .as_ref()
            .and_then(|job| job.0.draft.lock().ok()?.take());
        if let Some(result) = draft_result {
            self.management_state.draft_job = None;
            match result {
                Ok(draft) => self.management_state.pending_draft = Some(draft),
                Err(error) => {
                    let p = problem::OperationProblem::from_error(&error);
                    self.management_state.status = i18n::tr!(p.summary_key.clone());
                    self.management_state.output = p.detail.clone();
                    self.management_state.problem = Some(p);
                }
            }
        }
        if visible {
            if let Some(draft) = self.management_state.pending_draft.take() {
                self.begin_config_editor(
                    draft.path.clone(),
                    Some(draft),
                    None,
                    None,
                    "system".into(),
                );
            }
        }
        if !visible {
            self.cancel_management_pointer_gesture();
        }
        let result = self
            .management_state
            .query_job
            .as_ref()
            .and_then(|job| job.0.snapshot.lock().ok()?.take());
        if let Some(result) = result {
            self.management_state.query_job = None;
            match result {
                Ok(snapshot) => {
                    let mut snapshot = snapshot;
                    #[cfg(target_os = "linux")]
                    {
                        let uid = unsafe { libc::getuid() };
                        let sockets = platform::management::helper::recoverable_operations(uid)
                            .into_iter()
                            .filter(|socket| {
                                platform::management::helper::operation_kind(socket)
                                    == self.management_state.kind
                            })
                            .collect::<Vec<_>>();
                        if !sockets.is_empty() {
                            snapshot.actions.push(ManagementAction {
                                id: "recover".into(),
                                label: "Reconnect to a system operation".into(),
                                fields: vec![ManagementField {
                                    id: "socket".into(),
                                    label: "Operation".into(),
                                    value: sockets.last().unwrap().display().to_string(),
                                    choices: sockets
                                        .iter()
                                        .map(|p| p.display().to_string())
                                        .collect(),
                                    required: true,
                                    ..Default::default()
                                }],
                                ..Default::default()
                            });
                        }
                    }
                    self.management_state.sort_snapshot(&mut snapshot);
                    let selected = self
                        .management_state
                        .snapshot
                        .rows
                        .get(self.management_state.selected)
                        .map(|r| r.id.clone());
                    self.management_state.selected = selected
                        .and_then(|id| snapshot.rows.iter().position(|r| r.id == id))
                        .unwrap_or(self.management_state.selected)
                        .min(snapshot.rows.len().saturating_sub(1));
                    self.management_state.status =
                        self.management_state.outcome.clone().unwrap_or_else(|| {
                            if snapshot.notices.is_empty() {
                                format!("{} · {}", snapshot.backend, snapshot.rows.len())
                            } else {
                                snapshot.notices.join("; ")
                            }
                        });
                    self.management_state.snapshot = snapshot;
                    self.management_state.problem = None;
                }
                Err(e) => {
                    let problem = problem::OperationProblem::from_error(&e);
                    self.management_state.status = i18n::tr!(problem.summary_key.clone());
                    self.management_state.output = problem.detail.clone();
                    self.management_state.problem = Some(problem);
                }
            }
            self.management_state.refreshed = Some(Instant::now());
        }
        let events = self
            .management_state
            .operation_job
            .as_ref()
            .map(|j| {
                j.0.events
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .drain(..)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut completed = false;
        for event in events {
            self.management_state.revision = self.management_state.revision.wrapping_add(1);
            match event {
                OperationEvent::ConfigDocument { document } => {
                    self.editor_config.received = Some(document)
                }
                OperationEvent::Connected { .. } => {}
                OperationEvent::Problem { problem } => {
                    if self
                        .management_state
                        .problem
                        .as_ref()
                        .is_some_and(|p| p.native_exit_code.is_some())
                    {
                        continue;
                    }
                    self.management_state.status = i18n::tr!(problem.summary_key.clone());
                    self.management_state.output.push_str(&problem.detail);
                    self.management_state.problem = Some(problem);
                }
                OperationEvent::Started {
                    kind,
                    action,
                    target,
                } => {
                    if kind != ManagementKind::SystemConfig
                        && self.management_state.kind != Some(kind)
                    {
                        self.management_state.kind = Some(kind);
                        self.management_state.query = Some(ManagementQuery::new(kind));
                        self.management_state.snapshot = ManagementSnapshot::default();
                    }
                    self.management_state.status = format!(
                        "{} {}",
                        management_label(&action, &action),
                        target.unwrap_or_default()
                    );
                }
                OperationEvent::Snapshot { snapshot } => {
                    if self.management_state.configuration_operation {
                        if self.editor_config.pending_action.as_deref() == Some("history") {
                            self.editor_config.history = snapshot;
                        }
                        continue;
                    }
                    self.management_state.query_job = None;
                    let mut snapshot = snapshot;
                    self.management_state.sort_snapshot(&mut snapshot);
                    let selected = self
                        .management_state
                        .snapshot
                        .rows
                        .get(self.management_state.selected)
                        .map(|row| row.id.clone());
                    self.management_state.selected = selected
                        .and_then(|id| snapshot.rows.iter().position(|row| row.id == id))
                        .unwrap_or(self.management_state.selected)
                        .min(snapshot.rows.len().saturating_sub(1));
                    self.management_state.snapshot = snapshot;
                    self.management_state.received_snapshot = true;
                }
                OperationEvent::Progress { message, percent } => {
                    self.management_state.status =
                        percent.map_or_else(|| message.clone(), |p| format!("{p}% {message}"))
                }
                OperationEvent::Output { text } => {
                    self.management_state.output.push_str(&text);
                    if self.management_state.output.len() > 256 * 1024 {
                        let mut start = self.management_state.output.len() - 128 * 1024;
                        while !self.management_state.output.is_char_boundary(start) {
                            start += 1;
                        }
                        self.management_state.output.drain(..start);
                    }
                }
                OperationEvent::TerminalOutput { bytes } => {
                    let area = ui::management_layout(
                        self.management_main(),
                        &self.to_management_view_model(),
                    )
                    .output_text;
                    let parser = self
                        .management_state
                        .parser
                        .get_or_insert_with(|| {
                            ManagementTerminal(Arc::new(Mutex::new(vt100::Parser::new(
                                area.height.max(1),
                                area.width.max(1),
                                2000,
                            ))))
                        })
                        .clone();
                    if let Ok(mut parser) = parser.0.lock() {
                        parser.process(&bytes);
                        self.management_state.output = parser.screen().contents();
                    }
                }
                OperationEvent::Question {
                    id,
                    prompt,
                    choices,
                    secret,
                } => {
                    if self.management_state.auto_admin_job.is_some() {
                        // The same question is already displayed in AutoAdmin's terminal.
                        if !self.auto_admin_visible() {
                            self.notify_toast(i18n::msg!("aa-input-required"));
                        }
                        continue;
                    }
                    self.reset_management_form_view();
                    self.management_state.terminal_mode = false;
                    if !visible {
                        if let Some(kind) = self.management_state.kind {
                            self.notify_alert_with_key(
                                format!("management.{}", kind.id()),
                                i18n::msg!(
                                    "management-background-question",
                                    application = management_title(kind)
                                ),
                                ui::NotificationTone::Warning,
                            );
                        }
                    }
                    self.management_state.form = Some(ManagementEditor {
                        message_scroll: 0,
                        title: i18n::tr!("management-input-required"),
                        message: prompt,
                        fields: vec![ManagementField {
                            id: "answer".into(),
                            label: i18n::tr!("management-answer"),
                            value: choices
                                .iter()
                                .find(|value| matches!(value.as_str(), "Cancel" | "Restore" | "No"))
                                .or_else(|| choices.first())
                                .cloned()
                                .unwrap_or_default(),
                            secret,
                            required: false,
                            choices,
                        }],
                        selected: 0,
                        purpose: FormPurpose::Answer(id),
                    });
                }
                OperationEvent::Completed { message } | OperationEvent::Failed { message } => {
                    self.management_state.status = self
                        .management_state
                        .problem
                        .as_ref()
                        .map_or_else(|| message.clone(), |p| i18n::tr!(p.summary_key.clone()));
                    self.management_state.outcome = Some(self.management_state.status.clone());
                    self.management_state.form = None;
                    self.reset_management_form_view();
                    completed = true;
                }
                OperationEvent::Disconnected { message } => {
                    let status = i18n::tr!("management-connection-lost");
                    self.management_state.output.push_str(&message);
                    self.management_state.status = status.clone();
                    self.management_state.outcome = Some(status);
                    self.management_state.form = None;
                    self.reset_management_form_view();
                    completed = true;
                }
            }
        }
        if completed {
            self.management_state.operation_job = None;
            self.management_state.terminal_mode = false;
            if let Some(kind) = self.management_state.kind {
                self.resolve_notification_alert(&format!("management.{}", kind.id()));
                if !visible {
                    self.notify_toast(format!(
                        "{}: {}",
                        management_title(kind),
                        self.management_state.status
                    ));
                }
            }
            let config_operation = self.management_state.configuration_operation;
            self.management_state.configuration_operation = false;
            if config_operation {
                self.finish_config_operation();
                if self.editor_config.pending_action.is_none() {
                    self.management_state.outcome = None;
                    self.management_state.status =
                        if self.management_state.snapshot.notices.is_empty() {
                            format!(
                                "{} · {}",
                                self.management_state.snapshot.backend,
                                self.management_state.snapshot.rows.len()
                            )
                        } else {
                            self.management_state.snapshot.notices.join("; ")
                        };
                }
            }
            if !config_operation && !self.management_state.received_snapshot {
                self.refresh_management();
            }
        }
        if visible
            && self.management_state.kind == Some(ManagementKind::Processes)
            && self.management_state.query_job.is_none()
            && self.management_state.operation_job.is_none()
            && self.management_state.form.is_none()
            && self
                .management_state
                .refreshed
                .is_some_and(|t| t.elapsed() > Duration::from_secs(5))
        {
            self.refresh_management();
        }
        self.clamp_management_scroll();
        if visible
            && matches!(
                self.management_state.kind,
                Some(ManagementKind::Services | ManagementKind::Processes | ManagementKind::Users)
            )
            && self.management_state.query_job.is_none()
            && self.management_state.operation_job.is_none()
            && self.management_state.form.is_none()
        {
            let selected = self
                .management_state
                .snapshot
                .rows
                .get(self.management_state.selected)
                .map(|r| r.id.clone());
            if let Some(query) = &mut self.management_state.query {
                if selected.is_some() && query.target != selected {
                    query.target = selected;
                    self.refresh_management();
                }
            }
        }
    }

    fn management_main(&self) -> Rect {
        let bounds = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        match self.shell_layout_for(bounds) {
            ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
        }
    }
    pub(in crate::session) fn resize_management_terminal(&mut self) {
        if self.management_state.auto_admin_job.is_some() {
            self.resize_auto_admin();
            return;
        }
        let area = ui::management_layout(self.management_main(), &self.to_management_view_model())
            .output_text;
        let rows = area.height.max(1);
        let columns = area.width.max(1);
        if let Some(parser) = &self.management_state.parser {
            if let Ok(mut parser) = parser.0.lock() {
                parser.set_size(rows, columns);
            }
        }
        self.management_send(OperationInput::Resize { columns, rows });
    }
    pub(in crate::session) fn open_management_directory(&mut self, platform: &dyn Platform) {
        if let Some(path) = self.management_state.pending_directory.take() {
            if let Some(storage) = self.storage_manager.clone() {
                self.open_explorer_at(platform, &storage, path, ExplorerPurpose::Browse);
            }
        }
        if let Some(path) = self.management_state.pending_select.clone() {
            if let Some(state) = self.app.explorer_state() {
                if let Some(index) = state.entries.iter().position(|e| e.path == path) {
                    let mut state = state.clone();
                    state.select_index(index, app::explorer::ExplorerSelectionMode::Replace);
                    self.app.dispatch_at(
                        app::AppCommand::SetExplorerState(Some(state)),
                        Instant::now(),
                    );
                    self.management_state.pending_select = None;
                }
            }
        }
    }
    pub(in crate::session) fn to_management_view_model(&self) -> ui::ManagementViewModel {
        let s = &self.management_state;
        let actions = self.management_actions();
        ui::ManagementViewModel {
            scope_id: format!("{:?}", s.kind),
            detail_action_start: Some(0),
            sort: s.sort,
            column_width_limits: if s.kind == Some(ManagementKind::Packages) {
                match s.query.as_ref().map(|query| query.scope.as_str()) {
                    Some("sources") => vec![28, 10, 42],
                    Some("conflicts") => vec![36, 36],
                    Some("status") => Vec::new(),
                    _ => vec![22, 16, 16, 42],
                }
            } else {
                Vec::new()
            },
            action_ids: actions
                .iter()
                .map(|(action, row)| {
                    format!(
                        "management.action.{:?}.{}.{:?}",
                        s.kind,
                        action.id,
                        row.as_ref().map(|row| (&row.id, &row.identity))
                    )
                })
                .collect(),
            title: s.kind.map(|kind| {
                let title = management_title(kind);
                if kind == ManagementKind::Packages {
                    let scope = s.query.as_ref().map(|query| query.scope.as_str()).filter(|scope| !scope.is_empty()).unwrap_or("installed");
                    format!("{title} · {}", management_label(&format!("scope-{scope}"), scope))
                } else {
                    title
                }
            }).unwrap_or_default(),
            columns: s
                .snapshot
                .columns
                .iter()
                .map(|c| management_text("column", c, c))
                .collect(),
            rows: s.snapshot.rows.iter().map(|r| r.cells.iter().enumerate().map(|(i,v)| {
                if s.kind == Some(ManagementKind::Users) && s.snapshot.columns.get(i).is_some_and(|c| c == "Source") {management_detail_value(s.kind,"Source",v)} else if s.snapshot.columns.get(i).is_some_and(|c|matches!(c.as_str(),"Check"|"Result"|"Next step"|"Source"|"State"|"Status"|"Disk health"|"Result type")) {management_text("value",v,v)}else{v.clone()}
            }).collect()).collect(),
            selected: s.selected,
            scroll: s.scroll,
            table_scroll: s.table_scroll,
            action_scroll: s.action_scroll,
            details: s
                .snapshot
                .rows
                .get(s.selected)
                .map(|r| {
                    r.detail
                        .iter()
                        .map(|(k, v)| {
                            format!(
                                "{}: {}",
                                management_text("detail", k, k),
                                management_detail_value(s.kind, k, v)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_else(|| s.snapshot.notices.join("\n")),
            actions: actions
                .iter()
                .enumerate()
                .map(|(index, (a, _))| {
                    let label = action_label(a);
                    let shortcut = if a.id == "current_task" {
                        "Ctrl+T".to_string()
                    } else if let Some((shortcut, _)) = management_action_shortcut(s.kind, &a.id) {
                        shortcut.to_string()
                    } else {
                        ((index + 1) % 10).to_string()
                    };
                    (
                        format!("[{shortcut}] {label}"),
                        a.disabled_reason.is_none() && (s.operation_job.is_none() || management_view_action(a) || matches!(a.id.as_str(),"cancel_operation"|"more_actions"|"current_task")),
                    )
                })
                .collect(),
            action_help: actions
                .iter()
                .map(|(action, _)| {
                    action.disabled_reason.clone().unwrap_or_else(|| {
                        if s.operation_job.is_some() && action.id != "cancel_operation" && !management_view_action(action) {
                            i18n::tr!("management-operation-running")
                        } else if action.privileged {
                            i18n::tr!("management-touch-authorization")
                        } else if action.confirm || !action.fields.is_empty() {
                            i18n::tr!("management-touch-review")
                        } else {
                            i18n::tr!("management-touch-run")
                        }
                    })
                })
                .collect(),
            selected_action: s.selected_action,
            actions_focused: s.actions_focused,
            filter: s.filter_input.clone(),
            filtering: s.filtering,
            status: s.status.clone(),
            loading: s.query_job.is_some() || s.draft_job.is_some(),
            running: s.operation_job.is_some(),
            output: s.output.clone(),
            output_scroll: s.output_scroll,
            details_scroll: s.details_scroll,
            details_only: s.details_only,
            terminal: s.terminal_mode,
            terminal_snapshot: if s.terminal_mode {
                s.parser.as_ref().and_then(|parser| {
                    parser.0.lock().ok().map(|mut parser| {
                        Arc::new(crate::session::command_line_runtime::to_ui_snapshot(
                            &crate::session::command_line_runtime::TerminalSnapshot::from_parser(
                                &mut parser,
                            ),
                        ))
                    })
                })
            } else {
                None
            },
            form: s
                .form
                .as_ref()
                .filter(|_| !s.terminal_mode)
                .map(|f| ui::ManagementForm {
                    submit_label: Some(match &f.purpose {
                        FormPurpose::Action(action,_)=>action_label(action),
                        FormPurpose::Configuration(action)=>i18n::tr!(format!("config-editor-{}", match action.as_str() {
                            "preview"=>"check", "check"=>"apply", "history"=>"restore", "conflict"=>"rebase", "properties"=>"preview", "result"=>"continue", other=>other,
                        })),
                        _=>i18n::tr!("management-form-submit"),
                    }),
                    submit_disabled: matches!(&f.purpose,FormPurpose::Configuration(action) if action=="check" && self.config_save_disabled(&f.fields)),
                    identity: match &f.purpose {
                        FormPurpose::Action(action, row) => format!(
                            "{}.{:?}",
                            action.id,
                            row.as_ref().map(|row| (&row.id, &row.identity))
                        ),
                        FormPurpose::Answer(id) => id.clone(),
                        FormPurpose::Menu(_) => "more-actions".into(),
                        FormPurpose::Configuration(action) => format!("config-{action}"),
                    },
                    field_scroll: s.form_field_scroll,
                    cancel_disabled: matches!(&f.purpose, FormPurpose::Answer(id) if id.starts_with("package-config-")),
                    choice: s.choice_field.and_then(|index| {
                        f.fields.get(index).map(|field| ui::ManagementChoices {
                            disabled: match &f.purpose { FormPurpose::Menu(items)=>items.iter().map(|(a,_)|a.disabled_reason.is_some() || (s.operation_job.is_some() && a.id!="cancel_operation" && !management_view_action(a))).collect(),_=>vec![] },
                            field: index,
                            values: field.choices.clone(),
                            selected: s.choice_selected,
                            scroll: s.choice_scroll,
                            columns: s.choice_columns,
                        })
                    }),
                    message_scroll: f.message_scroll,
                    title: f.title.clone(),
                    message: f.message.clone(),
                    selected: f.selected,
                    fields: f
                        .fields
                        .iter()
                        .map(|v| ui::ManagementFormField {
                            id: v.id.clone(),
                            label: management_text("field", &v.id, &v.label),
                            value: if v.secret {
                                "•".repeat(v.value.chars().count())
                            } else {
                                v.value.clone()
                            },
                            secret: false,
                            choices: v.choices.clone(),
                        })
                        .collect(),
                }),
        }
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/management/tests.rs"]
mod tests;

mod touch;

mod config_editor;
pub(in crate::session) use config_editor::ConfigEditorState;
