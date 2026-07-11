impl ShellState {
    fn append_auth_char(&mut self, character: char) {
        match self.focused_component {
            ShellComponent::LoginPassword => self.login_password.push(character),
            ShellComponent::BootstrapUsername => self.bootstrap_username.push(character),
            ShellComponent::BootstrapPassword => self.bootstrap_password.push(character),
            _ => {}
        }
        self.error_message = None;
    }

    fn reset_login_idle_deadline_at(&mut self, now: Instant) {
        self.login_idle_deadline = now + LOGIN_IDLE_TIMEOUT;
    }

    fn login_idle_tracking_active(&self) -> bool {
        self.screen_stack.contains(&ShellScreen::Login)
            && self.auth_session.is_none()
            && !self.guest_mode
    }

    fn expire_login_password_visibility_at(&mut self, now: Instant) {
        if self
            .login_password_visible_until
            .is_some_and(|deadline| now >= deadline)
        {
            self.login_password_visible_until = None;
        }
    }

    fn login_password_is_visible_at(&self, now: Instant) -> bool {
        self.login_password_visible_until
            .is_some_and(|deadline| now < deadline)
    }

    fn auth_poll_timeout(&self, now: Instant, fallback: Duration) -> Duration {
        let mut timeout = fallback;
        if self.login_idle_tracking_active() {
            timeout = timeout.min(self.login_idle_deadline.saturating_duration_since(now));
        }
        if let Some(deadline) = self.login_password_visible_until {
            timeout = timeout.min(deadline.saturating_duration_since(now));
        }
        timeout
    }

    fn toggle_login_password_visibility_at(&mut self, now: Instant) {
        if self.login_password_is_visible_at(now) {
            self.login_password_visible_until = None;
        } else {
            self.login_password_visible_until = Some(now + PASSWORD_REVEAL_DURATION);
        }
        self.error_message = None;
    }

    fn prepare_return_to_lockscreen(&mut self) {
        self.login_password.clear();
        self.login_password_visible_until = None;
        self.error_message = None;
        self.return_to_lockscreen_requested = true;
    }

    fn auth_backspace(&mut self) {
        match self.focused_component {
            ShellComponent::LoginPassword => {
                self.login_password.pop();
            }
            ShellComponent::BootstrapUsername => {
                self.bootstrap_username.pop();
            }
            ShellComponent::BootstrapPassword => {
                self.bootstrap_password.pop();
            }
            _ => {}
        }
        self.error_message = None;
    }

    fn selected_login_username(&self) -> Option<&str> {
        self.login_users
            .get(self.login_selected_user)
            .map(|user| user.username.as_str())
    }

    fn selected_login_password_hint(&self) -> Option<&str> {
        self.login_users
            .get(self.login_selected_user)?
            .password_hint
            .as_deref()
    }

    fn sync_login_selection(&mut self) {
        if self.login_users.is_empty() {
            self.login_selected_user = 0;
            self.login_user_window_start = 0;
            self.login_username.clear();
            return;
        }

        self.login_selected_user = self.login_selected_user.min(self.login_users.len() - 1);
        self.login_username = self.login_users[self.login_selected_user].username.clone();
        self.sync_login_user_window();
    }

    fn sync_login_user_window(&mut self) {
        let count = self.login_users.len();
        if count == 0 {
            self.login_user_window_start = 0;
            return;
        }

        let visible_rows = self.login_user_visible_row_count().min(count).max(1);
        let max_start = count.saturating_sub(visible_rows);
        self.login_user_window_start = self.login_user_window_start.min(max_start);

        if self.login_selected_user < self.login_user_window_start {
            self.login_user_window_start = self.login_selected_user;
        }

        let window_end = self.login_user_window_start.saturating_add(visible_rows);
        if self.login_selected_user >= window_end {
            self.login_user_window_start = self
                .login_selected_user
                .saturating_add(1)
                .saturating_sub(visible_rows)
                .min(max_start);
        }
    }

    fn login_user_visible_row_count(&self) -> usize {
        login_user_visible_row_count(self.terminal_size).max(1)
    }

    fn select_login_user_delta(&mut self, delta: isize) {
        if self.login_users.is_empty() {
            self.sync_login_selection();
            return;
        }

        let current = self.login_selected_user.min(self.login_users.len() - 1) as isize;
        self.login_selected_user =
            (current + delta).clamp(0, self.login_users.len() as isize - 1) as usize;
        self.login_password.clear();
        self.login_password_visible_until = None;
        self.error_message = None;
        self.sync_login_selection();
    }

    fn select_first_login_user(&mut self) {
        self.login_selected_user = 0;
        self.login_password.clear();
        self.login_password_visible_until = None;
        self.error_message = None;
        self.sync_login_selection();
    }

    fn select_last_login_user(&mut self) {
        if self.login_users.is_empty() {
            self.sync_login_selection();
            return;
        }

        self.login_selected_user = self.login_users.len() - 1;
        self.login_password.clear();
        self.login_password_visible_until = None;
        self.error_message = None;
        self.sync_login_selection();
    }

    fn select_login_user_at(&mut self, index: usize) {
        if index >= self.login_users.len() {
            return;
        }

        self.login_selected_user = index;
        self.login_password.clear();
        self.login_password_visible_until = None;
        self.error_message = None;
        self.sync_login_selection();
    }

    fn refresh_login_users_from_storage(&mut self) -> Result<(), StorageError> {
        let Some(storage) = self.storage_manager.clone() else {
            return Ok(());
        };
        let previous_username = self.selected_login_username().map(str::to_string);
        let users = storage.load_users()?;
        self.login_users = users
            .users
            .iter()
            .map(ShellLoginUser::from_record)
            .collect();
        self.login_selected_user = previous_username
            .as_deref()
            .and_then(|username| {
                self.login_users
                    .iter()
                    .position(|user| user.username.eq_ignore_ascii_case(username))
            })
            .unwrap_or_else(|| default_login_user_index(&self.login_users));
        self.sync_login_selection();
        Ok(())
    }

    fn setup_next_language(&mut self) {
        let count = tundra_ui::setup_language_options().len();
        if count == 0 {
            return;
        }
        self.setup_selected_language_index = (self.setup_selected_language_index + 1) % count;
        self.error_message = None;
    }

    fn setup_previous_language(&mut self) {
        let count = tundra_ui::setup_language_options().len();
        if count == 0 {
            return;
        }
        self.setup_selected_language_index =
            (self.setup_selected_language_index + count - 1) % count;
        self.error_message = None;
    }

    fn setup_select_timezone_delta(&mut self, delta: isize) {
        let count = tundra_ui::setup_timezone_options().len();
        if count == 0 {
            self.setup_timezone_window_start = 0;
            return;
        }

        let current = self.setup_selected_timezone_index.min(count - 1) as isize;
        let next = (current + delta).clamp(0, count as isize - 1) as usize;
        self.setup_selected_timezone_index = next;
        self.sync_setup_timezone_window();
        self.error_message = None;
    }

    fn setup_select_first_timezone(&mut self) {
        self.setup_selected_timezone_index = 0;
        self.sync_setup_timezone_window();
        self.error_message = None;
    }

    fn setup_select_last_timezone(&mut self) {
        let count = tundra_ui::setup_timezone_options().len();
        if count == 0 {
            return;
        }
        self.setup_selected_timezone_index = count - 1;
        self.sync_setup_timezone_window();
        self.error_message = None;
    }

    fn sync_setup_timezone_window(&mut self) {
        let count = tundra_ui::setup_timezone_options().len();
        if count == 0 {
            self.setup_selected_timezone_index = 0;
            self.setup_timezone_window_start = 0;
            return;
        }

        self.setup_selected_timezone_index = self.setup_selected_timezone_index.min(count - 1);
        let visible_rows = self.setup_timezone_visible_row_count().min(count).max(1);
        let max_start = count.saturating_sub(visible_rows);
        self.setup_timezone_window_start = self.setup_timezone_window_start.min(max_start);

        if self.setup_selected_timezone_index < self.setup_timezone_window_start {
            self.setup_timezone_window_start = self.setup_selected_timezone_index;
        }

        let window_end = self
            .setup_timezone_window_start
            .saturating_add(visible_rows);
        if self.setup_selected_timezone_index >= window_end {
            self.setup_timezone_window_start = self
                .setup_selected_timezone_index
                .saturating_add(1)
                .saturating_sub(visible_rows)
                .min(max_start);
        }
    }

    fn setup_timezone_visible_row_count(&self) -> usize {
        setup_timezone_visible_row_count(self.terminal_size).max(1)
    }

    fn setup_continue(&mut self) {
        match self.setup_step {
            tundra_ui::SetupStep::Language => {
                self.setup_step = tundra_ui::SetupStep::Timezone;
                self.setup_focused_field = tundra_ui::SetupField::TimezoneList;
                self.focused_component = ShellComponent::SetupTimezone;
                self.sync_setup_timezone_window();
            }
            tundra_ui::SetupStep::Timezone => {
                self.setup_step = tundra_ui::SetupStep::Admin;
                self.setup_focused_field = tundra_ui::SetupField::AdminUsername;
                self.focused_component = ShellComponent::SetupAdminUsername;
            }
            tundra_ui::SetupStep::Admin => {}
        }
        self.error_message = None;
        self.refresh_hit_map();
    }

    fn move_setup_admin_focus(&mut self, direction: i8) {
        let order = [
            (
                tundra_ui::SetupField::AdminUsername,
                ShellComponent::SetupAdminUsername,
            ),
            (
                tundra_ui::SetupField::AdminPassword,
                ShellComponent::SetupAdminPassword,
            ),
            (
                tundra_ui::SetupField::AdminPasswordConfirm,
                ShellComponent::SetupAdminPasswordConfirm,
            ),
            (
                tundra_ui::SetupField::PasswordHint,
                ShellComponent::SetupAdminHint,
            ),
            (tundra_ui::SetupField::Submit, ShellComponent::SetupSubmit),
        ];
        let next = match order
            .iter()
            .position(|(field, _)| *field == self.setup_focused_field)
        {
            Some(current) => {
                (current as isize + direction as isize).rem_euclid(order.len() as isize) as usize
            }
            None if direction < 0 => order.len().saturating_sub(1),
            None => 0,
        };
        let (field, component) = order[next];
        self.setup_focused_field = field;
        self.focused_component = component;
        self.error_message = None;
    }

    fn focus_setup_component(&mut self, component: ShellComponent) {
        if !setup_component_active_for_step(component, self.setup_step) {
            return;
        }

        let Some(field) = setup_field_for_component(component) else {
            return;
        };

        self.setup_focused_field = field;
        self.focused_component = component;
    }

    fn setup_active_key_component(&self) -> ShellComponent {
        match self.setup_step {
            tundra_ui::SetupStep::Language => ShellComponent::SetupLanguage,
            tundra_ui::SetupStep::Timezone => ShellComponent::SetupTimezone,
            tundra_ui::SetupStep::Admin => {
                let component = setup_component_for_field(self.setup_focused_field);
                if setup_component_active_for_step(component, self.setup_step) {
                    component
                } else {
                    ShellComponent::SetupAdminUsername
                }
            }
        }
    }

    fn append_setup_admin_char(&mut self, character: char) {
        match self.setup_focused_field {
            tundra_ui::SetupField::AdminUsername => self.setup_admin_username.push(character),
            tundra_ui::SetupField::AdminPassword => self.setup_admin_password.push(character),
            tundra_ui::SetupField::AdminPasswordConfirm => {
                self.setup_admin_password_confirm.push(character);
            }
            tundra_ui::SetupField::PasswordHint => self.setup_admin_password_hint.push(character),
            _ => {}
        }
        self.error_message = None;
    }

    fn setup_admin_backspace(&mut self) {
        match self.setup_focused_field {
            tundra_ui::SetupField::AdminUsername => {
                self.setup_admin_username.pop();
            }
            tundra_ui::SetupField::AdminPassword => {
                self.setup_admin_password.pop();
            }
            tundra_ui::SetupField::AdminPasswordConfirm => {
                self.setup_admin_password_confirm.pop();
            }
            tundra_ui::SetupField::PasswordHint => {
                self.setup_admin_password_hint.pop();
            }
            _ => {}
        }
        self.error_message = None;
    }

    fn activate_setup(&mut self, target: ShellComponent, coordinates: CellPosition) {
        match self.setup_step {
            tundra_ui::SetupStep::Language => {
                if target == ShellComponent::SetupLanguage
                    && let Some(index) = self.setup_language_index_at(coordinates)
                {
                    self.setup_selected_language_index = index;
                    self.error_message = None;
                }
                self.focus_setup_component(ShellComponent::SetupLanguage);
            }
            tundra_ui::SetupStep::Timezone => {
                if target == ShellComponent::SetupTimezone
                    && let Some(index) = self.setup_timezone_index_at(coordinates)
                {
                    self.setup_selected_timezone_index = index;
                    self.sync_setup_timezone_window();
                    self.error_message = None;
                }
                self.focus_setup_component(ShellComponent::SetupTimezone);
            }
            tundra_ui::SetupStep::Admin => {
                self.focus_setup_component(target);
                if target == ShellComponent::SetupSubmit {
                    self.submit_first_run_setup();
                }
            }
        }
    }

    fn activate_login(&mut self, target: ShellComponent, coordinates: CellPosition) {
        match target {
            ShellComponent::LoginUserList => {
                if let Some(index) = self.login_user_index_at(coordinates) {
                    self.select_login_user_at(index);
                }
                self.focused_component = ShellComponent::LoginPassword;
            }
            ShellComponent::LoginUsername => {
                self.focused_component = ShellComponent::LoginUserList;
            }
            ShellComponent::LoginPassword => {
                self.focused_component = ShellComponent::LoginPassword;
            }
            _ => {}
        }
    }

    fn setup_language_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let row = setup_language_list_row_at(self.terminal_size, coordinates)?;
        (row < tundra_ui::setup_language_options().len()).then_some(row)
    }

    fn setup_timezone_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let row = setup_timezone_list_row_at(self.terminal_size, coordinates)?;
        let count = tundra_ui::setup_timezone_options().len();
        if count == 0 {
            return None;
        }

        let visible_rows = self.setup_timezone_visible_row_count().min(count);
        let start = self
            .setup_timezone_window_start
            .min(count.saturating_sub(visible_rows));
        let index = start.saturating_add(row);
        (row < visible_rows && index < count).then_some(index)
    }

    fn login_user_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let row = login_user_list_row_at(self.terminal_size, coordinates)?;
        let count = self.login_users.len();
        if count == 0 {
            return None;
        }

        let visible_rows = self.login_user_visible_row_count().min(count);
        let start = self
            .login_user_window_start
            .min(count.saturating_sub(visible_rows));
        let index = start.saturating_add(row);
        (row < visible_rows && index < count).then_some(index)
    }

    fn selected_setup_language_value(&self) -> String {
        let options = tundra_ui::setup_language_options();
        setup_language_code_at(&options, self.setup_selected_language_index)
            .unwrap_or_else(|| "en-US".to_string())
    }

    fn selected_setup_timezone_value(&self) -> String {
        let options = tundra_ui::setup_timezone_options();
        setup_timezone_id_at(&options, self.setup_selected_timezone_index)
            .unwrap_or_else(|| "UTC".to_string())
    }

    fn submit_login(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };
        let Some(username) = self.selected_login_username().map(str::to_string) else {
            self.error_message = Some("No user selected".to_string());
            self.notify_status("Login failed");
            return;
        };
        let password_hint = self.selected_login_password_hint().map(str::to_string);
        let mut sessions = SessionService::new(storage);
        match sessions.login(&username, &self.login_password) {
            Ok(session) => self.complete_login(session),
            Err(error) => {
                self.login_password_visible_until = None;
                self.error_message = Some(login_error_message(&error, password_hint.as_deref()));
                self.notify_status("Login failed");
                let _ = self.refresh_login_users_from_storage();
            }
        }
    }

    fn submit_bootstrap_admin(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };
        let users = UserService::with_debug_policy(storage.clone(), self.debug_policy);
        match users.bootstrap_admin(&self.bootstrap_username, &self.bootstrap_password) {
            Ok(_) => {
                self.login_username = self.bootstrap_username.clone();
                self.login_password = self.bootstrap_password.clone();
                let mut sessions = SessionService::new(storage);
                match sessions.login(&self.login_username, &self.login_password) {
                    Ok(session) => self.complete_login(session),
                    Err(error) => {
                        self.error_message = Some(format_core_error(&error));
                        self.notify_status("Login failed");
                    }
                }
            }
            Err(error) => {
                self.error_message = Some(format_core_error(&error));
                self.notify_status("Admin bootstrap failed");
            }
        }
    }

    fn submit_first_run_setup(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };

        let username = self.setup_admin_username.trim().to_string();
        let password = self.setup_admin_password.clone();
        if password != self.setup_admin_password_confirm {
            self.setup_focused_field = tundra_ui::SetupField::AdminPasswordConfirm;
            self.focused_component = ShellComponent::SetupAdminPasswordConfirm;
            self.error_message = Some("Passwords do not match".to_string());
            self.notify_status("Setup incomplete");
            return;
        }

        let hint = self.setup_admin_password_hint.trim().to_string();
        let hint = (!hint.is_empty()).then_some(hint);

        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.error_message = Some(error.to_string());
                self.notify_status("Setup failed");
                return;
            }
        };
        config.language = self.selected_setup_language_value();
        config.timezone = self.selected_setup_timezone_value();
        let selected_timezone = config.timezone.clone();
        if let Err(error) = storage.save_config(&config) {
            self.error_message = Some(error.to_string());
            self.notify_status("Setup failed");
            return;
        }
        self.set_clock_timezone(Some(selected_timezone));

        let users = UserService::with_debug_policy(storage.clone(), self.debug_policy);
        match users.bootstrap_admin_with_hint(&username, &password, hint.as_deref()) {
            Ok(account) => {
                let mut sessions = SessionService::new(storage);
                match sessions.login(&account.username, &password) {
                    Ok(session) => {
                        self.setup_admin_password.clear();
                        self.setup_admin_password_confirm.clear();
                        self.complete_login(session);
                    }
                    Err(error) => {
                        self.setup_admin_password.clear();
                        self.setup_admin_password_confirm.clear();
                        self.error_message = Some(format_core_error(&error));
                        self.notify_status("Login failed");
                    }
                }
            }
            Err(error) => {
                self.error_message = Some(format_core_error(&error));
                self.notify_status("Setup failed");
            }
        }
    }

    fn complete_login(&mut self, session: AuthSession) {
        self.guest_mode = false;
        self.auth_session = Some(session.clone());
        self.login_username = session.username.clone();
        self.login_password.clear();
        self.login_password_visible_until = None;
        self.bootstrap_password.clear();
        self.setup_admin_password.clear();
        self.setup_admin_password_confirm.clear();
        self.error_message = None;
        self.notify_status(format!("Signed in as {}", session.username));
        self.home_mode = ShellHomeMode::User;

        if self.requested_debug_mode {
            let permission = PermissionService::new(self.debug_policy).authorize(
                Some(&session),
                PermissionAction::EnterDebugMode,
                None,
            );
            if let Some(storage) = self.storage_manager.clone() {
                let audit = AuditService::new(storage);
                if permission.allowed {
                    self.home_mode = ShellHomeMode::Debug;
                    let _ = audit.record(
                        Some(&session),
                        PermissionAction::EnterDebugMode,
                        None,
                        AuditOutcome::Success,
                        Some("debug_entered"),
                    );
                } else {
                    let reason = permission
                        .reason
                        .as_deref()
                        .unwrap_or("debug_policy_denied");
                    let _ = audit.record(
                        Some(&session),
                        PermissionAction::EnterDebugMode,
                        None,
                        AuditOutcome::Denied,
                        Some(reason),
                    );
                    self.notify_toast("Debug mode denied");
                }
            }
        }

        self.screen_stack = vec![ShellScreen::Home];
        self.focused_component = ShellComponent::Home;
        self.active_popup = None;
        if session.role != UserRole::Guest {
            self.load_clock_for_session(&session);
        }
        self.refresh_hit_map();
    }

    fn complete_guest_login(&mut self) {
        self.auth_session = None;
        self.guest_mode = true;
        self.login_password.clear();
        self.login_password_visible_until = None;
        self.bootstrap_password.clear();
        self.setup_admin_password.clear();
        self.setup_admin_password_confirm.clear();
        self.error_message = None;
        self.home_mode = ShellHomeMode::User;
        self.selected_home_entry_index = 0;
        self.clock_scheduler = None;
        self.clock_selected_entry_id = None;
        self.clock_entry_window_start = 0;
        self.clock_create_state = None;
        self.clock_profile_pending_sync = None;
        self.screen_stack = vec![ShellScreen::Home];
        self.focused_component = ShellComponent::Home;
        self.active_popup = None;
        self.notify_status("Signed in as Guest");
        self.refresh_hit_map();
    }

    fn open_user_management(&mut self) {
        if self.is_strict_guest() {
            self.error_message = None;
            self.notify_status("Guest access is read-only");
            return;
        }
        if self.auth_session.is_none() {
            self.error_message = Some("Login required".to_string());
            return;
        };

        if self.refresh_user_management() {
            self.screen_stack.push(ShellScreen::UserManagement);
            self.focused_component = ShellComponent::UserManagement;
            self.user_management_mode = UserManagementMode::Browse;
            self.user_management_focus = UserManagementPageFocus::UserList;
            self.ensure_user_management_selection_visible();
            let status = if self.can_manage_all_users() {
                "User Management"
            } else {
                "User Profile"
            };
            self.notify_status(status);
            self.refresh_hit_map();
        }
    }
}
