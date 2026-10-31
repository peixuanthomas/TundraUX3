mod actions;
mod forms;
mod input;
mod packages;
mod scheduled;
mod selection;
mod task_center;
use scheduled::ScheduledWizard;
mod backups;
use crate::session::*;
use platform::management::*;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub(in crate::session) struct ManagementJob(Arc<ManagementJobShared>);
struct ManagementJobShared {
    id: u64,
    language: (String, u64),
    query: Option<ManagementQuery>,
    cancelled: Arc<AtomicBool>,
    discard_output: AtomicBool,
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
impl ManagementJob {
    fn stopped(&self) -> bool {
        self.0
            .worker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(ManagedThreadHandle::is_finished)
    }
    fn finished(&self) -> bool {
        self.0
            .worker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_none_or(ManagedThreadHandle::is_finished)
    }
    fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
        let _ = self.0.responses.send(OperationInput::Cancel);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::session) struct DetachedManagementOperation {
    job: ManagementJob,
    auto_admin_job: Option<AutoAdminJob>,
    kind: Option<ManagementKind>,
}
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
    Scheduled(ScheduledWizard),
    Action(ManagementAction, Option<ManagementRow>),
    Answer(String),
    Menu(Vec<(ManagementAction, Option<ManagementRow>)>),
    Configuration(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagementEditor {
    instance: u64,
    title: String,
    message: String,
    message_scroll: u16,
    fields: Vec<ManagementField>,
    input_states: BTreeMap<usize, ui::components::TextEditState>,
    selected: usize,
    purpose: FormPurpose,
}

impl ManagementEditor {
    fn edit_field(&mut self, index: usize, event: InputEvent, area: Rect) {
        let Some(field) = self.fields.get_mut(index) else {
            return;
        };
        if field.control != FieldControl::Text || !field.choices.is_empty() {
            return;
        }
        let added = match &event {
            InputEvent::Paste(value) => value.len(),
            InputEvent::Key(key) => match key.key {
                InputKey::Char(c) => c.len_utf8(),
                InputKey::Space => 1,
                _ => 0,
            },
            _ => 0,
        };
        if added > 0 && field.value.len() + added > 16 * 1024 {
            return;
        }
        let editing = self.input_states.entry(index).or_default();
        if field.secret && matches!(event, InputEvent::Mouse(_)) {
            let mut masked = "•".repeat(field.value.chars().count());
            editing.handle_event(&mut masked, event, area);
        } else {
            editing.handle_event(&mut field.value, event, area);
        }
    }
}

fn next_management_form_instance() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
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
    snapshot_language: Option<(String, u64)>,
    snapshot_query: Option<ManagementQuery>,
    last_language_attempt: Option<(String, u64)>,
    selected: usize,
    selection: Option<std::collections::BTreeSet<String>>,
    selection_mode: bool,
    scroll: usize,
    list_scroll_explicit: bool,
    table_scroll: usize,
    sort: Option<ui::TableSort>,
    sort_columns: Vec<String>,
    action_scroll: Option<usize>,
    form_field_scroll: Option<usize>,
    choice_field: Option<usize>,
    choice_search: String,
    choice_scroll: usize,
    choice_selected: usize,
    choice_columns: usize,
    scrollbar_grab: Option<(ui::ManagementScrollTarget, u16)>,
    selected_action: usize,
    permission_cell: usize,
    actions_focused: bool,
    workspace_focus: ui::components::WorkspaceFocus,
    package_advanced_open: bool,
    filtering: bool,
    filter_input: String,
    shortcut_repeat_guard: Option<InputKey>,
    status: String,
    outcome: Option<String>,
    received_snapshot: bool,
    refresh_requested: bool,
    refreshing_authorized: bool,
    pending_directory: Option<PathBuf>,
    pending_select: Option<PathBuf>,
    output: String,
    output_scroll: u16,
    details_scroll: u16,
    details_horizontal_scroll: u16,
    details_only: bool,
    query_job: Option<ManagementJob>,
    scheduled_preview: Option<ManagementJob>,
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
    fn can_show_details(&self) -> bool {
        !self.snapshot.rows.is_empty()
            || self.problem.is_some()
            || !self.snapshot.details.is_empty()
            || !self.snapshot.notices.is_empty()
    }

    fn refresh_command(&self) -> Option<ManagementCommand> {
        let query = self.query.as_ref()?;
        let action = self.snapshot.refresh_action.as_ref()?;
        let mut values = action.values.clone();
        values.retain(|key, _| {
            !matches!(key.as_str(), "scope" | "filter" | "query_target")
                && !key.starts_with("__query_option:")
        });
        values.extend(query.inspection_values());
        Some(ManagementCommand {
            kind: query.kind,
            action: action.id.clone(),
            target: None,
            values,
            identity: BTreeMap::new(),
        })
    }

    fn sort_snapshot(
        &mut self,
        snapshot: &mut ManagementSnapshot,
        language: &(String, u64),
        query: Option<&ManagementQuery>,
    ) {
        let same_view = self
            .snapshot_query
            .as_ref()
            .zip(query)
            .is_some_and(|(before, after)| {
                before.kind == after.kind
                    && before.scope == after.scope
                    && (before.options == after.options || before.kind == ManagementKind::Backups)
            });
        if self.sort_columns != snapshot.columns {
            if self
                .snapshot_language
                .as_ref()
                .is_none_or(|previous| previous == language)
                || !same_view
                || self.sort_columns.len() != snapshot.columns.len()
            {
                self.sort = None;
            }
            self.sort_columns = snapshot.columns.clone();
        }
        if let Some(sort) = self.sort
            && self.kind != Some(ManagementKind::Backups)
        {
            snapshot.rows.sort_by(|a, b| {
                sort.compare(
                    a.cells.get(sort.column).map_or("", String::as_str),
                    b.cells.get(sort.column).map_or("", String::as_str),
                )
            });
        }
        self.snapshot_language = Some(language.clone());
        self.snapshot_query = query.cloned();
        if let Some(selection) = &mut self.selection {
            selection.retain(|id| {
                snapshot
                    .rows
                    .iter()
                    .any(|row| selection::row_id(self.kind, row) == *id)
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
        ManagementKind::Firewall => i18n::tr!("management-firewall"),
        ManagementKind::Ssh => i18n::tr!("management-ssh"),
        ManagementKind::Backups => i18n::tr!("management-backups"),
        ManagementKind::ScheduledTasks => i18n::tr!("management-scheduled-tasks"),
        ManagementKind::Tasks => i18n::tr!("tasks-title"),
        ManagementKind::FilePermissions => i18n::tr!("permissions-title"),
        ManagementKind::SystemSettings => i18n::tr!("system-settings-title"),
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
        "query_details" => return Some(("F4", InputKey::F(4))),
        "package_advanced" => return Some(("F3", InputKey::F(3))),
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

fn editor_dialog_title_key(action: &str) -> Option<&'static str> {
    Some(match action {
        "edit-goto" => "ui-editor-go-to-line",
        "edit-file" => "ui-editor-go-to-file",
        "edit-language" => "ui-editor-language",
        "edit-newline" => "ui-editor-line-ending",
        "edit-encoding" => "ui-editor-encoding",
        "edit-indent" => "ui-editor-indentation",
        "edit-about" => "ui-editor-about",
        _ => return None,
    })
}

fn form_value_text(purpose: &FormPurpose, field: &ManagementField, value: &str) -> String {
    if matches!(purpose, FormPurpose::Scheduled(_))
        && let Some(label) = scheduled::choice_label(&field.id, value)
    {
        return label;
    }
    if field.choices.iter().all(|v| v == "true" || v == "false") && !field.choices.is_empty() {
        return match value {
            "true" => i18n::tr!("management-value-yes"),
            "false" => i18n::tr!("management-value-no"),
            _ => value.into(),
        };
    }
    if let FormPurpose::Action(action, _) = purpose {
        if action.id == "task_filter" && field.id == "status" {
            return value.into();
        }
        let key = match (action.id.as_str(), field.id.as_str(), value) {
            ("add_rule", "duration", "runtime") => Some("management-value-runtime"),
            ("add_rule", "duration", "permanent") => Some("management-value-permanent"),
            ("add_rule", "duration", "both") => Some("management-value-both"),
            ("ssh_configure", "password_authentication" | "pubkey_authentication", "yes") => {
                Some("management-value-yes")
            }
            ("ssh_configure", "password_authentication" | "pubkey_authentication", "no") => {
                Some("management-value-no")
            }
            ("ssh_configure", "permit_root_login", "yes") => {
                Some("management-value-root-login-yes")
            }
            ("ssh_configure", "permit_root_login", "no") => Some("management-value-root-login-no"),
            ("ssh_configure", "permit_root_login", "prohibit-password") => {
                Some("management-value-root-login-prohibit-password")
            }
            ("ssh_configure", "permit_root_login", "forced-commands-only") => {
                Some("management-value-root-login-forced-commands-only")
            }
            _ => None,
        };
        if let Some(key) = key {
            return i18n::tr!(key);
        }
    }
    if let FormPurpose::Configuration(action) = purpose {
        match (action.as_str(), value) {
            ("edit-language", "Auto") => return i18n::tr!("ui-editor-language-auto"),
            ("edit-language", "Text") => return i18n::tr!("ui-editor-language-text"),
            ("edit-indent", _) => {
                if let Some((kind, count)) = value.split_once(':') {
                    return i18n::tr!(
                        if kind == "Tab" {
                            "ui-editor-indent-tabs"
                        } else {
                            "ui-editor-indent-spaces"
                        },
                        count = count
                    );
                }
            }
            _ => {}
        }
    }
    value.into()
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
    matches!(
        action.id.as_str(),
        "set_view"
            | "show_listening_ports"
            | "show_firewall_rules"
            | "task_output"
            | "task_output_previous"
            | "task_output_next"
            | "task_return"
            | "task_filter"
            | "task_next_page"
            | "task_first_page"
            | "backups_next_page"
            | "backups_first_page"
            | "preview_cleanup"
            | "cancel_cleanup_preview"
            | "filter_backups"
    ) || action.id.starts_with("scope_")
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
    pub(in crate::session) fn management_help_screen(&self) -> ui::PageHelpScreen {
        match self.pages.management.management_state.kind {
            Some(ManagementKind::Packages) => ui::PageHelpScreen::Packages,
            Some(ManagementKind::Services) => ui::PageHelpScreen::Services,
            Some(ManagementKind::Processes) => ui::PageHelpScreen::Processes,
            Some(ManagementKind::ScheduledTasks) => ui::PageHelpScreen::ScheduledTasks,
            _ => ui::PageHelpScreen::Management,
        }
    }
    fn management_alert_key(&self) -> String {
        format!(
            "management.page.{}",
            self.pages
                .management
                .selected_id()
                .unwrap_or_else(|| self.active_page_id())
                .get()
        )
    }
    pub(in crate::session) fn open_management(&mut self, kind: ManagementKind) {
        let mut query = ManagementQuery::new(kind);
        if kind == ManagementKind::Packages {
            query.scope = "installed".into();
        }
        self.open_management_query(query);
    }

    pub(in crate::session) fn open_management_query(&mut self, mut query: ManagementQuery) {
        if query.kind == ManagementKind::Backups {
            query
                .options
                .entry("page_size".into())
                .or_insert_with(|| "50".into());
        }
        let kind = query.kind;
        let supported = kind == ManagementKind::Tasks
            || cfg!(target_os = "linux")
            || (cfg!(any(target_os = "macos", windows)) && kind == ManagementKind::Packages);
        if !supported || self.app.auth_session().is_none() || self.is_strict_guest() {
            return;
        }
        self.enter_new_page(ShellScreen::Management);
        if self.pages.management.management_state.kind != Some(kind) {
            let search = kind == ManagementKind::Packages && query.scope == "search";
            self.pages.management.management_state = ManagementState {
                kind: Some(kind),
                query: Some(query),
                filtering: search,
                workspace_focus: if search {
                    ui::components::WorkspaceFocus::Search
                } else {
                    ui::components::WorkspaceFocus::List
                },
                ..Default::default()
            };
        }
        self.pages.common.focused_component = ShellComponent::Management;
        self.resolve_notification_alert(&self.management_alert_key());
        if self
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
            && !self.pages.management.management_state.received_snapshot
        {
            self.refresh_management();
        }
    }

    fn management_job(&self) -> (ManagementJob, mpsc::Receiver<OperationInput>) {
        let (tx, rx) = mpsc::channel();
        (
            ManagementJob(Arc::new(ManagementJobShared {
                id: PageInstanceId::new().get(),
                language: (self.language.code().into(), self.language.generation()),
                query: self.pages.management.management_state.query.clone(),
                cancelled: Arc::new(AtomicBool::new(false)),
                discard_output: AtomicBool::new(false),
                snapshot: Mutex::new(None),
                draft: Mutex::new(None),
                events: Mutex::new(VecDeque::new()),
                responses: tx,
                worker: Mutex::new(None),
            })),
            rx,
        )
    }

    fn register_management_job(
        &mut self,
        job: &ManagementJob,
        domain: TaskDomain,
        class: TaskClass,
    ) {
        let cancel = job.clone();
        let finished = job.clone();
        let auto_admin = (domain == TaskDomain::ManagementOperation)
            .then(|| {
                self.pages
                    .management
                    .management_state
                    .auto_admin_job
                    .clone()
            })
            .flatten();
        self.register_page_task(
            domain,
            job.0.id,
            class,
            TaskControl::new(
                move || {
                    if let Some(aa) = &auto_admin {
                        if aa.phase() == ::auto_admin::JobPhase::Running {
                            cancel.0.discard_output.store(true, Ordering::Relaxed);
                            aa.request_stop(false);
                            // The client must keep reading until the helper reports
                            // completion. Its cancelled flag means detach, not stop.
                            if class == TaskClass::Operation {
                                return;
                            }
                        } else {
                            aa.cancel();
                        }
                    }
                    cancel.cancel();
                },
                move || finished.finished(),
            ),
        );
    }

    fn management_job_belongs_here(&self, domain: TaskDomain, job: &ManagementJob) -> bool {
        self.tasks.owner(domain, job.0.id) == self.pages.common.selected_id().map(TaskOwner::Page)
    }

    fn cancel_management_query(&mut self) {
        if let Some(job) = self.pages.management.management_state.query_job.take() {
            job.cancel();
            self.tasks.cancel(TaskDomain::ManagementQuery, job.0.id);
            self.tasks.complete(TaskDomain::ManagementQuery, job.0.id);
        }
    }
    pub(in crate::session) fn refresh_management(&mut self) {
        let Some(query) = self.pages.management.management_state.query.clone() else {
            return;
        };
        self.pages.management.management_state.last_language_attempt =
            Some((self.language.code().into(), self.language.generation()));
        // A requested refresh replaces the previous query, including when a new
        // worker cannot be started. Its late result must not restore an old filter.
        self.cancel_management_query();
        let authorized = self.pages.management.management_state.refresh_command();
        if authorized.is_some()
            && self
                .pages
                .management
                .management_state
                .operation_job
                .is_some()
        {
            self.pages.management.management_state.refresh_requested = true;
            return;
        }
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            self.pages.management.management_state.status =
                i18n::tr!("management-worker-unavailable");
            return;
        };
        if let Some(command) = authorized {
            self.pages.management.management_state.refresh_requested = false;
            self.start_management_operation(
                Some(command),
                true,
                false,
                None,
                Some(format!(
                    "{}\n{}",
                    management_title(query.kind),
                    i18n::tr!("management-control-refresh")
                )),
            );
            return;
        }
        let (job, _) = self.management_job();
        let output = Arc::downgrade(&job.0);
        let cancelled = job.0.cancelled.clone();
        self.pages.management.management_state.revision = self
            .pages
            .management
            .management_state
            .revision
            .wrapping_add(1);
        let task = TaskId::new(format!("management-query-{}-{}", query.kind.id(), job.0.id))
            .expect("bounded task identifier");
        let language = self.language.clone();
        match group.spawn_thread(TaskSpec::one_shot(task), move || {
            let _language = i18n::enter_snapshot(language.clone());
            let result = query_management_snapshot(&query, &cancelled);
            if let Some(output) = output.upgrade() {
                *output.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            }
        }) {
            Ok(worker) => {
                *job.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.register_management_job(&job, TaskDomain::ManagementQuery, TaskClass::Query);
                self.pages.management.management_state.query_job = Some(job);
                self.pages.management.management_state.status = i18n::tr!("management-loading");
            }
            Err(e) => {
                self.pages.management.management_state.status =
                    i18n::tr!("management-worker-unavailable");
                self.pages.management.management_state.output = e.to_string();
            }
        }
    }

    pub(in crate::session) fn poll_management(&mut self) {
        for id in self.pages.management.ids() {
            let visible = self.active_page_id() == id
                && matches!(
                    self.active_screen(),
                    ShellScreen::Management | ShellScreen::Editor
                );
            self.with_page_context(id, |session| session.poll_management_page(visible));
        }
        self.poll_detached_management_operations();
    }

    pub(in crate::session) fn drain_management_for_shutdown(&mut self) {
        for id in self.pages.management.ids() {
            if let Some(page) = self.pages.management.get(id)
                && let Some(job) = &page.management_state.operation_job
            {
                job.0
                    .events
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clear();
            }
        }
        for operation in &self.detached_management_operations {
            operation
                .job
                .0
                .events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clear();
        }
    }

    pub(in crate::session) fn detach_management_page(&mut self, id: PageInstanceId) {
        let Some(page) = self.pages.management.get_mut(id) else {
            return;
        };
        page.management_state.refresh_requested = false;
        page.management_state.refreshing_authorized = false;
        let detached =
            page.management_state
                .operation_job
                .take()
                .map(|job| DetachedManagementOperation {
                    job,
                    auto_admin_job: page.management_state.auto_admin_job.take(),
                    kind: page.management_state.kind,
                });
        if let Some(detached) = detached
            && self
                .tasks
                .owner(TaskDomain::ManagementOperation, detached.job.0.id)
                == Some(TaskOwner::Session)
        {
            self.detached_management_operations.push(detached);
        }
    }

    fn poll_detached_management_operations(&mut self) {
        let mut pending = Vec::new();
        for operation in std::mem::take(&mut self.detached_management_operations) {
            let finished = operation.job.finished();
            let events = operation
                .job
                .0
                .events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .drain(..)
                .collect::<Vec<_>>();
            let accepts_result = self
                .tasks
                .owner(TaskDomain::ManagementOperation, operation.job.0.id)
                == Some(TaskOwner::Session);
            let mut completed = false;
            for event in events {
                match event {
                    OperationEvent::Completed { message }
                    | OperationEvent::Failed { message }
                    | OperationEvent::Disconnected { message } => {
                        completed = true;
                        if accepts_result {
                            let title = operation
                                .kind
                                .map(management_title)
                                .unwrap_or_else(|| i18n::tr!("management-system-config"));
                            self.notify_toast(format!("{title}: {message}"));
                        }
                    }
                    OperationEvent::Question { .. } if accepts_result => {
                        if let Some(job) = &operation.auto_admin_job {
                            self.show_auto_admin_job(job.clone());
                        }
                    }
                    _ => {}
                }
            }
            if completed || finished {
                self.tasks
                    .complete(TaskDomain::ManagementOperation, operation.job.0.id);
            } else {
                pending.push(operation);
            }
        }
        self.detached_management_operations = pending;
    }

    fn poll_management_page(&mut self, visible: bool) {
        self.poll_scheduled_preview();
        if let Some(job) = self.pages.management.management_state.query_job.clone()
            && !self.management_job_belongs_here(TaskDomain::ManagementQuery, &job)
        {
            self.pages.management.management_state.query_job = None;
        }
        if let Some(job) = self.pages.management.management_state.draft_job.clone()
            && !self.management_job_belongs_here(TaskDomain::ManagementDraft, &job)
        {
            self.pages.management.management_state.draft_job = None;
        }
        if let Some(job) = self.pages.management.management_state.operation_job.clone()
            && !self.management_job_belongs_here(TaskDomain::ManagementOperation, &job)
        {
            self.pages.management.management_state.operation_job = None;
        }
        // Read completion before the result slot: a worker that finishes during
        // this poll must still get a later chance to publish its final result.
        if let Some(job) = &self.pages.management.management_state.query_job
            && job.stopped()
        {
            job.0
                .snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(|| {
                    Err(ManagementError::Unavailable(i18n::tr!(
                        "management-worker-unavailable"
                    )))
                });
        }
        if let Some(job) = &self.pages.management.management_state.draft_job
            && job.stopped()
        {
            job.0
                .draft
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(|| {
                    Err(ManagementError::Unavailable(i18n::tr!(
                        "management-worker-unavailable"
                    )))
                });
        }
        if let Some(job) = &self.pages.management.management_state.operation_job
            && job.stopped()
        {
            let mut events = job.0.events.lock().unwrap_or_else(|e| e.into_inner());
            if !events.iter().any(|event| {
                matches!(
                    event,
                    OperationEvent::Completed { .. }
                        | OperationEvent::Failed { .. }
                        | OperationEvent::Disconnected { .. }
                )
            }) {
                events.push_back(OperationEvent::Failed {
                    message: i18n::tr!("management-worker-unavailable"),
                });
            }
        }
        let draft_result = self
            .pages
            .management
            .management_state
            .draft_job
            .as_ref()
            .and_then(|job| job.0.draft.lock().ok()?.take());
        if let Some(result) = draft_result {
            if let Some(job) = self.pages.management.management_state.draft_job.take() {
                self.tasks.complete(TaskDomain::ManagementDraft, job.0.id);
            }
            match result {
                Ok(draft) => self.pages.management.management_state.pending_draft = Some(draft),
                Err(error) => {
                    let p = problem::OperationProblem::from_error(&error);
                    self.pages.management.management_state.status =
                        i18n::tr!(p.summary_key.clone());
                    self.pages.management.management_state.output = p.detail.clone();
                    self.pages.management.management_state.problem = Some(p);
                }
            }
        }
        if visible {
            if let Some(draft) = self.pages.management.management_state.pending_draft.take() {
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
            .pages
            .management
            .management_state
            .query_job
            .as_ref()
            .and_then(|job| {
                job.0
                    .snapshot
                    .lock()
                    .ok()?
                    .take()
                    .map(|result| (result, job.0.language.clone(), job.0.query.clone()))
            });
        if let Some((result, language, query)) = result {
            if let Some(job) = self.pages.management.management_state.query_job.take() {
                self.tasks.complete(TaskDomain::ManagementQuery, job.0.id);
            }
            match result {
                Ok(snapshot) => {
                    let mut snapshot = snapshot;
                    self.merge_task_history_snapshot(&mut snapshot, query.as_ref());
                    #[cfg(target_os = "linux")]
                    {
                        let uid = unsafe { libc::getuid() };
                        let sockets = platform::management::helper::recoverable_operations(uid)
                            .into_iter()
                            .filter(|socket| {
                                platform::management::helper::operation_kind(socket)
                                    == self.pages.management.management_state.kind
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
                    self.merge_backup_details(&mut snapshot, query.as_ref());
                    self.pages.management.management_state.sort_snapshot(
                        &mut snapshot,
                        &language,
                        query.as_ref(),
                    );
                    let selected = self
                        .pages
                        .management
                        .management_state
                        .snapshot
                        .rows
                        .get(self.pages.management.management_state.selected)
                        .map(|r| r.id.clone());
                    self.pages.management.management_state.selected = selected
                        .and_then(|id| snapshot.rows.iter().position(|r| r.id == id))
                        .unwrap_or(self.pages.management.management_state.selected)
                        .min(snapshot.rows.len().saturating_sub(1));
                    self.pages.management.management_state.status = self
                        .pages
                        .management
                        .management_state
                        .outcome
                        .clone()
                        .unwrap_or_else(|| {
                            if snapshot.notices.is_empty() {
                                format!("{} · {}", snapshot.backend, snapshot.rows.len())
                            } else {
                                snapshot.notices.join("; ")
                            }
                        });
                    self.pages.management.management_state.snapshot = snapshot;
                    self.pages.management.management_state.problem = None;
                }
                Err(e) => {
                    let problem = problem::OperationProblem::from_error(&e);
                    self.pages.management.management_state.status =
                        i18n::tr!(problem.summary_key.clone());
                    self.pages.management.management_state.output = problem.detail.clone();
                    self.pages.management.management_state.problem = Some(problem);
                }
            }
            self.pages.management.management_state.refreshed = Some(Instant::now());
        }
        let operation_source = self
            .pages
            .management
            .management_state
            .operation_job
            .as_ref()
            .map(|job| Arc::clone(&job.0));
        let events = self
            .pages
            .management
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
            if cfg!(windows)
                && self.pages.management.management_state.kind == Some(ManagementKind::Packages)
                && !self
                    .pages
                    .management
                    .management_state
                    .configuration_operation
                && matches!(&event, OperationEvent::Completed { .. })
            {
                self.pages.management.management_state.refresh_requested = true;
            }
            if matches!(
                &event,
                OperationEvent::Failed { .. } | OperationEvent::Disconnected { .. }
            ) {
                self.pages.management.management_state.refresh_requested = false;
            }
            self.pages.management.management_state.revision = self
                .pages
                .management
                .management_state
                .revision
                .wrapping_add(1);
            match event {
                OperationEvent::ConfigDocument { document } => {
                    self.pages.editor.editor_config.received = Some(document)
                }
                OperationEvent::Connected { .. } => {}
                OperationEvent::Problem { problem } => {
                    if self
                        .pages
                        .management
                        .management_state
                        .problem
                        .as_ref()
                        .is_some_and(|p| p.native_exit_code.is_some())
                    {
                        continue;
                    }
                    self.pages.management.management_state.status =
                        i18n::tr!(problem.summary_key.clone());
                    self.pages
                        .management
                        .management_state
                        .output
                        .push_str(&problem.detail);
                    self.pages.management.management_state.problem = Some(problem);
                }
                OperationEvent::Transaction { .. } => {}
                OperationEvent::Started {
                    kind,
                    action,
                    target,
                } => {
                    if kind != ManagementKind::SystemConfig
                        && self.pages.management.management_state.kind != Some(kind)
                    {
                        self.pages.management.management_state.kind = Some(kind);
                        self.pages.management.management_state.query =
                            Some(ManagementQuery::new(kind));
                        self.pages.management.management_state.snapshot =
                            ManagementSnapshot::default();
                        self.pages.management.management_state.snapshot_language = None;
                        self.pages.management.management_state.snapshot_query = None;
                        self.pages.management.management_state.last_language_attempt = None;
                        self.pages.management.management_state.refresh_requested = false;
                        self.pages.management.management_state.refreshing_authorized = false;
                    }
                    self.pages.management.management_state.status = format!(
                        "{} {}",
                        management_label(&action, &action),
                        target.unwrap_or_default()
                    );
                }
                OperationEvent::Snapshot { snapshot } => {
                    if self
                        .pages
                        .management
                        .management_state
                        .configuration_operation
                    {
                        if self.pages.editor.editor_config.pending_action.as_deref()
                            == Some("history")
                        {
                            self.pages.editor.editor_config.history = snapshot;
                        }
                        continue;
                    }
                    self.cancel_management_query();
                    let mut snapshot = snapshot;
                    self.merge_backup_details(
                        &mut snapshot,
                        operation_source
                            .as_ref()
                            .and_then(|source| source.query.as_ref()),
                    );
                    self.merge_task_history_snapshot(
                        &mut snapshot,
                        operation_source
                            .as_ref()
                            .and_then(|source| source.query.as_ref()),
                    );
                    if let Some(source) = &operation_source {
                        self.pages.management.management_state.sort_snapshot(
                            &mut snapshot,
                            &source.language,
                            source.query.as_ref(),
                        );
                    }
                    let selected = self
                        .pages
                        .management
                        .management_state
                        .snapshot
                        .rows
                        .get(self.pages.management.management_state.selected)
                        .map(|row| row.id.clone());
                    self.pages.management.management_state.selected = selected
                        .and_then(|id| snapshot.rows.iter().position(|row| row.id == id))
                        .unwrap_or(self.pages.management.management_state.selected)
                        .min(snapshot.rows.len().saturating_sub(1));
                    self.pages.management.management_state.snapshot = snapshot;
                    self.pages.management.management_state.received_snapshot = true;
                }
                OperationEvent::Progress { message, percent } => {
                    self.pages.management.management_state.status =
                        percent.map_or_else(|| message.clone(), |p| format!("{p}% {message}"))
                }
                OperationEvent::Output { text } => {
                    self.pages
                        .management
                        .management_state
                        .output
                        .push_str(&text);
                    if self.pages.management.management_state.output.len() > 256 * 1024 {
                        let mut start =
                            self.pages.management.management_state.output.len() - 128 * 1024;
                        while !self
                            .pages
                            .management
                            .management_state
                            .output
                            .is_char_boundary(start)
                        {
                            start += 1;
                        }
                        self.pages.management.management_state.output.drain(..start);
                    }
                }
                OperationEvent::TaskOutput {
                    offset,
                    end,
                    total,
                    text,
                } => {
                    self.apply_task_output(offset, end, total, text);
                }
                OperationEvent::TerminalOutput { bytes } => {
                    let area = ui::management_layout(
                        self.management_main(),
                        &self.to_management_view_model(),
                    )
                    .output_text;
                    let parser = self
                        .pages
                        .management
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
                        self.pages.management.management_state.output = parser.screen().contents();
                    }
                }
                OperationEvent::Question {
                    id,
                    prompt,
                    choices,
                    secret,
                } => {
                    if self
                        .pages
                        .management
                        .management_state
                        .auto_admin_job
                        .is_some()
                    {
                        // The same question is already displayed in AutoAdmin's terminal.
                        if let Some(job) = self
                            .pages
                            .management
                            .management_state
                            .auto_admin_job
                            .clone()
                        {
                            self.show_auto_admin_job(job);
                        }
                        continue;
                    }
                    self.reset_management_form_view();
                    self.pages.management.management_state.terminal_mode = false;
                    if !visible {
                        if let Some(kind) = self.pages.management.management_state.kind {
                            self.notify_alert_with_key(
                                self.management_alert_key(),
                                i18n::msg!(
                                    "management-background-question",
                                    application = management_title(kind)
                                ),
                                ui::NotificationTone::Warning,
                            );
                        }
                    }
                    self.pages.management.management_state.form = Some(ManagementEditor {
                        instance: next_management_form_instance(),
                        input_states: Default::default(),
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
                            control: FieldControl::Text,
                        }],
                        selected: 0,
                        purpose: FormPurpose::Answer(id),
                    });
                }
                OperationEvent::Completed { message } | OperationEvent::Failed { message } => {
                    self.pages.management.management_state.status = self
                        .pages
                        .management
                        .management_state
                        .problem
                        .as_ref()
                        .map_or_else(|| message.clone(), |p| i18n::tr!(p.summary_key.clone()));
                    self.pages.management.management_state.outcome =
                        Some(self.pages.management.management_state.status.clone());
                    self.pages.management.management_state.form = None;
                    self.reset_management_form_view();
                    completed = true;
                }
                OperationEvent::Disconnected { message } => {
                    let status = i18n::tr!("management-connection-lost");
                    self.pages
                        .management
                        .management_state
                        .output
                        .push_str(&message);
                    self.pages.management.management_state.status = status.clone();
                    self.pages.management.management_state.outcome = Some(status);
                    self.pages.management.management_state.form = None;
                    self.reset_management_form_view();
                    completed = true;
                }
            }
        }
        if completed {
            let refresh_finished =
                std::mem::take(&mut self.pages.management.management_state.refreshing_authorized);
            if let Some(job) = self.pages.management.management_state.operation_job.take() {
                self.tasks
                    .complete(TaskDomain::ManagementOperation, job.0.id);
            }
            self.pages.management.management_state.terminal_mode =
                self.pages.management.management_state.kind == Some(ManagementKind::Tasks)
                    && self
                        .pages
                        .management
                        .management_state
                        .query
                        .as_ref()
                        .is_some_and(|query| {
                            query
                                .options
                                .get("output_view")
                                .is_some_and(|value| value == "true")
                        });
            if let Some(kind) = self.pages.management.management_state.kind {
                self.resolve_notification_alert(&self.management_alert_key());
                if !visible {
                    self.notify_toast(format!(
                        "{}: {}",
                        management_title(kind),
                        self.pages.management.management_state.status
                    ));
                }
            }
            let config_operation = self
                .pages
                .management
                .management_state
                .configuration_operation;
            self.pages
                .management
                .management_state
                .configuration_operation = false;
            if config_operation {
                self.finish_config_operation();
                if self.pages.editor.editor_config.pending_action.is_none() {
                    self.pages.management.management_state.outcome = None;
                    self.pages.management.management_state.status = if self
                        .pages
                        .management
                        .management_state
                        .snapshot
                        .notices
                        .is_empty()
                    {
                        format!(
                            "{} · {}",
                            self.pages.management.management_state.snapshot.backend,
                            self.pages.management.management_state.snapshot.rows.len()
                        )
                    } else {
                        self.pages
                            .management
                            .management_state
                            .snapshot
                            .notices
                            .join("; ")
                    };
                }
            }
            if !config_operation
                && (self.pages.management.management_state.refresh_requested
                    || (!self.pages.management.management_state.received_snapshot
                        && !refresh_finished))
            {
                self.refresh_management();
            }
        }
        if visible
            && matches!(
                self.pages.management.management_state.kind,
                Some(ManagementKind::Processes | ManagementKind::Tasks)
            )
            && !self.pages.management.management_state.terminal_mode
            && self.pages.management.management_state.query_job.is_none()
            && self
                .pages
                .management
                .management_state
                .operation_job
                .is_none()
            && self.pages.management.management_state.form.is_none()
            && self
                .pages
                .management
                .management_state
                .refreshed
                .is_some_and(|t| t.elapsed() > Duration::from_secs(5))
        {
            if self.pages.management.management_state.kind == Some(ManagementKind::Tasks) {
                self.refresh_personal_task_history();
            } else {
                self.refresh_management();
            }
        }
        self.clamp_management_scroll();
        if visible
            && matches!(
                self.pages.management.management_state.kind,
                Some(ManagementKind::Services | ManagementKind::Processes | ManagementKind::Users)
            )
            && self.pages.management.management_state.query_job.is_none()
            && self
                .pages
                .management
                .management_state
                .operation_job
                .is_none()
            && self.pages.management.management_state.form.is_none()
        {
            let selected = self
                .pages
                .management
                .management_state
                .snapshot
                .rows
                .get(self.pages.management.management_state.selected)
                .map(|r| r.id.clone());
            if let Some(query) = &mut self.pages.management.management_state.query {
                if selected.is_some() && query.target != selected {
                    query.target = selected;
                    self.refresh_management();
                }
            }
        }
        let language = (self.language.code().to_owned(), self.language.generation());
        let state = &self.pages.management.management_state;
        if visible
            && self.active_screen() == ShellScreen::Management
            && state
                .snapshot_language
                .as_ref()
                .is_some_and(|previous| previous != &language)
            && state.last_language_attempt.as_ref() != Some(&language)
            && state.query_job.is_none()
            && state.operation_job.is_none()
            && state.draft_job.is_none()
            && state.form.is_none()
        {
            self.refresh_management();
        }
    }

    pub(in crate::session) fn management_keyboard_identity(&self) -> Option<String> {
        if self.active_screen() != ShellScreen::Management && !self.config_editor_form_visible() {
            return None;
        }
        let state = &self.pages.management.management_state;
        if state.terminal_mode {
            return None;
        }
        if let Some(index) = state.choice_field {
            return state
                .form
                .as_ref()
                .filter(|form| {
                    form.fields
                        .get(index)
                        .is_some_and(|field| field.control == FieldControl::SearchableChoice)
                })
                .map(|form| format!("choice:{}:{index}", form.instance));
        }
        if let Some(form) = &state.form {
            if !form
                .fields
                .iter()
                .any(|field| field.control == FieldControl::Text && field.choices.is_empty())
            {
                return None;
            }
            Some(format!("form:{}", form.instance))
        } else {
            Some(format!("search:{}", state.filtering))
        }
    }

    pub(in crate::session) fn focus_management_keyboard_input(&mut self) {
        let state = &mut self.pages.management.management_state;
        if state.choice_field.is_some() {
            return;
        }
        if let Some(form) = &mut state.form {
            if form.fields.get(form.selected).is_none_or(|field| {
                field.control != FieldControl::Text || !field.choices.is_empty()
            }) {
                if let Some(index) = form.fields.iter().position(|field| {
                    field.control == FieldControl::Text && field.choices.is_empty()
                }) {
                    form.selected = index;
                }
            }
        } else {
            state.filtering = true;
        }
    }

    fn management_main(&self) -> Rect {
        let bounds = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        self.input_keyboard_content_layout(
            ui::ShellFrameLayout::new(
                bounds,
                self.status_time_button_label().as_deref(),
                &ui::RenderContext::default(),
            )
            .with_help_button(self.help_button_available()),
        )
        .main
    }
    pub(in crate::session) fn resize_management_terminal(&mut self) {
        if self
            .pages
            .management
            .management_state
            .auto_admin_job
            .is_some()
        {
            self.resize_auto_admin();
            return;
        }
        let area = ui::management_layout(self.management_main(), &self.to_management_view_model())
            .output_text;
        let rows = area.height.max(1);
        let columns = area.width.max(1);
        if let Some(parser) = &self.pages.management.management_state.parser {
            if let Ok(mut parser) = parser.0.lock() {
                parser.set_size(rows, columns);
            }
        }
        self.management_send(OperationInput::Resize { columns, rows });
    }
    pub(in crate::session) fn open_management_directory(&mut self, platform: &dyn Platform) {
        if let Some(path) = self
            .pages
            .management
            .management_state
            .pending_directory
            .take()
        {
            if let Some(storage) = self.storage_manager.clone() {
                self.open_explorer_at(platform, &storage, path, ExplorerPurpose::Browse);
            }
        }
        if let Some(path) = self
            .pages
            .management
            .management_state
            .pending_select
            .clone()
        {
            if let Some(state) = self.app.explorer_state() {
                if let Some(index) = state.entries.iter().position(|e| e.path == path) {
                    let mut state = state.clone();
                    state.select_index(index, app::explorer::ExplorerSelectionMode::Replace);
                    self.app.dispatch_at(
                        app::AppCommand::SetExplorerState(Some(state)),
                        Instant::now(),
                    );
                    self.pages.management.management_state.pending_select = None;
                }
            }
        }
    }
    pub(in crate::session) fn to_management_view_model(&self) -> ui::ManagementViewModel {
        let s = &self.pages.management.management_state;
        let actions = self.management_actions();
        ui::ManagementViewModel {
            workspace: (s.kind == Some(ManagementKind::Packages)).then(|| self.package_workspace_model(&actions)),
            scope_id: format!("{:?}", s.kind),
            action_tones: actions.iter().map(|(action, _)| if action.confirm { ui::components::ComponentTone::Danger } else { ui::components::ComponentTone::Default }).collect(),
            detail_action_start: Some(0),
            sort: s.sort,
            column_width_limits: if s.kind == Some(ManagementKind::Packages) {
                match s.query.as_ref().map(|query| query.scope.as_str()) {
                    Some("sources") => vec![28, 10, 42],
                    Some("conflicts") => vec![36, 36],
                    Some("status") => Vec::new(),
                    _ => vec![22, 16, 16, 42],
                }
            } else if s.kind == Some(ManagementKind::Processes) {
                vec![20, 8, 8, 12]
            } else {
                Vec::new()
            },
            action_ids: actions
                .iter()
                .map(|(action, row)| {
                    let mut id = format!(
                        "management.action.{:?}.{}.{:?}",
                        s.kind,
                        action.id,
                        row.as_ref().map(|row| (&row.id, &row.identity))
                    );
                    if let Some(selection) = action.values.get(platform::management::selection::KEY) {
                        let mut hash = std::hash::DefaultHasher::new();
                        selection.hash(&mut hash);
                        id.push_str(&format!(".selection.{:x}", hash.finish()));
                    }
                    id
                })
                .collect(),
            title: {
                let title = s.kind.map(|kind| {
                let title = management_title(kind);
                if kind == ManagementKind::Packages {
                    let scope = s.query.as_ref().map(|query| query.scope.as_str()).filter(|scope| !scope.is_empty()).unwrap_or("installed");
                    format!("{title} · {}", management_label(&format!("scope-{scope}"), scope))
                } else if kind == ManagementKind::Tasks {
                    i18n::tr!("tasks-title-usage",size=crate::session::controller::system_status::format_bytes(s.snapshot.task_history_usage.iter().map(|usage|usage.bytes).sum()))
                } else {
                    title
                }
                }).unwrap_or_default();
                if let Some(selection) = &s.selection {
                    format!("{title} · {}", i18n::tr!("selection-count", count = selection.len()))
                } else { title }
            },
            columns: s
                .snapshot
                .columns
                .iter()
                .map(|c| if s.kind == Some(ManagementKind::Users) && c == "Source" {
                    i18n::tr!("startup-account-source")
                } else { management_text("column", c, c) })
                .collect(),
            rows: s.snapshot.rows.iter().map(|r| r.cells.iter().enumerate().map(|(i,v)| {
                let value = if s.kind == Some(ManagementKind::Users) && s.snapshot.columns.get(i).is_some_and(|c| c == "Source") {management_detail_value(s.kind,"Source",v)} else if s.snapshot.columns.get(i).is_some_and(|c|matches!(c.as_str(),"Check"|"Result"|"Next step"|"Source"|"State"|"Status"|"Disk health"|"Result type")) {management_text("value",v,v)}else{v.clone()};
                if i == 0 && s.selection_mode {
                    let checked = s.selection.as_ref().is_some_and(|selection| selection.contains(&selection::row_id(s.kind, r)));
                    format!("[{}] {value}", if checked { "x" } else { " " })
                } else { value }
            }).collect()).collect(),
            selected_rows: s.selection.as_ref().map(|selection| s.snapshot.rows.iter().enumerate().filter_map(|(index, row)| selection.contains(&selection::row_id(s.kind, row)).then_some(index)).collect()),
            selection_mode: s.selection_mode,
            selected: s.selected,
            scroll: s.scroll,
            table_scroll: s.table_scroll,
            action_scroll: s.action_scroll,
            details: if let Some(problem) = &s.problem {
                problem.detail.clone()
            } else {
                let mut details = s.snapshot.rows.get(s.selected)
                    .map(|row| row.detail.as_slice()).unwrap_or_default()
                        .iter().chain(&s.snapshot.details)
                        .map(|(k, v)| {
                            format!(
                                "{}: {}",
                                if s.kind == Some(ManagementKind::Users) && k == "Source" {
                                    i18n::tr!("startup-account-source")
                                } else { management_text("detail", k, k) },
                                management_detail_value(s.kind, k, v)
                            )
                        })
                        .collect::<Vec<_>>();
                if s.details_only {
                    details.extend(s.snapshot.notices.iter().cloned());
                }
                details.join("\n")
            },
            empty_message: s.problem.as_ref().map(|problem| i18n::tr!(problem.summary_key.clone()))
                .or_else(|| match s.kind {
                    Some(ManagementKind::Backups) if s.filter_input.is_empty() && !s.snapshot.needs_authorization => Some(i18n::tr!("management-backups-empty")),
                    Some(ManagementKind::Tasks) if s.filter_input.is_empty() => Some(i18n::tr!("tasks-empty")),
                    _ => (!s.snapshot.notices.is_empty()).then(|| s.snapshot.notices.join("\n")),
                })
                .unwrap_or_else(|| i18n::tr!("management-empty")),
            actions: actions
                .iter()
                .enumerate()
                .map(|(index, (a, _))| {
                    let label = if s.kind == Some(ManagementKind::Packages) {
                        match a.id.as_str() {
                            "scope_search" => i18n::tr!("management-package-nav-search"),
                            "scope_updates" => i18n::tr!("management-package-nav-updates"),
                            "scope_installed" => i18n::tr!("management-package-nav-installed"),
                            "package_advanced" => format!("{} {}", if s.package_advanced_open { "▾" } else { "▸" }, self.package_advanced_label()),
                            _ => action_label(a),
                        }
                    } else { action_label(a) };
                    let shortcut = if a.id == "current_task" {
                        "Ctrl+T".to_string()
                    } else if let Some((shortcut, _)) = management_action_shortcut(s.kind, &a.id) {
                        shortcut.to_string()
                    } else {
                        if s.kind == Some(ManagementKind::Packages) && index >= 10 { String::new() } else { ((index + 1) % 10).to_string() }
                    };
                    (
                        if shortcut.is_empty() { label } else { format!("{shortcut} {label}") },
                        self.management_action_enabled(a),
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
            status: if let Some(selection) = &s.selection {
                format!("{} · {}", i18n::tr!("selection-count", count = selection.len()), s.status)
            } else { s.status.clone() },
            loading: s.query_job.is_some() || s.draft_job.is_some(),
            running: s.operation_job.is_some(),
            output: s.output.clone(),
            output_scroll: s.output_scroll,
            details_scroll: s.details_scroll,
            details_horizontal_scroll: s.details_horizontal_scroll,
            details_only: s.details_only,
            details_expansion: self.details_motion.as_ref()
                .filter(|(page, _)| *page == self.active_page_id() && self.interaction.pointer_motion)
                .map(|(_, motion)| motion.value()),
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
                    cancel_label:match &f.purpose{FormPurpose::Scheduled(wizard)=>wizard.back_label(),_=>None},
                    dismiss_only: matches!(&f.purpose, FormPurpose::Configuration(action) if action == "edit-about"),
                    submit_label: Some(match &f.purpose {
                        FormPurpose::Scheduled(wizard)=>wizard.submit_label(),
                        FormPurpose::Action(action,_)=>action_label(action),
                        FormPurpose::Configuration(action) if action.starts_with("edit-") => match action.as_str() {
                            "edit-goto" => i18n::tr!("ui-editor-go-to-line"),
                            "edit-file" => i18n::tr!("ui-editor-go-to-file"),
                            "edit-about" => i18n::tr!("management-form-close"),
                            _ => i18n::tr!("management-form-apply"),
                        },
                        FormPurpose::Configuration(action) if action == "open" => i18n::tr!("ui-editor-open"),
                        FormPurpose::Configuration(action)=>i18n::tr!(format!("config-editor-{}", match action.as_str() {
                            "preview"=>"check", "check"=>"apply", "history"=>"restore", "conflict"=>"rebase", "properties"=>"preview", "result"=>"continue", other=>other,
                        })),
                        _=>i18n::tr!("management-form-submit"),
                    }),
                    submit_disabled: matches!(&f.purpose,FormPurpose::Configuration(action) if action=="check" && self.config_save_disabled(&f.fields)) || matches!(&f.purpose,FormPurpose::Scheduled(wizard) if wizard.submit_disabled()),
                    identity: format!("{}.{}",f.instance,match &f.purpose {
                        FormPurpose::Scheduled(_)=>"scheduled-task".into(),
                        FormPurpose::Action(action, row) => format!(
                            "{}.{:?}",
                            action.id,
                            row.as_ref().map(|row| (&row.id, &row.identity))
                        ),
                        FormPurpose::Answer(id) => id.clone(),
                        FormPurpose::Menu(_) => "more-actions".into(),
                        FormPurpose::Configuration(action) => format!("config-{action}"),
                    }),
                    field_scroll: s.form_field_scroll,
                    cancel_disabled: matches!(&f.purpose, FormPurpose::Answer(id) if id.starts_with("package-config-")),
                    choice: s.choice_field.and_then(|index| {
                        f.fields.get(index).map(|field| ui::ManagementChoices {
                            search: s.choice_search.clone(),
                            disabled: match &f.purpose { FormPurpose::Menu(items)=>items.iter().map(|(a,_)|a.disabled_reason.is_some() || (s.operation_job.is_some() && a.id!="cancel_operation" && !management_view_action(a))).collect(),_=>vec![] },
                            field: index,
                            values: field.choices.iter().map(|value| form_value_text(&f.purpose, field, value)).collect(),
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
                        .enumerate()
                        .map(|(index, v)| ui::ManagementFormField {
                            id: v.id.clone(),
                            label: if matches!(f.purpose, FormPurpose::Configuration(_))
                                || s.kind == Some(ManagementKind::ScheduledTasks) {
                                v.label.clone()
                            } else if s.kind == Some(ManagementKind::Users)
                                && v.id == "scope"
                                && matches!(&f.purpose, FormPurpose::Action(action, _) if action.id == "set_view") {
                                i18n::tr!("management-field-account-view")
                            } else { management_text("field", &v.id, &v.label) },
                            value: if v.secret {
                                "•".repeat(v.value.chars().count())
                            } else {
                                form_value_text(&f.purpose, v, &v.value)
                            },
                            secret: v.secret,
                            choices: v.choices.clone(),
                            control: match v.control {
                                FieldControl::Text | FieldControl::SearchableChoice => ui::ManagementFormControl::Text,
                                FieldControl::Checkbox => ui::ManagementFormControl::Checkbox,
                                FieldControl::Permissions => ui::ManagementFormControl::Permissions,
                            },
                            selected_cell: Some(self.pages.management.management_state.permission_cell),
                            input: (v.control == FieldControl::Text && v.choices.is_empty()).then(|| {
                                let value = if v.secret { "•".repeat(v.value.chars().count()) } else { v.value.clone() };
                                f.input_states.get(&index).cloned().unwrap_or_default()
                                    .input(v.id.as_str(), &value)
                            }),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
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
