use super::*;
use ui::components::WorkspaceFocus;

fn root_scope(scope: &str) -> &str {
    match scope {
        "search" | "sources" => "search",
        "updates" | "status" => "updates",
        _ => "installed",
    }
}

fn advanced_action(id: &str) -> bool {
    matches!(
        id,
        "scope_sources"
            | "scope_status"
            | "scope_conflicts"
            | "refresh"
            | "check_database"
            | "repair_configure"
            | "repair_dependencies"
    )
}

impl ShellSession {
    pub(super) fn package_multiselect_available(&self) -> bool {
        let state = &self.pages.management.management_state;
        state.kind == Some(ManagementKind::Packages)
            && matches!(
                self.package_scope(),
                "" | "installed" | "updates" | "search"
            )
    }

    pub(super) fn select_package_row(&mut self, index: usize, toggle: bool) {
        let state = &mut self.pages.management.management_state;
        let Some(row) = state.snapshot.rows.get(index) else {
            return;
        };
        if toggle {
            let selection = state.package_selection.get_or_insert_with(|| {
                state
                    .snapshot
                    .rows
                    .get(state.selected)
                    .map(|row| row.id.clone())
                    .into_iter()
                    .collect()
            });
            if !selection.remove(&row.id) {
                selection.insert(row.id.clone());
            }
        } else {
            state.package_selection = None;
        }
        state.selected = index;
        state.revision = state.revision.wrapping_add(1);
    }

    pub(super) fn selected_package_actions(
        &self,
    ) -> Vec<(ManagementAction, Option<ManagementRow>)> {
        let state = &self.pages.management.management_state;
        let rows = state
            .snapshot
            .rows
            .iter()
            .enumerate()
            .filter(|(index, row)| {
                state
                    .package_selection
                    .as_ref()
                    .map_or(*index == state.selected, |selection| {
                        selection.contains(&row.id)
                    })
            })
            .map(|(_, row)| row)
            .collect::<Vec<_>>();
        if rows.len() <= 1 {
            return rows
                .first()
                .map(|row| {
                    row.actions
                        .iter()
                        .map(|action| (action.clone(), Some((*row).clone())))
                        .collect()
                })
                .unwrap_or_default();
        }
        let targets = rows
            .iter()
            .map(
                |row| platform::management::package_selection::PackageTarget {
                    id: row.id.clone(),
                    identity: row.identity.clone(),
                },
            )
            .collect::<Vec<_>>();
        let selection = serde_json::to_string(&targets).expect("package targets serialize");
        let mut actions = Vec::new();
        for row in &rows {
            for action in &row.actions {
                if !matches!(
                    action.id.as_str(),
                    "install" | "remove" | "upgrade" | "pacman_install" | "pacman_upgrade"
                ) || actions.iter().any(
                    |(existing, _): &(ManagementAction, Option<ManagementRow>)| {
                        existing.id == action.id
                    },
                ) {
                    continue;
                }
                let mut action = action.clone();
                for candidate in rows
                    .iter()
                    .flat_map(|row| &row.actions)
                    .filter(|candidate| candidate.id == action.id)
                {
                    action.privileged |= candidate.privileged;
                    action.requires_approval |= candidate.requires_approval;
                    action.confirm |= candidate.confirm;
                }
                if !rows.iter().all(|row| {
                    row.actions.iter().any(|candidate| {
                        candidate.id == action.id
                            && candidate.disabled_reason.is_none()
                            && candidate.values == action.values
                            && candidate.fields == action.fields
                    })
                }) {
                    action.disabled_reason =
                        Some(i18n::tr!("management-package-selection-unavailable"));
                }
                action.values.insert(
                    platform::management::package_selection::KEY.into(),
                    selection.clone(),
                );
                actions.push((action, None));
            }
        }
        actions
    }

    fn package_scope(&self) -> &str {
        self.pages
            .management
            .management_state
            .query
            .as_ref()
            .map_or("installed", |query| query.scope.as_str())
    }

    pub(super) fn package_advanced_label(&self) -> String {
        if self.pages.management.management_state.snapshot.backend == "brew" {
            return i18n::tr!("management-brew-advanced");
        }
        match root_scope(self.package_scope()) {
            "search" => i18n::tr!("management-package-advanced-sources"),
            "updates" => i18n::tr!("management-package-advanced-status"),
            _ => i18n::tr!("management-package-advanced-config"),
        }
    }

    pub(super) fn package_actions(&self) -> Vec<(ManagementAction, Option<ManagementRow>)> {
        let all = self.all_management_actions();
        let mut actions = ["scope_installed", "scope_updates", "scope_search"]
            .into_iter()
            .filter_map(|id| all.iter().find(|(action, _)| action.id == id).cloned())
            .collect::<Vec<_>>();
        let root = root_scope(self.package_scope());
        let advanced_scope = match root {
            "search" => "scope_sources",
            "updates" => "scope_status",
            _ => "scope_conflicts",
        };
        actions.extend(
            all.iter()
                .filter(|(action, _)| action.id == advanced_scope)
                .cloned(),
        );
        actions.extend(all.into_iter().filter(|(action, _)| {
            !management_view_action(action)
                && (!matches!(action.id.as_str(), "upgrade_all" | "pacman_upgrade_all")
                    || root == "updates")
                && (action.id != "refresh"
                    || root != "installed"
                    || self.pages.management.management_state.snapshot.backend == "brew")
                && (action.id != "remove" || self.package_scope() != "updates")
        }));
        if actions
            .iter()
            .any(|(action, _)| advanced_action(&action.id))
        {
            actions.insert(
                3,
                (
                    ManagementAction {
                        id: "package_advanced".into(),
                        label: self.package_advanced_label(),
                        ..Default::default()
                    },
                    None,
                ),
            );
        }
        let state = &self.pages.management.management_state;
        if state.can_show_details() {
            actions.push((
                ManagementAction {
                    id: "query_details".into(),
                    ..Default::default()
                },
                None,
            ));
        }
        if state.auto_admin_job.is_some() {
            actions.push((
                ManagementAction {
                    id: "current_task".into(),
                    ..Default::default()
                },
                None,
            ));
        }
        actions
    }

    pub(super) fn package_workspace_model(
        &self,
        actions: &[(ManagementAction, Option<ManagementRow>)],
    ) -> ui::ManagementWorkspace {
        ui::ManagementWorkspace {
            navigation_actions: vec![0, 1, 2],
            active_navigation: match root_scope(self.package_scope()) {
                "search" => 2,
                "updates" => 1,
                _ => 0,
            },
            primary_actions: actions
                .iter()
                .enumerate()
                .filter_map(|(index, (action, _))| {
                    (index >= 3 && action.id != "package_advanced" && !advanced_action(&action.id))
                        .then_some(index)
                })
                .collect(),
            advanced_toggle: actions
                .iter()
                .position(|(action, _)| action.id == "package_advanced"),
            advanced_actions: actions
                .iter()
                .enumerate()
                .filter_map(|(index, (action, _))| advanced_action(&action.id).then_some(index))
                .collect(),
            advanced_open: self.pages.management.management_state.package_advanced_open,
            focus: self.pages.management.management_state.workspace_focus,
        }
    }

    pub(super) fn focus_package_workspace(&mut self, focus: WorkspaceFocus) {
        let actions = self.management_actions();
        let workspace = self.package_workspace_model(&actions);
        let mut group = workspace
            .action_group(focus)
            .into_iter()
            .filter(|index| self.management_action_enabled(&actions[*index].0))
            .collect::<Vec<_>>();
        let focus = if focus == WorkspaceFocus::Actions && group.is_empty() {
            group = workspace.action_group(WorkspaceFocus::Advanced);
            WorkspaceFocus::Advanced
        } else {
            focus
        };
        let state = &mut self.pages.management.management_state;
        state.workspace_focus = focus;
        if focus == WorkspaceFocus::List && state.details_only {
            state.details_only = false;
            if let Some(query) = &mut state.query {
                query.target = None;
            }
        }
        state.filtering = focus == WorkspaceFocus::Search;
        state.actions_focused = matches!(
            focus,
            WorkspaceFocus::Navigation | WorkspaceFocus::Actions | WorkspaceFocus::Advanced
        );
        if !group.contains(&state.selected_action) {
            state.selected_action = group.first().copied().unwrap_or(0);
        }
    }

    pub(super) fn handle_package_workspace_key(&mut self, key: &KeyInput) -> bool {
        if self.pages.management.management_state.kind != Some(ManagementKind::Packages)
            || self.pages.management.management_state.terminal_mode
            || self.pages.management.management_state.form.is_some()
            || key.has_non_shift_modifier()
        {
            return false;
        }
        if key.key == InputKey::Enter
            && key.modifiers.shift
            && !self.pages.management.management_state.filtering
            && !self.pages.management.management_state.details_only
            && self.package_multiselect_available()
        {
            if key.phase == InputPhase::Press {
                let index = self.pages.management.management_state.selected;
                self.select_package_row(index, true);
                self.focus_package_workspace(WorkspaceFocus::List);
                self.clamp_management_scroll();
            }
            return true;
        }
        if matches!(key.key, InputKey::Tab | InputKey::BackTab) {
            let actions = self.management_actions();
            let workspace = self.package_workspace_model(&actions);
            let backwards = key.key == InputKey::BackTab || key.modifiers.shift;
            let next = (workspace.active_navigation + if backwards { 2 } else { 1 }) % 3;
            self.activate_management_action(workspace.navigation_actions[next]);
            return true;
        }
        if matches!(key.key, InputKey::F(3 | 6..=8)) && !key.modifiers.shift {
            if key.phase == InputPhase::Press {
                let index = match key.key {
                    InputKey::F(6) => Some(2),
                    InputKey::F(8) => Some(1),
                    InputKey::F(7) => Some(0),
                    _ => {
                        self.package_workspace_model(&self.management_actions())
                            .advanced_toggle
                    }
                };
                if let Some(index) = index {
                    self.activate_management_action(index);
                }
            }
            return true;
        }
        if !self.pages.management.management_state.filtering
            && matches!(key.key, InputKey::Enter | InputKey::Space)
            && matches!(
                self.pages.management.management_state.workspace_focus,
                WorkspaceFocus::Navigation | WorkspaceFocus::Actions | WorkspaceFocus::Advanced
            )
        {
            if key.phase == InputPhase::Press {
                self.activate_management_action(
                    self.pages.management.management_state.selected_action,
                );
            }
            return true;
        }
        if !matches!(
            key.key,
            InputKey::Up
                | InputKey::Down
                | InputKey::Left
                | InputKey::Right
                | InputKey::Home
                | InputKey::End
                | InputKey::PageUp
                | InputKey::PageDown
        ) {
            return false;
        }
        if self.pages.management.management_state.details_only {
            self.focus_package_workspace(WorkspaceFocus::Details);
            let scroll = if matches!(key.key, InputKey::Left | InputKey::Right) {
                &mut self
                    .pages
                    .management
                    .management_state
                    .details_horizontal_scroll
            } else {
                &mut self.pages.management.management_state.details_scroll
            };
            *scroll = match key.key {
                InputKey::Up | InputKey::Left => scroll.saturating_sub(1),
                InputKey::Down | InputKey::Right => scroll.saturating_add(1),
                InputKey::PageUp => scroll.saturating_sub(5),
                InputKey::PageDown => scroll.saturating_add(5),
                InputKey::Home => 0,
                _ => u16::MAX,
            };
            self.clamp_management_scroll();
            return true;
        }
        if !matches!(key.key, InputKey::Left | InputKey::Right) {
            self.focus_package_workspace(WorkspaceFocus::List);
        }
        false
    }
}
