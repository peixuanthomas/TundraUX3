impl ShellState {
    fn refresh_hit_map(&mut self) {
        self.hit_map_generation = self.hit_map_generation.saturating_add(1);
        let active_screen = self.active_screen();
        let content_screen = self.content_screen();
        if content_screen == ShellScreen::Login {
            self.sync_login_selection();
        }
        let time_button_label = self.status_time_button_label();
        let notification_model = self.notifications.active_modal_view_model();
        let home_model =
            (content_screen == ShellScreen::Home).then(|| self.to_home_view_model());
        let clock_model =
            (content_screen == ShellScreen::Clock).then(|| self.to_clock_view_model());
        let explorer_model =
            (content_screen == ShellScreen::Explorer).then(|| self.to_explorer_view_model());
        self.hit_map = build_shell_hit_map(
            self.terminal_size,
            content_screen,
            active_screen == ShellScreen::ExitConfirm,
            self.active_popup,
            self.setup_step,
            self.hit_map_generation,
            time_button_label.as_deref(),
            self.time_sync_dialog_visible,
            self.notifications.active_modal_component(),
            notification_model.as_ref(),
            home_model.as_ref(),
            clock_model.as_ref(),
            explorer_model.as_ref(),
        );
        self.sync_home_entry_selection();

        let focus_order = self.focus_order();
        if !focus_order.contains(&self.focused_component) {
            self.focused_component = focus_order.first().copied().unwrap_or(ShellComponent::Home);
            if let Some(field) = setup_field_for_component(self.focused_component) {
                self.setup_focused_field = field;
            }
        }
    }

    fn focus_order(&self) -> Vec<ShellComponent> {
        if let Some(component) = self.notifications.active_modal_component() {
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
                tundra_ui::SetupStep::Language => vec![ShellComponent::SetupLanguage],
                tundra_ui::SetupStep::Timezone => vec![ShellComponent::SetupTimezone],
                tundra_ui::SetupStep::Admin => vec![
                    ShellComponent::SetupAdminUsername,
                    ShellComponent::SetupAdminPassword,
                    ShellComponent::SetupAdminPasswordConfirm,
                    ShellComponent::SetupAdminHint,
                    ShellComponent::SetupSubmit,
                ],
            };
        }
        if self.active_screen() == ShellScreen::Login {
            let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
            if matches!(
                tundra_ui::compute_shell_layout(area),
                tundra_ui::ShellLayout::Compact(_)
            ) {
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
        if self.active_screen() == ShellScreen::Clock {
            let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
            if matches!(
                tundra_ui::compute_shell_layout(area),
                tundra_ui::ShellLayout::Compact(_)
            ) {
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
        if self.auth_session.is_some() {
            order.push(ShellComponent::HomeLogout);
        }
        order.extend([
            ShellComponent::ClockButton,
            ShellComponent::StatusBar,
            ShellComponent::TopBar,
        ]);
        order
    }

    fn move_focus(&mut self, direction: i8) {
        let focus_order = self.focus_order();
        if focus_order.is_empty() {
            return;
        }

        let current_index = focus_order
            .iter()
            .position(|component| *component == self.focused_component)
            .unwrap_or(0);
        let len = focus_order.len() as isize;
        let next_index = (current_index as isize + direction as isize).rem_euclid(len) as usize;
        self.focused_component = focus_order[next_index];
    }

    fn focus_component(&mut self, component: ShellComponent) {
        if self.focus_order().contains(&component) {
            self.focused_component = component;
        }
    }

    fn apply_restored_session(&mut self, session: &ShellRestoredSession) {
        self.screen_stack = vec![ShellScreen::Home];
        self.active_popup = None;

        let focus_order = self.focus_order();
        self.focused_component = if focus_order.contains(&session.focused_component) {
            session.focused_component
        } else {
            focus_order.first().copied().unwrap_or(ShellComponent::Home)
        };
        self.refresh_hit_map();
    }

    fn pop_to_home(&mut self) {
        self.screen_stack.truncate(1);
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.focused_component = ShellComponent::Home;
        self.refresh_hit_map();
    }

    fn cancel_exit_confirmation(&mut self) {
        if self.active_screen() == ShellScreen::ExitConfirm {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.refresh_hit_map();
    }
}
