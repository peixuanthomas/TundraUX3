use super::super::*;
impl ShellSession {
    pub(in crate::session) fn refresh_user_management(&mut self) -> bool {
        let Some(storage) = self.storage_manager.clone() else {
            self.report_user_management_refresh_error("Storage unavailable".to_string());
            return false;
        };
        let Some(session) = self.app.auth_session().cloned() else {
            self.report_user_management_refresh_error("Login required".to_string());
            return false;
        };
        let users = match UserService::with_debug_policy(storage, self.debug_policy)
            .list_accessible_users(&session)
        {
            Ok(users) => users,
            Err(error) => {
                self.report_user_management_refresh_error(format_core_error(&error));
                return false;
            }
        };
        let selected_username = self.selected_managed_username();
        self.app
            .dispatch_at(app::AppCommand::SetManagedUsers(users), Instant::now());
        if self.app.managed_users().is_empty() {
            self.user_management_selected = 0;
            self.user_management_window_start = 0;
            self.user_management_focus = UserManagementPageFocus::UserList;
        } else if let Some(username) = selected_username {
            self.user_management_selected = self
                .app
                .managed_users()
                .iter()
                .position(|user| user.username.eq_ignore_ascii_case(&username))
                .unwrap_or_else(|| {
                    self.user_management_selected
                        .min(self.app.managed_users().len() - 1)
                });
        } else {
            self.user_management_selected = self
                .user_management_selected
                .min(self.app.managed_users().len() - 1);
        }
        self.ensure_user_management_selection_visible();
        self.normalize_user_management_focus();
        self.resolve_user_management_refresh_alert();
        true
    }

    pub(in crate::session) fn resolve_user_management_refresh_alert(&mut self) {
        let resolved_message = self
            .notification_alert_message_for_key(USER_MANAGEMENT_REFRESH_ALERT_KEY)
            .map(str::to_string);
        if self.user_management_message.as_ref() == resolved_message.as_ref() {
            self.user_management_message = None;
        }
        if self.error_message.as_ref() == resolved_message.as_ref() {
            self.error_message = None;
        }
        self.resolve_notification_alert(USER_MANAGEMENT_REFRESH_ALERT_KEY);
    }

    pub(in crate::session) fn report_user_management_refresh_error(&mut self, message: String) {
        self.error_message = Some(message.clone());
        self.user_management_message = Some(message.clone());
        self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
        self.notify_alert_with_key(
            USER_MANAGEMENT_REFRESH_ALERT_KEY,
            message,
            ui::NotificationTone::Error,
        );
    }

    pub(in crate::session) fn select_user_management_row(&mut self, direction: isize) {
        if self.app.managed_users().is_empty() {
            return;
        }
        let last = self.app.managed_users().len().saturating_sub(1) as isize;
        let next = (self.user_management_selected as isize + direction).clamp(0, last);
        self.user_management_selected = next as usize;
        self.user_management_focus = UserManagementPageFocus::UserList;
        self.ensure_user_management_selection_visible();
    }

    pub(in crate::session) fn select_user_management_edge(&mut self, last: bool) {
        if self.app.managed_users().is_empty() {
            return;
        }
        self.user_management_selected = if last {
            self.app.managed_users().len().saturating_sub(1)
        } else {
            0
        };
        self.user_management_focus = UserManagementPageFocus::UserList;
        self.ensure_user_management_selection_visible();
    }

    pub(in crate::session) fn select_user_management_page(&mut self, direction: isize) {
        let page = self.user_management_visible_row_count().max(1) as isize;
        self.select_user_management_row(direction.saturating_mul(page));
    }

    pub(in crate::session) fn begin_create_managed_user(&mut self) {
        if !self.can_manage_all_users() {
            return;
        }
        self.user_management_mode = UserManagementMode::Create(UserManagementCreateForm {
            username: String::new(),
            display_name: String::new(),
            password: String::new(),
            role: UserRole::User,
            focused_field: UserManagementFormField::Username,
        });
        self.user_management_message = None;
        self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
    }

    pub(in crate::session) fn begin_edit_selected_user_info(&mut self) {
        if let Some(user) = self
            .app
            .managed_users()
            .get(self.user_management_selected)
            .cloned()
        {
            self.user_management_mode = UserManagementMode::EditInfo(UserManagementInfoForm {
                username: user.username,
                display_name: user.display_name,
                focused_field: UserManagementFormField::DisplayName,
            });
            self.user_management_message = None;
            self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
        }
    }

    pub(in crate::session) fn begin_set_selected_password(&mut self) {
        if let Some(username) = self.selected_managed_username() {
            self.user_management_mode = UserManagementMode::Password(UserManagementPasswordForm {
                username,
                password: String::new(),
                focused_field: UserManagementFormField::Password,
            });
            self.user_management_message = None;
            self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
        }
    }

    pub(in crate::session) fn disable_selected_user(&mut self) {
        if let Some(username) = self.selected_managed_username() {
            let current_user = self.is_current_username(&username);
            let disabled = self.run_selected_user_operation("Disabled", |service, session| {
                service.disable_user(session, &username)
            });
            if disabled && current_user {
                self.return_to_login("Account disabled");
            }
        }
    }

    pub(in crate::session) fn unlock_selected_user(&mut self) {
        if let Some(username) = self.selected_managed_username() {
            self.run_selected_user_operation("Enabled/unlocked", |service, session| {
                service.enable_user(session, &username)
            });
        }
    }

    pub(in crate::session) fn reset_selected_password(&mut self) {
        self.begin_set_selected_password();
    }

    pub(in crate::session) fn cycle_selected_role(&mut self) {
        if let Some(username) = self.selected_managed_username() {
            let next_role = self
                .app
                .managed_users()
                .get(self.user_management_selected)
                .map(|user| match user.role {
                    UserRole::User | UserRole::Guest => UserRole::Admin,
                    UserRole::Admin => UserRole::User,
                })
                .unwrap_or(UserRole::User);
            let changed = self
                .run_selected_user_operation("Changed role for", |service, session| {
                    service.change_role(session, &username, next_role)
                });
            if changed {
                self.sync_current_session_role();
                let _refresh_succeeded = self.refresh_user_management();
            }
        }
    }

    pub(in crate::session) fn run_selected_user_operation(
        &mut self,
        success_prefix: &'static str,
        operation: impl FnOnce(UserService, &AuthSession) -> Result<(), CoreError>,
    ) -> bool {
        let Some(storage) = self.storage_manager.clone() else {
            return false;
        };
        let Some(session) = self.app.auth_session() else {
            return false;
        };
        let username = self
            .selected_managed_username()
            .unwrap_or_else(|| "user".to_string());
        let service = UserService::with_debug_policy(storage, self.debug_policy);
        let succeeded = match operation(service, session) {
            Ok(()) => {
                self.user_management_message = Some(format!("{success_prefix} {username}"));
                self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                true
            }
            Err(error) => {
                self.user_management_message = Some(format_core_error(&error));
                self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                false
            }
        };
        let _refresh_succeeded = self.refresh_user_management();
        succeeded
    }

    pub(in crate::session) fn submit_user_management_form(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let Some(session) = self.app.auth_session() else {
            return;
        };
        let service = UserService::with_debug_policy(storage, self.debug_policy);
        match self.user_management_mode.clone() {
            UserManagementMode::Browse => {}
            UserManagementMode::Create(form) => {
                let username = form.username.trim().to_string();
                let result = service.create_user(
                    session,
                    &form.username,
                    &form.display_name,
                    form.role,
                    &form.password,
                );
                self.user_management_message = Some(match result {
                    Ok(account) => {
                        self.user_management_mode = UserManagementMode::Browse;
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                        format!("Created {}", account.username)
                    }
                    Err(error) => {
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                        format_core_error(&error)
                    }
                });
                if !self.refresh_user_management() {
                    return;
                }
                if !username.is_empty() {
                    self.select_managed_username(&username);
                }
                return;
            }
            UserManagementMode::EditInfo(form) => {
                let result = service.update_user_info(session, &form.username, &form.display_name);
                self.user_management_message = Some(match result {
                    Ok(account) => {
                        self.user_management_mode = UserManagementMode::Browse;
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                        format!("Updated {}", account.username)
                    }
                    Err(error) => {
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                        format_core_error(&error)
                    }
                });
            }
            UserManagementMode::Password(form) => {
                let result = service.set_user_password(session, &form.username, &form.password);
                self.user_management_message = Some(match result {
                    Ok(()) => {
                        self.user_management_mode = UserManagementMode::Browse;
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                        format!("Updated password for {}", form.username)
                    }
                    Err(error) => {
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                        format_core_error(&error)
                    }
                });
            }
        }
        let _refresh_succeeded = self.refresh_user_management();
    }

    pub(in crate::session) fn delete_selected_user(&mut self) {
        let Some(username) = self.selected_managed_username() else {
            return;
        };
        let deleted_user_id = self
            .app
            .managed_users()
            .iter()
            .find(|user| user.username.eq_ignore_ascii_case(&username))
            .map(|user| user.id.clone());
        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let Some(session) = self.app.auth_session() else {
            return;
        };
        let deleting_current_user = self.is_current_username(&username);
        let deleted = match UserService::with_debug_policy(storage.clone(), self.debug_policy)
            .delete_user(session, &username)
        {
            Ok(()) => {
                self.user_management_message = Some(format!("Deleted {username}"));
                self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                true
            }
            Err(error) => {
                self.user_management_message = Some(format_core_error(&error));
                self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                false
            }
        };
        if deleted && let Some(user_id) = deleted_user_id {
            match storage.load_clock() {
                Ok(mut document) => {
                    document.profiles.remove(&user_id);
                    if let Err(error) = storage.save_clock(&document) {
                        self.report_clock_storage_error(error.to_string());
                    }
                }
                Err(error) => self.report_clock_storage_error(error.to_string()),
            }
        }
        if deleted && deleting_current_user {
            self.return_to_login("Account deleted");
            return;
        }
        let _refresh_succeeded = self.refresh_user_management();
    }

    pub(in crate::session) fn append_user_management_char(&mut self, character: char) {
        match &mut self.user_management_mode {
            UserManagementMode::Create(form) => match form.focused_field {
                UserManagementFormField::Username => form.username.push(character),
                UserManagementFormField::DisplayName => form.display_name.push(character),
                UserManagementFormField::Password => form.password.push(character),
                UserManagementFormField::Role
                | UserManagementFormField::Submit
                | UserManagementFormField::Cancel => {}
            },
            UserManagementMode::EditInfo(form)
                if form.focused_field == UserManagementFormField::DisplayName =>
            {
                form.display_name.push(character);
            }
            UserManagementMode::Password(form)
                if form.focused_field == UserManagementFormField::Password =>
            {
                form.password.push(character);
            }
            UserManagementMode::EditInfo(_) | UserManagementMode::Password(_) => {}
            UserManagementMode::Browse => {}
        }
    }

    pub(in crate::session) fn user_management_backspace(&mut self) {
        match &mut self.user_management_mode {
            UserManagementMode::Create(form) => match form.focused_field {
                UserManagementFormField::Username => {
                    form.username.pop();
                }
                UserManagementFormField::DisplayName => {
                    form.display_name.pop();
                }
                UserManagementFormField::Password => {
                    form.password.pop();
                }
                UserManagementFormField::Role
                | UserManagementFormField::Submit
                | UserManagementFormField::Cancel => {}
            },
            UserManagementMode::EditInfo(form)
                if form.focused_field == UserManagementFormField::DisplayName =>
            {
                form.display_name.pop();
            }
            UserManagementMode::Password(form)
                if form.focused_field == UserManagementFormField::Password =>
            {
                form.password.pop();
            }
            UserManagementMode::EditInfo(_) | UserManagementMode::Password(_) => {}
            UserManagementMode::Browse => {}
        }
    }

    pub(in crate::session) fn move_user_management_form_focus(&mut self, direction: i8) {
        let fields: &[UserManagementFormField] = match self.user_management_mode {
            UserManagementMode::Create(_) => &[
                UserManagementFormField::Username,
                UserManagementFormField::DisplayName,
                UserManagementFormField::Role,
                UserManagementFormField::Password,
                UserManagementFormField::Submit,
                UserManagementFormField::Cancel,
            ],
            UserManagementMode::EditInfo(_) => &[
                UserManagementFormField::DisplayName,
                UserManagementFormField::Submit,
                UserManagementFormField::Cancel,
            ],
            UserManagementMode::Password(_) => &[
                UserManagementFormField::Password,
                UserManagementFormField::Submit,
                UserManagementFormField::Cancel,
            ],
            UserManagementMode::Browse => return,
        };
        let current = self.user_management_form_field();
        let index = fields
            .iter()
            .position(|field| Some(*field) == current)
            .unwrap_or(0);
        let next = (index as isize + direction as isize).rem_euclid(fields.len() as isize) as usize;
        self.set_user_management_form_field(fields[next]);
    }

    pub(in crate::session) fn cancel_user_management_form(&mut self) {
        if self.user_management_mode != UserManagementMode::Browse {
            self.user_management_mode = UserManagementMode::Browse;
            self.user_management_message = Some("Cancelled".to_string());
            self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
            self.ensure_user_management_selection_visible();
        }
    }

    pub(in crate::session) fn user_management_form_field(&self) -> Option<UserManagementFormField> {
        match &self.user_management_mode {
            UserManagementMode::Browse => None,
            UserManagementMode::Create(form) => Some(form.focused_field),
            UserManagementMode::EditInfo(form) => Some(form.focused_field),
            UserManagementMode::Password(form) => Some(form.focused_field),
        }
    }

    pub(in crate::session) fn set_user_management_form_field(
        &mut self,
        field: UserManagementFormField,
    ) {
        match &mut self.user_management_mode {
            UserManagementMode::Browse => {}
            UserManagementMode::Create(form) => form.focused_field = field,
            UserManagementMode::EditInfo(form) => form.focused_field = field,
            UserManagementMode::Password(form) => form.focused_field = field,
        }
    }

    pub(in crate::session) fn set_user_management_form_focus(
        &mut self,
        field: ui::UserManagementField,
    ) {
        let field = from_ui_user_management_field(field);
        let valid = match self.user_management_mode {
            UserManagementMode::Browse => false,
            UserManagementMode::Create(_) => true,
            UserManagementMode::EditInfo(_) => matches!(
                field,
                UserManagementFormField::DisplayName
                    | UserManagementFormField::Submit
                    | UserManagementFormField::Cancel
            ),
            UserManagementMode::Password(_) => matches!(
                field,
                UserManagementFormField::Password
                    | UserManagementFormField::Submit
                    | UserManagementFormField::Cancel
            ),
        };
        if valid {
            self.set_user_management_form_field(field);
        }
    }

    pub(in crate::session) fn toggle_user_management_form_role(&mut self) {
        if let UserManagementMode::Create(form) = &mut self.user_management_mode {
            form.role = if form.role == UserRole::Admin {
                UserRole::User
            } else {
                UserRole::Admin
            };
        }
    }

    pub(in crate::session) fn move_user_management_page_focus(&mut self, direction: i8) {
        let order = self.user_management_focus_order();
        if order.is_empty() {
            self.user_management_focus = UserManagementPageFocus::UserList;
            return;
        }
        let current = order
            .iter()
            .position(|focus| *focus == self.user_management_focus)
            .unwrap_or(0);
        let next = (current as isize + direction as isize).rem_euclid(order.len() as isize);
        self.user_management_focus = order[next as usize];
    }

    pub(in crate::session) fn user_management_focus_order(&self) -> Vec<UserManagementPageFocus> {
        let mut order = vec![UserManagementPageFocus::UserList];
        order.extend(
            self.user_management_action_view_models()
                .into_iter()
                .filter(|action| action.enabled)
                .map(|action| UserManagementPageFocus::Action(action.action)),
        );
        order
    }

    pub(in crate::session) fn normalize_user_management_focus(&mut self) {
        if !self
            .user_management_focus_order()
            .contains(&self.user_management_focus)
        {
            self.user_management_focus = UserManagementPageFocus::UserList;
        }
    }

    pub(in crate::session) fn focus_user_management_action(
        &mut self,
        action: ui::UserManagementAction,
    ) {
        if self.user_management_action_enabled(action) {
            self.user_management_focus = UserManagementPageFocus::Action(action);
        }
    }

    pub(in crate::session) fn user_management_action_enabled(
        &self,
        action: ui::UserManagementAction,
    ) -> bool {
        self.user_management_action_view_models()
            .iter()
            .find(|model| model.action == action)
            .is_some_and(|model| model.enabled)
    }

    pub(in crate::session) fn activate_focused_user_management_control(&mut self) {
        match self.user_management_focus {
            UserManagementPageFocus::UserList => {}
            UserManagementPageFocus::Action(action) => {
                self.activate_user_management_action(action);
            }
        }
    }

    pub(in crate::session) fn activate_user_management_action(
        &mut self,
        action: ui::UserManagementAction,
    ) {
        use ui::UserManagementAction;

        let action_model = self
            .user_management_action_view_models()
            .into_iter()
            .find(|model| model.action == action);
        let Some(action_model) = action_model else {
            return;
        };
        if !action_model.enabled {
            if let Some(reason) = action_model.disabled_reason {
                self.user_management_message = Some(reason);
                self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                self.ensure_user_management_selection_visible();
            }
            return;
        }

        match action {
            UserManagementAction::NewUser => self.begin_create_managed_user(),
            UserManagementAction::EditInfo => self.begin_edit_selected_user_info(),
            UserManagementAction::SetPassword => self.begin_set_selected_password(),
            UserManagementAction::ToggleEnabled => {
                let should_disable = self
                    .app
                    .managed_users()
                    .get(self.user_management_selected)
                    .is_some_and(|user| user.enabled && !user_is_locked(user));
                if should_disable {
                    self.disable_selected_user();
                } else {
                    self.unlock_selected_user();
                }
            }
            UserManagementAction::ToggleRole => self.cycle_selected_role(),
            UserManagementAction::Delete => self.request_delete_selected_user(),
            UserManagementAction::Back => self.close_user_management(),
        }
        self.normalize_user_management_focus();
    }

    pub(in crate::session) fn request_delete_selected_user(&mut self) {
        use ui::UserManagementAction;

        if !self.user_management_action_enabled(UserManagementAction::Delete) {
            self.activate_user_management_action(UserManagementAction::Delete);
            return;
        }
        let Some(username) = self.selected_managed_username() else {
            return;
        };
        let deleting_current_user = self.is_current_username(&username);
        let title = if deleting_current_user {
            "Delete your account"
        } else {
            "Delete user"
        };
        let message = if deleting_current_user {
            format!("Delete {username}? You will be signed out immediately.")
        } else {
            format!("Delete {username}? This action cannot be undone.")
        };
        self.notify_modal_with_options(
            ShellNotification::modal(
                title,
                message,
                ui::NotificationTone::Warning,
                vec![
                    ShellNotificationAction::new("delete", "Delete")
                        .with_shortcut(InputKey::Char('x'))
                        .with_follow_up(ShellCommand::DeleteManagedUser),
                    ShellNotificationAction::new("cancel", "Cancel")
                        .with_shortcut(InputKey::Escape)
                        .cancel(),
                ],
            )
            .with_selected_action(1)
            .with_key(USER_MANAGEMENT_DELETE_NOTIFICATION_KEY)
            .with_component(ShellComponent::NotificationDialog),
        );
    }

    pub(in crate::session) fn selected_is_last_enabled_admin(&self) -> bool {
        let Some(selected) = self.app.managed_users().get(self.user_management_selected) else {
            return false;
        };
        selected.enabled
            && selected.role == UserRole::Admin
            && self
                .app
                .managed_users()
                .iter()
                .filter(|user| user.enabled && user.role == UserRole::Admin)
                .count()
                <= 1
    }

    pub(in crate::session) fn user_management_visible_row_count(&self) -> usize {
        self.user_management_layout()
            .map(|layout| layout.visible_capacity)
            .unwrap_or(0)
    }

    pub(in crate::session) fn ensure_user_management_selection_visible(&mut self) {
        let count = self.app.managed_users().len();
        if count == 0 {
            self.user_management_selected = 0;
            self.user_management_window_start = 0;
            return;
        }
        self.user_management_selected = self.user_management_selected.min(count - 1);
        let capacity = self.user_management_visible_row_count().min(count);
        if capacity == 0 {
            self.user_management_window_start = 0;
            return;
        }
        let max_start = count.saturating_sub(capacity);
        self.user_management_window_start = self.user_management_window_start.min(max_start);
        if self.user_management_selected < self.user_management_window_start {
            self.user_management_window_start = self.user_management_selected;
        } else if self.user_management_selected
            >= self.user_management_window_start.saturating_add(capacity)
        {
            self.user_management_window_start = self
                .user_management_selected
                .saturating_add(1)
                .saturating_sub(capacity);
        }
    }

    pub(in crate::session) fn user_management_layout(&self) -> Option<ui::UserManagementLayout> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return None;
        };
        Some(ui::user_management_layout(
            main,
            &self.to_user_management_view_model(),
        ))
    }

    pub(in crate::session) fn close_user_management(&mut self) {
        self.user_management_mode = UserManagementMode::Browse;
        self.resolve_user_management_refresh_alert();
        self.pop_to_home();
        self.notify_status("Ready");
        self.refresh_hit_map();
    }

    pub(in crate::session) fn selected_managed_username(&self) -> Option<String> {
        self.app
            .managed_users()
            .get(self.user_management_selected)
            .map(|user| user.username.clone())
    }

    pub(in crate::session) fn select_managed_username(&mut self, username: &str) {
        if let Some(index) = self
            .app
            .managed_users()
            .iter()
            .position(|user| user.username.eq_ignore_ascii_case(username))
        {
            self.user_management_selected = index;
        }
    }

    pub(in crate::session) fn is_current_username(&self, username: &str) -> bool {
        self.app
            .auth_session()
            .map(|session| session.username.eq_ignore_ascii_case(username))
            .unwrap_or(false)
    }

    pub(in crate::session) fn sync_current_session_role(&mut self) {
        let Some(mut session) = self.app.auth_session().cloned() else {
            return;
        };
        let Some(role) = self
            .app
            .managed_users()
            .iter()
            .find(|user| user.username.eq_ignore_ascii_case(&session.username))
            .map(|user| user.role)
        else {
            return;
        };
        session.role = role;
        self.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(session)),
            Instant::now(),
        );
    }
}
