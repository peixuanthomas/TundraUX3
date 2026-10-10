use super::*;
use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn all_management_actions(
        &self,
    ) -> Vec<(ManagementAction, Option<ManagementRow>)> {
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
        if self.management_state.kind == Some(ManagementKind::Packages) {
            // Navigation must also remain available after a failed query or
            // when a subview has no rows.
            for scope in [
                "search",
                "installed",
                "updates",
                "sources",
                "conflicts",
                "status",
            ] {
                let id = format!("scope_{scope}");
                if !actions.iter().any(|(action, _)| {
                    action.id == id
                        || (action.id == "set_view"
                            && action
                                .values
                                .get("scope")
                                .is_some_and(|value| value == scope))
                }) {
                    actions.push((
                        ManagementAction {
                            id,
                            label: management_label(&format!("scope-{scope}"), scope),
                            group: "view".into(),
                            ..Default::default()
                        },
                        None,
                    ));
                }
            }
        }
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

    pub(in crate::session) fn management_actions(
        &self,
    ) -> Vec<(ManagementAction, Option<ManagementRow>)> {
        let all = self.all_management_actions();
        let kind = self.management_state.kind;
        let preferred: &[&str] = match kind {
            Some(ManagementKind::Packages) => &["refresh", "scope_installed"],
            Some(ManagementKind::Users) => &["user_create", "group_create", "set_view"],
            Some(ManagementKind::Services) => &["start", "stop", "restart", "view_logs"],
            Some(ManagementKind::Processes) => &["term", "cont"],
            Some(ManagementKind::Network) => &[
                "wifi-connect",
                "configure",
                "check",
                "wifi-scan",
                "check-again",
                "network-list",
            ],
            Some(ManagementKind::Disks) => {
                &["open_path", "open_directory", "mount", "unmount", "scan"]
            }
            _ => &[],
        };
        let mut primary = preferred
            .iter()
            .filter_map(|id| all.iter().find(|(action, _)| action.id == *id).cloned())
            .collect::<Vec<_>>();
        if kind == Some(ManagementKind::Packages)
            && !primary.iter().any(|(action, _)| action.id == "refresh")
        {
            primary.insert(
                0,
                (
                    ManagementAction {
                        id: "refresh".into(),
                        disabled_reason: Some(i18n::tr!("management-action-not-available")),
                        ..Default::default()
                    },
                    None,
                ),
            );
        }
        if primary.is_empty()
            && !matches!(kind, Some(ManagementKind::Packages | ManagementKind::Users))
        {
            primary = all
                .iter()
                .filter(|(a, _)| a.primary)
                .take(2)
                .cloned()
                .collect::<Vec<_>>();
        }
        if primary.is_empty()
            && !matches!(kind, Some(ManagementKind::Packages | ManagementKind::Users))
        {
            primary.extend(
                all.iter()
                    .filter(|(a, _)| {
                        a.disabled_reason.is_none()
                            && matches!(
                                a.id.as_str(),
                                "start"
                                    | "stop"
                                    | "view_logs"
                                    | "terminate"
                                    | "install"
                                    | "upgrade"
                                    | "wifi-connect"
                                    | "mount"
                                    | "open_path"
                                    | "open_directory"
                            )
                    })
                    .take(2)
                    .cloned(),
            );
        }
        if !all.is_empty() {
            primary.push((
                ManagementAction {
                    id: "more_actions".into(),
                    label: i18n::tr!("management-more-actions"),
                    ..Default::default()
                },
                None,
            ));
        }
        if self.management_state.auto_admin_job.is_some() {
            primary.push((
                ManagementAction {
                    id: "current_task".into(),
                    label: i18n::tr!("management-current-task"),
                    ..Default::default()
                },
                None,
            ));
        }
        let detail_actions: &[&[&str]] = match kind {
            Some(ManagementKind::Packages) => &[
                &["remove"],
                &["install", "pacman_install"],
                &["upgrade", "pacman_upgrade"],
                &["scope_search"],
            ],
            Some(ManagementKind::Users)
                if self
                    .management_state
                    .query
                    .as_ref()
                    .is_some_and(|query| query.scope == "groups") =>
            {
                &[&["group_members"], &["group_rename"]]
            }
            Some(ManagementKind::Users) => &[&["user_info"], &["user_password"], &["user_groups"]],
            _ => &[],
        };
        for ids in detail_actions {
            primary.push(
                all.iter()
                    .find(|(action, _)| ids.contains(&action.id.as_str()))
                    .cloned()
                    .unwrap_or_else(|| {
                        (
                            ManagementAction {
                                id: ids[0].into(),
                                disabled_reason: Some(i18n::tr!("management-action-not-available")),
                                ..Default::default()
                            },
                            None,
                        )
                    }),
            );
        }
        primary
    }

    pub(in crate::session) fn activate_management_action(&mut self, index: usize) {
        self.reset_management_form_view();
        let Some((action, row)) = self.management_actions().get(index).cloned() else {
            return;
        };
        self.activate_management_item(action, row);
    }

    pub(in crate::session) fn activate_management_item(
        &mut self,
        action: ManagementAction,
        row: Option<ManagementRow>,
    ) {
        if action.id == "current_task" {
            self.management_touch_control(ui::ManagementControl::Terminal);
            return;
        }
        if action.id == "more_actions" {
            let mut items = self.all_management_actions();
            items.sort_by(|(a, _), (b, _)| a.group.cmp(&b.group));
            let choices = items
                .iter()
                .map(|(a, _)| {
                    let label = action_label(a);
                    let group = if a.group.is_empty() {
                        String::new()
                    } else {
                        format!("{} · ", management_text("group", &a.group, &a.group))
                    };
                    let shortcut = management_action_shortcut(self.management_state.kind, &a.id)
                        .map(|(key, _)| format!("    {key}"))
                        .unwrap_or_default();
                    format!("{group}{label}{shortcut}")
                })
                .collect::<Vec<_>>();
            self.management_state.form = Some(ManagementEditor {
                title: i18n::tr!("management-more-actions"),
                message: String::new(),
                message_scroll: 0,
                fields: vec![ManagementField {
                    id: "action".into(),
                    choices,
                    ..Default::default()
                }],
                selected: 0,
                purpose: FormPurpose::Menu(items),
            });
            self.open_management_choice_field(0);
            return;
        }
        if let Some(reason) = &action.disabled_reason {
            self.management_state.status = i18n::tr!("management-problem-unavailable");
            self.management_state.output = reason.clone();
            return;
        }
        if (self.management_state.operation_job.is_some()
            || self.management_state.draft_job.is_some())
            && action.id != "cancel_operation"
            && !management_view_action(&action)
        {
            self.management_state.status = i18n::tr!("management-operation-running");
            return;
        }
        let message = row
            .as_ref()
            .map(|r| r.cells.join(" · "))
            .unwrap_or_default();
        if (action.confirm && !action.privileged) || !action.fields.is_empty() {
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

    pub(in crate::session) fn perform_management_action(
        &mut self,
        action: ManagementAction,
        row: Option<ManagementRow>,
    ) {
        let mut values = action.values.clone();
        values.extend(
            action
                .fields
                .iter()
                .map(|f| (f.id.clone(), f.value.clone())),
        );
        if action.id == "open_path" {
            if let Some(path) = values
                .get("path")
                .or_else(|| row.as_ref().and_then(|r| r.identity.get("path")))
            {
                let path = PathBuf::from(path);
                if !path.exists() {
                    self.management_state.status = i18n::tr!("management-path-missing");
                    return;
                }
                #[cfg(target_os = "linux")]
                if let Some(identity) = row.as_ref().map(|r| &r.identity) {
                    use std::os::unix::fs::MetadataExt;
                    if let (Some(device), Some(inode)) =
                        (identity.get("device"), identity.get("inode"))
                    {
                        let valid = std::fs::symlink_metadata(&path).is_ok_and(|m| {
                            !m.file_type().is_symlink()
                                && device.parse::<u64>().ok() == Some(m.dev())
                                && inode.parse::<u64>().ok() == Some(m.ino())
                        });
                        if !valid {
                            self.management_state.status = i18n::tr!("management-path-missing");
                            return;
                        }
                    }
                }
                if path.is_dir() {
                    self.management_state.pending_directory = Some(path);
                } else {
                    self.management_state.pending_directory = path.parent().map(PathBuf::from);
                    self.management_state.pending_select = Some(path);
                }
            }
            return;
        }
        if action.id == "open_service" {
            if let Some(unit) = values.get("unit").cloned() {
                self.open_management(ManagementKind::Services);
                if let Some(query) = &mut self.management_state.query {
                    query.target = Some(unit);
                    query.scope = values
                        .get("scope")
                        .cloned()
                        .unwrap_or_else(|| "system".into());
                }
                self.refresh_management();
            }
            return;
        }
        if action.id == "show_dependencies" {
            if let Some(query) = &mut self.management_state.query {
                query.target = row.as_ref().map(|r| r.id.clone());
            }
            self.refresh_management();
            return;
        }
        #[cfg(target_os = "linux")]
        if action.id == "sort_scan" {
            if let Some(sort) = values.get("sort") {
                if let Err(error) = platform::management::disks::sort_scan_results(
                    &mut self.management_state.snapshot,
                    sort,
                ) {
                    self.management_state.status =
                        i18n::tr!(problem::OperationProblem::from_error(&error).summary_key);
                    self.management_state.output = error.to_string();
                }
            }
            return;
        }
        if matches!(
            action.id.as_str(),
            "edit_system_config" | "edit_package_source" | "compare_package_config"
        ) {
            if let Some(path) = values.get("path") {
                let path = PathBuf::from(path);
                let draft = values.get("content").map(|content| ConfigDraft {
                    path: path.clone(),
                    content: content.clone(),
                    validator: values
                        .get("validator")
                        .cloned()
                        .unwrap_or_else(|| "auto".into()),
                    service: values.get("service").cloned(),
                    scope: values
                        .get("scope")
                        .cloned()
                        .unwrap_or_else(|| "system".into()),
                    expected_content: values.get("expected_content").cloned(),
                });
                self.begin_config_editor(
                    path,
                    draft,
                    values.get("validator").cloned(),
                    values.get("service").cloned(),
                    values
                        .get("scope")
                        .cloned()
                        .unwrap_or_else(|| "system".into()),
                );
                self.editor_config.compare_path = values.get("compare_path").cloned();
            }
            return;
        }
        #[cfg(target_os = "linux")]
        if matches!(
            action.id.as_str(),
            "create_service"
                | "create_instance"
                | "source_add"
                | "source_enable"
                | "source_disable"
                | "source_remove"
                | "auto_mount"
        ) {
            let command = ManagementCommand {
                kind: self
                    .management_state
                    .kind
                    .unwrap_or(ManagementKind::SystemConfig),
                action: action.id.clone(),
                target: row.as_ref().map(|r| r.id.clone()),
                values,
                identity: row.as_ref().map(|r| r.identity.clone()).unwrap_or_default(),
            };
            self.prepare_management_draft(command);
            return;
        }
        if action.id == "open_directory" {
            self.management_state.pending_directory =
                values.get("directory").map(PathBuf::from).or_else(|| {
                    row.as_ref()
                        .and_then(|r| r.identity.get("directory").map(PathBuf::from))
                });
            return;
        }
        if management_view_action(&action) {
            let package_view = self.management_state.kind == Some(ManagementKind::Packages);
            let mut keep_package_filter = false;
            if let Some(query) = &mut self.management_state.query {
                let old_package_list = matches!(
                    query.scope.as_str(),
                    "" | "search" | "installed" | "updates"
                );
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
                if package_view {
                    keep_package_filter = old_package_list
                        && matches!(
                            query.scope.as_str(),
                            "" | "search" | "installed" | "updates"
                        );
                    query.target = None;
                    if !keep_package_filter {
                        query.filter.clear();
                    }
                    query.options.clear();
                }
            }
            if package_view {
                if !keep_package_filter {
                    self.management_state.filter_input.clear();
                }
                self.management_state.filtering = false;
                self.management_state.details_only = false;
                self.management_state.terminal_mode = false;
                self.management_state.details_scroll = 0;
                self.management_state.selected = 0;
                self.management_state.scroll = 0;
                self.management_state.table_scroll = 0;
                self.management_state.action_scroll = None;
                self.management_state.selected_action = 0;
                self.management_state.actions_focused = false;
                self.management_state.list_scroll_explicit = false;
                self.management_state.outcome = None;
            }
            self.refresh_management();
            return;
        }
        if action.id == "view_logs" {
            if let Some(row) = row {
                let unit = values.get("unit").unwrap_or(&row.id);
                let scope = values
                    .get("scope")
                    .or(row.identity.get("scope"))
                    .map(String::as_str)
                    .unwrap_or("system");
                self.open_related_logs(
                    unit,
                    scope,
                    values.get("boot_id").map(String::as_str),
                    values.get("invocation_id").map(String::as_str),
                    values.get("since_usec").and_then(|s| s.parse().ok()),
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
                self.start_management_operation(None, false, Some(PathBuf::from(socket)), None);
            }
            return;
        }
        let Some(kind) = self.management_state.kind else {
            return;
        };
        let description = std::iter::once(management_title(kind))
            .chain(std::iter::once(management_label(&action.id, &action.label)))
            .chain(row.as_ref().map(|r| r.id.clone()))
            .chain(
                action
                    .fields
                    .iter()
                    .filter(|field| !field.secret)
                    .map(|field| format!("{}: {}", field.label, field.value)),
            )
            .collect::<Vec<_>>()
            .join("\n");
        let command = ManagementCommand {
            kind,
            action: action.id,
            target: row.as_ref().map(|r| r.id.clone()),
            identity: row.as_ref().map(|r| r.identity.clone()).unwrap_or_default(),
            values,
        };
        self.start_management_operation(Some(command), action.privileged, None, Some(description));
    }

    pub(in crate::session) fn management_send(&self, input: OperationInput) {
        if let Some(job) = &self.management_state.operation_job {
            let _ = job.0.responses.send(input);
        }
    }

    #[cfg(target_os = "linux")]
    pub(in crate::session) fn prepare_management_draft(&mut self, command: ManagementCommand) {
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            self.management_state.status = i18n::tr!("management-worker-unavailable");
            return;
        };
        let (job, _) = self.management_job();
        let output = Arc::downgrade(&job.0);
        let cancelled = job.0.cancelled.clone();
        self.management_state.revision = self.management_state.revision.wrapping_add(1);
        let id = TaskId::new(format!(
            "management-draft-{}",
            self.management_state.revision % 64
        ))
        .expect("bounded draft task name");
        match group.spawn_thread(TaskSpec::one_shot(id), move || {
            let result = match command.kind {
                ManagementKind::Services => {
                    platform::management::services::prepare_config_draft(&command, &cancelled)
                }
                ManagementKind::Packages => {
                    platform::management::packages::prepare_config_draft(&command, &cancelled)
                }
                ManagementKind::Disks => {
                    platform::management::disks::automatic_mount_draft(&command, &cancelled)
                }
                _ => Err(ManagementError::InvalidInput(
                    "No configuration draft for this application".into(),
                )),
            };
            if let Some(output) = output.upgrade() {
                *output.draft.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            }
        }) {
            Ok(worker) => {
                *job.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.management_state.draft_job = Some(job);
                self.management_state.status = i18n::tr!("management-working");
            }
            Err(e) => {
                self.management_state.status = i18n::tr!("management-worker-unavailable");
                self.management_state.output = e.to_string();
            }
        }
    }

    pub(in crate::session) fn start_management_operation(
        &mut self,
        command: Option<ManagementCommand>,
        privileged: bool,
        socket: Option<PathBuf>,
        description: Option<String>,
    ) {
        let configuration_operation = command
            .as_ref()
            .is_some_and(|c| c.kind == ManagementKind::SystemConfig);
        #[cfg(target_os = "linux")]
        let privileged = privileged
            || socket
                .as_ref()
                .is_some_and(|path| platform::management::helper::requires_authorized_attach(path));
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            return;
        };
        let (job, rx) = self.management_job();
        let description = description.unwrap_or_else(|| i18n::tr!("aa-reconnect"));
        let requires_approval = privileged
            || command
                .as_ref()
                .is_some_and(|c| c.kind == ManagementKind::SystemConfig && c.action == "reload");
        let Some(aa) =
            self.begin_auto_admin(description, requires_approval, job.0.responses.clone())
        else {
            return;
        };
        self.management_state.auto_admin_job = Some(aa.clone());
        #[cfg(target_os = "linux")]
        aa.enable_helper_control();
        let worker_aa = aa.clone();
        #[cfg(target_os = "linux")]
        let authority = self.privilege_session.clone();
        let output = Arc::downgrade(&job.0);
        let cancelled = job.0.cancelled.clone();
        let task = TaskId::new(format!(
            "management-operation-{}",
            self.management_state
                .kind
                .map(ManagementKind::id)
                .unwrap_or("unknown")
        ))
        .expect("fixed management task name");
        match crate::session::controller::auto_admin::spawn_task(
            &group,
            task,
            self.language.clone(),
            Some(&aa),
            move || {
                let emit = |event| {
                    worker_aa.emit(&event);
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
                let result = worker_aa.run_approved(std::convert::identity, || {
                    platform::management::client::run(
                        #[cfg(target_os = "linux")]
                        &authority,
                        command,
                        privileged,
                        socket,
                        rx,
                        &cancelled,
                        &emit,
                    )
                });
                if let Err(error) = result {
                    if matches!(
                        &error,
                        ManagementError::Cancelled
                            | ManagementError::PermissionDenied(_)
                            | ManagementError::InvalidInput(_)
                            | ManagementError::Unavailable(_)
                    ) {
                        emit(OperationEvent::Problem {
                            problem: problem::OperationProblem::from_error(&error),
                        });
                        emit(OperationEvent::Failed {
                            message: error.to_string(),
                        });
                    } else {
                        emit(OperationEvent::Disconnected {
                            message: error.to_string(),
                        });
                    }
                }
            },
        ) {
            Ok(worker) => {
                self.management_state.configuration_operation = configuration_operation;
                self.management_state.problem = None;
                *job.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.management_state.operation_job = Some(job);
                self.management_state.outcome = None;
                if !configuration_operation {
                    self.management_state.received_snapshot = false;
                }
                self.management_state.output.clear();
                self.management_state.parser = None;
                self.management_state.status = i18n::tr!("management-working");
                self.resize_management_terminal();
            }
            Err(error) => {
                self.management_state.status = i18n::tr!("management-worker-unavailable");
                self.management_state.output = error.to_string();
            }
        }
    }

    pub(in crate::session) fn submit_management_form(&mut self) {
        if self.management_state.form.as_ref().is_some_and(|form| {
            matches!(&form.purpose, FormPurpose::Configuration(action) if action == "check")
                && self.config_save_disabled(&form.fields)
        }) {
            return;
        }
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
            FormPurpose::Configuration(action) => self.submit_config_form(&action, form.fields),
            FormPurpose::Menu(items) => {
                let selected = self.management_state.choice_selected;
                self.reset_management_form_view();
                if let Some((action, row)) = items.get(selected).cloned() {
                    self.activate_management_item(action, row);
                }
            }
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

    pub(in crate::session) fn cancel_management_form(&mut self) {
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
}
