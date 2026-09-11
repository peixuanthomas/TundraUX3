use super::super::*;
pub(in crate::session) const SETTINGS_RESTORE_NOTIFICATION_KEY: &str = "settings.restore-defaults";
pub(in crate::session) const SETTINGS_WEATHER_LOCATION_NOTIFICATION_KEY: &str =
    "settings.weather-location";
pub(in crate::session) const WEATHER_LOCATION_MAX_LEN: usize = 120;
pub(in crate::session) const EDITOR_EXTENSIONS_INPUT_MAX_LEN: usize = 1_024;

pub(in crate::session) const APPEARANCE_SETTINGS_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::Theme,
    ui::SettingsField::BorderShape,
    ui::SettingsField::BorderColor,
    ui::SettingsField::AccentColor,
    ui::SettingsField::MotionPreference,
    ui::SettingsField::AnimationSpeed,
    ui::SettingsField::ResetAnimationSpeed,
    ui::SettingsField::RestoreDefaults,
];
pub(in crate::session) const REGION_SETTINGS_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::Language,
    ui::SettingsField::Timezone,
    ui::SettingsField::WeatherLocation,
    ui::SettingsField::TimeSyncSource,
    ui::SettingsField::TimeSyncServer,
    ui::SettingsField::RestoreDefaults,
];
pub(in crate::session) const SYSTEM_SETTINGS_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::SystemLowAvailable,
    ui::SettingsField::SystemLowPercentage,
    ui::SettingsField::SystemCriticalAvailable,
    ui::SettingsField::SystemCriticalPercentage,
    ui::SettingsField::RestoreDefaults,
];
pub(in crate::session) const EXPLORER_SETTINGS_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::ShowHidden,
    ui::SettingsField::ShowSystem,
    ui::SettingsField::ShowExtensions,
    ui::SettingsField::FoldersFirst,
    ui::SettingsField::ShowSidebar,
    ui::SettingsField::CaseSensitiveSort,
    ui::SettingsField::SizeFormat,
    ui::SettingsField::DateZone,
    ui::SettingsField::SortField,
    ui::SettingsField::SortDirection,
    ui::SettingsField::ConfirmDelete,
    ui::SettingsField::ConfirmNameConflicts,
    ui::SettingsField::RestoreDefaults,
];
pub(in crate::session) const EDITOR_SETTINGS_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::ExplorerOpenExtensions,
    ui::SettingsField::CursorAcceleration,
    ui::SettingsField::CursorDelay,
    ui::SettingsField::CursorRamp,
    ui::SettingsField::CursorHorizontalStep,
    ui::SettingsField::CursorVerticalStep,
    ui::SettingsField::RestoreDefaults,
];
pub(in crate::session) const UPDATE_SETTINGS_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::InstalledVersion,
    ui::SettingsField::RemoteVersion,
    ui::SettingsField::CheckUpdates,
    ui::SettingsField::StartUpdate,
];

impl ShellSession {
    pub(in crate::session) fn open_settings(&mut self) {
        if self.is_strict_guest() {
            self.notify_status(i18n::msg!("settings-guest-read-only"));
            return;
        }
        let Some(actor) = self.app.auth_session().cloned() else {
            self.error_message = Some(i18n::msg!("settings-login-required").into());
            return;
        };
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some(i18n::msg!("settings-storage-unavailable").into());
            return;
        };
        let config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.error_message =
                    Some(i18n::msg!("settings-load-failed", reason = error.to_string()).into());
                self.notify_status(i18n::msg!("settings-unavailable"));
                return;
            }
        };
        let users = UserService::with_debug_policy(storage, self.debug_policy)
            .with_backend(self.identity_backend);
        let appearance = match users.list_accessible_users(&actor).and_then(|users| {
            users
                .into_iter()
                .find(|user| user.id == actor.user_id)
                .map(|user| user.appearance)
                .ok_or(CoreError::UserNotFound)
        }) {
            Ok(appearance) => appearance,
            Err(error) => {
                self.error_message = Some(
                    i18n::msg!(
                        "settings-appearance-load-failed",
                        reason = error.to_string()
                    )
                    .into(),
                );
                self.notify_status(i18n::msg!("settings-unavailable"));
                return;
            }
        };

        self.replace_storage_config(config);
        self.app.dispatch_at(
            app::AppCommand::SetActiveAppearance(Some(appearance)),
            Instant::now(),
        );
        self.settings_state = Some(SettingsState {
            category: ui::SettingsCategory::Appearance,
            selected_field: ui::SettingsField::Theme,
            status: i18n::msg!("settings-ready").into(),
            scroll_offset: 0,
            picker: None,
            color_editor: None,
            weather_location_editor: None,
            file_extensions_editor: None,
            time_sync_server_editor: None,
            time_sync_validation_request_id: None,
        });
        if self.active_screen() != ShellScreen::Settings {
            self.screen_stack.push(ShellScreen::Settings);
        }
        self.focused_component = ShellComponent::Settings;
        self.error_message = None;
        self.notify_status(i18n::msg!("settings-title"));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_settings(&mut self) {
        if self.active_screen() == ShellScreen::Settings {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.settings_state = None;
        self.focused_component = if self.active_screen() == ShellScreen::Home {
            ShellComponent::Home
        } else {
            ShellComponent::Settings
        };
        self.notify_status(i18n::msg!("settings-ready"));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn can_change_global_settings(&self) -> bool {
        PermissionService::new(self.debug_policy)
            .authorize(
                self.app.auth_session(),
                PermissionAction::ChangeSettings,
                Some("change_settings"),
            )
            .allowed
    }

    pub fn set_terminal_image_support(&mut self, supported: bool) {
        self.terminal_image_support = supported;
    }

    pub fn set_terminal_text_sizing_support(&mut self, supported: bool) {
        self.terminal_text_sizing_support = supported;
    }

    pub fn graphical_icons_enabled(&self) -> bool {
        self.terminal_image_support
            && self.ascii_assets.theme_id() == ui::DEFAULT_THEME_ID
            && self.app.active_appearance().is_none_or(|appearance| {
                appearance.icon_display_mode == storage::IconDisplayMode::Image
            })
    }

    pub(in crate::session) fn handle_settings_key(
        &mut self,
        key: &KeyInput,
        platform: &dyn Platform,
    ) {
        if self.settings_state.is_none() {
            return;
        }
        if self.settings_update_state.confirmation_open {
            if key.has_non_shift_modifier() {
                return;
            }
            match key.key {
                InputKey::Escape => self.cancel_update_confirmation(),
                InputKey::Left | InputKey::Right | InputKey::Tab | InputKey::BackTab => {
                    self.settings_update_state.confirm_selected =
                        !self.settings_update_state.confirm_selected;
                }
                InputKey::Enter | InputKey::Char(' ') => {
                    if self.settings_update_state.confirm_selected {
                        self.begin_confirmed_update();
                    } else {
                        self.cancel_update_confirmation();
                    }
                }
                _ => {}
            }
            self.refresh_hit_map();
            return;
        }
        if self
            .settings_state
            .as_ref()
            .is_some_and(|state| state.time_sync_server_editor.is_some())
        {
            self.handle_settings_time_sync_server_key(key);
            return;
        }
        if self
            .settings_state
            .as_ref()
            .is_some_and(|state| state.file_extensions_editor.is_some())
        {
            self.handle_settings_file_extensions_key(key);
            return;
        }
        if self
            .settings_state
            .as_ref()
            .is_some_and(|state| state.weather_location_editor.is_some())
        {
            self.handle_settings_weather_location_key(key);
            return;
        }
        if self
            .settings_state
            .as_ref()
            .is_some_and(|state| state.color_editor.is_some())
        {
            self.handle_settings_color_key(key);
            return;
        }
        if self
            .settings_state
            .as_ref()
            .is_some_and(|state| state.picker.is_some())
        {
            self.handle_settings_picker_key(key);
            return;
        }
        if key.has_non_shift_modifier() {
            return;
        }

        match &key.key {
            InputKey::Escape => self.close_settings(),
            InputKey::Tab => self.select_settings_category_delta(1),
            InputKey::BackTab => self.select_settings_category_delta(-1),
            InputKey::Up => self.select_settings_field_delta(-1),
            InputKey::Down => self.select_settings_field_delta(1),
            InputKey::Home => self.select_settings_field_at(0),
            InputKey::End => {
                let last = self
                    .settings_state
                    .as_ref()
                    .map(|state| settings_fields(state.category).len().saturating_sub(1))
                    .unwrap_or(0);
                self.select_settings_field_at(last);
            }
            InputKey::PageUp => self.scroll_settings(-6),
            InputKey::PageDown => self.scroll_settings(6),
            InputKey::Left => self.adjust_selected_setting(-1, platform),
            InputKey::Right => self.adjust_selected_setting(1, platform),
            InputKey::Enter | InputKey::Char(' ') => self.activate_selected_setting(platform),
            _ => {}
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn handle_settings_pointer(
        &mut self,
        input: MouseInput,
        platform: &dyn Platform,
    ) {
        if self.settings_state.is_none() {
            return;
        }
        if let Some(direction) = input.scroll_direction() {
            let delta = match direction {
                ScrollDirection::Up => -3,
                ScrollDirection::Down => 3,
                ScrollDirection::Left | ScrollDirection::Right => return,
            };
            if self
                .settings_state
                .as_ref()
                .is_some_and(|state| state.picker.is_some())
            {
                self.select_settings_picker_delta(delta);
            } else {
                self.scroll_settings(delta as i16);
            }
            self.refresh_hit_map();
            return;
        }
        let MouseInput {
            kind: ui::MouseEventKind::Down(PointerButton::Left),
            position: ui::Point { column, row },
            ..
        } = input
        else {
            return;
        };
        let coordinates = (column, row);
        let Some(model) = self.to_settings_view_model() else {
            return;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let app_area = match ui::compute_shell_layout(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = ui::settings_layout(app_area, &model);
        match ui::settings_hit_test(&layout, coordinates) {
            Some(ui::SettingsHitTarget::UpdateConfirm) => self.begin_confirmed_update(),
            Some(ui::SettingsHitTarget::UpdateCancel) => self.cancel_update_confirmation(),
            Some(ui::SettingsHitTarget::Category(category)) => {
                self.select_settings_category(category);
            }
            Some(ui::SettingsHitTarget::Field(field)) => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.selected_field = field;
                }
                if field == ui::SettingsField::AnimationSpeed {
                    self.open_settings_picker(ui::SettingsPickerKind::AnimationSpeed);
                } else {
                    self.activate_selected_setting(platform);
                }
            }
            Some(ui::SettingsHitTarget::PickerOption(index)) => {
                if let Some(state) = self.settings_state.as_mut()
                    && let Some(picker) = state.picker.as_mut()
                {
                    picker.selected_index = index;
                }
                self.apply_settings_picker_selection();
            }
            Some(ui::SettingsHitTarget::ColorEditor)
            | Some(ui::SettingsHitTarget::WeatherLocationEditor)
            | Some(ui::SettingsHitTarget::FileExtensionsEditor)
            | Some(ui::SettingsHitTarget::TimeSyncServerEditor)
            | None => {}
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn select_settings_category_delta(&mut self, delta: isize) {
        let Some(current) = self.settings_state.as_ref().map(|state| state.category) else {
            return;
        };
        let index = ui::SettingsCategory::ALL
            .iter()
            .position(|category| *category == current)
            .unwrap_or(0) as isize;
        let count = ui::SettingsCategory::ALL.len() as isize;
        let next = (index + delta).rem_euclid(count) as usize;
        self.select_settings_category(ui::SettingsCategory::ALL[next]);
    }

    pub(in crate::session) fn select_settings_category(&mut self, category: ui::SettingsCategory) {
        if let Some(state) = self.settings_state.as_mut() {
            state.category = category;
            state.selected_field = settings_fields(category)[0];
            state.scroll_offset = 0;
            state.picker = None;
            state.color_editor = None;
            state.weather_location_editor = None;
            state.file_extensions_editor = None;
            state.time_sync_server_editor = None;
            state.time_sync_validation_request_id = None;
            state.status = i18n::msg!("settings-ready").into();
        }
        self.notify_status(i18n::msg!(
            "settings-category-status",
            category = format!("{category:?}")
        ));
        if category == ui::SettingsCategory::Update && !self.settings_update_state.checked_once {
            self.begin_update_check();
        }
    }

    pub(in crate::session) fn select_settings_field_delta(&mut self, delta: isize) {
        let Some(state) = self.settings_state.as_ref() else {
            return;
        };
        let fields = settings_fields(state.category);
        let index = fields
            .iter()
            .position(|field| *field == state.selected_field)
            .unwrap_or(0) as isize;
        let next = (index + delta).clamp(0, fields.len().saturating_sub(1) as isize) as usize;
        self.select_settings_field_at(next);
    }

    pub(in crate::session) fn select_settings_field_at(&mut self, index: usize) {
        {
            let Some(state) = self.settings_state.as_mut() else {
                return;
            };
            let fields = settings_fields(state.category);
            state.selected_field = fields[index.min(fields.len().saturating_sub(1))];
            state.scroll_offset = u16::try_from(index).unwrap_or(u16::MAX).saturating_sub(6);
        }
        self.clamp_settings_scroll();
    }

    pub(in crate::session) fn scroll_settings(&mut self, delta: i16) {
        let Some(layout) = self.current_settings_layout() else {
            return;
        };
        let next = settings_scroll_offset(layout.scroll_offset, delta, layout.max_scroll_offset);
        if let Some(state) = self.settings_state.as_mut() {
            state.scroll_offset = next;
        }
    }

    fn current_settings_layout(&self) -> Option<ui::SettingsLayout> {
        let model = self.to_settings_view_model()?;
        let terminal = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let main = match ui::compute_shell_layout(terminal) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        Some(ui::settings_layout(main, &model))
    }

    pub(in crate::session) fn clamp_settings_scroll(&mut self) {
        let Some(scroll_offset) = self
            .current_settings_layout()
            .map(|layout| layout.scroll_offset)
        else {
            return;
        };
        if let Some(state) = self.settings_state.as_mut() {
            state.scroll_offset = scroll_offset;
        }
    }

    pub(in crate::session) fn activate_selected_setting(&mut self, platform: &dyn Platform) {
        let Some(field) = self
            .settings_state
            .as_ref()
            .map(|state| state.selected_field)
        else {
            return;
        };
        match field {
            ui::SettingsField::Theme => {
                if self.ascii_assets.theme_id() == ui::DEFAULT_THEME_ID {
                    self.open_settings_picker(ui::SettingsPickerKind::Theme)
                } else {
                    self.set_settings_error(i18n::msg!("settings-icon-theme-only"))
                }
            }
            ui::SettingsField::BorderColor => {
                self.open_settings_picker(ui::SettingsPickerKind::BorderColor)
            }
            ui::SettingsField::AccentColor => {
                self.open_settings_picker(ui::SettingsPickerKind::AccentColor)
            }
            ui::SettingsField::Language => {
                self.open_settings_picker(ui::SettingsPickerKind::Language)
            }
            ui::SettingsField::Timezone => {
                self.open_settings_picker(ui::SettingsPickerKind::Timezone)
            }
            ui::SettingsField::TimeSyncServer => self.open_settings_time_sync_server(),
            ui::SettingsField::WeatherLocation => self.open_settings_weather_location(),
            ui::SettingsField::ExplorerOpenExtensions => self.open_settings_file_extensions(),
            ui::SettingsField::ResetAnimationSpeed => self.reset_settings_animation_speed(),
            ui::SettingsField::RestoreDefaults => self.request_settings_restore_defaults(),
            ui::SettingsField::CheckUpdates => self.begin_update_check(),
            ui::SettingsField::StartUpdate => self.open_update_confirmation(),
            ui::SettingsField::InstalledVersion | ui::SettingsField::RemoteVersion => {}
            _ => self.adjust_selected_setting(1, platform),
        }
    }

    pub(in crate::session) fn adjust_selected_setting(
        &mut self,
        direction: i8,
        platform: &dyn Platform,
    ) {
        let Some(field) = self
            .settings_state
            .as_ref()
            .map(|state| state.selected_field)
        else {
            return;
        };
        if matches!(
            field,
            ui::SettingsField::Theme
                | ui::SettingsField::BorderColor
                | ui::SettingsField::AccentColor
                | ui::SettingsField::Language
                | ui::SettingsField::Timezone
                | ui::SettingsField::WeatherLocation
                | ui::SettingsField::ExplorerOpenExtensions
                | ui::SettingsField::TimeSyncServer
                | ui::SettingsField::CheckUpdates
                | ui::SettingsField::StartUpdate
        ) {
            self.activate_selected_setting(platform);
            return;
        }
        if matches!(
            field,
            ui::SettingsField::InstalledVersion | ui::SettingsField::RemoteVersion
        ) {
            return;
        }
        if field == ui::SettingsField::RestoreDefaults {
            self.request_settings_restore_defaults();
            return;
        }
        if field == ui::SettingsField::ResetAnimationSpeed {
            self.reset_settings_animation_speed();
            return;
        }
        if field == ui::SettingsField::BorderShape {
            let Some(mut appearance) = self.app.active_appearance().cloned() else {
                return;
            };
            appearance.border_shape = match appearance.border_shape {
                storage::BorderShape::Rounded => storage::BorderShape::Square,
                storage::BorderShape::Square => storage::BorderShape::Rounded,
            };
            self.save_settings_appearance(
                appearance,
                settings_saved_field(ui::SettingsField::BorderShape),
            );
            return;
        }
        if field == ui::SettingsField::MotionPreference {
            let Some(mut appearance) = self.app.active_appearance().cloned() else {
                return;
            };
            appearance.motion_preference = match appearance.motion_preference {
                storage::MotionPreference::Full => storage::MotionPreference::Reduced,
                storage::MotionPreference::Reduced => storage::MotionPreference::Full,
            };
            self.save_settings_appearance(
                appearance,
                settings_saved_field(ui::SettingsField::MotionPreference),
            );
            return;
        }
        if field == ui::SettingsField::AnimationSpeed {
            let Some(mut appearance) = self.app.active_appearance().cloned() else {
                return;
            };
            if appearance.motion_preference.reduced() {
                self.set_settings_error(i18n::msg!("settings-full-motion-required"));
                return;
            }
            let speed = appearance.normalized_animation_speed_percent();
            appearance.animation_speed_percent = if direction >= 0 {
                speed
                    .saturating_add(storage::ANIMATION_SPEED_STEP_PERCENT)
                    .min(storage::MAX_ANIMATION_SPEED_PERCENT)
            } else {
                speed
                    .saturating_sub(storage::ANIMATION_SPEED_STEP_PERCENT)
                    .max(storage::MIN_ANIMATION_SPEED_PERCENT)
            };
            self.save_settings_appearance(
                appearance,
                settings_saved_field(ui::SettingsField::AnimationSpeed),
            );
            return;
        }
        if field == ui::SettingsField::TimeSyncSource {
            self.change_time_sync_source(platform);
            return;
        }
        self.save_global_setting(field, direction);
    }

    pub(in crate::session) fn save_global_setting(
        &mut self,
        field: ui::SettingsField,
        direction: i8,
    ) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error(i18n::msg!("settings-storage-unavailable"));
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(i18n::msg!(
                    "settings-load-failed",
                    reason = error.to_string()
                ));
                return;
            }
        };
        let increase = direction >= 0;
        match field {
            ui::SettingsField::ShowHidden => {
                config.explorer.show_hidden = !config.explorer.show_hidden
            }
            ui::SettingsField::ShowSystem => {
                config.explorer.show_system = !config.explorer.show_system
            }
            ui::SettingsField::ShowExtensions => {
                config.explorer.show_extensions = !config.explorer.show_extensions
            }
            ui::SettingsField::FoldersFirst => {
                config.explorer.folders_first = !config.explorer.folders_first
            }
            ui::SettingsField::ShowSidebar => {
                config.explorer.show_sidebar = !config.explorer.show_sidebar
            }
            ui::SettingsField::CaseSensitiveSort => {
                config.explorer.case_sensitive_sort = !config.explorer.case_sensitive_sort
            }
            ui::SettingsField::SizeFormat => {
                config.explorer.size_format = match config.explorer.size_format {
                    storage::ExplorerSizeFormat::HumanBinary => storage::ExplorerSizeFormat::Bytes,
                    storage::ExplorerSizeFormat::Bytes => storage::ExplorerSizeFormat::HumanBinary,
                }
            }
            ui::SettingsField::DateZone => {
                config.explorer.date_zone = match config.explorer.date_zone {
                    storage::ExplorerDateZone::ConfiguredTimezone => storage::ExplorerDateZone::Utc,
                    storage::ExplorerDateZone::Utc => storage::ExplorerDateZone::ConfiguredTimezone,
                }
            }
            ui::SettingsField::SortField => {
                config.explorer.sort_field = cycle_explorer_sort_field(
                    config.explorer.sort_field,
                    if increase { 1 } else { -1 },
                )
            }
            ui::SettingsField::SortDirection => {
                config.explorer.sort_direction = match config.explorer.sort_direction {
                    storage::ExplorerSortDirection::Ascending => {
                        storage::ExplorerSortDirection::Descending
                    }
                    storage::ExplorerSortDirection::Descending => {
                        storage::ExplorerSortDirection::Ascending
                    }
                }
            }
            ui::SettingsField::ConfirmDelete => {
                config.explorer.confirm_delete = !config.explorer.confirm_delete
            }
            ui::SettingsField::ConfirmNameConflicts => {
                config.explorer.confirm_name_conflicts = !config.explorer.confirm_name_conflicts
            }
            ui::SettingsField::SystemLowAvailable => {
                config.system_status.low_available_gib = adjust_u16_setting(
                    config.system_status.low_available_gib,
                    increase,
                    storage::SYSTEM_STATUS_MIN_AVAILABLE_GIB,
                    storage::SYSTEM_STATUS_MAX_AVAILABLE_GIB,
                )
            }
            ui::SettingsField::SystemLowPercentage => {
                config.system_status.low_percentage = adjust_u8_setting_in_range(
                    config.system_status.low_percentage,
                    increase,
                    storage::SYSTEM_STATUS_MIN_PERCENTAGE,
                    storage::SYSTEM_STATUS_MAX_PERCENTAGE,
                )
            }
            ui::SettingsField::SystemCriticalAvailable => {
                config.system_status.critical_available_gib = adjust_u16_setting(
                    config.system_status.critical_available_gib,
                    increase,
                    storage::SYSTEM_STATUS_MIN_AVAILABLE_GIB,
                    storage::SYSTEM_STATUS_MAX_AVAILABLE_GIB,
                )
            }
            ui::SettingsField::SystemCriticalPercentage => {
                config.system_status.critical_percentage = adjust_u8_setting_in_range(
                    config.system_status.critical_percentage,
                    increase,
                    storage::SYSTEM_STATUS_MIN_PERCENTAGE,
                    storage::SYSTEM_STATUS_MAX_PERCENTAGE,
                )
            }
            ui::SettingsField::CursorAcceleration => {
                config.editor.cursor_acceleration_enabled =
                    !config.editor.cursor_acceleration_enabled
            }
            ui::SettingsField::CursorDelay => {
                config.editor.cursor_acceleration_delay_ms = adjust_u32_setting(
                    config.editor.cursor_acceleration_delay_ms,
                    EDITOR_CURSOR_TIME_STEP_MS,
                    increase,
                )
            }
            ui::SettingsField::CursorRamp => {
                config.editor.cursor_acceleration_ramp_ms = adjust_u32_setting(
                    config.editor.cursor_acceleration_ramp_ms,
                    EDITOR_CURSOR_TIME_STEP_MS,
                    increase,
                )
            }
            ui::SettingsField::CursorHorizontalStep => {
                config.editor.cursor_horizontal_max_step =
                    adjust_u8_setting(config.editor.cursor_horizontal_max_step, increase)
            }
            ui::SettingsField::CursorVerticalStep => {
                config.editor.cursor_vertical_max_step =
                    adjust_u8_setting(config.editor.cursor_vertical_max_step, increase)
            }
            _ => return,
        }
        config.editor = normalized_editor_config(config.editor);
        config.system_status.normalize();
        if let Err(error) = self.save_settings_config_logged(&storage, &config) {
            self.set_settings_error(i18n::msg!(
                "settings-save-failed",
                reason = error.to_string()
            ));
            return;
        }
        self.replace_storage_config(config);
        if let Some(state) = self.settings_state.as_mut() {
            state.status = settings_saved_field(field).into();
        }
        self.notify_status(settings_saved_field(field));
    }

    pub(in crate::session) fn save_settings_appearance(
        &mut self,
        appearance: storage::AppearanceConfig,
        message: impl Into<i18n::LocalizedText>,
    ) -> bool {
        let message = message.into();
        if appearance.border_color == appearance.accent_color {
            self.set_settings_error(i18n::msg!("settings-accent-distinct"));
            return false;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error(i18n::msg!("settings-storage-unavailable"));
            return false;
        };
        let Some(actor) = self.app.auth_session().cloned() else {
            self.set_settings_error(i18n::msg!("settings-login-required"));
            return false;
        };
        let users = UserService::with_debug_policy(storage, self.debug_policy)
            .with_backend(self.identity_backend);
        match users.update_user_appearance(&actor, &actor.username, appearance) {
            Ok(account) => {
                self.app.dispatch_at(
                    app::AppCommand::SetActiveAppearance(Some(account.appearance)),
                    Instant::now(),
                );
                if let Some(state) = self.settings_state.as_mut() {
                    state.status = message.clone();
                }
                self.notify_status(message);
                true
            }
            Err(error) => {
                self.set_settings_error(i18n::msg!(
                    "settings-appearance-save-failed",
                    reason = error.to_string()
                ));
                false
            }
        }
    }

    pub(in crate::session) fn reset_settings_animation_speed(&mut self) {
        let Some(mut appearance) = self.app.active_appearance().cloned() else {
            return;
        };
        if appearance.motion_preference.reduced() {
            self.set_settings_error(i18n::msg!("settings-full-motion-reset"));
            return;
        }
        appearance.animation_speed_percent = storage::DEFAULT_ANIMATION_SPEED_PERCENT;
        self.save_settings_appearance(
            appearance,
            settings_saved_field(ui::SettingsField::ResetAnimationSpeed),
        );
    }

    pub(in crate::session) fn refresh_asset_cache_for_theme(
        &mut self,
        theme_id: &str,
    ) -> Result<(), ui::AssetError> {
        let root = self.ascii_assets.store().root().to_path_buf();
        self.ascii_assets = ui::RuntimeAsciiAssets::load_with_root(&root, theme_id)?;
        Ok(())
    }

    pub(in crate::session) fn open_settings_picker(&mut self, kind: ui::SettingsPickerKind) {
        if kind == ui::SettingsPickerKind::AnimationSpeed
            && self
                .app
                .active_appearance()
                .is_some_and(|appearance| appearance.motion_preference.reduced())
        {
            self.set_settings_error(i18n::msg!("settings-full-motion-required"));
            return;
        }
        if !matches!(
            kind,
            ui::SettingsPickerKind::Theme
                | ui::SettingsPickerKind::DefaultThemeIcons
                | ui::SettingsPickerKind::AnimationSpeed
                | ui::SettingsPickerKind::BorderColor
                | ui::SettingsPickerKind::AccentColor
        ) && !self.can_change_global_settings()
        {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let selected_index = self.settings_picker_initial_index(kind);
        let image_icons_supported = self.terminal_image_support;
        if let Some(state) = self.settings_state.as_mut() {
            state.picker = Some(SettingsPickerState {
                kind,
                query: String::new(),
                selected_index,
                window_start: selected_index.saturating_sub(4),
                image_icons_supported,
            });
            state.color_editor = None;
            state.weather_location_editor = None;
            state.file_extensions_editor = None;
            state.time_sync_server_editor = None;
            state.time_sync_validation_request_id = None;
            state.status = i18n::msg!("settings-choose-value").into();
        }
    }

    pub(in crate::session) fn settings_picker_initial_index(
        &self,
        kind: ui::SettingsPickerKind,
    ) -> usize {
        if self.settings_state.is_none() {
            return 0;
        }
        let config = self.app.storage_config();
        match kind {
            ui::SettingsPickerKind::Theme => 0,
            ui::SettingsPickerKind::DefaultThemeIcons => {
                if !self.terminal_image_support {
                    0
                } else {
                    self.app
                        .active_appearance()
                        .map(|appearance| match appearance.icon_display_mode {
                            storage::IconDisplayMode::Ascii => 0,
                            storage::IconDisplayMode::Image => 1,
                        })
                        .unwrap_or(0)
                }
            }
            ui::SettingsPickerKind::AnimationSpeed => self
                .app
                .active_appearance()
                .map(|appearance| {
                    animation_speed_picker_index(appearance.normalized_animation_speed_percent())
                })
                .unwrap_or(0),
            ui::SettingsPickerKind::Language => self
                .language_options()
                .iter()
                .position(|option| option.code == config.language)
                .unwrap_or(0),
            ui::SettingsPickerKind::Timezone => app::setup_timezone_options()
                .iter()
                .position(|option| option.id == config.timezone)
                .unwrap_or(0),
            ui::SettingsPickerKind::BorderColor => self
                .app
                .active_appearance()
                .map(|appearance| color_picker_initial_index(appearance.border_color))
                .unwrap_or(0),
            ui::SettingsPickerKind::AccentColor => self
                .app
                .active_appearance()
                .map(|appearance| color_picker_initial_index(appearance.accent_color))
                .unwrap_or(0),
        }
    }

    pub(in crate::session) fn handle_settings_picker_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() {
            return;
        }
        match &key.key {
            InputKey::Escape => {
                let return_to_theme = self
                    .settings_state
                    .as_ref()
                    .and_then(|state| state.picker.as_ref())
                    .is_some_and(|picker| picker.kind == ui::SettingsPickerKind::DefaultThemeIcons);
                if return_to_theme {
                    self.open_settings_picker(ui::SettingsPickerKind::Theme);
                } else if let Some(state) = self.settings_state.as_mut() {
                    state.picker = None;
                    state.status = i18n::msg!("settings-ready").into();
                }
            }
            InputKey::Up => self.select_settings_picker_delta(-1),
            InputKey::Down => self.select_settings_picker_delta(1),
            InputKey::PageUp => self.select_settings_picker_delta(-8),
            InputKey::PageDown => self.select_settings_picker_delta(8),
            InputKey::Home => self.select_settings_picker_at(0),
            InputKey::End => {
                let last = self
                    .settings_state
                    .as_ref()
                    .and_then(|state| state.picker.as_ref())
                    .map(|picker| settings_picker_options(picker, &self.language_options()))
                    .map(|options| options.len().saturating_sub(1))
                    .unwrap_or(0);
                self.select_settings_picker_at(last);
            }
            InputKey::Enter => self.apply_settings_picker_selection(),
            InputKey::Backspace => {
                if let Some(state) = self.settings_state.as_mut()
                    && let Some(picker) = state.picker.as_mut()
                    && matches!(
                        picker.kind,
                        ui::SettingsPickerKind::Language | ui::SettingsPickerKind::Timezone
                    )
                {
                    picker.query.pop();
                    picker.selected_index = 0;
                    picker.window_start = 0;
                }
            }
            InputKey::Char(character)
                if !character.is_control()
                    && self
                        .settings_state
                        .as_ref()
                        .and_then(|state| state.picker.as_ref())
                        .is_some_and(|picker| {
                            matches!(
                                picker.kind,
                                ui::SettingsPickerKind::Language | ui::SettingsPickerKind::Timezone
                            )
                        }) =>
            {
                if let Some(state) = self.settings_state.as_mut()
                    && let Some(picker) = state.picker.as_mut()
                {
                    picker.query.push(*character);
                    picker.selected_index = 0;
                    picker.window_start = 0;
                }
            }
            _ => {}
        }
    }

    pub(in crate::session) fn select_settings_picker_delta(&mut self, delta: isize) {
        let count = self
            .settings_state
            .as_ref()
            .and_then(|state| state.picker.as_ref())
            .map(|picker| settings_picker_options(picker, &self.language_options()))
            .map(|options| options.len())
            .unwrap_or(0);
        if count == 0 {
            return;
        }
        let current = self
            .settings_state
            .as_ref()
            .and_then(|state| state.picker.as_ref())
            .map(|picker| picker.selected_index)
            .unwrap_or(0) as isize;
        self.select_settings_picker_at(
            (current + delta).clamp(0, count.saturating_sub(1) as isize) as usize,
        );
    }

    pub(in crate::session) fn select_settings_picker_at(&mut self, index: usize) {
        let count = self
            .settings_state
            .as_ref()
            .and_then(|state| state.picker.as_ref())
            .map(|picker| settings_picker_options(picker, &self.language_options()))
            .map(|options| options.len())
            .unwrap_or(0);
        let visible = settings_picker_visible_rows(self.terminal_size.1);
        if let Some(state) = self.settings_state.as_mut()
            && let Some(picker) = state.picker.as_mut()
            && count > 0
        {
            picker.selected_index = index.min(count - 1);
            if picker.selected_index < picker.window_start {
                picker.window_start = picker.selected_index;
            } else if picker.selected_index >= picker.window_start.saturating_add(visible) {
                picker.window_start = picker
                    .selected_index
                    .saturating_add(1)
                    .saturating_sub(visible);
            }
        }
    }

    pub(in crate::session) fn apply_settings_picker_selection(&mut self) {
        let Some(picker) = self
            .settings_state
            .as_ref()
            .and_then(|state| state.picker.as_ref())
            .cloned()
        else {
            return;
        };
        let options = settings_picker_options(&picker, &self.language_options());
        let Some(option) = options.get(picker.selected_index).cloned() else {
            self.set_settings_error(i18n::msg!("settings-no-options"));
            return;
        };
        if !option.enabled {
            if let Some(state) = self.settings_state.as_mut() {
                state.status = i18n::msg!("settings-images-unavailable").into();
            }
            return;
        }
        match picker.kind {
            ui::SettingsPickerKind::Theme => {
                if self.ascii_assets.theme_id() != ui::DEFAULT_THEME_ID {
                    self.set_settings_error(i18n::msg!("settings-icon-theme-only"));
                    return;
                }
                self.open_settings_picker(ui::SettingsPickerKind::DefaultThemeIcons);
            }
            ui::SettingsPickerKind::DefaultThemeIcons => {
                let Some(mut appearance) = self.app.active_appearance().cloned() else {
                    return;
                };
                let icon_display_mode = if picker.selected_index == 0 {
                    storage::IconDisplayMode::Ascii
                } else {
                    storage::IconDisplayMode::Image
                };
                if appearance.icon_display_mode != icon_display_mode {
                    let theme_id = self.ascii_assets.theme_id().to_string();
                    if let Err(error) = self.refresh_asset_cache_for_theme(&theme_id) {
                        self.set_settings_error(i18n::msg!(
                            "settings-cache-refresh-failed",
                            theme = theme_id,
                            reason = error.to_string()
                        ));
                        return;
                    }
                }
                appearance.icon_display_mode = icon_display_mode;
                if self.save_settings_appearance(appearance, i18n::msg!("settings-saved-icon-mode"))
                    && let Some(state) = self.settings_state.as_mut()
                {
                    state.picker = None;
                }
            }
            ui::SettingsPickerKind::AnimationSpeed => {
                let Some(mut appearance) = self.app.active_appearance().cloned() else {
                    return;
                };
                if appearance.motion_preference.reduced() {
                    self.set_settings_error(i18n::msg!("settings-full-motion-required"));
                    return;
                }
                appearance.animation_speed_percent =
                    animation_speed_for_picker_index(picker.selected_index);
                if self.save_settings_appearance(
                    appearance,
                    settings_saved_field(ui::SettingsField::AnimationSpeed),
                ) && let Some(state) = self.settings_state.as_mut()
                {
                    state.picker = None;
                }
            }
            ui::SettingsPickerKind::Language => {
                self.save_region_picker_value(Some(option.detail), None)
            }
            ui::SettingsPickerKind::Timezone => {
                self.save_region_picker_value(None, option.timezone_id)
            }
            ui::SettingsPickerKind::BorderColor | ui::SettingsPickerKind::AccentColor => {
                if option.detail == "#RRGGBB" {
                    if let Some(state) = self.settings_state.as_mut() {
                        state.picker = None;
                        state.color_editor = Some(SettingsColorEditorState {
                            kind: picker.kind,
                            value: "#".to_string(),
                            error: None,
                        });
                    }
                    return;
                }
                let Ok(color) = option.detail.parse::<storage::BorderColor>() else {
                    self.set_settings_error(i18n::msg!("settings-invalid-color-option"));
                    return;
                };
                let Some(mut appearance) = self.app.active_appearance().cloned() else {
                    return;
                };
                match picker.kind {
                    ui::SettingsPickerKind::BorderColor => appearance.border_color = color,
                    ui::SettingsPickerKind::AccentColor => appearance.accent_color = color,
                    _ => {}
                }
                if self.save_settings_appearance(appearance, settings_saved_picker(picker.kind))
                    && let Some(state) = self.settings_state.as_mut()
                {
                    state.picker = None;
                }
            }
        }
    }

    pub(in crate::session) fn save_region_picker_value(
        &mut self,
        language: Option<String>,
        timezone: Option<String>,
    ) {
        self.save_region_picker_value_with(language, timezone, |session, storage, config| {
            session.save_settings_config_logged(storage, config)
        });
    }

    pub(in crate::session) fn save_region_picker_value_with(
        &mut self,
        language: Option<String>,
        timezone: Option<String>,
        persist: impl FnOnce(
            &Self,
            &StorageManager,
            &storage::StorageConfig,
        ) -> Result<(), storage::StorageError>,
    ) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error(i18n::msg!("settings-storage-unavailable"));
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(i18n::msg!(
                    "settings-load-failed",
                    reason = error.to_string()
                ));
                return;
            }
        };
        let previous_config = config.clone();
        let candidate = if let Some(code) = language {
            match self.prepare_language(&code) {
                Ok((catalog, loaded)) => {
                    config.language = loaded.snapshot.code().to_string();
                    Some((catalog, loaded))
                }
                Err(error) => {
                    self.report_language_failure(&error);
                    return;
                }
            }
        } else {
            None
        };
        if let Some(timezone) = timezone.clone() {
            config.timezone = timezone;
        }
        if let Err(error) = persist(self, &storage, &config) {
            if candidate.is_some() {
                self.rollback_failed_language_config(&storage, &previous_config, &config);
            }
            if let Some((_, loaded)) = &candidate {
                self.repaired_resource_paths.clear();
                self.fallback_resource_paths.clear();
                self.report_language_diagnostics(&loaded.diagnostics);
            }
            self.set_settings_error(i18n::msg!(
                "settings-save-failed",
                reason = error.to_string()
            ));
            return;
        }
        self.replace_storage_config(config);
        if let Some((catalog, loaded)) = candidate {
            self.publish_language(catalog, loaded);
        }
        if let Some(state) = self.settings_state.as_mut() {
            state.picker = None;
            state.status = i18n::msg!("settings-saved-region").into();
        }
        self.notify_status(i18n::msg!("settings-saved-region"));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn open_settings_weather_location(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let value = self
            .app
            .storage_config()
            .weather_location
            .clone()
            .unwrap_or_default();
        if let Some(state) = self.settings_state.as_mut() {
            state.weather_location_editor =
                Some(SettingsWeatherLocationEditorState { value, error: None });
            state.picker = None;
            state.color_editor = None;
            state.file_extensions_editor = None;
            state.time_sync_server_editor = None;
            state.time_sync_validation_request_id = None;
            state.status = i18n::msg!("settings-enter-weather").into();
        }
    }

    pub(in crate::session) fn open_settings_file_extensions(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let value = format_editor_explorer_open_extensions(
            &self.app.storage_config().editor.explorer_open_extensions,
        );
        if let Some(state) = self.settings_state.as_mut() {
            state.file_extensions_editor =
                Some(SettingsFileExtensionsEditorState { value, error: None });
            state.picker = None;
            state.color_editor = None;
            state.weather_location_editor = None;
            state.time_sync_server_editor = None;
            state.time_sync_validation_request_id = None;
            state.status = i18n::msg!("settings-enter-suffixes").into();
        }
    }

    pub(in crate::session) fn open_settings_time_sync_server(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        if self.app.storage_config().time_sync.source != storage::TimeSyncSource::NetworkServer {
            self.set_settings_error(i18n::msg!("settings-network-source-required"));
            return;
        }
        let value = self
            .app
            .storage_config()
            .time_sync
            .server_url
            .clone()
            .unwrap_or_default();
        if let Some(state) = self.settings_state.as_mut() {
            state.time_sync_server_editor = Some(SettingsTimeSyncServerEditorState {
                value,
                error: None,
                validating: false,
            });
            state.time_sync_validation_request_id = None;
            state.picker = None;
            state.color_editor = None;
            state.weather_location_editor = None;
            state.file_extensions_editor = None;
            state.status = i18n::msg!("settings-enter-time-server").into();
        }
    }

    pub(in crate::session) fn handle_settings_time_sync_server_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() {
            return;
        }
        let validating = self
            .settings_state
            .as_ref()
            .and_then(|state| state.time_sync_server_editor.as_ref())
            .is_some_and(|editor| editor.validating);
        match &key.key {
            InputKey::Escape => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.time_sync_server_editor = None;
                    state.time_sync_validation_request_id = None;
                    state.status = i18n::msg!("settings-ready").into();
                }
            }
            _ if validating => {}
            InputKey::Backspace => {
                if let Some(editor) = self
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.time_sync_server_editor.as_mut())
                {
                    editor.value.pop();
                    editor.error = None;
                }
            }
            InputKey::Char(character) if !character.is_control() => {
                if let Some(editor) = self
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.time_sync_server_editor.as_mut())
                {
                    if editor.value.len() >= time::MAX_TIME_SERVER_URL_LEN {
                        editor.error = Some(
                            i18n::msg!(
                                "settings-server-limit",
                                limit = time::MAX_TIME_SERVER_URL_LEN
                            )
                            .into(),
                        );
                    } else {
                        editor.value.push(*character);
                        editor.error = None;
                    }
                }
            }
            InputKey::Enter => self.validate_settings_time_sync_server(),
            _ => {}
        }
    }

    pub(in crate::session) fn validate_settings_time_sync_server(&mut self) {
        let Some(value) = self
            .settings_state
            .as_ref()
            .and_then(|state| state.time_sync_server_editor.as_ref())
            .map(|editor| editor.value.clone())
        else {
            return;
        };
        let server_url = match time::normalize_time_server_url(&value) {
            Ok(server_url) => server_url,
            Err(error) => {
                if let Some(editor) = self
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.time_sync_server_editor.as_mut())
                {
                    editor.error = Some(error.clone().into());
                }
                self.show_time_sync_failure_dialog(i18n::msg!(
                    "settings-server-validation-failed",
                    reason = error.to_string()
                ));
                return;
            }
        };
        self.begin_settings_time_sync_validation(storage::TimeSyncConfig {
            source: storage::TimeSyncSource::NetworkServer,
            server_url: Some(server_url),
        });
    }

    pub(in crate::session) fn change_time_sync_source(&mut self, platform: &dyn Platform) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let current = self.app.storage_config().time_sync.clone();
        match current.source {
            storage::TimeSyncSource::NetworkServer => match platform.system_time() {
                Ok(system_time) => {
                    let mut config = current;
                    config.source = storage::TimeSyncSource::OperatingSystem;
                    self.persist_validated_time_sync_config(
                        config,
                        DateTime::<Utc>::from(system_time),
                    );
                }
                Err(error) => self.show_time_sync_failure_dialog(i18n::msg!(
                    "settings-system-time-failed",
                    reason = error.to_string()
                )),
            },
            storage::TimeSyncSource::OperatingSystem => {
                let mut config = current;
                config.source = storage::TimeSyncSource::NetworkServer;
                self.begin_settings_time_sync_validation(config);
            }
        }
    }

    pub(in crate::session) fn begin_settings_time_sync_validation(
        &mut self,
        config: storage::TimeSyncConfig,
    ) {
        if self
            .settings_state
            .as_ref()
            .is_some_and(|state| state.time_sync_validation_request_id.is_some())
        {
            self.set_settings_error(i18n::msg!("settings-validation-running"));
            return;
        }
        match self
            .settings_task_runtime
            .submit_time_sync_validation(config)
        {
            Ok(request_id) => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.time_sync_validation_request_id = Some(request_id);
                    state.status = i18n::msg!("settings-testing-time").into();
                    if let Some(editor) = state.time_sync_server_editor.as_mut() {
                        editor.validating = true;
                        editor.error = None;
                    }
                }
                self.notify_status(i18n::msg!("settings-testing-time"));
            }
            Err(error) => self.show_time_sync_failure_dialog(error),
        }
    }

    pub(in crate::session) fn begin_update_check(&mut self) {
        self.settings_update_state.checked_once = true;
        self.settings_update_state.confirmation_open = false;
        self.settings_update_state.error = None;
        if !self.settings_task_runtime.update_supported() {
            self.settings_update_state.status = i18n::msg!("settings-update-unsupported").into();
            self.settings_update_state.phase = None;
            return;
        }
        if self.settings_update_state.busy || self.settings_task_runtime.update_busy() {
            self.settings_update_state.status = i18n::msg!("settings-update-running").into();
            return;
        }
        match self
            .settings_task_runtime
            .submit_update_check(app::update::current_build_identity())
        {
            Ok(()) => {
                self.settings_update_state.busy = true;
                self.settings_update_state.phase = Some(app::update::UpdatePhase::Checking);
                self.settings_update_state.status = i18n::msg!("settings-checking-github").into();
                self.notify_status(i18n::msg!("settings-checking-updates"));
            }
            Err(error) => self.set_update_error(error),
        }
    }

    pub(in crate::session) fn open_update_confirmation(&mut self) {
        if !self.settings_task_runtime.update_supported() {
            self.set_update_error(i18n::msg!("settings-update-unsupported"));
            return;
        }
        if !self.can_change_global_settings() {
            self.set_update_error(i18n::msg!("settings-update-admin-required"));
            return;
        }
        if self.settings_update_state.busy {
            self.set_update_error(i18n::msg!("settings-update-wait"));
            return;
        }
        let Some(check) = self.settings_update_state.check_result.as_ref() else {
            self.set_update_error(i18n::msg!("settings-update-check-first"));
            return;
        };
        let identity = app::update::current_build_identity();
        if matches!(check.relation, app::update::UpdateRelation::Identical) && !identity.dirty {
            self.settings_update_state.status = i18n::msg!("settings-update-current-build").into();
            return;
        }
        self.settings_update_state.confirmation_open = true;
        self.settings_update_state.confirm_selected = true;
    }

    pub(in crate::session) fn cancel_update_confirmation(&mut self) {
        self.settings_update_state.confirmation_open = false;
        self.settings_update_state.confirm_selected = true;
        self.settings_update_state.status = i18n::msg!("settings-update-cancelled").into();
    }

    pub(in crate::session) fn begin_confirmed_update(&mut self) {
        if !self.settings_update_state.confirmation_open {
            return;
        }
        self.settings_update_state.confirmation_open = false;
        if !self.can_change_global_settings() {
            self.set_update_error(i18n::msg!("settings-update-admin-required"));
            return;
        }
        let Some(check) = self.settings_update_state.check_result.clone() else {
            self.set_update_error(i18n::msg!("settings-update-check-expired"));
            return;
        };
        let install_dir = match std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        {
            Some(path) => path,
            None => {
                self.set_update_error(i18n::msg!("settings-installation-unavailable"));
                return;
            }
        };
        match self
            .settings_task_runtime
            .submit_update_prepare(check, install_dir)
        {
            Ok(()) => {
                self.settings_update_state.busy = true;
                self.settings_update_state.error = None;
                self.settings_update_state.activity =
                    Some(ui::components::UpdateActivityViewModel::default());
                self.settings_update_state.phase = Some(app::update::UpdatePhase::Downloading);
                self.settings_update_state.status =
                    i18n::msg!("settings-update-downloading").into();
                self.notify_status(i18n::msg!("settings-update-started"));
            }
            Err(error) => self.set_update_error(error),
        }
    }

    fn set_update_error(&mut self, message: impl Into<i18n::LocalizedText>) {
        let message = message.into();
        self.settings_update_state.busy = false;
        self.settings_update_state.phase = Some(app::update::UpdatePhase::Failed);
        self.settings_update_state.status =
            i18n::msg!("settings-update-failed", reason = message.clone()).into();
        self.settings_update_state.error = Some(message.clone());
        // Diagnostic output uses raw external text or stable message identity, never translated UI text.
        let diagnostic = match &message {
            i18n::LocalizedText::Raw(raw) => raw.clone(),
            i18n::LocalizedText::Message(message) => format!("{} {:?}", message.id, message.args),
        };
        self.settings_update_state
            .append_output(&format!("ERROR: {diagnostic}"));
        self.notify_status(i18n::msg!("settings-update-failed", reason = message));
    }

    pub(in crate::session) fn poll_settings_background_tasks(&mut self) {
        let events = self
            .settings_task_runtime
            .drain_time_sync_validation_events();
        for event in events {
            let active = self.settings_state.as_ref().is_some_and(|state| {
                state.time_sync_validation_request_id == Some(event.request_id)
            });
            if !active {
                continue;
            }
            if let Some(state) = self.settings_state.as_mut() {
                state.time_sync_validation_request_id = None;
                if let Some(editor) = state.time_sync_server_editor.as_mut() {
                    editor.validating = false;
                }
            }
            match event.result {
                Ok(utc) => self.persist_validated_time_sync_config(event.config, utc),
                Err(error) => {
                    let message = match event.config.server_url.as_deref() {
                        Some(server) => i18n::msg!(
                            "settings-server-sync-failed",
                            server = server,
                            reason = error.to_string()
                        ),
                        None => {
                            i18n::msg!("settings-default-sync-failed", reason = error.to_string())
                        }
                    };
                    if let Some(state) = self.settings_state.as_mut() {
                        state.status = i18n::msg!("settings-sync-test-failed").into();
                        if let Some(editor) = state.time_sync_server_editor.as_mut() {
                            editor.error = Some(i18n::msg!("settings-sync-review-error").into());
                        }
                    }
                    self.show_time_sync_failure_dialog(message);
                }
            }
        }

        for event in self.settings_task_runtime.drain_update_events() {
            match event {
                SettingsUpdateTaskEvent::Progress(progress) => {
                    self.settings_update_state.apply_progress(progress);
                }
                SettingsUpdateTaskEvent::CheckCompleted(Ok(result)) => {
                    self.settings_update_state.busy = false;
                    self.settings_update_state.phase = None;
                    self.settings_update_state.error = None;
                    self.settings_update_state.checked_at = Some(Utc::now());
                    self.settings_update_state.status =
                        update_relation_label(&result.relation).into();
                    self.settings_update_state.check_result = Some(result);
                }
                SettingsUpdateTaskEvent::CheckCompleted(Err(error))
                | SettingsUpdateTaskEvent::PrepareCompleted(Err(error)) => {
                    self.set_update_error(error);
                }
                SettingsUpdateTaskEvent::PrepareCompleted(Ok(manifest_path)) => {
                    self.settings_update_state.busy = false;
                    self.settings_update_state.phase =
                        Some(app::update::UpdatePhase::WaitingForRestart);
                    self.settings_update_state.status =
                        i18n::msg!("settings-update-restarting").into();
                    self.update_apply_manifest = manifest_path;
                    self.shutdown_requested = true;
                }
            }
        }
    }

    pub(in crate::session) fn persist_validated_time_sync_config(
        &mut self,
        time_sync: storage::TimeSyncConfig,
        utc: DateTime<Utc>,
    ) {
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error(i18n::msg!("settings-storage-unavailable"));
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(i18n::msg!(
                    "settings-load-failed",
                    reason = error.to_string()
                ));
                return;
            }
        };
        config.time_sync = time_sync;
        if let Err(error) = self.save_settings_config_logged(&storage, &config) {
            self.set_settings_error(i18n::msg!(
                "settings-save-failed",
                reason = error.to_string()
            ));
            return;
        }
        self.replace_storage_config(config);
        self.apply_time_sync_utc(utc);
        if let Some(state) = self.settings_state.as_mut() {
            state.time_sync_server_editor = None;
            state.time_sync_validation_request_id = None;
            state.status = i18n::msg!("settings-saved-time-sync").into();
        }
        self.notify_status(i18n::msg!("settings-saved-time-sync"));
    }

    pub(in crate::session) fn handle_settings_file_extensions_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() {
            return;
        }
        match &key.key {
            InputKey::Escape => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.file_extensions_editor = None;
                    state.status = i18n::msg!("settings-ready").into();
                }
            }
            InputKey::Backspace => {
                if let Some(editor) = self
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.file_extensions_editor.as_mut())
                {
                    editor.value.pop();
                    editor.error = None;
                }
            }
            InputKey::Char(character) => {
                let Some(editor) = self
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.file_extensions_editor.as_mut())
                else {
                    return;
                };
                if !is_editor_extension_input_character(*character) {
                    editor.error = Some(i18n::msg!("settings-suffix-characters").into());
                } else if editor.value.len() >= EDITOR_EXTENSIONS_INPUT_MAX_LEN {
                    editor.error = Some(
                        i18n::msg!(
                            "settings-suffix-limit",
                            limit = EDITOR_EXTENSIONS_INPUT_MAX_LEN
                        )
                        .into(),
                    );
                } else {
                    editor.value.push(*character);
                    editor.error = None;
                }
            }
            InputKey::Enter => self.save_settings_file_extensions(),
            _ => {}
        }
    }

    pub(in crate::session) fn save_settings_file_extensions(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let Some(value) = self
            .settings_state
            .as_ref()
            .and_then(|state| state.file_extensions_editor.as_ref())
            .map(|editor| editor.value.clone())
        else {
            return;
        };
        let extensions = match parse_editor_explorer_open_extensions(&value) {
            Ok(extensions) => extensions,
            Err(error) => {
                if let Some(editor) = self
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.file_extensions_editor.as_mut())
                {
                    editor.error = Some(error);
                }
                return;
            }
        };
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error(i18n::msg!("settings-storage-unavailable"));
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(i18n::msg!(
                    "settings-load-failed",
                    reason = error.to_string()
                ));
                return;
            }
        };
        config.editor.explorer_open_extensions = extensions;
        if let Err(error) = self.save_settings_config_logged(&storage, &config) {
            self.set_settings_error(i18n::msg!(
                "settings-save-failed",
                reason = error.to_string()
            ));
            return;
        }
        self.replace_storage_config(config);
        if let Some(state) = self.settings_state.as_mut() {
            state.file_extensions_editor = None;
            state.status = i18n::msg!("settings-saved-suffixes").into();
        }
        self.notify_status(i18n::msg!("settings-saved-suffixes"));
    }

    pub(in crate::session) fn handle_settings_weather_location_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() {
            return;
        }
        match &key.key {
            InputKey::Escape => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.weather_location_editor = None;
                    state.status = i18n::msg!("settings-ready").into();
                }
            }
            InputKey::Backspace => {
                if let Some(state) = self.settings_state.as_mut()
                    && let Some(editor) = state.weather_location_editor.as_mut()
                {
                    editor.value.pop();
                    editor.error = None;
                }
            }
            InputKey::Char(character) => {
                let Some(editor) = self
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.weather_location_editor.as_mut())
                else {
                    return;
                };
                if !is_weather_location_character(*character) {
                    editor.error = Some(i18n::msg!("settings-weather-characters").into());
                } else if editor.value.len() >= WEATHER_LOCATION_MAX_LEN {
                    editor.error = Some(
                        i18n::msg!("settings-weather-limit", limit = WEATHER_LOCATION_MAX_LEN)
                            .into(),
                    );
                } else {
                    editor.value.push(*character);
                    editor.error = None;
                }
            }
            InputKey::Enter => self.request_settings_weather_location_confirmation(),
            _ => {}
        }
    }

    pub(in crate::session) fn request_settings_weather_location_confirmation(&mut self) {
        let Some(value) = self
            .settings_state
            .as_ref()
            .and_then(|state| state.weather_location_editor.as_ref())
            .map(|editor| editor.value.trim().to_string())
        else {
            return;
        };
        if value.is_empty() {
            self.save_settings_weather_location();
            return;
        }
        let notification = ShellNotification::modal(
            i18n::msg!("settings-confirm-weather"),
            i18n::msg!(
                "settings-weather-confirmation",
                location = format!("{value:?}")
            ),
            ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("save", i18n::msg!("settings-save"))
                    .with_shortcut(InputKey::Char('s'))
                    .with_follow_up(ShellCommand::SettingsWeatherLocationConfirmed),
                ShellNotificationAction::new("cancel", i18n::msg!("settings-cancel"))
                    .with_shortcut(InputKey::Escape)
                    .cancel(),
            ],
        )
        .with_key(SETTINGS_WEATHER_LOCATION_NOTIFICATION_KEY);
        self.notify_modal_with_options(notification);
    }

    pub(in crate::session) fn save_settings_weather_location(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let Some(value) = self
            .settings_state
            .as_ref()
            .and_then(|state| state.weather_location_editor.as_ref())
            .map(|editor| editor.value.trim().to_string())
        else {
            return;
        };
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error(i18n::msg!("settings-storage-unavailable"));
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(i18n::msg!(
                    "settings-load-failed",
                    reason = error.to_string()
                ));
                return;
            }
        };
        config.weather_location = (!value.is_empty()).then_some(value);
        if let Err(error) = self.save_settings_config_logged(&storage, &config) {
            self.set_settings_error(i18n::msg!(
                "settings-save-failed",
                reason = error.to_string()
            ));
            return;
        }
        self.replace_storage_config(config);
        if let Some(state) = self.settings_state.as_mut() {
            state.weather_location_editor = None;
            state.status = i18n::msg!("settings-saved-weather").into();
        }
        self.notify_status(i18n::msg!("settings-saved-weather"));
    }

    pub(in crate::session) fn handle_settings_color_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() {
            return;
        }
        match &key.key {
            InputKey::Escape => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.color_editor = None;
                    state.status = i18n::msg!("settings-ready").into();
                }
            }
            InputKey::Backspace => {
                if let Some(state) = self.settings_state.as_mut()
                    && let Some(editor) = state.color_editor.as_mut()
                {
                    editor.value.pop();
                    editor.error = None;
                }
            }
            InputKey::Char(character)
                if (*character == '#' || character.is_ascii_hexdigit())
                    && self
                        .settings_state
                        .as_ref()
                        .and_then(|state| state.color_editor.as_ref())
                        .is_some_and(|editor| editor.value.len() < 7) =>
            {
                if let Some(state) = self.settings_state.as_mut()
                    && let Some(editor) = state.color_editor.as_mut()
                {
                    editor.value.push(*character);
                    editor.error = None;
                }
            }
            InputKey::Enter => self.apply_settings_custom_color(),
            _ => {}
        }
    }

    pub(in crate::session) fn apply_settings_custom_color(&mut self) {
        let Some(editor) = self
            .settings_state
            .as_ref()
            .and_then(|state| state.color_editor.as_ref())
            .cloned()
        else {
            return;
        };
        let color = match editor.value.parse::<storage::BorderColor>() {
            Ok(color) => color,
            Err(_) => {
                if let Some(state) = self.settings_state.as_mut()
                    && let Some(color_editor) = state.color_editor.as_mut()
                {
                    color_editor.error = Some(i18n::msg!("settings-invalid-custom-color").into());
                }
                return;
            }
        };
        let Some(mut appearance) = self.app.active_appearance().cloned() else {
            return;
        };
        match editor.kind {
            ui::SettingsPickerKind::BorderColor => appearance.border_color = color,
            ui::SettingsPickerKind::AccentColor => appearance.accent_color = color,
            _ => return,
        }
        if self.save_settings_appearance(appearance, settings_saved_picker(editor.kind))
            && let Some(state) = self.settings_state.as_mut()
        {
            state.color_editor = None;
        }
    }

    pub(in crate::session) fn request_settings_restore_defaults(&mut self) {
        let Some(category) = self.settings_state.as_ref().map(|state| state.category) else {
            return;
        };
        if category != ui::SettingsCategory::Appearance && !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let notification = ShellNotification::modal(
            i18n::msg!("settings-restore-defaults"),
            i18n::msg!(
                "settings-restore-confirmation",
                category = format!("{category:?}")
            ),
            ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("restore", i18n::msg!("settings-restore"))
                    .with_shortcut(InputKey::Char('r'))
                    .with_follow_up(ShellCommand::SettingsRestoreDefaultsConfirmed),
                ShellNotificationAction::new("cancel", i18n::msg!("settings-cancel"))
                    .with_shortcut(InputKey::Escape)
                    .cancel(),
            ],
        )
        .with_key(SETTINGS_RESTORE_NOTIFICATION_KEY);
        self.notify_modal_with_options(notification);
    }

    pub(in crate::session) fn restore_settings_defaults(&mut self) {
        let Some(category) = self.settings_state.as_ref().map(|state| state.category) else {
            return;
        };
        if category == ui::SettingsCategory::Appearance {
            self.save_settings_appearance(
                storage::AppearanceConfig::default(),
                i18n::msg!("settings-saved-appearance-defaults"),
            );
            return;
        }
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error(i18n::msg!("settings-storage-unavailable"));
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(i18n::msg!(
                    "settings-load-failed",
                    reason = error.to_string()
                ));
                return;
            }
        };
        let previous_config = config.clone();
        let defaults = storage::StorageConfig::default();
        match category {
            ui::SettingsCategory::RegionTime => {
                config.language = defaults.language;
                config.timezone = defaults.timezone;
                config.time_sync = defaults.time_sync;
                config.weather_location = defaults.weather_location;
            }
            ui::SettingsCategory::System => config.system_status = defaults.system_status,
            ui::SettingsCategory::FileExplorer => config.explorer = defaults.explorer,
            ui::SettingsCategory::Editor => config.editor = defaults.editor,
            ui::SettingsCategory::Appearance => unreachable!(),
            ui::SettingsCategory::Update => return,
        }
        let candidate = if category == ui::SettingsCategory::RegionTime {
            match self.prepare_language(&config.language) {
                Ok(candidate) => Some(candidate),
                Err(error) => {
                    self.report_language_failure(&error);
                    return;
                }
            }
        } else {
            None
        };
        if let Err(error) = self.save_settings_config_logged(&storage, &config) {
            if candidate.is_some() {
                self.rollback_failed_language_config(&storage, &previous_config, &config);
            }
            if let Some((_, loaded)) = &candidate {
                self.repaired_resource_paths.clear();
                self.fallback_resource_paths.clear();
                self.report_language_diagnostics(&loaded.diagnostics);
            }
            self.set_settings_error(i18n::msg!(
                "settings-restore-failed",
                reason = error.to_string()
            ));
            return;
        }
        self.replace_storage_config(config);
        if let Some((catalog, loaded)) = candidate {
            self.publish_language(catalog, loaded);
        }
        if let Some(state) = self.settings_state.as_mut() {
            state.status = i18n::msg!(
                "settings-restored-category",
                category = format!("{category:?}")
            )
            .into();
        }
        self.notify_status(i18n::msg!(
            "settings-restored-category",
            category = format!("{category:?}")
        ));
    }

    pub(in crate::session) fn set_settings_error(
        &mut self,
        message: impl Into<i18n::LocalizedText>,
    ) {
        let message = message.into();
        if let Some(state) = self.settings_state.as_mut() {
            state.status = i18n::msg!("settings-error", reason = message.clone()).into();
        }
        self.notify_status(i18n::msg!("settings-status-error", reason = message));
    }

    pub fn to_settings_view_model(&self) -> Option<ui::SettingsViewModel> {
        let _language = i18n::enter_snapshot(self.language.clone());
        let state = self.settings_state.as_ref()?;
        let config = self.app.storage_config();
        let appearance = self.app.active_appearance()?;
        let global_enabled = self.can_change_global_settings();
        let identity = app::update::current_build_identity();
        let cards = if state.category == ui::SettingsCategory::Update {
            update_settings_cards(
                &identity,
                &self.settings_update_state,
                self.settings_task_runtime.update_supported(),
                global_enabled,
            )
        } else {
            settings_cards(
                state,
                config,
                appearance,
                global_enabled,
                self.ascii_assets.theme_id(),
                self.terminal_image_support,
                &self.language_options(),
            )
        };
        let appearance_preview = (state.category == ui::SettingsCategory::Appearance).then_some(
            ui::SettingsAppearancePreview {
                border_shape: match appearance.border_shape {
                    storage::BorderShape::Rounded => ui::BorderShape::Rounded,
                    storage::BorderShape::Square => ui::BorderShape::Square,
                },
                border_color: ui_theme_color(appearance.border_color),
                accent_color: ui_theme_color(appearance.accent_color),
            },
        );
        let picker = state.picker.as_ref().map(|picker| {
            let options = settings_picker_options(picker, &self.language_options());
            ui::SettingsPickerViewModel {
                kind: picker.kind,
                title: picker_title(picker.kind),
                query: picker.query.clone(),
                selected_index: picker.selected_index.min(options.len().saturating_sub(1)),
                window_start: picker.window_start,
                searchable: matches!(
                    picker.kind,
                    ui::SettingsPickerKind::Language | ui::SettingsPickerKind::Timezone
                ),
                options,
            }
        });
        let color_editor =
            state
                .color_editor
                .as_ref()
                .map(|editor| ui::SettingsColorEditorViewModel {
                    title: i18n::tr!("settings-custom-label", label = picker_label(editor.kind)),
                    value: editor.value.clone(),
                    error: editor
                        .error
                        .as_ref()
                        .map(i18n::LocalizedText::render_current),
                });
        let weather_location_editor = state.weather_location_editor.as_ref().map(|editor| {
            ui::SettingsWeatherLocationEditorViewModel {
                value: editor.value.clone(),
                error: editor
                    .error
                    .as_ref()
                    .map(i18n::LocalizedText::render_current),
            }
        });
        let file_extensions_editor = state.file_extensions_editor.as_ref().map(|editor| {
            ui::SettingsFileExtensionsEditorViewModel {
                value: editor.value.clone(),
                error: editor
                    .error
                    .as_ref()
                    .map(i18n::LocalizedText::render_current),
            }
        });
        let time_sync_server_editor = state.time_sync_server_editor.as_ref().map(|editor| {
            ui::SettingsTimeSyncServerEditorViewModel {
                value: editor.value.clone(),
                error: editor
                    .error
                    .as_ref()
                    .map(i18n::LocalizedText::render_current),
                validating: editor.validating,
            }
        });
        let update = (state.category == ui::SettingsCategory::Update).then(|| {
            let check = self.settings_update_state.check_result.as_ref();
            let replacement = identity.dirty
                || check.is_some_and(|result| {
                    !matches!(result.relation, app::update::UpdateRelation::Behind { .. })
                });
            ui::SettingsUpdateViewModel {
                activity: self.settings_update_state.activity.clone(),
                commits: check
                    .map(|result| {
                        result
                            .commits
                            .iter()
                            .map(|commit| ui::SettingsUpdateCommitViewModel {
                                sha: short_sha(&commit.sha),
                                message: commit.message.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                empty_message: self.settings_update_state.status.render_current(),
                confirmation: self.settings_update_state.confirmation_open.then(|| {
                    ui::SettingsUpdateConfirmationViewModel {
                        title: if replacement {
                            i18n::tr!("settings-replace-github")
                        } else {
                            i18n::tr!("settings-install-update")
                        },
                        body: if replacement {
                            i18n::tr!("settings-update-replace-body")
                        } else {
                            i18n::tr!("settings-update-install-body")
                        },
                        confirm_label: if replacement {
                            i18n::tr!("settings-replace-restart")
                        } else {
                            i18n::tr!("settings-update-restart")
                        },
                        confirm_selected: self.settings_update_state.confirm_selected,
                    }
                }),
            }
        });
        Some(ui::SettingsViewModel {
            selected_category: state.category,
            selected_field: state.selected_field,
            cards,
            appearance_preview,
            status: if state.category == ui::SettingsCategory::Update {
                self.settings_update_state.status.render_current()
            } else {
                state.status.render_current()
            },
            locked_message: (!global_enabled && state.category != ui::SettingsCategory::Appearance)
                .then_some(i18n::tr!("settings-locked")),
            scroll_offset: state.scroll_offset,
            picker,
            color_editor,
            weather_location_editor,
            file_extensions_editor,
            time_sync_server_editor,
            update,
        })
    }
}

pub(in crate::session) fn settings_fields(
    category: ui::SettingsCategory,
) -> &'static [ui::SettingsField] {
    match category {
        ui::SettingsCategory::Appearance => APPEARANCE_SETTINGS_FIELDS,
        ui::SettingsCategory::RegionTime => REGION_SETTINGS_FIELDS,
        ui::SettingsCategory::System => SYSTEM_SETTINGS_FIELDS,
        ui::SettingsCategory::FileExplorer => EXPLORER_SETTINGS_FIELDS,
        ui::SettingsCategory::Editor => EDITOR_SETTINGS_FIELDS,
        ui::SettingsCategory::Update => UPDATE_SETTINGS_FIELDS,
    }
}

fn update_settings_cards(
    identity: &app::update::BuildIdentity,
    update: &SettingsUpdateState,
    supported: bool,
    admin: bool,
) -> Vec<ui::SettingsCardViewModel> {
    use ui::{
        SettingsCardViewModel as Card, SettingsControlKind as Kind, SettingsField as Field,
        SettingsItemViewModel as Item,
    };
    let local_sha = identity
        .commit_sha
        .as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| i18n::tr!("settings-unknown"));
    let local_state = if identity.dirty {
        i18n::tr!("settings-dirty")
    } else {
        i18n::tr!("settings-clean")
    };
    let (remote_value, remote_description) = update
        .check_result
        .as_ref()
        .map(|result| {
            let checked = update
                .checked_at
                .map(|value| value.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| i18n::tr!("settings-unknown-time"));
            (
                format!(
                    "{} @ {}",
                    result.default_branch,
                    short_sha(&result.head_sha)
                ),
                i18n::tr!(
                    "settings-remote-build-details",
                    sha = result.head_sha.clone(),
                    checked = checked,
                    relation = update_relation_label(&result.relation).render_current()
                ),
            )
        })
        .unwrap_or_else(|| {
            (
                i18n::tr!("settings-not-checked"),
                i18n::tr!("settings-check-help"),
            )
        });
    let identity_requires_replacement = identity.dirty
        || identity.commit_sha.is_none()
        || update.check_result.as_ref().is_some_and(|result| {
            matches!(
                result.relation,
                app::update::UpdateRelation::Ahead { .. }
                    | app::update::UpdateRelation::Diverged { .. }
                    | app::update::UpdateRelation::Unknown
            )
        });
    let can_start = supported
        && admin
        && !update.busy
        && update.check_result.as_ref().is_some_and(|result| {
            identity.dirty || !matches!(result.relation, app::update::UpdateRelation::Identical)
        });
    let start_label = if update
        .check_result
        .as_ref()
        .is_some_and(|result| matches!(result.relation, app::update::UpdateRelation::Identical))
        && !identity.dirty
    {
        i18n::tr!("settings-up-to-date")
    } else if identity_requires_replacement {
        i18n::tr!("settings-replace-github")
    } else {
        i18n::tr!("settings-start-update")
    };
    vec![
        Card::new(
            i18n::tr!("settings-installed-build"),
            vec![Item::new(
                Field::InstalledVersion,
                i18n::tr!("settings-version"),
                format!("{} ({})", identity.package_version, short_sha(&local_sha)),
                i18n::tr!(
                    "settings-local-build-details",
                    sha = local_sha,
                    state = local_state
                ),
                Kind::ReadOnly,
            )],
        ),
        Card::new(
            i18n::tr!("settings-github-default-branch"),
            vec![Item::new(
                Field::RemoteVersion,
                i18n::tr!("settings-latest-commit"),
                remote_value,
                remote_description,
                Kind::ReadOnly,
            )],
        ),
        Card::new(
            i18n::tr!("settings-actions"),
            vec![
                Item::new(
                    Field::CheckUpdates,
                    i18n::tr!("settings-check-again"),
                    if update.busy {
                        i18n::tr!("settings-working")
                    } else {
                        i18n::tr!("settings-check-github")
                    },
                    i18n::tr!("settings-check-description"),
                    Kind::Action,
                )
                .enabled(supported && !update.busy),
                Item::new(
                    Field::StartUpdate,
                    start_label,
                    if admin {
                        i18n::tr!("settings-confirm-once")
                    } else {
                        i18n::tr!("settings-admin-only")
                    },
                    "",
                    Kind::Action,
                )
                .enabled(can_start),
            ],
        ),
    ]
}

fn short_sha(value: &str) -> String {
    value.chars().take(7).collect()
}

fn update_relation_label(relation: &app::update::UpdateRelation) -> i18n::LocalizedMessage {
    match relation {
        app::update::UpdateRelation::Identical => i18n::msg!("settings-up-to-date"),
        app::update::UpdateRelation::Behind { remote_ahead } => {
            i18n::msg!("settings-update-behind", count = *remote_ahead)
        }
        app::update::UpdateRelation::Ahead { local_ahead } => {
            i18n::msg!("settings-update-ahead", count = *local_ahead)
        }
        app::update::UpdateRelation::Diverged {
            remote_ahead,
            local_ahead,
        } => i18n::msg!(
            "settings-update-diverged",
            remote = *remote_ahead,
            local = *local_ahead
        ),
        app::update::UpdateRelation::Unknown => i18n::msg!("settings-unknown-commit"),
    }
}

pub(in crate::session) fn settings_cards(
    state: &SettingsState,
    config: &storage::StorageConfig,
    appearance: &storage::AppearanceConfig,
    global_enabled: bool,
    asset_theme_id: &str,
    image_icons_supported: bool,
    languages: &[app::SetupLanguageOption],
) -> Vec<ui::SettingsCardViewModel> {
    use ui::{
        SettingsCardViewModel as Card, SettingsControlKind as Kind, SettingsField as Field,
        SettingsItemViewModel as Item,
    };
    let toggle = |field, label, value: bool, description, enabled| {
        Item::new(
            field,
            label,
            if value {
                i18n::tr!("settings-on")
            } else {
                i18n::tr!("settings-off")
            },
            description,
            Kind::Toggle,
        )
        .enabled(enabled)
    };
    let reset = |enabled| {
        Item::new(
            Field::RestoreDefaults,
            i18n::tr!("settings-restore-defaults"),
            i18n::tr!("settings-confirm"),
            i18n::tr!("settings-restore-help"),
            Kind::Action,
        )
        .enabled(enabled)
    };
    let motion_enabled = !appearance.motion_preference.reduced();
    match state.category {
        ui::SettingsCategory::Appearance => vec![
            Card::new(
                i18n::tr!("settings-theme"),
                vec![
                    Item::new(
                        Field::Theme,
                        i18n::tr!("settings-theme"),
                        if asset_theme_id == ui::DEFAULT_THEME_ID {
                            match (appearance.icon_display_mode, image_icons_supported) {
                                (storage::IconDisplayMode::Image, true) => {
                                    i18n::tr!("settings-default-image-icons")
                                }
                                _ => i18n::tr!("settings-default-ascii-icons"),
                            }
                        } else {
                            asset_theme_id.to_string()
                        },
                        if asset_theme_id == ui::DEFAULT_THEME_ID {
                            i18n::tr!("settings-icon-options-help")
                        } else {
                            i18n::tr!("settings-icon-switch-help")
                        },
                        Kind::Picker,
                    )
                    .enabled(asset_theme_id == ui::DEFAULT_THEME_ID),
                ],
            ),
            Card::new(
                i18n::tr!("settings-visual-style"),
                vec![
                    Item::new(
                        Field::BorderShape,
                        i18n::tr!("settings-border-shape"),
                        match appearance.border_shape {
                            storage::BorderShape::Rounded => i18n::tr!("settings-rounded"),
                            storage::BorderShape::Square => i18n::tr!("settings-square"),
                        },
                        i18n::tr!("settings-border-shape-help"),
                        Kind::Cycle,
                    ),
                    Item::new(
                        Field::BorderColor,
                        i18n::tr!("settings-border-color"),
                        settings_color_label(appearance.border_color),
                        i18n::tr!("settings-border-color-help"),
                        Kind::Palette,
                    ),
                    Item::new(
                        Field::AccentColor,
                        i18n::tr!("settings-accent-color"),
                        settings_color_label(appearance.accent_color),
                        i18n::tr!("settings-accent-color-help"),
                        Kind::Palette,
                    ),
                ],
            ),
            Card::new(
                i18n::tr!("settings-animation"),
                vec![
                    Item::new(
                        Field::MotionPreference,
                        i18n::tr!("settings-motion"),
                        match appearance.motion_preference {
                            storage::MotionPreference::Full => i18n::tr!("settings-full"),
                            storage::MotionPreference::Reduced => i18n::tr!("settings-reduced"),
                        },
                        i18n::tr!("settings-motion-help"),
                        Kind::Cycle,
                    ),
                    Item::new(
                        Field::AnimationSpeed,
                        i18n::tr!("settings-animation-speed"),
                        i18n::tr!(
                            "settings-value-percent",
                            value = appearance.normalized_animation_speed_percent()
                        ),
                        i18n::tr!("settings-animation-speed-help"),
                        Kind::Stepper,
                    )
                    .enabled(motion_enabled),
                    Item::new(
                        Field::ResetAnimationSpeed,
                        i18n::tr!("settings-restore-speed"),
                        i18n::tr!("settings-reset"),
                        i18n::tr!("settings-restore-speed-help"),
                        Kind::Action,
                    )
                    .enabled(motion_enabled),
                ],
            ),
            Card::new(i18n::tr!("settings-reset"), vec![reset(true)]),
        ],
        ui::SettingsCategory::RegionTime => vec![
            Card::new(
                i18n::tr!("settings-language-timezone"),
                vec![
                    Item::new(
                        Field::Language,
                        i18n::tr!("settings-language"),
                        language_label(&config.language, languages),
                        i18n::tr!("settings-language-help"),
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::Timezone,
                        i18n::tr!("settings-city-timezone"),
                        timezone_label(&config.timezone),
                        i18n::tr!("settings-timezone-help"),
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::WeatherLocation,
                        i18n::tr!("settings-weather-location"),
                        config
                            .weather_location
                            .clone()
                            .unwrap_or_else(|| i18n::tr!("settings-same-timezone")),
                        i18n::tr!("settings-weather-help"),
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                i18n::tr!("settings-time-sync"),
                vec![
                    Item::new(
                        Field::TimeSyncSource,
                        i18n::tr!("settings-time-source"),
                        time_sync_source_label(config.time_sync.source),
                        i18n::tr!("settings-time-source-help"),
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::TimeSyncServer,
                        i18n::tr!("settings-sync-server"),
                        config
                            .time_sync
                            .server_url
                            .clone()
                            .unwrap_or_else(|| i18n::tr!("settings-automatic-servers")),
                        i18n::tr!("settings-time-server-help"),
                        Kind::Picker,
                    )
                    .enabled(
                        global_enabled
                            && config.time_sync.source == storage::TimeSyncSource::NetworkServer,
                    ),
                ],
            ),
            Card::new(i18n::tr!("settings-reset"), vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::System => vec![
            Card::new(
                i18n::tr!("settings-storage-pressure"),
                vec![
                    Item::new(
                        Field::SystemLowAvailable,
                        i18n::tr!("settings-low-available"),
                        i18n::tr!(
                            "settings-value-gib",
                            value = config.system_status.low_available_gib
                        ),
                        i18n::tr!("settings-low-available-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SystemLowPercentage,
                        i18n::tr!("settings-low-percentage"),
                        i18n::tr!(
                            "settings-value-percent",
                            value = config.system_status.low_percentage
                        ),
                        i18n::tr!("settings-low-percentage-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SystemCriticalAvailable,
                        i18n::tr!("settings-critical-available"),
                        i18n::tr!(
                            "settings-value-gib",
                            value = config.system_status.critical_available_gib
                        ),
                        i18n::tr!("settings-critical-available-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SystemCriticalPercentage,
                        i18n::tr!("settings-critical-percentage"),
                        i18n::tr!(
                            "settings-value-percent",
                            value = config.system_status.critical_percentage
                        ),
                        i18n::tr!("settings-critical-percentage-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(i18n::tr!("settings-reset"), vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::FileExplorer => vec![
            Card::new(
                i18n::tr!("settings-display"),
                vec![
                    toggle(
                        Field::ShowHidden,
                        i18n::tr!("settings-show-hidden"),
                        config.explorer.show_hidden,
                        i18n::tr!("settings-show-hidden-help"),
                        global_enabled,
                    ),
                    toggle(
                        Field::ShowSystem,
                        i18n::tr!("settings-show-system"),
                        config.explorer.show_system,
                        i18n::tr!("settings-show-system-help"),
                        global_enabled,
                    ),
                    toggle(
                        Field::ShowExtensions,
                        i18n::tr!("settings-show-extensions"),
                        config.explorer.show_extensions,
                        i18n::tr!("settings-show-extensions-help"),
                        global_enabled,
                    ),
                    toggle(
                        Field::FoldersFirst,
                        i18n::tr!("settings-folders-first"),
                        config.explorer.folders_first,
                        i18n::tr!("settings-folders-first-help"),
                        global_enabled,
                    ),
                    toggle(
                        Field::ShowSidebar,
                        i18n::tr!("settings-show-quick-access"),
                        config.explorer.show_sidebar,
                        i18n::tr!("settings-quick-access-help"),
                        global_enabled,
                    ),
                ],
            ),
            Card::new(
                i18n::tr!("settings-sorting-format"),
                vec![
                    toggle(
                        Field::CaseSensitiveSort,
                        i18n::tr!("settings-case-sensitive"),
                        config.explorer.case_sensitive_sort,
                        i18n::tr!("settings-case-sensitive-help"),
                        global_enabled,
                    ),
                    Item::new(
                        Field::SizeFormat,
                        i18n::tr!("settings-size-format"),
                        size_format_label(config.explorer.size_format),
                        i18n::tr!("settings-size-format-help"),
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::DateZone,
                        i18n::tr!("settings-date-timezone"),
                        date_zone_label(config.explorer.date_zone),
                        i18n::tr!("settings-date-timezone-help"),
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SortField,
                        i18n::tr!("settings-default-sort-field"),
                        sort_field_label(config.explorer.sort_field),
                        i18n::tr!("settings-sort-field-help"),
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SortDirection,
                        i18n::tr!("settings-default-direction"),
                        sort_direction_label(config.explorer.sort_direction),
                        i18n::tr!("settings-sort-direction-help"),
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                i18n::tr!("settings-safety"),
                vec![
                    toggle(
                        Field::ConfirmDelete,
                        i18n::tr!("settings-confirm-delete"),
                        config.explorer.confirm_delete,
                        i18n::tr!("settings-confirm-delete-help"),
                        global_enabled,
                    ),
                    toggle(
                        Field::ConfirmNameConflicts,
                        i18n::tr!("settings-confirm-conflicts"),
                        config.explorer.confirm_name_conflicts,
                        i18n::tr!("settings-confirm-conflicts-help"),
                        global_enabled,
                    ),
                ],
            ),
            Card::new(i18n::tr!("settings-reset"), vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::Editor => vec![
            Card::new(
                i18n::tr!("settings-explorer-opening"),
                vec![
                    Item::new(
                        Field::ExplorerOpenExtensions,
                        i18n::tr!("settings-open-editor"),
                        editor_extensions_summary(&config.editor.explorer_open_extensions),
                        i18n::tr!("settings-open-editor-help"),
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                i18n::tr!("settings-cursor-acceleration"),
                vec![
                    toggle(
                        Field::CursorAcceleration,
                        i18n::tr!("settings-cursor-acceleration"),
                        config.editor.cursor_acceleration_enabled,
                        i18n::tr!("settings-cursor-acceleration-help"),
                        global_enabled,
                    ),
                    Item::new(
                        Field::CursorDelay,
                        i18n::tr!("settings-start-delay"),
                        i18n::tr!(
                            "settings-value-ms",
                            value = config.editor.cursor_acceleration_delay_ms
                        ),
                        i18n::tr!("settings-cursor-delay-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::CursorRamp,
                        i18n::tr!("settings-ramp-maximum"),
                        i18n::tr!(
                            "settings-value-ms",
                            value = config.editor.cursor_acceleration_ramp_ms
                        ),
                        i18n::tr!("settings-cursor-ramp-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::CursorHorizontalStep,
                        i18n::tr!("settings-horizontal-maximum"),
                        i18n::tr!(
                            "settings-value-cells",
                            value = config.editor.cursor_horizontal_max_step
                        ),
                        i18n::tr!("settings-horizontal-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::CursorVerticalStep,
                        i18n::tr!("settings-vertical-maximum"),
                        i18n::tr!(
                            "settings-value-lines",
                            value = config.editor.cursor_vertical_max_step
                        ),
                        i18n::tr!("settings-vertical-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(i18n::tr!("settings-reset"), vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::Update => Vec::new(),
    }
}

pub(in crate::session) fn settings_picker_options(
    picker: &SettingsPickerState,
    languages: &[app::SetupLanguageOption],
) -> Vec<ui::SettingsPickerOptionViewModel> {
    let query = picker.query.trim().to_ascii_lowercase();
    match picker.kind {
        ui::SettingsPickerKind::Theme => vec![ui::SettingsPickerOptionViewModel::new(
            i18n::tr!("settings-default-theme"),
            i18n::tr!("settings-built-in-theme"),
        )],
        ui::SettingsPickerKind::DefaultThemeIcons => vec![
            ui::SettingsPickerOptionViewModel::new(
                i18n::tr!("settings-ascii-icons"),
                i18n::tr!("settings-ascii-icons-help"),
            ),
            ui::SettingsPickerOptionViewModel::new(
                i18n::tr!("settings-image-icons"),
                i18n::tr!("settings-image-icons-help"),
            )
            .enabled(picker.image_icons_supported),
        ],
        ui::SettingsPickerKind::AnimationSpeed => (storage::MIN_ANIMATION_SPEED_PERCENT
            ..=storage::MAX_ANIMATION_SPEED_PERCENT)
            .step_by(usize::from(storage::ANIMATION_SPEED_STEP_PERCENT))
            .map(|speed| {
                let detail = match speed.cmp(&storage::DEFAULT_ANIMATION_SPEED_PERCENT) {
                    std::cmp::Ordering::Less => i18n::tr!("settings-slower-default"),
                    std::cmp::Ordering::Equal => i18n::tr!("settings-default"),
                    std::cmp::Ordering::Greater => i18n::tr!("settings-faster-default"),
                };
                ui::SettingsPickerOptionViewModel::new(
                    i18n::tr!("settings-value-percent", value = speed),
                    detail,
                )
            })
            .collect(),
        ui::SettingsPickerKind::Language => languages
            .iter()
            .cloned()
            .filter(|option| {
                query.is_empty()
                    || option.code.to_ascii_lowercase().contains(&query)
                    || option.label.to_ascii_lowercase().contains(&query)
            })
            .map(|option| ui::SettingsPickerOptionViewModel::new(option.label, option.code))
            .collect(),
        ui::SettingsPickerKind::Timezone => app::setup_timezone_options()
            .into_iter()
            .filter(|option| {
                query.is_empty()
                    || option.id.to_ascii_lowercase().contains(&query)
                    || option.label.to_ascii_lowercase().contains(&query)
                    || option.description.to_ascii_lowercase().contains(&query)
                    || option
                        .localized_label()
                        .render_current()
                        .to_ascii_lowercase()
                        .contains(&query)
                    || option
                        .localized_description()
                        .render_current()
                        .to_ascii_lowercase()
                        .contains(&query)
            })
            .map(|option| {
                ui::SettingsPickerOptionViewModel::new(
                    option.localized_label().render_current(),
                    option.localized_description().render_current(),
                )
                .timezone(option.id, option.longitude, option.latitude)
            })
            .collect(),
        ui::SettingsPickerKind::BorderColor | ui::SettingsPickerKind::AccentColor => {
            let mut options = ui::setup_standard_color_options()
                .iter()
                .map(|option| {
                    ui::SettingsPickerOptionViewModel::new(option.label.clone(), option.value)
                })
                .collect::<Vec<_>>();
            options.push(ui::SettingsPickerOptionViewModel::new(
                i18n::tr!("settings-custom-color"),
                "#RRGGBB",
            ));
            options
        }
    }
}

pub(in crate::session) fn color_picker_initial_index(color: storage::BorderColor) -> usize {
    ui::setup_standard_color_options()
        .iter()
        .position(|option| option.value == color.to_string())
        .unwrap_or_else(|| ui::setup_standard_color_options().len())
}

pub(in crate::session) fn animation_speed_picker_index(speed: u16) -> usize {
    let normalized = speed.clamp(
        storage::MIN_ANIMATION_SPEED_PERCENT,
        storage::MAX_ANIMATION_SPEED_PERCENT,
    );
    let offset = normalized.saturating_sub(storage::MIN_ANIMATION_SPEED_PERCENT);
    usize::from(
        offset.saturating_add(storage::ANIMATION_SPEED_STEP_PERCENT / 2)
            / storage::ANIMATION_SPEED_STEP_PERCENT,
    )
}

pub(in crate::session) fn animation_speed_for_picker_index(index: usize) -> u16 {
    storage::MIN_ANIMATION_SPEED_PERCENT
        .saturating_add(
            u16::try_from(index)
                .unwrap_or(u16::MAX)
                .saturating_mul(storage::ANIMATION_SPEED_STEP_PERCENT),
        )
        .min(storage::MAX_ANIMATION_SPEED_PERCENT)
}

pub(in crate::session) fn settings_picker_visible_rows(terminal_height: u16) -> usize {
    usize::from(terminal_height.saturating_sub(10).clamp(4, 18))
}

pub(in crate::session) fn picker_title(kind: ui::SettingsPickerKind) -> String {
    match kind {
        ui::SettingsPickerKind::Theme => i18n::tr!("settings-choose-theme"),
        ui::SettingsPickerKind::DefaultThemeIcons => i18n::tr!("settings-default-theme"),
        ui::SettingsPickerKind::AnimationSpeed => i18n::tr!("settings-choose-speed"),
        ui::SettingsPickerKind::Language => i18n::tr!("settings-choose-language"),
        ui::SettingsPickerKind::Timezone => i18n::tr!("settings-choose-timezone"),
        ui::SettingsPickerKind::BorderColor => i18n::tr!("settings-choose-border-color"),
        ui::SettingsPickerKind::AccentColor => i18n::tr!("settings-choose-accent-color"),
    }
}

pub(in crate::session) fn picker_label(kind: ui::SettingsPickerKind) -> String {
    match kind {
        ui::SettingsPickerKind::Theme => i18n::tr!("settings-theme"),
        ui::SettingsPickerKind::DefaultThemeIcons => i18n::tr!("settings-default-icon-mode"),
        ui::SettingsPickerKind::AnimationSpeed => i18n::tr!("settings-animation-speed"),
        ui::SettingsPickerKind::BorderColor => i18n::tr!("settings-border-color"),
        ui::SettingsPickerKind::AccentColor => i18n::tr!("settings-accent-color"),
        ui::SettingsPickerKind::Language => i18n::tr!("settings-language"),
        ui::SettingsPickerKind::Timezone => i18n::tr!("settings-timezone"),
    }
}

pub(in crate::session) fn language_label(
    code: &str,
    languages: &[app::SetupLanguageOption],
) -> String {
    languages
        .iter()
        .find(|option| option.code == code)
        .map(|option| format!("{} ({})", option.label, option.code))
        .unwrap_or_else(|| code.to_string())
}

fn settings_color_label(color: storage::BorderColor) -> String {
    let value = color.to_string();
    ui::setup_standard_color_options()
        .into_iter()
        .find(|option| option.value == value)
        .map(|option| option.label)
        .unwrap_or(value)
}

pub(in crate::session) fn timezone_label(id: &str) -> String {
    app::setup_timezone_options()
        .into_iter()
        .find(|option| option.id == id)
        .map(|option| {
            format!(
                "{} ({})",
                option.localized_label().render_current(),
                option.id
            )
        })
        .unwrap_or_else(|| id.to_string())
}

pub(in crate::session) fn time_sync_source_label(source: storage::TimeSyncSource) -> String {
    match source {
        storage::TimeSyncSource::NetworkServer => i18n::tr!("settings-network-server"),
        storage::TimeSyncSource::OperatingSystem => i18n::tr!("settings-operating-system"),
    }
}

pub(in crate::session) fn cycle_explorer_sort_field(
    value: storage::ExplorerSortField,
    delta: isize,
) -> storage::ExplorerSortField {
    let values = [
        storage::ExplorerSortField::Name,
        storage::ExplorerSortField::Type,
        storage::ExplorerSortField::Size,
        storage::ExplorerSortField::Modified,
    ];
    let index = values.iter().position(|item| *item == value).unwrap_or(0) as isize;
    values[(index + delta).clamp(0, values.len().saturating_sub(1) as isize) as usize]
}

fn adjust_u16_setting(value: u16, increase: bool, minimum: u16, maximum: u16) -> u16 {
    if increase {
        value.saturating_add(1).min(maximum)
    } else {
        value.saturating_sub(1).max(minimum)
    }
}

fn adjust_u8_setting_in_range(value: u8, increase: bool, minimum: u8, maximum: u8) -> u8 {
    if increase {
        value.saturating_add(1).min(maximum)
    } else {
        value.saturating_sub(1).max(minimum)
    }
}

fn settings_saved_field(field: ui::SettingsField) -> i18n::LocalizedMessage {
    i18n::msg!("settings-saved-field", field = format!("{field:?}"))
}

fn settings_saved_picker(kind: ui::SettingsPickerKind) -> i18n::LocalizedMessage {
    let field = match kind {
        ui::SettingsPickerKind::BorderColor => ui::SettingsField::BorderColor,
        ui::SettingsPickerKind::AccentColor => ui::SettingsField::AccentColor,
        _ => unreachable!("only color pickers save appearance through this helper"),
    };
    settings_saved_field(field)
}

pub(in crate::session) fn is_weather_location_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, ' ' | ',' | '.' | '-' | '\'' | '/' | '(' | ')')
}

pub(in crate::session) fn is_editor_extension_input_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || character.is_ascii_whitespace()
        || matches!(character, '.' | ',' | ';' | '_' | '-' | '+')
}

pub(in crate::session) fn parse_editor_explorer_open_extensions(
    value: &str,
) -> Result<Vec<String>, i18n::LocalizedText> {
    let mut extensions = Vec::new();
    for raw in value.split(|character: char| {
        character == ',' || character == ';' || character.is_ascii_whitespace()
    }) {
        if raw.is_empty() {
            continue;
        }
        let Some(extension) = storage::normalize_editor_explorer_open_extension(raw) else {
            return Err(i18n::msg!("settings-invalid-suffix", suffix = format!("{raw:?}")).into());
        };
        if extensions.contains(&extension) {
            continue;
        }
        if extensions.len() >= storage::MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS {
            return Err(i18n::msg!(
                "settings-suffix-count-limit",
                limit = storage::MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS
            )
            .into());
        }
        extensions.push(extension);
    }
    Ok(extensions)
}

fn settings_scroll_offset(current: u16, delta: i16, maximum: u16) -> u16 {
    let next = if delta < 0 {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as u16)
    };
    next.min(maximum)
}

#[cfg(test)]
mod update_tests {
    use super::*;

    fn settings_language_snapshots() -> Vec<std::sync::Arc<i18n::LanguageSnapshot>> {
        let root = std::env::temp_dir().join(format!(
            "tux3-settings-locales-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let canonical =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets/locales");
        let mut snapshots = Vec::new();
        for code in ["en-US", "zh-CN"] {
            let locale = root.join("locales").join(code);
            std::fs::create_dir_all(locale.join("modules")).unwrap();
            for relative in [
                "manifest.toml",
                "modules/settings.ftl",
                "modules/ui-settings.ftl",
            ] {
                std::fs::copy(canonical.join(code).join(relative), locale.join(relative)).unwrap();
            }
            snapshots.push(std::sync::Arc::new(
                i18n::LanguageSnapshot::load(&root, code, 1)
                    .unwrap()
                    .snapshot,
            ));
        }
        std::fs::remove_dir_all(root).unwrap();
        snapshots
    }

    #[test]
    fn saved_settings_and_validation_messages_rerender_with_their_arguments() {
        let saved: i18n::LocalizedText =
            settings_saved_field(ui::SettingsField::BorderShape).into();
        let category: i18n::LocalizedText = i18n::msg!(
            "settings-category-status",
            category = format!("{:?}", ui::SettingsCategory::RegionTime)
        )
        .into();
        let invalid = parse_editor_explorer_open_extensions("bad/suffix").unwrap_err();
        let relation = update_relation_label(&app::update::UpdateRelation::Diverged {
            remote_ahead: 2,
            local_ahead: 3,
        });
        let error: i18n::LocalizedText = i18n::msg!(
            "settings-update-failed",
            reason = i18n::msg!("settings-admin-required")
        )
        .into();
        let retained = (
            saved.clone(),
            category.clone(),
            invalid.clone(),
            relation.clone(),
        );
        for (snapshot, expected_saved, expected_category, expected_invalid, expected_relation) in
            settings_language_snapshots()
                .into_iter()
                .zip([
                    (
                        "Saved Border shape",
                        "Settings: Region & Time",
                        "Invalid suffix",
                        "Builds diverged",
                    ),
                    (
                        "已保存边框形状",
                        "设置：区域与时间",
                        "无效后缀",
                        "构建已分叉",
                    ),
                ])
                .map(|(snapshot, (saved, category, invalid, relation))| {
                    (snapshot, saved, category, invalid, relation)
                })
        {
            let expected_error = if snapshot.code() == "en-US" {
                "Update failed: Administrator permission is required"
            } else {
                "更新失败：需要管理员权限"
            };
            let _language = i18n::enter_snapshot(snapshot);
            assert_eq!(saved.render_current(), expected_saved);
            assert_eq!(category.render_current(), expected_category);
            assert!(invalid.render_current().starts_with(expected_invalid));
            assert!(invalid.render_current().contains("bad/suffix"));
            let rendered_relation = relation.render_current();
            assert!(rendered_relation.starts_with(expected_relation));
            assert!(rendered_relation.contains('2'));
            assert!(rendered_relation.contains('3'));
            assert_eq!(error.render_current(), expected_error);
            assert_eq!(
                (&saved, &category, &invalid, &relation),
                (&retained.0, &retained.1, &retained.2, &retained.3)
            );
        }
    }

    #[test]
    fn update_cards_and_picker_labels_rerender_without_changing_action_or_color_values() {
        let identity = app::update::BuildIdentity {
            package_version: "1.2.3".to_string(),
            commit_sha: Some("1111111111111111".to_string()),
            dirty: false,
        };
        let update = checked_update_state(app::update::UpdateRelation::Behind { remote_ahead: 1 });
        let picker = SettingsPickerState {
            kind: ui::SettingsPickerKind::BorderColor,
            query: String::new(),
            selected_index: 0,
            window_start: 0,
            image_icons_supported: false,
        };
        for (snapshot, (start_label, custom_label)) in
            settings_language_snapshots().into_iter().zip([
                ("Start update", "Custom color…"),
                ("开始更新", "自定义颜色…"),
            ])
        {
            let _language = i18n::enter_snapshot(snapshot);
            let cards = update_settings_cards(&identity, &update, true, true);
            let start = cards
                .iter()
                .flat_map(|card| &card.items)
                .find(|item| item.field == ui::SettingsField::StartUpdate)
                .unwrap();
            assert_eq!(start.label, start_label);
            assert!(start.enabled);
            let options = settings_picker_options(&picker, &[]);
            let custom = options.last().unwrap();
            assert_eq!(custom.label, custom_label);
            assert_eq!(custom.detail, "#RRGGBB");
            assert_eq!(options[0].detail, "white");
        }
    }

    fn checked_update_state(relation: app::update::UpdateRelation) -> SettingsUpdateState {
        SettingsUpdateState {
            activity: None,
            check_result: Some(app::update::UpdateCheckResult {
                system_release: None,
                default_branch: "master".to_string(),
                head_sha: "abcdef1234567890".to_string(),
                relation,
                commits: vec![app::update::UpdateCommit {
                    sha: "abcdef1234567890".to_string(),
                    message: "Complete commit message\nwith body".to_string(),
                }],
            }),
            checked_at: Some(Utc::now()),
            phase: None,
            status: "Checked".into(),
            error: None,
            confirmation_open: false,
            confirm_selected: true,
            busy: false,
            checked_once: true,
        }
    }

    #[test]
    fn update_settings_enable_install_only_for_supported_admin_builds() {
        let identity = app::update::BuildIdentity {
            package_version: "0.1.1".to_string(),
            commit_sha: Some("1111111111111111".to_string()),
            dirty: false,
        };
        let update = checked_update_state(app::update::UpdateRelation::Behind { remote_ahead: 1 });
        let admin_cards = update_settings_cards(&identity, &update, true, true);
        let admin_start = admin_cards
            .iter()
            .flat_map(|card| &card.items)
            .find(|item| item.field == ui::SettingsField::StartUpdate)
            .unwrap();
        assert!(admin_start.enabled);
        assert_eq!(admin_start.label, "Start update");

        let user_cards = update_settings_cards(&identity, &update, true, false);
        let user_start = user_cards
            .iter()
            .flat_map(|card| &card.items)
            .find(|item| item.field == ui::SettingsField::StartUpdate)
            .unwrap();
        assert!(!user_start.enabled);

        let unsupported = update_settings_cards(&identity, &update, false, true);
        assert!(
            unsupported
                .iter()
                .flat_map(|card| &card.items)
                .filter(|item| {
                    matches!(
                        item.field,
                        ui::SettingsField::CheckUpdates | ui::SettingsField::StartUpdate
                    )
                })
                .all(|item| !item.enabled)
        );
    }

    #[test]
    fn update_settings_warn_for_dirty_and_diverged_builds() {
        let identity = app::update::BuildIdentity {
            package_version: "0.1.1".to_string(),
            commit_sha: Some("1111111111111111".to_string()),
            dirty: true,
        };
        let update = checked_update_state(app::update::UpdateRelation::Diverged {
            remote_ahead: 2,
            local_ahead: 3,
        });
        let cards = update_settings_cards(&identity, &update, true, true);
        let start = cards
            .iter()
            .flat_map(|card| &card.items)
            .find(|item| item.field == ui::SettingsField::StartUpdate)
            .unwrap();
        assert!(start.enabled);
        assert_eq!(start.label, "Replace with GitHub version");
        let remote = cards
            .iter()
            .flat_map(|card| &card.items)
            .find(|item| item.field == ui::SettingsField::RemoteVersion)
            .unwrap();
        assert!(remote.description.contains("Builds diverged"));
        assert!(remote.description.contains("abcdef1234567890"));
    }

    #[test]
    fn settings_scroll_stops_at_the_content_boundaries() {
        assert_eq!(settings_scroll_offset(2_001, 6, 2_004), 2_004);
        assert_eq!(settings_scroll_offset(2_007, -6, 2_100), 2_001);
        assert_eq!(settings_scroll_offset(0, -6, 2_100), 0);
        assert_eq!(settings_scroll_offset(0, 6, 0), 0);
    }
}

pub(in crate::session) fn format_editor_explorer_open_extensions(extensions: &[String]) -> String {
    extensions
        .iter()
        .map(|extension| format!(".{extension}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(in crate::session) fn editor_extensions_summary(extensions: &[String]) -> String {
    if extensions.is_empty() {
        return i18n::tr!("settings-system-default");
    }
    if extensions.len() <= 4 {
        return format_editor_explorer_open_extensions(extensions);
    }
    i18n::tr!(
        "settings-more-suffixes",
        suffixes = format_editor_explorer_open_extensions(&extensions[..3]),
        count = extensions.len() - 3
    )
}

pub(in crate::session) fn size_format_label(value: storage::ExplorerSizeFormat) -> String {
    match value {
        storage::ExplorerSizeFormat::HumanBinary => i18n::tr!("settings-human-binary"),
        storage::ExplorerSizeFormat::Bytes => i18n::tr!("settings-bytes"),
    }
}

pub(in crate::session) fn date_zone_label(value: storage::ExplorerDateZone) -> String {
    match value {
        storage::ExplorerDateZone::ConfiguredTimezone => i18n::tr!("settings-configured-timezone"),
        storage::ExplorerDateZone::Utc => i18n::tr!("settings-utc"),
    }
}

pub(in crate::session) fn sort_field_label(value: storage::ExplorerSortField) -> String {
    match value {
        storage::ExplorerSortField::Name => i18n::tr!("settings-name"),
        storage::ExplorerSortField::Type => i18n::tr!("settings-type"),
        storage::ExplorerSortField::Size => i18n::tr!("settings-size"),
        storage::ExplorerSortField::Modified => i18n::tr!("settings-modified"),
    }
}

pub(in crate::session) fn sort_direction_label(value: storage::ExplorerSortDirection) -> String {
    match value {
        storage::ExplorerSortDirection::Ascending => i18n::tr!("settings-ascending"),
        storage::ExplorerSortDirection::Descending => i18n::tr!("settings-descending"),
    }
}
