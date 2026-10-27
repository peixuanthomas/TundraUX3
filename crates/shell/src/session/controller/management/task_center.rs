use super::*;
use platform::task_history::HistoryStore;
use std::collections::BTreeSet;

impl ShellSession {
    pub(in crate::session) fn task_origin(&self) -> String {
        match self.active_screen() {
            ShellScreen::Management => self
                .pages
                .management
                .management_state
                .kind
                .map(ManagementKind::id)
                .unwrap_or("management"),
            ShellScreen::Editor if self.pages.editor.script.enabled => "script",
            ShellScreen::Editor => "editor",
            ShellScreen::Explorer => "explorer",
            ShellScreen::Launcher => "launcher",
            ShellScreen::CommandLine => "command-line",
            ShellScreen::Settings => "settings",
            ShellScreen::Diagnostics => "diagnostics",
            ShellScreen::UserManagement => "users",
            _ => "home",
        }
        .into()
    }

    pub(in crate::session) fn open_tasks(&mut self) {
        let mut query = ManagementQuery::new(ManagementKind::Tasks);
        if let Some(storage) = &self.storage_manager {
            query.options.insert(
                "local_history_root".into(),
                storage
                    .layout()
                    .data_path
                    .join("task-history")
                    .display()
                    .to_string(),
            );
        }
        self.open_management_query(query);
    }

    fn local_task_store(&self) -> Result<HistoryStore, ManagementError> {
        self.storage_manager
            .as_ref()
            .map(|storage| {
                Ok(HistoryStore::new(
                    storage.layout().data_path.join("task-history"),
                ))
            })
            .unwrap_or_else(platform::management::task_history::local_store)
    }

    pub(super) fn merge_task_history_snapshot(
        &mut self,
        snapshot: &mut ManagementSnapshot,
        query: Option<&ManagementQuery>,
    ) {
        if self.pages.management.management_state.kind != Some(ManagementKind::Tasks) {
            return;
        }
        let query = query
            .cloned()
            .or_else(|| self.pages.management.management_state.query.clone())
            .unwrap_or_else(|| ManagementQuery::new(ManagementKind::Tasks));
        if query
            .options
            .get("personal_refresh")
            .is_some_and(|value| value == "true")
            && !snapshot
                .task_history_usage
                .iter()
                .any(|usage| usage.source != "local")
        {
            let old = &self.pages.management.management_state.snapshot;
            snapshot.refresh_action = old.refresh_action.clone();
            snapshot.task_history_usage.extend(
                old.task_history_usage
                    .iter()
                    .filter(|usage| usage.source != "local")
                    .cloned(),
            );
            snapshot.rows.extend(
                old.rows
                    .iter()
                    .filter(|row| {
                        row.identity
                            .get("bucket")
                            .is_some_and(|bucket| bucket != "local")
                    })
                    .cloned()
                    .map(|mut row| {
                        row.identity.insert("cached_system".into(), "true".into());
                        row
                    }),
            );
            if snapshot
                .task_history_usage
                .iter()
                .any(|usage| usage.source != "local")
            {
                snapshot.notices.push(i18n::tr!("task-system-cached"));
            }
        }
        if !snapshot
            .task_history_usage
            .iter()
            .any(|usage| usage.source == "local")
        {
            if let Ok(store) = self.local_task_store() {
                if let Err(error) = platform::management::task_history::append_store(
                    snapshot, &query, &store, "local", false,
                ) {
                    snapshot.notices.push(error.to_string());
                }
            }
        }
        platform::management::task_history::finish_snapshot(snapshot);
        let mut active = snapshot
            .task_history_usage
            .iter()
            .flat_map(|usage| usage.active_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        if let Ok(store) = self.local_task_store() {
            active.retain(|id| {
                store.read(id).ok().is_none_or(|summary| {
                    summary.status.active() && platform::task_history::owner_running(&summary)
                })
            });
        }
        self.tasks.known_active_history(active);
    }

    pub(super) fn refresh_personal_task_history(&mut self) {
        let authorized = self
            .pages
            .management
            .management_state
            .snapshot
            .refresh_action
            .take();
        if let Some(query) = &mut self.pages.management.management_state.query {
            query
                .options
                .insert("personal_refresh".into(), "true".into());
        }
        self.refresh_management();
        self.pages
            .management
            .management_state
            .snapshot
            .refresh_action = authorized;
        if let Some(query) = &mut self.pages.management.management_state.query {
            query.options.remove("personal_refresh");
        }
    }

    pub(super) fn task_center_actions(&self) -> Vec<(ManagementAction, Option<ManagementRow>)> {
        let all = self.all_management_actions();
        let preferred = if self.pages.management.management_state.terminal_mode {
            [
                "task_output_previous",
                "task_output_next",
                "task_return",
                "task_filter",
            ]
        } else {
            ["task_output", "task_return", "task_cancel", "task_filter"]
        };
        let mut result = preferred
            .into_iter()
            .filter_map(|id| all.iter().find(|(action, _)| action.id == id).cloned())
            .collect::<Vec<_>>();
        let options = self
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .map(|query| &query.options);
        let offset = options
            .and_then(|options| options.get("output_offset"))
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let total = options
            .and_then(|options| options.get("output_total"))
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let end = options
            .and_then(|options| options.get("output_end"))
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        for (action, _) in &mut result {
            if (action.id == "task_output_previous" && offset == 0)
                || (action.id == "task_output_next" && end >= total)
            {
                action.disabled_reason = Some(i18n::tr!("task-action-unavailable"));
            }
        }
        result.push((
            ManagementAction {
                id: "more_actions".into(),
                label: i18n::tr!("management-more-actions"),
                ..Default::default()
            },
            None,
        ));
        result
    }

    pub(super) fn apply_task_output(&mut self, offset: u64, end: u64, total: u64, text: String) {
        let state = &mut self.pages.management.management_state;
        state.output = format!(
            "{}\n{text}",
            i18n::tr!(
                "task-output-range",
                start = offset,
                end = end,
                total = total
            )
        );
        state.output_scroll = 0;
        state.terminal_mode = true;
        state.parser = None;
        if let Some(query) = &mut state.query {
            query.options.insert("output_view".into(), "true".into());
            query
                .options
                .insert("output_offset".into(), offset.to_string());
            query.options.insert("output_end".into(), end.to_string());
            query
                .options
                .insert("output_total".into(), total.to_string());
        }
    }

    pub(super) fn perform_task_center_action(
        &mut self,
        action: &ManagementAction,
        row: Option<&ManagementRow>,
    ) -> bool {
        if self.pages.management.management_state.kind != Some(ManagementKind::Tasks) {
            return false;
        }
        if !action.id.starts_with("task_output") {
            self.pages.management.management_state.terminal_mode = false;
            if let Some(query) = &mut self.pages.management.management_state.query {
                query.options.remove("output_view");
            }
        }
        let mut values = action.values.clone();
        values.extend(
            action
                .fields
                .iter()
                .map(|field| (field.id.clone(), field.value.clone())),
        );
        match action.id.as_str() {
            "task_filter" | "task_next_page" | "task_first_page" => {
                if let Some(query) = &mut self.pages.management.management_state.query {
                    query.options.remove("before");
                    if action.id == "task_filter" {
                        if let Some(status) = values.get("status") {
                            if let Some(id) = ["all", "active", "attention", "finished"]
                                .into_iter()
                                .find(|id| {
                                    *id == status
                                        || i18n::tr!(format!("task-state-{id}")) == *status
                                })
                            {
                                query.options.insert("status".into(), id.into());
                            }
                        }
                    }
                    if action.id == "task_next_page" {
                        if let Some(before) = values.get("before") {
                            query.options.insert("before".into(), before.clone());
                        }
                    }
                }
                self.refresh_management();
                true
            }
            "task_return" => {
                if let Some(row) = row {
                    let id = row
                        .identity
                        .get("history_id")
                        .map(String::as_str)
                        .unwrap_or("");
                    if let Some((_, _, Some(page))) = self.tasks.history_task(id) {
                        if self.activate_task_page(page) {
                            return true;
                        }
                    }
                    self.notify_status(i18n::msg!("task-page-closed"));
                    match row.identity.get("origin").map(String::as_str).unwrap_or("") {
                        "script" => self.open_script_editor(),
                        "editor" => self.open_editor(),
                        "explorer" => self.enter_screen(ShellScreen::Explorer),
                        "launcher" => self.enter_screen(ShellScreen::Launcher),
                        "command-line" => self.open_command_line(),
                        "settings" | "update" => self.enter_screen(ShellScreen::Settings),
                        origin => {
                            let kind = match origin {
                                "services" => Some(ManagementKind::Services),
                                "processes" => Some(ManagementKind::Processes),
                                "packages" => Some(ManagementKind::Packages),
                                "network" => Some(ManagementKind::Network),
                                "disks" => Some(ManagementKind::Disks),
                                "users" => Some(ManagementKind::Users),
                                "backups" => Some(ManagementKind::Backups),
                                "firewall" => Some(ManagementKind::Firewall),
                                "ssh" => Some(ManagementKind::Ssh),
                                "scheduled-tasks" => Some(ManagementKind::ScheduledTasks),
                                _ => None,
                            };
                            if let Some(kind) = kind {
                                self.open_management(kind);
                            }
                        }
                    }
                }
                true
            }
            "task_cancel" => {
                if let Some(row) = row {
                    if let Some((domain, request, _)) = row
                        .identity
                        .get("history_id")
                        .and_then(|id| self.tasks.history_task(id))
                    {
                        self.tasks.cancel(domain, request);
                        self.refresh_management();
                        return true;
                    }
                }
                false
            }
            "task_output" | "task_output_previous" | "task_output_next" => {
                let Some(row) = row else {
                    return true;
                };
                let offset = self
                    .pages
                    .management
                    .management_state
                    .query
                    .as_ref()
                    .and_then(|query| query.options.get("output_offset"))
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                let offset = match action.id.as_str() {
                    "task_output_previous" => offset.saturating_sub(64 * 1024),
                    "task_output_next" => self
                        .pages
                        .management
                        .management_state
                        .query
                        .as_ref()
                        .and_then(|query| query.options.get("output_end"))
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(offset.saturating_add(64 * 1024)),
                    _ => 0,
                };
                if let Some(query) = &mut self.pages.management.management_state.query {
                    query
                        .options
                        .insert("output_offset".into(), offset.to_string());
                }
                if row
                    .identity
                    .get("bucket")
                    .is_some_and(|bucket| bucket != "local")
                {
                    let mut action = action.clone();
                    action.id = "task_output".into();
                    action.values.insert("offset".into(), offset.to_string());
                    let command = ManagementCommand {
                        kind: ManagementKind::Tasks,
                        action: action.id,
                        target: Some(row.id.clone()),
                        identity: row.identity.clone(),
                        values: action.values,
                    };
                    self.start_management_operation(
                        Some(command),
                        action.privileged,
                        false,
                        None,
                        Some(i18n::tr!("task-action-output")),
                    );
                    return true;
                }
                let result = self.local_task_store().and_then(|store| {
                    store.output_text(
                        row.identity
                            .get("history_id")
                            .map(String::as_str)
                            .unwrap_or(""),
                        offset,
                        64 * 1024,
                    )
                });
                match result {
                    Ok(window) => {
                        self.apply_task_output(
                            window.offset,
                            window.end,
                            window.total,
                            window.text,
                        );
                    }
                    Err(error) => self.pages.management.management_state.status = error.to_string(),
                }
                true
            }
            "task_cleanup"
                if row.is_some_and(|row| {
                    row.identity
                        .get("bucket")
                        .is_some_and(|bucket| bucket == "local")
                }) =>
            {
                let Some(row) = row else {
                    return true;
                };
                let id = row.identity.get("history_id").cloned().unwrap_or_default();
                match self
                    .local_task_store()
                    .and_then(|store| store.preview_cleanup(&[id]).map(|preview| (store, preview)))
                {
                    Ok((store, preview)) => {
                        let text = platform::management::task_history::cleanup_message(&preview);
                        let mut action = action.clone();
                        action.id = "task_cleanup_confirm".into();
                        action.values.insert(
                            "preview".into(),
                            serde_json::to_string(&preview).expect("task preview serializes"),
                        );
                        action.confirm = true;
                        self.pages.management.management_state.form = Some(ManagementEditor {
                            title: i18n::tr!("task-action-cleanup"),
                            message: text,
                            message_scroll: 0,
                            instance: PageInstanceId::new().get(),
                            fields: vec![],
                            selected: 0,
                            purpose: FormPurpose::Action(action, Some(row.clone())),
                        });
                        let _ = store;
                    }
                    Err(error) => self.pages.management.management_state.status = error.to_string(),
                }
                true
            }
            "task_cleanup_confirm" => {
                let result = values
                    .get("preview")
                    .ok_or_else(|| {
                        ManagementError::InvalidInput(i18n::tr!("task-history-invalid-id"))
                    })
                    .and_then(|text| {
                        serde_json::from_str(text)
                            .map_err(|error| ManagementError::InvalidInput(error.to_string()))
                    })
                    .and_then(|preview| self.local_task_store()?.cleanup(&preview));
                match result {
                    Ok(()) => self.refresh_management(),
                    Err(error) => self.pages.management.management_state.status = error.to_string(),
                };
                true
            }
            _ => false,
        }
    }
}
