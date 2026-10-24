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
        actions.push((
            ManagementAction {
                id: "package_advanced".into(),
                label: self.package_advanced_label(),
                ..Default::default()
            },
            None,
        ));
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
                    (index > 3 && !advanced_action(&action.id)).then_some(index)
                })
                .collect(),
            advanced_toggle: 3,
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
                    InputKey::F(6) => 2,
                    InputKey::F(8) => 1,
                    InputKey::F(7) => 0,
                    _ => 3,
                };
                self.activate_management_action(index);
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
