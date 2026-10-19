mod input;
pub(in crate::session) mod tasks;
pub(in crate::session) mod users;
use crate::session::*;
impl ShellSession {
    pub(in crate::session) fn append_auth_char(&mut self, character: char) {
        match self.pages.common.focused_component {
            ShellComponent::LoginPassword => self.pages.login.login_password.push(character),
            ShellComponent::BootstrapUsername => {
                self.pages.bootstrap.bootstrap_username.push(character)
            }
            ShellComponent::BootstrapPassword => {
                self.pages.bootstrap.bootstrap_password.push(character)
            }
            _ => {}
        }
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn reset_login_idle_deadline_at(&mut self, now: Instant) {
        self.pages.login.login_idle_deadline = now + LOGIN_IDLE_TIMEOUT;
    }

    pub(in crate::session) fn login_idle_tracking_active(&self) -> bool {
        self.identity_backend != identity::IdentityBackend::Linux
            && self.screen_stack().contains(&ShellScreen::Login)
            && self.app.auth_session().is_none()
    }

    pub(in crate::session) fn expire_login_password_visibility_at(&mut self, now: Instant) {
        if self
            .pages
            .login
            .login_password_visible_until
            .is_some_and(|deadline| now >= deadline)
        {
            self.pages.login.login_password_visible_until = None;
        }
    }

    pub(in crate::session) fn login_password_is_visible_at(&self, now: Instant) -> bool {
        self.pages
            .login
            .login_password_visible_until
            .is_some_and(|deadline| now < deadline)
    }

    pub(in crate::session) fn auth_poll_timeout(
        &self,
        now: Instant,
        fallback: Duration,
    ) -> Duration {
        let mut timeout = fallback;
        if self.login_idle_tracking_active() {
            timeout = timeout.min(
                self.pages
                    .login
                    .login_idle_deadline
                    .saturating_duration_since(now),
            );
        }
        if let Some(deadline) = self.pages.login.login_password_visible_until {
            timeout = timeout.min(deadline.saturating_duration_since(now));
        }
        timeout
    }

    pub(in crate::session) fn toggle_login_password_visibility_at(&mut self, now: Instant) {
        if self.login_password_is_visible_at(now) {
            self.pages.login.login_password_visible_until = None;
        } else {
            self.pages.login.login_password_visible_until = Some(now + PASSWORD_REVEAL_DURATION);
        }
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn prepare_return_to_lockscreen(&mut self) {
        if self.identity_backend == identity::IdentityBackend::Linux {
            return;
        }
        self.pages.login.login_password.clear();
        self.pages.login.login_password_visible_until = None;
        self.pages.common.error_message = None;
        self.return_to_lockscreen_requested = true;
    }

    pub(in crate::session) fn auth_backspace(&mut self) {
        match self.pages.common.focused_component {
            ShellComponent::LoginPassword => {
                self.pages.login.login_password.pop();
            }
            ShellComponent::BootstrapUsername => {
                self.pages.bootstrap.bootstrap_username.pop();
            }
            ShellComponent::BootstrapPassword => {
                self.pages.bootstrap.bootstrap_password.pop();
            }
            _ => {}
        }
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn selected_login_username(&self) -> Option<&str> {
        self.login_users
            .get(self.pages.login.login_selected_user)
            .map(|user| user.username.as_str())
    }

    pub(in crate::session) fn selected_login_password_hint(&self) -> Option<&str> {
        self.login_users
            .get(self.pages.login.login_selected_user)?
            .password_hint
            .as_deref()
    }

    pub(in crate::session) fn sync_login_selection(&mut self) {
        if self.login_users.is_empty() {
            self.pages.login.login_selected_user = 0;
            self.pages.login.login_user_window_start = 0;
            self.pages.login.login_username.clear();
            return;
        }

        self.pages.login.login_selected_user = self
            .pages
            .login
            .login_selected_user
            .min(self.login_users.len() - 1);
        self.pages.login.login_username = self.login_users[self.pages.login.login_selected_user]
            .username
            .clone();
        self.sync_login_user_window();
    }

    pub(in crate::session) fn sync_login_user_window(&mut self) {
        let count = self.login_users.len();
        if count == 0 {
            self.pages.login.login_user_window_start = 0;
            return;
        }

        let visible_rows = self.login_user_visible_row_count().min(count).max(1);
        let max_start = count.saturating_sub(visible_rows);
        self.pages.login.login_user_window_start =
            self.pages.login.login_user_window_start.min(max_start);

        if self.pages.login.login_selected_user < self.pages.login.login_user_window_start {
            self.pages.login.login_user_window_start = self.pages.login.login_selected_user;
        }

        let window_end = self
            .pages
            .login
            .login_user_window_start
            .saturating_add(visible_rows);
        if self.pages.login.login_selected_user >= window_end {
            self.pages.login.login_user_window_start = self
                .pages
                .login
                .login_selected_user
                .saturating_add(1)
                .saturating_sub(visible_rows)
                .min(max_start);
        }
    }

    pub(in crate::session) fn login_user_visible_row_count(&self) -> usize {
        let model = self.to_login_view_model();
        let terminal = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let main = match self.shell_layout_for(terminal) {
            ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
        };
        ui::login_user_list_visible_rows(ui::login_viewport(main, &model).content).max(1)
    }

    pub(in crate::session) fn select_login_user_delta(&mut self, delta: isize) {
        if self.login_users.is_empty() {
            self.sync_login_selection();
            return;
        }

        let current = self
            .pages
            .login
            .login_selected_user
            .min(self.login_users.len() - 1) as isize;
        self.pages.login.login_selected_user =
            (current + delta).clamp(0, self.login_users.len() as isize - 1) as usize;
        self.pages.login.login_password.clear();
        self.pages.login.login_password_visible_until = None;
        self.pages.common.error_message = None;
        self.sync_login_selection();
    }

    pub(in crate::session) fn select_first_login_user(&mut self) {
        self.pages.login.login_selected_user = 0;
        self.pages.login.login_password.clear();
        self.pages.login.login_password_visible_until = None;
        self.pages.common.error_message = None;
        self.sync_login_selection();
    }

    pub(in crate::session) fn select_last_login_user(&mut self) {
        if self.login_users.is_empty() {
            self.sync_login_selection();
            return;
        }

        self.pages.login.login_selected_user = self.login_users.len() - 1;
        self.pages.login.login_password.clear();
        self.pages.login.login_password_visible_until = None;
        self.pages.common.error_message = None;
        self.sync_login_selection();
    }

    pub(in crate::session) fn select_login_user_at(&mut self, index: usize) {
        if index >= self.login_users.len() {
            return;
        }

        self.pages.login.login_selected_user = index;
        self.pages.login.login_password.clear();
        self.pages.login.login_password_visible_until = None;
        self.pages.common.error_message = None;
        self.sync_login_selection();
    }

    pub(in crate::session) fn refresh_login_users_from_storage(&mut self) -> Result<(), CoreError> {
        let Some(storage) = self.storage_manager.clone() else {
            return Ok(());
        };
        let previous_username = self.selected_login_username().map(str::to_string);
        let users = UserService::new(storage)
            .with_backend(self.identity_backend)
            .login_records()?;
        self.login_users = users.iter().map(ShellLoginUser::from_record).collect();
        self.pages.login.login_selected_user = previous_username
            .as_deref()
            .and_then(|username| {
                self.login_users.iter().position(|user| {
                    self.identity_backend
                        .usernames_match(&user.username, username)
                })
            })
            .unwrap_or_else(|| default_login_user_index(&self.login_users));
        self.sync_login_selection();
        Ok(())
    }

    pub(in crate::session) fn setup_next_language(&mut self) {
        let count = self.language_options().len();
        if count == 0 {
            return;
        }
        self.pages.setup.setup_selected_language_index =
            (self.pages.setup.setup_selected_language_index + 1) % count;
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn setup_previous_language(&mut self) {
        let count = self.language_options().len();
        if count == 0 {
            return;
        }
        self.pages.setup.setup_selected_language_index =
            (self.pages.setup.setup_selected_language_index + count - 1) % count;
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn setup_select_timezone_delta(&mut self, delta: isize) {
        let count = app::setup_timezone_options().len();
        if count == 0 {
            self.pages.setup.setup_timezone_window_start = 0;
            return;
        }

        let current = self
            .pages
            .setup
            .setup_selected_timezone_index
            .min(count - 1) as isize;
        let next = (current + delta).clamp(0, count as isize - 1) as usize;
        self.pages.setup.setup_selected_timezone_index = next;
        self.sync_setup_timezone_window();
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn setup_select_first_timezone(&mut self) {
        self.pages.setup.setup_selected_timezone_index = 0;
        self.sync_setup_timezone_window();
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn setup_select_last_timezone(&mut self) {
        let count = app::setup_timezone_options().len();
        if count == 0 {
            return;
        }
        self.pages.setup.setup_selected_timezone_index = count - 1;
        self.sync_setup_timezone_window();
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn sync_setup_timezone_window(&mut self) {
        let count = app::setup_timezone_options().len();
        if count == 0 {
            self.pages.setup.setup_selected_timezone_index = 0;
            self.pages.setup.setup_timezone_window_start = 0;
            return;
        }

        self.pages.setup.setup_selected_timezone_index = self
            .pages
            .setup
            .setup_selected_timezone_index
            .min(count - 1);
        let visible_rows = self.setup_timezone_visible_row_count().min(count).max(1);
        let max_start = count.saturating_sub(visible_rows);
        self.pages.setup.setup_timezone_window_start =
            self.pages.setup.setup_timezone_window_start.min(max_start);

        if self.pages.setup.setup_selected_timezone_index
            < self.pages.setup.setup_timezone_window_start
        {
            self.pages.setup.setup_timezone_window_start =
                self.pages.setup.setup_selected_timezone_index;
        }

        let window_end = self
            .pages
            .setup
            .setup_timezone_window_start
            .saturating_add(visible_rows);
        if self.pages.setup.setup_selected_timezone_index >= window_end {
            self.pages.setup.setup_timezone_window_start = self
                .pages
                .setup
                .setup_selected_timezone_index
                .saturating_add(1)
                .saturating_sub(visible_rows)
                .min(max_start);
        }
    }

    pub(in crate::session) fn setup_timezone_visible_row_count(&self) -> usize {
        ui::setup_timezone_visible_rows(self.touch_setup_viewport().content).max(1)
    }

    pub(in crate::session) fn setup_previous_step(&mut self) {
        if self.pages.setup.setup_custom_color_target.is_some()
            || self.pages.setup.setup_step != ui::SetupStep::Timezone
        {
            return;
        }
        self.pages.setup.setup_step = ui::SetupStep::Language;
        self.pages.setup.setup_focused_field = ui::SetupField::LanguageList;
        self.pages.common.focused_component = ShellComponent::SetupLanguage;
        self.pages.common.page_touch.setup_scroll = 0;
        self.pages.common.error_message = None;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn setup_primary_action(&mut self) {
        if self.pages.setup.setup_custom_color_target.is_some() {
            return;
        }
        match self.pages.setup.setup_step {
            ui::SetupStep::Language | ui::SetupStep::Timezone => self.setup_continue(),
            ui::SetupStep::Admin => self.submit_first_run_setup(),
            ui::SetupStep::Appearance => self.finish_first_run_setup(),
        }
    }

    pub(in crate::session) fn setup_continue(&mut self) {
        self.pages.common.page_touch.setup_scroll = 0;
        match self.pages.setup.setup_step {
            ui::SetupStep::Language => {
                let code = self.selected_setup_language_value();
                match self.prepare_language(&code) {
                    Ok((catalog, loaded)) => self.publish_language(catalog, loaded),
                    Err(error) => {
                        self.report_language_failure(&error);
                        return;
                    }
                }
                self.pages.setup.setup_step = ui::SetupStep::Timezone;
                self.pages.setup.setup_focused_field = ui::SetupField::TimezoneList;
                self.pages.common.focused_component = ShellComponent::SetupTimezone;
                self.sync_setup_timezone_window();
            }
            ui::SetupStep::Timezone => {
                if self.pending_personalization_session.is_some() {
                    self.pages.setup.setup_step = ui::SetupStep::Appearance;
                    self.pages.setup.setup_focused_field = ui::SetupField::AppearanceShape;
                    self.pages.common.focused_component = ShellComponent::SetupAppearanceShape;
                } else {
                    self.pages.setup.setup_step = ui::SetupStep::Admin;
                    self.pages.setup.setup_focused_field = ui::SetupField::AdminUsername;
                    self.pages.common.focused_component = ShellComponent::SetupAdminUsername;
                }
            }
            ui::SetupStep::Admin | ui::SetupStep::Appearance => {}
        }
        self.pages.common.error_message = None;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn move_setup_admin_focus(&mut self, direction: i8) {
        if self.pages.setup.setup_step == ui::SetupStep::Appearance {
            let order = [
                (
                    ui::SetupField::AppearanceShape,
                    ShellComponent::SetupAppearanceShape,
                ),
                (
                    ui::SetupField::AppearanceThemeColor,
                    ShellComponent::SetupAppearanceThemeColor,
                ),
                (
                    ui::SetupField::AppearanceThemeCustom,
                    ShellComponent::SetupAppearanceThemeCustom,
                ),
                (
                    ui::SetupField::AppearanceAccentColor,
                    ShellComponent::SetupAppearanceAccentColor,
                ),
                (
                    ui::SetupField::AppearanceAccentCustom,
                    ShellComponent::SetupAppearanceAccentCustom,
                ),
                (
                    ui::SetupField::AppearanceSubmit,
                    ShellComponent::SetupAppearanceSubmit,
                ),
            ];
            let next = match order
                .iter()
                .position(|(field, _)| *field == self.pages.setup.setup_focused_field)
            {
                Some(current) => (current as isize + direction as isize)
                    .rem_euclid(order.len() as isize) as usize,
                None if direction < 0 => order.len().saturating_sub(1),
                None => 0,
            };
            let (field, component) = order[next];
            self.pages.setup.setup_focused_field = field;
            self.pages.common.focused_component = component;
            self.pages.common.error_message = None;
            self.ensure_touch_auth_focus_visible();
            return;
        }

        let order = [
            (
                ui::SetupField::AdminUsername,
                ShellComponent::SetupAdminUsername,
            ),
            (
                ui::SetupField::AdminPassword,
                ShellComponent::SetupAdminPassword,
            ),
            (
                ui::SetupField::AdminPasswordConfirm,
                ShellComponent::SetupAdminPasswordConfirm,
            ),
            (ui::SetupField::PasswordHint, ShellComponent::SetupAdminHint),
            (ui::SetupField::Submit, ShellComponent::SetupSubmit),
        ];
        let next = match order
            .iter()
            .position(|(field, _)| *field == self.pages.setup.setup_focused_field)
        {
            Some(current) => {
                (current as isize + direction as isize).rem_euclid(order.len() as isize) as usize
            }
            None if direction < 0 => order.len().saturating_sub(1),
            None => 0,
        };
        let (field, component) = order[next];
        self.pages.setup.setup_focused_field = field;
        self.pages.common.focused_component = component;
        self.pages.common.error_message = None;
        self.ensure_touch_auth_focus_visible();
    }

    pub(in crate::session) fn focus_setup_component(&mut self, component: ShellComponent) {
        if !setup_component_active_for_step(component, self.pages.setup.setup_step) {
            return;
        }

        let Some(field) = setup_field_for_component(component) else {
            return;
        };

        self.pages.setup.setup_focused_field = field;
        self.pages.common.focused_component = component;
        self.ensure_touch_auth_focus_visible();
    }

    pub(in crate::session) fn setup_active_key_component(&self) -> ShellComponent {
        match self.pages.setup.setup_step {
            ui::SetupStep::Language => ShellComponent::SetupLanguage,
            ui::SetupStep::Timezone => ShellComponent::SetupTimezone,
            ui::SetupStep::Admin => {
                let component = setup_component_for_field(self.pages.setup.setup_focused_field);
                if setup_component_active_for_step(component, self.pages.setup.setup_step) {
                    component
                } else {
                    ShellComponent::SetupAdminUsername
                }
            }
            ui::SetupStep::Appearance => {
                if self.pages.setup.setup_custom_color_target.is_some() {
                    ShellComponent::SetupCustomColorDialog
                } else {
                    let component = setup_component_for_field(self.pages.setup.setup_focused_field);
                    if setup_component_active_for_step(component, self.pages.setup.setup_step) {
                        component
                    } else {
                        ShellComponent::SetupAppearanceShape
                    }
                }
            }
        }
    }

    pub(in crate::session) fn append_setup_admin_char(&mut self, character: char) {
        match self.pages.setup.setup_focused_field {
            ui::SetupField::AdminUsername => self.pages.setup.setup_admin_username.push(character),
            ui::SetupField::AdminPassword => self.pages.setup.setup_admin_password.push(character),
            ui::SetupField::AdminPasswordConfirm => {
                self.pages
                    .setup
                    .setup_admin_password_confirm
                    .push(character);
            }
            ui::SetupField::PasswordHint => {
                self.pages.setup.setup_admin_password_hint.push(character)
            }
            _ => {}
        }
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn setup_admin_backspace(&mut self) {
        match self.pages.setup.setup_focused_field {
            ui::SetupField::AdminUsername => {
                self.pages.setup.setup_admin_username.pop();
            }
            ui::SetupField::AdminPassword => {
                self.pages.setup.setup_admin_password.pop();
            }
            ui::SetupField::AdminPasswordConfirm => {
                self.pages.setup.setup_admin_password_confirm.pop();
            }
            ui::SetupField::PasswordHint => {
                self.pages.setup.setup_admin_password_hint.pop();
            }
            _ => {}
        }
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn activate_setup(
        &mut self,
        target: ShellComponent,
        coordinates: CellPosition,
    ) {
        match self.pages.setup.setup_step {
            ui::SetupStep::Language => {
                if target == ShellComponent::SetupLanguage
                    && let Some(index) = self.setup_language_index_at(coordinates)
                {
                    self.pages.setup.setup_selected_language_index = index;
                    self.pages.common.error_message = None;
                }
                self.focus_setup_component(ShellComponent::SetupLanguage);
            }
            ui::SetupStep::Timezone => {
                if target == ShellComponent::SetupTimezone
                    && let Some(index) = self.setup_timezone_index_at(coordinates)
                {
                    self.pages.setup.setup_selected_timezone_index = index;
                    self.sync_setup_timezone_window();
                    self.pages.common.error_message = None;
                }
                self.focus_setup_component(ShellComponent::SetupTimezone);
            }
            ui::SetupStep::Admin => {
                self.focus_setup_component(target);
                if target == ShellComponent::SetupSubmit {
                    self.submit_first_run_setup();
                }
            }
            ui::SetupStep::Appearance => {
                self.activate_setup_appearance(target, coordinates);
            }
        }
    }

    pub(in crate::session) fn activate_setup_appearance(
        &mut self,
        target: ShellComponent,
        coordinates: CellPosition,
    ) {
        if self.pages.setup.setup_custom_color_target.is_some() {
            return;
        }
        self.pages.common.error_message = None;

        match target {
            ShellComponent::SetupAppearanceShape => {
                {
                    let main = self.touch_setup_viewport().content;
                    for (shape, area) in ui::setup_appearance_shape_option_areas(main) {
                        if rect_contains(area, coordinates) {
                            self.pages.setup.setup_border_shape = match shape {
                                ui::BorderShape::Rounded => storage::BorderShape::Rounded,
                                ui::BorderShape::Square => storage::BorderShape::Square,
                            };
                            break;
                        }
                    }
                }
                self.focus_setup_component(target);
            }
            ShellComponent::SetupAppearanceThemeColor
            | ShellComponent::SetupAppearanceAccentColor => {
                {
                    let main = self.touch_setup_viewport().content;
                    let field = if target == ShellComponent::SetupAppearanceThemeColor {
                        ui::SetupField::AppearanceThemeColor
                    } else {
                        ui::SetupField::AppearanceAccentColor
                    };
                    if let Some((index, _)) = ui::setup_appearance_palette_option_areas(main, field)
                        .into_iter()
                        .find(|(_, area)| rect_contains(*area, coordinates))
                    {
                        self.set_setup_standard_color(field, index);
                    }
                }
                self.focus_setup_component(target);
            }
            ShellComponent::SetupAppearanceThemeCustom => {
                self.focus_setup_component(target);
                self.open_setup_custom_color(ui::SetupCustomColorTarget::Theme);
            }
            ShellComponent::SetupAppearanceAccentCustom => {
                self.focus_setup_component(target);
                self.open_setup_custom_color(ui::SetupCustomColorTarget::Accent);
            }
            ShellComponent::SetupAppearanceSubmit => {
                self.focus_setup_component(target);
                self.setup_primary_action();
            }
            ShellComponent::SetupCustomColorDialog => {}
            _ => {}
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn setup_select_appearance_choice(&mut self, delta: isize) {
        match self.pages.setup.setup_focused_field {
            ui::SetupField::AppearanceShape => {
                self.pages.setup.setup_border_shape = match self.pages.setup.setup_border_shape {
                    storage::BorderShape::Rounded => storage::BorderShape::Square,
                    storage::BorderShape::Square => storage::BorderShape::Rounded,
                };
            }
            field @ (ui::SetupField::AppearanceThemeColor
            | ui::SetupField::AppearanceAccentColor) => {
                let options = ui::setup_standard_color_options();
                if options.is_empty() {
                    return;
                }
                let current_value = match field {
                    ui::SetupField::AppearanceThemeColor => {
                        self.pages.setup.setup_theme_color.to_string()
                    }
                    _ => self.pages.setup.setup_accent_color.to_string(),
                };
                let current = options
                    .iter()
                    .position(|option| option.value.eq_ignore_ascii_case(&current_value));
                let mut next =
                    current.unwrap_or_else(|| if delta < 0 { 0 } else { options.len() - 1 });
                for _ in 0..options.len() {
                    next = (next as isize + delta).rem_euclid(options.len() as isize) as usize;
                    let Ok(candidate) = options[next].value.parse::<storage::BorderColor>() else {
                        continue;
                    };
                    if field == ui::SetupField::AppearanceAccentColor
                        && candidate == self.pages.setup.setup_theme_color
                    {
                        continue;
                    }
                    self.set_setup_standard_color(field, next);
                    break;
                }
            }
            _ => return,
        }
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn set_setup_standard_color(
        &mut self,
        field: ui::SetupField,
        index: usize,
    ) {
        let options = ui::setup_standard_color_options();
        let Some(option) = options.get(index) else {
            return;
        };
        let Ok(color) = option.value.parse::<storage::BorderColor>() else {
            return;
        };
        match field {
            ui::SetupField::AppearanceThemeColor => {
                self.pages.setup.setup_theme_color = color;
                self.ensure_setup_accent_differs();
            }
            ui::SetupField::AppearanceAccentColor
                if color != self.pages.setup.setup_theme_color =>
            {
                self.pages.setup.setup_accent_color = color;
            }
            _ => {}
        }
    }

    pub(in crate::session) fn ensure_setup_accent_differs(&mut self) {
        if self.pages.setup.setup_accent_color != self.pages.setup.setup_theme_color {
            return;
        }
        if let Some(color) = ui::setup_standard_color_options()
            .iter()
            .filter_map(|option| option.value.parse::<storage::BorderColor>().ok())
            .find(|color| *color != self.pages.setup.setup_theme_color)
        {
            self.pages.setup.setup_accent_color = color;
        }
    }

    pub(in crate::session) fn activate_setup_appearance_control(&mut self) {
        match self.pages.setup.setup_focused_field {
            ui::SetupField::AppearanceThemeCustom => {
                self.open_setup_custom_color(ui::SetupCustomColorTarget::Theme);
            }
            ui::SetupField::AppearanceAccentCustom => {
                self.open_setup_custom_color(ui::SetupCustomColorTarget::Accent);
            }
            ui::SetupField::AppearanceSubmit => self.finish_first_run_setup(),
            _ => self.move_setup_admin_focus(1),
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn open_setup_custom_color(
        &mut self,
        target: ui::SetupCustomColorTarget,
    ) {
        self.capture_modal_focus_context();
        self.pages.setup.setup_custom_color_target = Some(target);
        self.pages.setup.setup_custom_color_input = match target {
            ui::SetupCustomColorTarget::Theme => self.pages.setup.setup_theme_color.to_string(),
            ui::SetupCustomColorTarget::Accent => self.pages.setup.setup_accent_color.to_string(),
        };
        self.pages.setup.setup_custom_color_error = None;
        self.synchronize_overlay_focus();
    }

    pub(in crate::session) fn append_setup_custom_color_char(&mut self, character: char) {
        if self.pages.setup.setup_custom_color_target.is_none()
            || character.is_control()
            || self.pages.setup.setup_custom_color_input.chars().count() >= 32
        {
            return;
        }
        self.pages.setup.setup_custom_color_input.push(character);
        self.pages.setup.setup_custom_color_error = None;
    }

    pub(in crate::session) fn setup_custom_color_backspace(&mut self) {
        if self.pages.setup.setup_custom_color_target.is_some() {
            self.pages.setup.setup_custom_color_input.pop();
            self.pages.setup.setup_custom_color_error = None;
        }
    }

    pub(in crate::session) fn apply_setup_custom_color(&mut self) {
        let Some(target) = self.pages.setup.setup_custom_color_target else {
            return;
        };
        let color = match self
            .pages
            .setup
            .setup_custom_color_input
            .parse::<storage::BorderColor>()
        {
            Ok(color) => color,
            Err(_) => {
                self.pages.setup.setup_custom_color_error =
                    Some(i18n::msg!("account-invalid-color").into());
                return;
            }
        };
        match target {
            ui::SetupCustomColorTarget::Theme => {
                self.pages.setup.setup_theme_color = color;
                self.ensure_setup_accent_differs();
            }
            ui::SetupCustomColorTarget::Accent if color == self.pages.setup.setup_theme_color => {
                self.pages.setup.setup_custom_color_error =
                    Some(i18n::msg!("account-accent-must-differ").into());
                return;
            }
            ui::SetupCustomColorTarget::Accent => self.pages.setup.setup_accent_color = color,
        }
        self.cancel_setup_custom_color();
    }

    pub(in crate::session) fn cancel_setup_custom_color(&mut self) {
        self.pages.setup.setup_custom_color_target = None;
        self.pages.setup.setup_custom_color_input.clear();
        self.pages.setup.setup_custom_color_error = None;
        self.pages.common.focused_component =
            setup_component_for_field(self.pages.setup.setup_focused_field);
        self.refresh_hit_map();
    }

    pub(in crate::session) fn activate_login(
        &mut self,
        target: ShellComponent,
        coordinates: CellPosition,
    ) {
        match target {
            ShellComponent::LoginUserList => {
                if let Some(index) = self.login_user_index_at(coordinates) {
                    self.select_login_user_at(index);
                }
                self.pages.common.focused_component = ShellComponent::LoginPassword;
            }
            ShellComponent::LoginUsername => {
                self.pages.common.focused_component = ShellComponent::LoginUserList;
            }
            ShellComponent::LoginPassword => {
                self.pages.common.focused_component = ShellComponent::LoginPassword;
            }
            _ => {}
        }
    }

    pub(in crate::session) fn setup_language_index_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<usize> {
        let row = setup_language_list_row_at(
            self.terminal_size,
            self.language_catalog.options().len(),
            coordinates,
        )?;
        (row < self.language_options().len()).then_some(row)
    }

    pub(in crate::session) fn setup_timezone_index_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<usize> {
        let row = setup_timezone_list_row_at(self.terminal_size, coordinates)?;
        let count = app::setup_timezone_options().len();
        if count == 0 {
            return None;
        }

        let visible_rows = self.setup_timezone_visible_row_count().min(count);
        let start = self
            .pages
            .setup
            .setup_timezone_window_start
            .min(count.saturating_sub(visible_rows));
        let index = start.saturating_add(row);
        (row < visible_rows && index < count).then_some(index)
    }

    pub(in crate::session) fn login_user_index_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<usize> {
        let row = login_user_list_row_at(self.terminal_size, coordinates)?;
        let count = self.login_users.len();
        if count == 0 {
            return None;
        }

        let visible_rows = self.login_user_visible_row_count().min(count);
        let start = self
            .pages
            .login
            .login_user_window_start
            .min(count.saturating_sub(visible_rows));
        let index = start.saturating_add(row);
        (row < visible_rows && index < count).then_some(index)
    }

    pub(in crate::session) fn selected_setup_language_value(&self) -> String {
        let options = self.language_options();
        setup_language_code_at(&options, self.pages.setup.setup_selected_language_index)
            .unwrap_or_else(|| "en-US".to_string())
    }

    pub(in crate::session) fn selected_setup_timezone_value(&self) -> String {
        let options = app::setup_timezone_options();
        setup_timezone_id_at(&options, self.pages.setup.setup_selected_timezone_index)
            .unwrap_or_else(|| "UTC".to_string())
    }

    pub(in crate::session) fn submit_login(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.pages.common.error_message =
                Some(i18n::msg!("account-storage-unavailable").into());
            return;
        };
        let Some(username) = self.selected_login_username().map(str::to_string) else {
            self.pages.common.error_message = Some(i18n::msg!("account-no-user-selected").into());
            self.notify_status(i18n::msg!("account-login-failed"));
            return;
        };
        let password_hint = self.selected_login_password_hint().map(str::to_string);
        let mut sessions = SessionService::new(storage).with_backend(self.identity_backend);
        match sessions.login(&username, &self.pages.login.login_password) {
            Ok(session) => self.complete_login(session),
            Err(error) => {
                self.pages.login.login_password_visible_until = None;
                self.pages.login.login_password.clear();
                self.pages.common.error_message =
                    Some(login_error_message(&error, password_hint.as_deref()));
                self.notify_status(i18n::msg!("account-login-failed"));
                let _ = self.refresh_login_users_from_storage();
            }
        }
    }

    pub(in crate::session) fn submit_bootstrap_admin(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.pages.common.error_message =
                Some(i18n::msg!("account-storage-unavailable").into());
            return;
        };
        let users = UserService::with_debug_policy(storage.clone(), self.debug_policy)
            .with_backend(self.identity_backend);
        match users.bootstrap_admin(
            &self.pages.bootstrap.bootstrap_username,
            &self.pages.bootstrap.bootstrap_password,
        ) {
            Ok(_) => {
                self.pages.login.login_username = self.pages.bootstrap.bootstrap_username.clone();
                self.pages.login.login_password = self.pages.bootstrap.bootstrap_password.clone();
                let mut sessions = SessionService::new(storage).with_backend(self.identity_backend);
                match sessions.login(
                    &self.pages.login.login_username,
                    &self.pages.login.login_password,
                ) {
                    Ok(session) => self.complete_login(session),
                    Err(error) => {
                        self.pages.common.error_message = Some(format_core_error(&error));
                        self.notify_status(i18n::msg!("account-login-failed"));
                    }
                }
            }
            Err(error) => {
                self.pages.common.error_message = Some(format_core_error(&error));
                self.notify_status(i18n::msg!("account-admin-bootstrap-failed"));
            }
        }
    }

    pub(in crate::session) fn submit_first_run_setup(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.pages.common.error_message =
                Some(i18n::msg!("account-storage-unavailable").into());
            return;
        };

        let username = self.pages.setup.setup_admin_username.trim().to_string();
        let password = self.pages.setup.setup_admin_password.clone();
        if password != self.pages.setup.setup_admin_password_confirm {
            self.pages.setup.setup_focused_field = ui::SetupField::AdminPasswordConfirm;
            self.pages.common.focused_component = ShellComponent::SetupAdminPasswordConfirm;
            self.pages.common.error_message =
                Some(i18n::msg!("account-passwords-do-not-match").into());
            self.ensure_touch_auth_focus_visible();
            self.notify_status(i18n::msg!("account-setup-incomplete"));
            return;
        }

        let hint = self
            .pages
            .setup
            .setup_admin_password_hint
            .trim()
            .to_string();
        let hint = (!hint.is_empty()).then_some(hint);

        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.pages.common.error_message = Some(i18n::LocalizedText::Raw(error.to_string()));
                self.notify_status(i18n::msg!("account-setup-failed"));
                return;
            }
        };
        config.language = self.selected_setup_language_value();
        config.timezone = self.selected_setup_timezone_value();
        if let Err(error) = storage.save_config(&config) {
            self.pages.common.error_message = Some(i18n::LocalizedText::Raw(error.to_string()));
            self.notify_status(i18n::msg!("account-setup-failed"));
            return;
        }
        self.replace_storage_config(config);

        let users = UserService::with_debug_policy(storage, self.debug_policy)
            .with_backend(self.identity_backend);
        match users.validate_bootstrap_admin(&username, &password, hint.as_deref()) {
            Ok(()) => {
                self.pages.setup.setup_step = ui::SetupStep::Appearance;
                self.pages.common.page_touch.setup_scroll = 0;
                self.pages.setup.setup_focused_field = ui::SetupField::AppearanceShape;
                self.pages.common.focused_component = ShellComponent::SetupAppearanceShape;
                self.pages.common.error_message = None;
                self.notify_status(i18n::msg!("account-choose-appearance"));
                self.refresh_hit_map();
            }
            Err(error) => {
                self.pages.common.error_message = Some(format_core_error(&error));
                self.notify_status(i18n::msg!("account-setup-failed"));
            }
        }
    }

    pub(in crate::session) fn finish_first_run_setup(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.pages.common.error_message =
                Some(i18n::msg!("account-storage-unavailable").into());
            return;
        };

        let username = self.pages.setup.setup_admin_username.trim().to_string();
        let password = self.pages.setup.setup_admin_password.clone();
        if self.pages.setup.setup_theme_color == self.pages.setup.setup_accent_color {
            self.pages.setup.setup_focused_field = ui::SetupField::AppearanceAccentColor;
            self.pages.common.focused_component = ShellComponent::SetupAppearanceAccentColor;
            self.pages.common.error_message = Some(i18n::msg!("account-accent-must-differ").into());
            self.notify_status(i18n::msg!("account-setup-incomplete"));
            return;
        }
        let hint = self
            .pages
            .setup
            .setup_admin_password_hint
            .trim()
            .to_string();
        let hint = (!hint.is_empty()).then_some(hint);
        let appearance = storage::AppearanceConfig {
            border_shape: self.pages.setup.setup_border_shape,
            border_color: self.pages.setup.setup_theme_color,
            accent_color: self.pages.setup.setup_accent_color,
            icon_display_mode: if self.terminal_image_support {
                storage::IconDisplayMode::Image
            } else {
                storage::IconDisplayMode::Ascii
            },
            ..storage::AppearanceConfig::default()
        };
        let users = UserService::with_debug_policy(storage.clone(), self.debug_policy)
            .with_backend(self.identity_backend);
        if let Some(session) = self.pending_personalization_session.clone() {
            // Save setup preferences before marking the profile complete so a
            // failed write leaves the onboarding flow available for retry.
            let result = storage.load_config().and_then(|mut config| {
                config.language = self.selected_setup_language_value();
                config.timezone = self.selected_setup_timezone_value();
                storage.save_config(&config)?;
                Ok(config)
            });
            match result {
                Ok(config) => self.replace_storage_config(config),
                Err(error) => {
                    self.pages.common.error_message =
                        Some(i18n::LocalizedText::Raw(error.to_string()));
                    self.notify_status(i18n::msg!("account-personalization-save-failed"));
                    return;
                }
            }
            let mut appearance = appearance;
            if !self.terminal_image_support {
                appearance.icon_display_mode = storage::IconDisplayMode::Ascii;
            }
            match users.complete_personalization(&session, appearance) {
                Ok(_) => self.complete_login(session),
                Err(error) => {
                    self.pages.common.error_message = Some(format_core_error(&error));
                    self.notify_status(i18n::msg!("account-personalization-save-failed"));
                }
            }
            return;
        }
        if let Err(error) = users.bootstrap_admin_with_hint_and_appearance(
            &username,
            &password,
            hint.as_deref(),
            appearance,
        ) {
            self.pages.common.error_message = Some(format_core_error(&error));
            self.notify_status(i18n::msg!("account-setup-failed"));
            return;
        }

        let mut sessions = SessionService::new(storage).with_backend(self.identity_backend);
        match sessions.login(&username, &password) {
            Ok(session) => self.complete_login(session),
            Err(error) => {
                self.pages.setup.setup_admin_password.clear();
                self.pages.setup.setup_admin_password_confirm.clear();
                self.pages.common.error_message = Some(format_core_error(&error));
                self.notify_status(i18n::msg!("account-login-failed"));
            }
        }
    }

    fn clear_auth_passwords(&mut self) {
        self.pages.login.login_password.clear();
        self.pages.login.login_password_visible_until = None;
        self.pages.bootstrap.bootstrap_password.clear();
        self.pages.setup.setup_admin_password.clear();
        self.pages.setup.setup_admin_password_confirm.clear();
    }

    fn begin_linux_personalization(
        &mut self,
        session: AuthSession,
        appearance: storage::AppearanceConfig,
    ) {
        self.clear_auth_passwords();
        self.reset_navigation(ShellScreen::FirstRunSetup);
        self.pages.login.login_username = session.username.clone();
        self.pages.setup.setup_admin_username = session.username.clone();
        self.pages.setup.setup_admin_password_hint.clear();
        self.pages.setup.setup_border_shape = appearance.border_shape;
        self.pages.setup.setup_theme_color = appearance.border_color;
        self.pages.setup.setup_accent_color = appearance.accent_color;
        self.ensure_setup_accent_differs();
        self.pages.setup.setup_custom_color_target = None;
        self.pages.setup.setup_custom_color_input.clear();
        self.pages.setup.setup_custom_color_error = None;
        self.pending_personalization_session = Some(session);
        self.app
            .dispatch_at(app::AppCommand::SetAuthSession(None), Instant::now());
        self.app
            .dispatch_at(app::AppCommand::SetActiveAppearance(None), Instant::now());
        self.app.dispatch_at(
            app::AppCommand::SetActiveSystemStatusDashboard(None),
            Instant::now(),
        );
        self.pages.setup.setup_step = ui::SetupStep::Language;
        self.pages.common.page_touch.setup_scroll = 0;
        self.pages.setup.setup_focused_field = ui::SetupField::LanguageList;
        self.pages.common.focused_component = ShellComponent::SetupLanguage;
        self.pages.common.active_popup = None;
        self.pages.common.error_message = None;
        self.notify_status(i18n::msg!("account-first-sign-in-setup"));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn complete_login(&mut self, session: AuthSession) {
        self.reset_system_status_trackers();
        let profile = self
            .storage_manager
            .as_ref()
            .map(|storage| storage.load_users())
            .transpose();
        let profile = match profile {
            Ok(document) => document.and_then(|document| {
                document.users.into_iter().find(|user| {
                    user.id == session.user_id
                        && self
                            .identity_backend
                            .usernames_match(&user.username, &session.username)
                })
            }),
            Err(error) if self.identity_backend == identity::IdentityBackend::Linux => {
                self.clear_auth_passwords();
                self.pages.common.error_message = Some(
                    i18n::msg!("account-profile-load-failed", error = error.to_string()).into(),
                );
                self.notify_status(i18n::msg!("account-login-incomplete"));
                return;
            }
            Err(_) => None,
        };
        if self.identity_backend == identity::IdentityBackend::Linux
            && profile
                .as_ref()
                .is_none_or(|profile| profile.personalization_pending)
        {
            self.begin_linux_personalization(
                session,
                profile
                    .map(|profile| profile.appearance)
                    .unwrap_or_default(),
            );
            return;
        }
        let (mut active_appearance, active_dashboard) = profile
            .map(|user| (Some(user.appearance), Some(user.system_status_dashboard)))
            .unwrap_or((None, None));
        let mut icon_fallback_error = None;
        let needs_ascii = std::mem::take(&mut self.pending_default_ascii_icon_fallback)
            || (self.identity_backend == identity::IdentityBackend::Linux
                && !self.terminal_image_support);
        if needs_ascii
            && self.ascii_assets.theme_id() == ui::DEFAULT_THEME_ID
            && active_appearance.as_ref().is_some_and(|appearance| {
                appearance.icon_display_mode == storage::IconDisplayMode::Image
            })
            && let (Some(storage), Some(mut appearance)) =
                (self.storage_manager.clone(), active_appearance.clone())
        {
            appearance.icon_display_mode = storage::IconDisplayMode::Ascii;
            active_appearance = Some(appearance.clone());
            match UserService::with_debug_policy(storage, self.debug_policy)
                .with_backend(self.identity_backend)
                .update_user_appearance(&session, &session.username, appearance)
            {
                Ok(account) => active_appearance = Some(account.appearance),
                Err(error) => icon_fallback_error = Some(error.to_string()),
            }
        }
        self.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(session.clone())),
            Instant::now(),
        );
        if let Some(snapshot) = self.app.system_status_snapshot().cloned() {
            self.evaluate_system_status_alerts(&snapshot);
        }
        self.app.dispatch_at(
            app::AppCommand::SetActiveAppearance(active_appearance),
            Instant::now(),
        );
        self.app.dispatch_at(
            app::AppCommand::SetActiveSystemStatusDashboard(active_dashboard),
            Instant::now(),
        );
        self.pages.login.login_username = session.username.clone();
        self.clear_auth_passwords();
        self.pages.common.error_message = None;
        self.notify_status(i18n::msg!(
            "account-signed-in",
            username = session.username.clone()
        ));
        if let Some(error) = icon_fallback_error {
            self.notify_alert_with_tone(
                i18n::msg!("account-ascii-fallback-save-failed", error = error),
                ui::NotificationTone::Warning,
            );
        }
        self.home_mode = ShellHomeMode::User;

        if self.debug_home_after_login {
            let permission = PermissionService::new(self.debug_policy).authorize(
                Some(&session),
                PermissionAction::EnterDebugMode,
                None,
            );
            if permission.allowed {
                self.home_mode = ShellHomeMode::Debug;
            } else {
                self.notify_toast(i18n::msg!("account-debug-mode-denied"));
            }
        }

        self.pending_personalization_session = None;
        self.reset_navigation(ShellScreen::Home);
        self.pages.common.focused_component = ShellComponent::Home;
        self.pages.common.active_popup = None;
        if session.role != UserRole::Guest {
            self.load_clock_for_session(&session);
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn open_user_management(&mut self) {
        if self.is_strict_guest() {
            self.pages.common.error_message = None;
            self.notify_status(i18n::msg!("account-guest-read-only"));
            return;
        }
        if self.app.auth_session().is_none() {
            self.pages.common.error_message = Some(i18n::msg!("account-login-required").into());
            return;
        };

        #[cfg(target_os = "linux")]
        if self.identity_backend == identity::IdentityBackend::Linux {
            self.open_management(platform::management::ManagementKind::Users);
            return;
        }

        self.enter_new_page(ShellScreen::UserManagement);
        if self.refresh_user_management() {
            self.pages.user_management.user_management_mode = UserManagementMode::Browse;
            self.pages.user_management.user_management_focus = UserManagementPageFocus::UserList;
            self.ensure_user_management_selection_visible();
            let status = if self.can_manage_all_users() {
                i18n::msg!("account-user-management")
            } else {
                i18n::msg!("account-user-profile")
            };
            self.notify_status(status);
            self.refresh_hit_map();
        }
    }
}
