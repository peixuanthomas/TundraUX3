use super::*;

pub(super) fn row_id(kind: Option<ManagementKind>, row: &ManagementRow) -> String {
    if kind == Some(ManagementKind::Processes) {
        format!("{}:{:?}", row.id, row.identity)
    } else if kind == Some(ManagementKind::ScheduledTasks)
        && let Some(key) = row.identity.get("result_key")
    {
        format!("{}:{key}", row.id)
    } else {
        row.id.clone()
    }
}

impl ShellSession {
    pub(super) fn management_multiselect_available(&self) -> bool {
        let state = &self.pages.management.management_state;
        match state.kind {
            Some(ManagementKind::Packages) => state.query.as_ref().is_none_or(|query| {
                matches!(
                    query.scope.as_str(),
                    "" | "installed" | "updates" | "search"
                )
            }),
            Some(
                ManagementKind::Services
                | ManagementKind::Processes
                | ManagementKind::ScheduledTasks,
            ) => true,
            _ => false,
        }
    }

    pub(super) fn select_management_row(&mut self, index: usize, toggle: bool) {
        let state = &mut self.pages.management.management_state;
        let Some(row) = state.snapshot.rows.get(index) else {
            return;
        };
        if toggle {
            let selection = state.selection.get_or_insert_with(|| {
                state
                    .snapshot
                    .rows
                    .get(state.selected)
                    .map(|row| row_id(state.kind, row))
                    .into_iter()
                    .collect()
            });
            let id = row_id(state.kind, row);
            if !selection.remove(&id) {
                selection.insert(id);
            }
        } else {
            state.selection = None;
        }
        state.selected = index;
        state.revision = state.revision.wrapping_add(1);
    }

    pub(super) fn selected_management_actions(
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
                    .selection
                    .as_ref()
                    .map_or(*index == state.selected, |selection| {
                        selection.contains(&row_id(state.kind, row))
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
            .map(|row| platform::management::selection::Target {
                id: row.id.clone(),
                identity: row.identity.clone(),
            })
            .collect::<Vec<_>>();
        let selection = serde_json::to_string(&targets).expect("package targets serialize");
        let mut actions = Vec::new();
        for row in &rows {
            for action in &row.actions {
                if !state
                    .kind
                    .is_some_and(|kind| platform::management::selection::supports(kind, &action.id))
                    || actions.iter().any(
                        |(existing, _): &(ManagementAction, Option<ManagementRow>)| {
                            existing.id == action.id
                        },
                    )
                {
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
                    })
                }) {
                    action.disabled_reason = Some(i18n::tr!("management-selection-unavailable"));
                }
                action.values.insert(
                    platform::management::selection::KEY.into(),
                    selection.clone(),
                );
                actions.push((action, None));
            }
        }
        actions
    }

    pub(super) fn handle_management_selection_key(&mut self, key: &KeyInput) -> bool {
        let state = &self.pages.management.management_state;
        if key.key != InputKey::Enter
            || !key.modifiers.shift
            || key.has_non_shift_modifier()
            || state.filtering
            || state.details_only
            || state.terminal_mode
            || !self.management_multiselect_available()
        {
            return false;
        }
        if key.phase == InputPhase::Press {
            let index = state.selected;
            self.select_management_row(index, true);
            self.pages.management.management_state.actions_focused = false;
            if self.pages.management.management_state.kind == Some(ManagementKind::Packages) {
                self.focus_package_workspace(ui::components::WorkspaceFocus::List);
            }
            self.clamp_management_scroll();
        }
        true
    }
}
