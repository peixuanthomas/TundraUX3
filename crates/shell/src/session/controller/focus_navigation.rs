use super::super::*;
impl ShellSession {
    pub(in crate::session) fn refresh_hit_map(&mut self) {
        self.hit_map_generation = self.hit_map_generation.saturating_add(1);
        let active_screen = self.active_screen();
        let content_screen = self.content_screen();
        if content_screen == ShellScreen::Login {
            self.sync_login_selection();
        }
        let time_button_label = self.status_time_button_label();
        let notification_model = self.notification_active_modal_view_model();
        let home_model = (content_screen == ShellScreen::Home).then(|| self.to_home_view_model());
        let clock_model =
            (content_screen == ShellScreen::Clock).then(|| self.to_clock_view_model());
        let explorer_model =
            (content_screen == ShellScreen::Explorer).then(|| self.to_explorer_view_model());
        let diagnostics_model =
            (content_screen == ShellScreen::Diagnostics).then(|| self.to_diagnostics_view_model());
        self.hit_map = build_shell_hit_map(
            self.terminal_size,
            content_screen,
            active_screen == ShellScreen::ExitConfirm,
            self.active_popup,
            self.setup_step,
            self.setup_custom_color_target.is_some(),
            self.hit_map_generation,
            time_button_label.as_deref(),
            self.time_sync_dialog_visible,
            self.notification_active_modal_component(),
            notification_model.as_ref(),
            home_model.as_ref(),
            clock_model.as_ref(),
            explorer_model.as_ref(),
            diagnostics_model.as_ref(),
        );
        self.sync_home_entry_selection();

        let (focus_manager, focus_order) = self.focus_manager(Some(self.focused_component));
        if let Some(focused_component) =
            self.focused_component_from_manager(&focus_manager, &focus_order)
            && focused_component != self.focused_component
        {
            self.focused_component = focused_component;
            if let Some(field) = setup_field_for_component(self.focused_component) {
                self.setup_focused_field = field;
            }
        }
    }

    pub(in crate::session) fn focus_order(&self) -> Vec<ShellComponent> {
        if let Some(component) = self.notification_active_modal_component() {
            return vec![component];
        }
        if self.time_sync_dialog_visible {
            return vec![ShellComponent::TimeSyncDialog];
        }
        if self.active_screen() == ShellScreen::ExitConfirm {
            return vec![ShellComponent::ExitDialog];
        }
        if self.active_screen() == ShellScreen::FirstRunSetup {
            return match self.setup_step {
                ui::SetupStep::Language => vec![ShellComponent::SetupLanguage],
                ui::SetupStep::Timezone => vec![ShellComponent::SetupTimezone],
                ui::SetupStep::Admin => vec![
                    ShellComponent::SetupAdminUsername,
                    ShellComponent::SetupAdminPassword,
                    ShellComponent::SetupAdminPasswordConfirm,
                    ShellComponent::SetupAdminHint,
                    ShellComponent::SetupSubmit,
                ],
                ui::SetupStep::Appearance if self.setup_custom_color_target.is_some() => {
                    vec![ShellComponent::SetupCustomColorDialog]
                }
                ui::SetupStep::Appearance => vec![
                    ShellComponent::SetupAppearanceShape,
                    ShellComponent::SetupAppearanceThemeColor,
                    ShellComponent::SetupAppearanceThemeCustom,
                    ShellComponent::SetupAppearanceAccentColor,
                    ShellComponent::SetupAppearanceAccentCustom,
                    ShellComponent::SetupAppearanceSubmit,
                ],
            };
        }
        if self.active_screen() == ShellScreen::Login {
            let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
            if matches!(ui::compute_shell_layout(area), ui::ShellLayout::Compact(_)) {
                return vec![ShellComponent::CompactHome];
            }
            return vec![
                ShellComponent::LoginUserList,
                ShellComponent::LoginPassword,
                ShellComponent::LoginPasswordVisibility,
            ];
        }
        if self.active_screen() == ShellScreen::BootstrapAdmin {
            return vec![
                ShellComponent::BootstrapUsername,
                ShellComponent::BootstrapPassword,
            ];
        }
        if self.active_screen() == ShellScreen::UserManagement {
            return vec![ShellComponent::UserManagement];
        }
        if self.active_screen() == ShellScreen::Explorer {
            return vec![ShellComponent::Explorer];
        }
        if self.active_screen() == ShellScreen::Launcher {
            return vec![ShellComponent::Launcher];
        }
        if self.active_screen() == ShellScreen::Editor {
            return vec![ShellComponent::Editor];
        }
        if self.active_screen() == ShellScreen::Settings {
            return vec![ShellComponent::Settings];
        }
        if self.active_screen() == ShellScreen::Diagnostics {
            if !self.diagnostics_repair_preview.is_empty() {
                return vec![ShellComponent::DiagnosticsRepairDialog];
            }
            return vec![ShellComponent::Diagnostics];
        }
        if self.active_screen() == ShellScreen::Clock {
            let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
            if matches!(ui::compute_shell_layout(area), ui::ShellLayout::Compact(_)) {
                return vec![ShellComponent::CompactHome];
            }
            if self.clock_create_state.is_some() {
                return vec![
                    ShellComponent::ClockCreateInput,
                    ShellComponent::ClockCreateAlarmButton,
                    ShellComponent::ClockCreateCountdownButton,
                ];
            }
            if self.is_strict_guest() {
                return vec![ShellComponent::ClockButton];
            }
            let mut order = vec![ShellComponent::ClockNewButton];
            if !self.ordered_clock_entry_ids_at(Instant::now()).is_empty() {
                order.push(ShellComponent::ClockEntryList);
            }
            return order;
        }
        if self.active_popup.is_some() {
            return vec![ShellComponent::ContextMenu];
        }
        if self
            .hit_map
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::CompactHome)
        {
            return vec![ShellComponent::CompactHome];
        }

        let mut order = vec![ShellComponent::Home];
        if self.app.auth_session().is_some() {
            order.push(ShellComponent::HomeLogout);
        }
        order.extend([
            ShellComponent::ClockButton,
            ShellComponent::StatusBar,
            ShellComponent::TopBar,
        ]);
        order
    }

    pub(in crate::session) fn focus_manager(
        &self,
        preferred_focus: Option<ShellComponent>,
    ) -> (ui::FocusManager, Vec<ShellComponent>) {
        let focus_order = self.focus_order();
        let mut focus_manager =
            ui::FocusManager::with_order(focus_order.iter().copied().map(ShellComponent::focus_id))
                .expect("Shell focus order must contain unique component ids");

        if let Some(component) = preferred_focus {
            let _ = focus_manager.set_focus(component.focus_id());
        }

        (focus_manager, focus_order)
    }

    pub(in crate::session) fn focused_component_from_manager(
        &self,
        focus_manager: &ui::FocusManager,
        focus_order: &[ShellComponent],
    ) -> Option<ShellComponent> {
        focus_manager
            .focused()
            .and_then(|id| ShellComponent::from_focus_id(id, focus_order))
    }

    pub(in crate::session) fn move_focus(&mut self, direction: ui::FocusDirection) {
        let (mut focus_manager, focus_order) = self.focus_manager(Some(self.focused_component));
        focus_manager.move_focus(direction);
        if let Some(component) = self.focused_component_from_manager(&focus_manager, &focus_order) {
            self.focused_component = component;
        }
    }

    pub(in crate::session) fn focus_component(&mut self, component: ShellComponent) {
        let (mut focus_manager, focus_order) = self.focus_manager(Some(self.focused_component));
        if focus_manager.set_focus(component.focus_id()).is_ok()
            && let Some(component) =
                self.focused_component_from_manager(&focus_manager, &focus_order)
        {
            self.focused_component = component;
        }
    }

    pub(in crate::session) fn apply_restored_session(&mut self, session: &ShellRestoredSession) {
        self.screen_stack = vec![ShellScreen::Home];
        self.active_popup = None;

        let (focus_manager, focus_order) = self.focus_manager(Some(session.focused_component));
        self.focused_component = self
            .focused_component_from_manager(&focus_manager, &focus_order)
            .unwrap_or(ShellComponent::Home);
        self.refresh_hit_map();
    }

    pub(in crate::session) fn pop_to_home(&mut self) {
        self.screen_stack.truncate(1);
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.focused_component = ShellComponent::Home;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn cancel_exit_confirmation(&mut self) {
        if self.active_screen() == ShellScreen::ExitConfirm {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.refresh_hit_map();
    }
}
