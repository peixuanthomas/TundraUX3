use super::*;
use ui::components::WorkspaceFocus;

impl ShellSession {
    pub(super) fn focus_management_workspace(&mut self, focus: WorkspaceFocus) {
        let actions = self.management_actions();
        let Some(workspace) = self.management_workspace_model(&actions) else {
            return;
        };
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

    pub(super) fn handle_management_workspace_key(&mut self, key: &KeyInput) -> bool {
        if self.pages.management.management_state.terminal_mode
            || self.pages.management.management_state.form.is_some()
            || key.has_non_shift_modifier()
        {
            return false;
        }
        let actions = self.management_actions();
        let Some(workspace) = self.management_workspace_model(&actions) else {
            return false;
        };
        if matches!(key.key, InputKey::Tab | InputKey::BackTab) {
            let backwards = key.key == InputKey::BackTab || key.modifiers.shift;
            let mut next = workspace.focus.step(backwards);
            loop {
                let available = match next {
                    WorkspaceFocus::Navigation
                    | WorkspaceFocus::Actions
                    | WorkspaceFocus::Advanced => workspace
                        .action_group(next)
                        .iter()
                        .any(|index| self.management_action_enabled(&actions[*index].0)),
                    WorkspaceFocus::Details => {
                        self.pages.management.management_state.can_show_details()
                    }
                    _ => true,
                };
                if available {
                    break;
                }
                next = next.step(backwards);
            }
            self.focus_management_workspace(next);
            return true;
        }
        if !key.modifiers.shift
            && (key.key == InputKey::F(3)
                || (self.pages.management.management_state.kind == Some(ManagementKind::Packages)
                    && matches!(key.key, InputKey::F(6..=8))))
        {
            if key.phase == InputPhase::Press {
                let index = match key.key {
                    InputKey::F(6) => Some(2),
                    InputKey::F(8) => Some(1),
                    InputKey::F(7) => Some(0),
                    _ => workspace.advanced_toggle,
                };
                if let Some(index) = index {
                    self.activate_management_action(index);
                }
            }
            return true;
        }
        if !self.pages.management.management_state.filtering
            && matches!(
                key.key,
                InputKey::Enter | InputKey::Space | InputKey::Char(' ')
            )
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
        if workspace.focus == WorkspaceFocus::Search {
            return false;
        }
        if matches!(
            workspace.focus,
            WorkspaceFocus::Navigation | WorkspaceFocus::Actions | WorkspaceFocus::Advanced
        ) {
            let group = workspace
                .action_group(workspace.focus)
                .into_iter()
                .filter(|index| self.management_action_enabled(&actions[*index].0))
                .collect::<Vec<_>>();
            let state = &mut self.pages.management.management_state;
            let index = group
                .iter()
                .position(|index| *index == state.selected_action)
                .unwrap_or(0);
            let next = match key.key {
                InputKey::Up | InputKey::Left | InputKey::PageUp => index.saturating_sub(1),
                InputKey::Down | InputKey::Right | InputKey::PageDown => {
                    (index + 1).min(group.len().saturating_sub(1))
                }
                InputKey::Home => 0,
                _ => group.len().saturating_sub(1),
            };
            if let Some(index) = group.get(next) {
                state.selected_action = *index;
            }
            self.clamp_management_scroll();
            return true;
        }
        if workspace.focus == WorkspaceFocus::Details {
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
            self.focus_management_workspace(WorkspaceFocus::List);
        }
        false
    }
}
