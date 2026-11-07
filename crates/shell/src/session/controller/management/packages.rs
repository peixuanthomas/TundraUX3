use super::*;

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
}
