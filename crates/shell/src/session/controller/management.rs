use super::super::*;
use platform::management::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub(in crate::session) struct ManagementJob(Arc<ManagementJobShared>);
struct ManagementJobShared {
    cancelled: Arc<AtomicBool>,
    snapshot: Mutex<Option<Result<ManagementSnapshot, ManagementError>>>,
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
    status: String,
    outcome: Option<String>,
    received_snapshot: bool,
    pending_directory: Option<PathBuf>,
    output: String,
    output_scroll: u16,
    details_scroll: u16,
    details_only: bool,
    query_job: Option<ManagementJob>,
    operation_job: Option<ManagementJob>,
    form: Option<ManagementEditor>,
    parser: Option<ManagementTerminal>,
    terminal_mode: bool,
    revision: u64,
    refreshed: Option<Instant>,
}

pub(in crate::session) fn management_title(kind: ManagementKind) -> String {
    match kind {
        ManagementKind::Services => i18n::tr!("management-services"),
        ManagementKind::Processes => i18n::tr!("management-processes"),
        ManagementKind::Packages => i18n::tr!("management-packages"),
        ManagementKind::Network => i18n::tr!("management-network"),
        ManagementKind::Disks => i18n::tr!("management-disks"),
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
fn management_text(prefix: &str, id: &str, fallback: &str) -> String {
    let id = id
        .to_ascii_lowercase()
        .replace([' ', '_', '(', ')', '%'], "-");
    let key = format!("management-{prefix}-{}", id.trim_matches('-'));
    let text = i18n::tr!(key.clone());
    if text.contains(&key) {
        fallback.to_string()
    } else {
        text
    }
}

impl ShellSession {
    pub(in crate::session) fn open_management(&mut self, kind: ManagementKind) {
        if !cfg!(target_os = "linux") || self.app.auth_session().is_none() || self.is_strict_guest()
        {
            return;
        }
        if self.active_screen() != ShellScreen::Management {
            self.screen_stack.push(ShellScreen::Management);
        }
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
            let result = platform::management::query(&query, &cancelled);
            if let Some(output) = output.upgrade() {
                *output.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            }
        }) {
            Ok(worker) => {
                *job.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.management_state.query_job = Some(job);
                self.management_state.status = i18n::tr!("management-loading");
            }
            Err(e) => self.management_state.status = e.to_string(),
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
                    #[cfg(target_os = "linux")]
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
                }
                Err(e) => self.management_state.status = e.to_string(),
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
                OperationEvent::Started {
                    kind,
                    action,
                    target,
                } => {
                    if self.management_state.kind != Some(kind) {
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
                    self.management_state.query_job = None;
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
                    self.management_state.status = message.clone();
                    self.management_state.outcome = Some(message);
                    self.management_state.form = None;
                    self.reset_management_form_view();
                    completed = true;
                }
                OperationEvent::Disconnected { message } => {
                    let status = i18n::tr!("management-connection-lost", reason = message);
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
            if !self.management_state.received_snapshot {
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
    }

    fn management_actions(&self) -> Vec<(ManagementAction, Option<ManagementRow>)> {
        let mut actions = Vec::new();
        if let Some(row) = self
            .management_state
            .snapshot
            .rows
            .get(self.management_state.selected)
        {
            for action in &row.actions {
                actions.push((action.clone(), Some(row.clone())));
            }
        }
        actions.extend(
            self.management_state
                .snapshot
                .actions
                .iter()
                .cloned()
                .map(|a| (a, None)),
        );
        if self.management_state.operation_job.is_some() {
            actions.push((
                ManagementAction {
                    id: "cancel_operation".into(),
                    label: "Request cancellation".into(),
                    confirm: true,
                    ..Default::default()
                },
                None,
            ));
        }
        actions
    }
    fn activate_management_action(&mut self, index: usize) {
        self.reset_management_form_view();
        let Some((action, row)) = self.management_actions().get(index).cloned() else {
            return;
        };
        if let Some(reason) = &action.disabled_reason {
            self.management_state.status = reason.clone();
            return;
        }
        if self.management_state.operation_job.is_some() && action.id != "cancel_operation" {
            self.management_state.status = i18n::tr!("management-operation-running");
            return;
        }
        let message = row
            .as_ref()
            .map(|r| r.cells.join(" · "))
            .unwrap_or_default();
        if action.confirm || !action.fields.is_empty() {
            self.management_state.form = Some(ManagementEditor {
                message_scroll: 0,
                title: management_label(&action.id, &action.label),
                message,
                fields: action.fields.clone(),
                selected: 0,
                purpose: FormPurpose::Action(action, row),
            });
        } else {
            self.perform_management_action(action, row);
        }
    }
    fn perform_management_action(&mut self, action: ManagementAction, row: Option<ManagementRow>) {
        let values = action
            .fields
            .iter()
            .map(|f| (f.id.clone(), f.value.clone()))
            .collect::<BTreeMap<_, _>>();
        if action.id == "open_directory" {
            self.management_state.pending_directory =
                values.get("directory").map(PathBuf::from).or_else(|| {
                    row.as_ref()
                        .and_then(|r| r.identity.get("directory").map(PathBuf::from))
                });
            return;
        }
        if action.id == "set_view" || action.id.starts_with("scope_") {
            if let Some(query) = &mut self.management_state.query {
                if let Some(scope) = action.id.strip_prefix("scope_") {
                    query.scope = scope.into();
                }
                for (key, value) in values {
                    if key == "scope" {
                        query.scope = value;
                    } else {
                        query.options.insert(key, value);
                    }
                }
            }
            self.refresh_management();
            return;
        }
        if action.id == "view_logs" {
            if let Some(row) = row {
                self.open_service_logs(
                    &row.id,
                    row.identity
                        .get("scope")
                        .map(String::as_str)
                        .unwrap_or("system"),
                );
            }
            return;
        }
        if action.id == "cancel_operation" {
            self.management_send(OperationInput::Cancel);
            return;
        }
        if action.id == "recover" {
            if let Some(socket) = values.get("socket") {
                self.start_management_operation(None, false, Some(PathBuf::from(socket)));
            }
            return;
        }
        let Some(kind) = self.management_state.kind else {
            return;
        };
        let command = ManagementCommand {
            kind,
            action: action.id,
            target: row.as_ref().map(|r| r.id.clone()),
            identity: row.as_ref().map(|r| r.identity.clone()).unwrap_or_default(),
            values,
        };
        self.start_management_operation(Some(command), action.privileged, None);
    }
    fn management_send(&self, input: OperationInput) {
        if let Some(job) = &self.management_state.operation_job {
            let _ = job.0.responses.send(input);
        }
    }

    fn start_management_operation(
        &mut self,
        command: Option<ManagementCommand>,
        privileged: bool,
        socket: Option<PathBuf>,
    ) {
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            return;
        };
        let (job, rx) = self.management_job();
        let output = Arc::downgrade(&job.0);
        let cancelled = job.0.cancelled.clone();
        let mut input = Some((command, socket, rx));
        let task = TaskId::new(format!(
            "management-operation-{}",
            self.management_state
                .kind
                .map(ManagementKind::id)
                .unwrap_or("unknown")
        ))
        .expect("fixed management task name");
        match group.spawn_thread(TaskSpec::one_shot(task), move || {
            let Some((command, socket, rx)) = input.take() else {
                return;
            };
            let emit = |event| {
                let mut event = Some(event);
                while !cancelled.load(Ordering::Relaxed) {
                    let Some(shared) = output.upgrade() else {
                        return;
                    };
                    let mut events = shared.events.lock().unwrap_or_else(|e| e.into_inner());
                    if events.len() < 512 {
                        events.push_back(event.take().expect("event queued once"));
                        return;
                    }
                    drop(events);
                    drop(shared);
                    std::thread::sleep(Duration::from_millis(10));
                }
            };
            let result =
                super::management_client::run(command, privileged, socket, rx, &cancelled, &emit);
            if let Err(error) = result {
                emit(OperationEvent::Disconnected {
                    message: error.to_string(),
                });
            }
        }) {
            Ok(worker) => {
                *job.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.management_state.operation_job = Some(job);
                self.management_state.outcome = None;
                self.management_state.received_snapshot = false;
                self.management_state.output.clear();
                self.management_state.parser = None;
                self.management_state.status = i18n::tr!("management-working");
                self.resize_management_terminal();
            }
            Err(error) => self.management_state.status = error.to_string(),
        }
    }

    fn submit_management_form(&mut self) {
        let Some(mut form) = self.management_state.form.take() else {
            return;
        };
        if form
            .fields
            .iter()
            .any(|f| f.required && f.value.trim().is_empty())
        {
            self.management_state.status = i18n::tr!("management-required");
            self.management_state.form = Some(form);
            return;
        }
        match form.purpose {
            FormPurpose::Action(mut action, row) => {
                action.fields = form.fields;
                self.perform_management_action(action, row);
            }
            FormPurpose::Answer(id) => {
                let value = form
                    .fields
                    .first_mut()
                    .map(|f| std::mem::take(&mut f.value))
                    .unwrap_or_default();
                self.management_send(OperationInput::Answer { id, value });
            }
        }
    }
    fn cancel_management_form(&mut self) {
        if self.management_state.form.as_ref().is_some_and(|form|matches!(&form.purpose,FormPurpose::Answer(id) if id.starts_with("package-config-"))){self.management_state.status=i18n::tr!("management-config-needs-answer");return;}
        if self
            .management_state
            .form
            .as_ref()
            .is_some_and(|form| matches!(form.purpose, FormPurpose::Answer(_)))
        {
            self.management_send(OperationInput::Cancel);
        }
        if let Some(mut form) = self.management_state.form.take() {
            use zeroize::Zeroize;
            for field in &mut form.fields {
                if field.secret {
                    field.value.zeroize();
                }
            }
        }
    }
    pub(in crate::session) fn handle_management_paste(&mut self, value: &str) {
        if let Some(form) = &mut self.management_state.form {
            if let Some(field) = form.fields.get_mut(form.selected) {
                if field.choices.is_empty() && field.value.len() + value.len() <= 16 * 1024 {
                    field
                        .value
                        .extend(value.chars().filter(|c| !c.is_control()));
                }
            }
        } else if self.management_state.filtering {
            if self.management_state.filter_input.len() + value.len() <= 16 * 1024 {
                self.management_state
                    .filter_input
                    .extend(value.chars().filter(|c| !c.is_control()));
            }
        } else if self.management_state.terminal_mode {
            let bracketed = self
                .management_state
                .parser
                .as_ref()
                .and_then(|p| p.0.lock().ok().map(|p| p.screen().bracketed_paste()))
                .unwrap_or(false);
            self.management_send(OperationInput::Terminal {
                bytes: if bracketed {
                    format!("\x1b[200~{value}\x1b[201~").into_bytes()
                } else {
                    value.as_bytes().to_vec()
                },
            });
        }
    }
    pub(in crate::session) fn handle_management_key(&mut self, key: &KeyInput) {
        if self.handle_management_choice_key(key) {
            return;
        }
        if matches!(
            key.key,
            InputKey::Tab
                | InputKey::BackTab
                | InputKey::Up
                | InputKey::Down
                | InputKey::Home
                | InputKey::End
                | InputKey::PageUp
                | InputKey::PageDown
        ) {
            self.management_state.list_scroll_explicit = false;
            self.management_state.action_scroll = None;
            if !matches!(key.key, InputKey::PageUp | InputKey::PageDown) {
                self.management_state.form_field_scroll = None;
            }
        }
        if key.key == InputKey::Escape {
            self.cancel_management_pointer_gesture();
        }
        if key.phase == InputPhase::Repeat
            && (matches!(key.key, InputKey::Enter | InputKey::Escape)
                || (!self.management_state.filtering
                    && self.management_state.form.is_none()
                    && matches!(key.key, InputKey::Char('1'..='9' | 'r' | 'R'))))
        {
            return;
        }
        if key.is_ctrl_c() && !self.management_state.terminal_mode {
            return;
        }
        if (key.modifiers.control || key.modifiers.ctrl)
            && matches!(key.key, InputKey::Char('t' | 'T'))
        {
            self.management_state.terminal_mode = !self.management_state.terminal_mode;
            self.resize_management_terminal();
            return;
        }
        if self.management_state.terminal_mode && self.management_state.form.is_some() {
            if matches!(key.key, InputKey::PageUp | InputKey::PageDown) {
                self.scroll_management_terminal(if key.key == InputKey::PageUp { 10 } else { -10 });
            }
            return;
        }
        if let Some(form) = &mut self.management_state.form {
            if (key.modifiers.control || key.modifiers.ctrl)
                && matches!(key.key, InputKey::Char('u' | 'U'))
            {
                if let Some(field) = form.fields.get_mut(form.selected) {
                    if field.choices.is_empty() {
                        use zeroize::Zeroize;
                        field.value.zeroize();
                    }
                }
                return;
            }
            match key.key {
                InputKey::Escape => self.cancel_management_form(),
                InputKey::PageUp => form.message_scroll = form.message_scroll.saturating_sub(5),
                InputKey::PageDown => {
                    form.message_scroll = form
                        .message_scroll
                        .saturating_add(5)
                        .min(form.message.chars().count().min(u16::MAX as usize) as u16)
                }
                InputKey::Tab | InputKey::Down => {
                    form.selected = (form.selected + 1) % (form.fields.len() + 1)
                }
                InputKey::BackTab | InputKey::Up => {
                    form.selected = if form.selected == 0 {
                        form.fields.len()
                    } else {
                        form.selected - 1
                    }
                }
                InputKey::Enter => {
                    if form.selected >= form.fields.len() {
                        self.submit_management_form();
                    } else {
                        form.selected += 1;
                    }
                }
                InputKey::Left | InputKey::Right => {
                    if let Some(field) = form.fields.get_mut(form.selected) {
                        if !field.choices.is_empty() {
                            let current = field
                                .choices
                                .iter()
                                .position(|v| v == &field.value)
                                .unwrap_or(0);
                            let next = if key.key == InputKey::Right {
                                (current + 1) % field.choices.len()
                            } else {
                                (current + field.choices.len() - 1) % field.choices.len()
                            };
                            field.value = field.choices[next].clone();
                        }
                    }
                }
                InputKey::Backspace => {
                    if let Some(field) = form.fields.get_mut(form.selected) {
                        if field.choices.is_empty() {
                            field.value.pop();
                        }
                    }
                }
                InputKey::Space => {
                    if let Some(field) = form.fields.get_mut(form.selected) {
                        if field.choices.is_empty() && field.value.len() < 16 * 1024 {
                            field.value.push(' ');
                        }
                    }
                }
                InputKey::Char(c) => {
                    if let Some(field) = form.fields.get_mut(form.selected) {
                        if field.choices.is_empty()
                            && field.value.len() < 16 * 1024
                            && !c.is_control()
                        {
                            field.value.push(c);
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if self.management_state.filtering {
            match key.key {
                InputKey::Escape => self.management_state.filtering = false,
                InputKey::Enter => {
                    let filter = self.management_state.filter_input.clone();
                    if let Some(query) = &mut self.management_state.query {
                        query.filter = filter;
                        query.target = None;
                    }
                    self.management_state.filtering = false;
                    self.refresh_management();
                }
                InputKey::Backspace => {
                    self.management_state.filter_input.pop();
                }
                InputKey::Space => self.management_state.filter_input.push(' '),
                InputKey::Char(c) => {
                    if !c.is_control() {
                        self.management_state.filter_input.push(c);
                    }
                }
                _ => {}
            }
            return;
        }
        if key.modifiers.alt && matches!(key.key, InputKey::PageUp | InputKey::PageDown) {
            self.management_state.details_scroll = if key.key == InputKey::PageUp {
                self.management_state.details_scroll.saturating_sub(5)
            } else {
                self.management_state.details_scroll.saturating_add(5)
            };
            return;
        }
        if self.management_state.terminal_mode {
            if key.modifiers.shift && matches!(key.key, InputKey::PageUp | InputKey::PageDown) {
                self.scroll_management_terminal(if key.key == InputKey::PageUp { 10 } else { -10 });
                return;
            }
            let input = match key.key {
                InputKey::Char(c) if key.modifiers.control && c.is_ascii() => {
                    TerminalInput::Bytes(vec![(c.to_ascii_lowercase() as u8) & 0x1f])
                }
                InputKey::Char(c) => TerminalInput::Text(c.to_string()),
                InputKey::Enter => TerminalInput::Enter,
                InputKey::Backspace => TerminalInput::Backspace,
                InputKey::Space => TerminalInput::Text(" ".into()),
                InputKey::Tab => TerminalInput::Tab,
                InputKey::Escape => TerminalInput::Escape,
                InputKey::Up => TerminalInput::Up,
                InputKey::Down => TerminalInput::Down,
                InputKey::Left => TerminalInput::Left,
                InputKey::Right => TerminalInput::Right,
                InputKey::Home => TerminalInput::Home,
                InputKey::End => TerminalInput::End,
                InputKey::Delete => TerminalInput::Delete,
                InputKey::PageUp => TerminalInput::PageUp,
                InputKey::PageDown => TerminalInput::PageDown,
                _ => return,
            };
            self.management_send(OperationInput::Terminal {
                bytes: encode_terminal_input(
                    &input,
                    self.management_state
                        .parser
                        .as_ref()
                        .and_then(|p| p.0.lock().ok().map(|p| p.screen().application_cursor()))
                        .unwrap_or(false),
                ),
            });
            return;
        }
        match key.key {
            InputKey::Escape => {
                self.screen_stack.pop();
                self.focused_component = if self.active_screen() == ShellScreen::Launcher {
                    ShellComponent::Launcher
                } else {
                    ShellComponent::Home
                };
            }
            InputKey::Char('/') => self.management_state.filtering = true,
            InputKey::Char('r' | 'R') => {
                self.management_state.outcome = None;
                self.refresh_management();
            }
            InputKey::Tab | InputKey::BackTab => {
                self.management_state.actions_focused = !self.management_state.actions_focused
            }
            InputKey::Enter => {
                if self.management_state.actions_focused {
                    self.activate_management_action(self.management_state.selected_action);
                } else {
                    self.management_state.actions_focused = true;
                    if let Some(id) = self
                        .management_state
                        .snapshot
                        .rows
                        .get(self.management_state.selected)
                        .map(|r| r.id.clone())
                    {
                        if let Some(query) = &mut self.management_state.query {
                            query.target = Some(id);
                        }
                        self.refresh_management();
                    }
                }
            }
            InputKey::Char(c) if ('1'..='9').contains(&c) => {
                self.activate_management_action(c as usize - '1' as usize)
            }
            InputKey::Up
            | InputKey::Down
            | InputKey::PageUp
            | InputKey::PageDown
            | InputKey::Home
            | InputKey::End => {
                let actions = self.management_actions();
                let rows_count = self.management_state.snapshot.rows.len();
                let (selected, count) = if self.management_state.actions_focused {
                    (&mut self.management_state.selected_action, actions.len())
                } else {
                    (&mut self.management_state.selected, rows_count)
                };
                *selected = match key.key {
                    InputKey::Up => selected.saturating_sub(1),
                    InputKey::Down => (*selected + 1).min(count.saturating_sub(1)),
                    InputKey::PageUp => selected.saturating_sub(10),
                    InputKey::PageDown => (*selected + 10).min(count.saturating_sub(1)),
                    InputKey::Home => 0,
                    InputKey::End => count.saturating_sub(1),
                    _ => *selected,
                };
            }
            _ => {}
        }
        self.clamp_management_scroll();
    }
    fn management_main(&self) -> Rect {
        let bounds = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        match self.shell_layout_for(bounds) {
            ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
        }
    }
    pub(in crate::session) fn resize_management_terminal(&mut self) {
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
    fn scroll_management_terminal(&mut self, delta: isize) {
        if let Some(parser) = &self.management_state.parser {
            if let Ok(mut parser) = parser.0.lock() {
                let position = parser.screen().scrollback().saturating_add_signed(delta);
                parser.set_scrollback(position);
            }
        } else {
            self.management_state.output_scroll = self
                .management_state
                .output_scroll
                .saturating_add_signed((-delta).clamp(i16::MIN as isize, i16::MAX as isize) as i16);
        }
    }
    pub(in crate::session) fn open_management_directory(&mut self, platform: &dyn Platform) {
        if let Some(path) = self.management_state.pending_directory.take() {
            if let Some(storage) = self.storage_manager.clone() {
                self.open_explorer_at(platform, &storage, path, ExplorerPurpose::Browse);
            }
        }
    }
    pub(in crate::session) fn to_management_view_model(&self) -> ui::ManagementViewModel {
        let s = &self.management_state;
        let actions = self.management_actions();
        ui::ManagementViewModel {
            scope_id: format!("{:?}", s.kind),
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
            title: s.kind.map(management_title).unwrap_or_default(),
            columns: s
                .snapshot
                .columns
                .iter()
                .map(|c| management_text("column", c, c))
                .collect(),
            rows: s.snapshot.rows.iter().map(|r| r.cells.clone()).collect(),
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
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_else(|| s.snapshot.notices.join("\n")),
            actions: actions
                .iter()
                .map(|(a, _)| {
                    (
                        management_label(&a.id, &a.label),
                        a.disabled_reason.is_none(),
                    )
                })
                .collect(),
            action_help: actions
                .iter()
                .map(|(action, _)| {
                    action.disabled_reason.clone().unwrap_or_else(|| {
                        if action.privileged {
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
            loading: s.query_job.is_some(),
            running: s.operation_job.is_some(),
            output: s.output.clone(),
            output_scroll: s.output_scroll,
            details_scroll: s.details_scroll,
            details_only: s.details_only,
            terminal: s.terminal_mode,
            terminal_snapshot: if s.terminal_mode {
                s.parser.as_ref().and_then(|parser| {
                    parser.0.lock().ok().map(|mut parser| {
                        Arc::new(super::super::command_line_runtime::to_ui_snapshot(
                            &super::super::command_line_runtime::TerminalSnapshot::from_parser(
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
                    identity: match &f.purpose {
                        FormPurpose::Action(action, row) => format!(
                            "{}.{:?}",
                            action.id,
                            row.as_ref().map(|row| (&row.id, &row.identity))
                        ),
                        FormPurpose::Answer(id) => id.clone(),
                    },
                    field_scroll: s.form_field_scroll,
                    cancel_disabled: matches!(&f.purpose, FormPurpose::Answer(id) if id.starts_with("package-config-")),
                    choice: s.choice_field.and_then(|index| {
                        f.fields.get(index).map(|field| ui::ManagementChoices {
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
#[path = "../../../tests/unit/session/controller/management/tests.rs"]
mod tests;

#[path = "management_touch.rs"]
mod touch;
