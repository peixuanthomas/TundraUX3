impl ShellState {
    pub fn active_screen(&self) -> ShellScreen {
        self.screen_stack
            .last()
            .copied()
            .unwrap_or(ShellScreen::Home)
    }

    pub fn home_mode(&self) -> ShellHomeMode {
        self.home_mode
    }

    pub fn screen_stack(&self) -> &[ShellScreen] {
        &self.screen_stack
    }

    pub fn terminal_size(&self) -> (u16, u16) {
        self.terminal_size
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    pub fn last_key_event(&self) -> Option<&str> {
        self.last_key_event.as_deref()
    }

    pub fn last_mouse_event(&self) -> Option<&str> {
        self.last_mouse_event.as_deref()
    }

    pub fn last_resize_event(&self) -> Option<&str> {
        self.last_resize_event.as_deref()
    }

    pub fn mouse_coordinates(&self) -> Option<(u16, u16)> {
        self.mouse_coordinates
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub fn terminal_flags(&self) -> ShellTerminalFlags {
        self.terminal_flags
    }

    pub fn mouse_scroll_direction(&self) -> Option<&str> {
        self.mouse_scroll_direction.as_deref()
    }

    pub fn mouse_drag_direction(&self) -> Option<&str> {
        self.mouse_drag_direction.as_deref()
    }

    pub fn platform_capability_summary(&self) -> &str {
        &self.platform_capability_summary
    }

    pub fn focused_component(&self) -> ShellComponent {
        self.focused_component
    }

    pub fn selected_home_entry_index(&self) -> usize {
        let count = self.user_home_entries().len();
        if count == 0 {
            0
        } else {
            self.selected_home_entry_index.min(count - 1)
        }
    }

    pub fn hovered_component(&self) -> Option<ShellComponent> {
        self.hovered_component
    }

    pub fn active_popup(&self) -> Option<ShellPopup> {
        self.active_popup
    }

    pub fn hit_map(&self) -> &ShellHitMap {
        &self.hit_map
    }

    pub fn hit_map_generation(&self) -> u64 {
        self.hit_map.generation()
    }

    pub fn hit_target_at(&self, coordinates: CellPosition) -> Option<ShellComponent> {
        self.hit_map.target_at(coordinates)
    }

    pub fn last_command(&self) -> Option<&ShellCommand> {
        self.last_command.as_ref()
    }

    pub fn last_routed_target(&self) -> Option<RoutedTarget> {
        self.last_routed_target
    }

    fn home_display_mode(&self) -> tundra_ui::HomeDisplayMode {
        if matches!(
            self.active_screen(),
            ShellScreen::FirstRunSetup | ShellScreen::Login | ShellScreen::BootstrapAdmin
        ) {
            return tundra_ui::HomeDisplayMode::Auth;
        }

        match self.home_mode {
            ShellHomeMode::Debug => tundra_ui::HomeDisplayMode::Debug,
            ShellHomeMode::User => tundra_ui::HomeDisplayMode::User,
        }
    }

    pub fn auth_session(&self) -> Option<&AuthSession> {
        self.auth_session.as_ref()
    }

    pub fn guest_mode(&self) -> bool {
        self.guest_mode
    }

    #[doc(hidden)]
    pub fn login_idle_deadline_for_test(&self) -> Instant {
        self.login_idle_deadline
    }

    #[doc(hidden)]
    pub fn login_password_visible_until_for_test(&self) -> Option<Instant> {
        self.login_password_visible_until
    }

    fn return_to_lockscreen_requested(&self) -> bool {
        self.return_to_lockscreen_requested
    }
}
