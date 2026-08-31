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
            self.notify_status("Guest access is read-only");
            return;
        }
        let Some(actor) = self.app.auth_session().cloned() else {
            self.error_message = Some("Login required".to_string());
            return;
        };
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };
        let config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.error_message = Some(format!("Could not load Settings: {error}"));
                self.notify_status("Settings unavailable");
                return;
            }
        };
        let users = UserService::with_debug_policy(storage, self.debug_policy);
        let appearance = match users.list_accessible_users(&actor).and_then(|users| {
            users
                .into_iter()
                .find(|user| user.id == actor.user_id)
                .map(|user| user.appearance)
                .ok_or(CoreError::UserNotFound)
        }) {
            Ok(appearance) => appearance,
            Err(error) => {
                self.error_message = Some(format!("Could not load your appearance: {error}"));
                self.notify_status("Settings unavailable");
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
            status: "Ready".to_string(),
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
        self.notify_status("Settings");
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
        self.notify_status("Ready");
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
            if self
                .settings_state
                .as_ref()
                .is_some_and(|state| state.picker.is_some())
            {
                self.select_settings_picker_delta(if direction == ScrollDirection::Up {
                    -3
                } else {
                    3
                });
            } else {
                self.scroll_settings(if direction == ScrollDirection::Up {
                    -3
                } else {
                    3
                });
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
            state.status = "Ready".to_string();
        }
        self.notify_status(format!("Settings: {}", category.label()));
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
        let Some(state) = self.settings_state.as_mut() else {
            return;
        };
        let fields = settings_fields(state.category);
        state.selected_field = fields[index.min(fields.len().saturating_sub(1))];
        state.scroll_offset = (index as u16).saturating_sub(6);
    }

    pub(in crate::session) fn scroll_settings(&mut self, delta: i16) {
        if let Some(state) = self.settings_state.as_mut() {
            state.scroll_offset = if delta < 0 {
                state.scroll_offset.saturating_sub(delta.unsigned_abs())
            } else {
                state.scroll_offset.saturating_add(delta as u16).min(2_000)
            };
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
                    self.set_settings_error(
                        "Icon mode can only be changed for the Default asset theme",
                    )
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
            self.save_settings_appearance(appearance, "Border shape");
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
            self.save_settings_appearance(appearance, "Motion");
            return;
        }
        if field == ui::SettingsField::AnimationSpeed {
            let Some(mut appearance) = self.app.active_appearance().cloned() else {
                return;
            };
            if appearance.motion_preference.reduced() {
                self.set_settings_error("Enable Full motion to adjust animation speed");
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
            self.save_settings_appearance(appearance, "Animation speed");
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
            self.set_settings_error("Administrator permission is required");
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error("Storage unavailable");
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(format!("Could not load Settings: {error}"));
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
        if let Err(error) = storage.save_config(&config) {
            self.set_settings_error(format!("Could not save Settings: {error}"));
            return;
        }
        self.replace_storage_config(config);
        if let Some(state) = self.settings_state.as_mut() {
            state.status = format!("Saved {}", settings_field_label(field));
        }
        self.notify_status(format!("Saved {}", settings_field_label(field)));
    }

    pub(in crate::session) fn save_settings_appearance(
        &mut self,
        appearance: storage::AppearanceConfig,
        label: &str,
    ) -> bool {
        if appearance.border_color == appearance.accent_color {
            self.set_settings_error("Accent color must differ from the border color");
            return false;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error("Storage unavailable");
            return false;
        };
        let Some(actor) = self.app.auth_session().cloned() else {
            self.set_settings_error("Login required");
            return false;
        };
        let users = UserService::with_debug_policy(storage, self.debug_policy);
        match users.update_user_appearance(&actor, &actor.username, appearance) {
            Ok(account) => {
                self.app.dispatch_at(
                    app::AppCommand::SetActiveAppearance(Some(account.appearance)),
                    Instant::now(),
                );
                if let Some(state) = self.settings_state.as_mut() {
                    state.status = format!("Saved {label}");
                }
                self.notify_status(format!("Saved {label}"));
                true
            }
            Err(error) => {
                self.set_settings_error(format!("Could not save appearance: {error}"));
                false
            }
        }
    }

    pub(in crate::session) fn reset_settings_animation_speed(&mut self) {
        let Some(mut appearance) = self.app.active_appearance().cloned() else {
            return;
        };
        if appearance.motion_preference.reduced() {
            self.set_settings_error("Enable Full motion to restore animation speed");
            return;
        }
        appearance.animation_speed_percent = storage::DEFAULT_ANIMATION_SPEED_PERCENT;
        self.save_settings_appearance(appearance, "Animation speed default");
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
            self.set_settings_error("Enable Full motion to adjust animation speed");
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
            self.set_settings_error("Administrator permission is required");
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
            state.status = "Choose a value".to_string();
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
            ui::SettingsPickerKind::Language => app::setup_language_options()
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
                    state.status = "Ready".to_string();
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
                    .map(settings_picker_options)
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
            .map(settings_picker_options)
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
            .map(settings_picker_options)
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
        let options = settings_picker_options(&picker);
        let Some(option) = options.get(picker.selected_index).cloned() else {
            self.set_settings_error("No matching options");
            return;
        };
        if !option.enabled {
            if let Some(state) = self.settings_state.as_mut() {
                state.status =
                    "Image icons are unavailable in this terminal; ASCII icons remain active"
                        .to_string();
            }
            return;
        }
        match picker.kind {
            ui::SettingsPickerKind::Theme => {
                if self.ascii_assets.theme_id() != ui::DEFAULT_THEME_ID {
                    self.set_settings_error(
                        "Icon mode can only be changed for the Default asset theme",
                    );
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
                        self.set_settings_error(format!(
                            "Could not refresh the {theme_id} asset cache: {error}"
                        ));
                        return;
                    }
                }
                appearance.icon_display_mode = icon_display_mode;
                if self.save_settings_appearance(appearance, "Default theme icon mode")
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
                    self.set_settings_error("Enable Full motion to adjust animation speed");
                    return;
                }
                appearance.animation_speed_percent =
                    animation_speed_for_picker_index(picker.selected_index);
                if self.save_settings_appearance(appearance, "Animation speed")
                    && let Some(state) = self.settings_state.as_mut()
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
                if option.label == "Custom color…" {
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
                    self.set_settings_error("Invalid color option");
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
                if self.save_settings_appearance(appearance, picker_label(picker.kind))
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
        if !self.can_change_global_settings() {
            self.set_settings_error("Administrator permission is required");
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error("Storage unavailable");
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(format!("Could not load Settings: {error}"));
                return;
            }
        };
        if let Some(language) = language {
            config.language = language;
        }
        if let Some(timezone) = timezone.clone() {
            config.timezone = timezone;
        }
        if let Err(error) = storage.save_config(&config) {
            self.set_settings_error(format!("Could not save Settings: {error}"));
            return;
        }
        self.replace_storage_config(config);
        if let Some(state) = self.settings_state.as_mut() {
            state.picker = None;
            state.status = "Saved region and time".to_string();
        }
        self.notify_status("Saved region and time");
    }

    pub(in crate::session) fn open_settings_weather_location(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error("Administrator permission is required");
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
            state.status = "Enter an English weather location".to_string();
        }
    }

    pub(in crate::session) fn open_settings_file_extensions(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error("Administrator permission is required");
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
            state.status = "Enter Explorer file suffixes".to_string();
        }
    }

    pub(in crate::session) fn open_settings_time_sync_server(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error("Administrator permission is required");
            return;
        }
        if self.app.storage_config().time_sync.source != storage::TimeSyncSource::NetworkServer {
            self.set_settings_error("Choose Network server as the time source first");
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
            state.status = "Enter a time synchronization server".to_string();
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
                    state.status = "Ready".to_string();
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
                        editor.error = Some(format!(
                            "The server address is limited to {} characters",
                            time::MAX_TIME_SERVER_URL_LEN
                        ));
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
                    editor.error = Some(error.clone());
                }
                self.show_time_sync_failure_dialog(format!(
                    "Could not validate the time synchronization server: {error}. The setting was not saved."
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
            self.set_settings_error("Administrator permission is required");
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
                Err(error) => self.show_time_sync_failure_dialog(format!(
                    "Could not read the operating system time: {error}"
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
            self.set_settings_error("A time sync validation is already running");
            return;
        }
        match self
            .settings_task_runtime
            .submit_time_sync_validation(config)
        {
            Ok(request_id) => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.time_sync_validation_request_id = Some(request_id);
                    state.status = "Testing time synchronization…".to_string();
                    if let Some(editor) = state.time_sync_server_editor.as_mut() {
                        editor.validating = true;
                        editor.error = None;
                    }
                }
                self.notify_status("Testing time synchronization…");
            }
            Err(error) => self.show_time_sync_failure_dialog(error),
        }
    }

    pub(in crate::session) fn begin_update_check(&mut self) {
        self.settings_update_state.checked_once = true;
        self.settings_update_state.confirmation_open = false;
        self.settings_update_state.error = None;
        if !self.settings_task_runtime.update_supported() {
            self.settings_update_state.status =
                "Automatic updates are supported only on Windows".to_string();
            self.settings_update_state.phase = None;
            return;
        }
        if self.settings_update_state.busy || self.settings_task_runtime.update_busy() {
            self.settings_update_state.status = "An update task is already running".to_string();
            return;
        }
        match self
            .settings_task_runtime
            .submit_update_check(app::update::current_build_identity())
        {
            Ok(()) => {
                self.settings_update_state.busy = true;
                self.settings_update_state.phase = Some(app::update::UpdatePhase::Checking);
                self.settings_update_state.status = "Checking GitHub…".to_string();
                self.notify_status("Checking for updates…");
            }
            Err(error) => self.set_update_error(error),
        }
    }

    pub(in crate::session) fn open_update_confirmation(&mut self) {
        if !self.settings_task_runtime.update_supported() {
            self.set_update_error("Automatic updates are supported only on Windows");
            return;
        }
        if !self.can_change_global_settings() {
            self.set_update_error("Administrator permission is required to install updates");
            return;
        }
        if self.settings_update_state.busy {
            self.set_update_error("Wait for the current update task to finish");
            return;
        }
        let Some(check) = self.settings_update_state.check_result.as_ref() else {
            self.set_update_error("Check GitHub before starting an update");
            return;
        };
        let identity = app::update::current_build_identity();
        if matches!(check.relation, app::update::UpdateRelation::Identical) && !identity.dirty {
            self.settings_update_state.status = "This build is already up to date".to_string();
            return;
        }
        self.settings_update_state.confirmation_open = true;
        self.settings_update_state.confirm_selected = true;
    }

    pub(in crate::session) fn cancel_update_confirmation(&mut self) {
        self.settings_update_state.confirmation_open = false;
        self.settings_update_state.confirm_selected = true;
        self.settings_update_state.status = "Update cancelled".to_string();
    }

    pub(in crate::session) fn begin_confirmed_update(&mut self) {
        if !self.settings_update_state.confirmation_open {
            return;
        }
        self.settings_update_state.confirmation_open = false;
        if !self.can_change_global_settings() {
            self.set_update_error("Administrator permission is required to install updates");
            return;
        }
        let Some(check) = self.settings_update_state.check_result.clone() else {
            self.set_update_error("The update check result is no longer available");
            return;
        };
        let install_dir = match std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        {
            Some(path) => path,
            None => {
                self.set_update_error("Could not locate the running TundraUX installation");
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
                self.settings_update_state.phase = Some(app::update::UpdatePhase::Downloading);
                self.settings_update_state.status =
                    "Downloading the selected GitHub source snapshot…".to_string();
                self.notify_status("Update started; TundraUX will restart automatically");
            }
            Err(error) => self.set_update_error(error),
        }
    }

    fn set_update_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.settings_update_state.busy = false;
        self.settings_update_state.phase = Some(app::update::UpdatePhase::Failed);
        self.settings_update_state.status = format!("Update failed: {message}");
        self.settings_update_state.error = Some(message.clone());
        self.notify_status(format!("Update failed: {message}"));
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
                        Some(server) => format!(
                            "Could not synchronize with {server}: {error}. The setting was not saved."
                        ),
                        None => format!(
                            "Could not synchronize with the default time servers: {error}. The setting was not saved."
                        ),
                    };
                    if let Some(state) = self.settings_state.as_mut() {
                        state.status = "Time synchronization test failed".to_string();
                        if let Some(editor) = state.time_sync_server_editor.as_mut() {
                            editor.error =
                                Some("Synchronization failed; review the error dialog".to_string());
                        }
                    }
                    self.show_time_sync_failure_dialog(message);
                }
            }
        }

        for event in self.settings_task_runtime.drain_update_events() {
            match event {
                SettingsUpdateTaskEvent::Progress(progress) => {
                    self.settings_update_state.phase = Some(progress.phase);
                    self.settings_update_state.status = progress.message;
                }
                SettingsUpdateTaskEvent::CheckCompleted(Ok(result)) => {
                    self.settings_update_state.busy = false;
                    self.settings_update_state.phase = None;
                    self.settings_update_state.error = None;
                    self.settings_update_state.checked_at = Some(Utc::now());
                    self.settings_update_state.status = update_relation_label(&result.relation);
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
                        "Update prepared; restarting TundraUX…".to_string();
                    self.update_apply_manifest = Some(manifest_path);
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
            self.set_settings_error("Storage unavailable");
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(format!("Could not load Settings: {error}"));
                return;
            }
        };
        config.time_sync = time_sync;
        if let Err(error) = storage.save_config(&config) {
            self.set_settings_error(format!("Could not save Settings: {error}"));
            return;
        }
        self.replace_storage_config(config);
        self.apply_time_sync_utc(utc);
        if let Some(state) = self.settings_state.as_mut() {
            state.time_sync_server_editor = None;
            state.time_sync_validation_request_id = None;
            state.status = "Saved time synchronization settings".to_string();
        }
        self.notify_status("Saved time synchronization settings");
    }

    pub(in crate::session) fn handle_settings_file_extensions_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() {
            return;
        }
        match &key.key {
            InputKey::Escape => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.file_extensions_editor = None;
                    state.status = "Ready".to_string();
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
                    editor.error = Some(
                        "Use ASCII letters, numbers, dots, commas, spaces, +, - or _".to_string(),
                    );
                } else if editor.value.len() >= EDITOR_EXTENSIONS_INPUT_MAX_LEN {
                    editor.error = Some(format!(
                        "The suffix list is limited to {EDITOR_EXTENSIONS_INPUT_MAX_LEN} characters"
                    ));
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
            self.set_settings_error("Administrator permission is required");
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
            self.set_settings_error("Storage unavailable");
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(format!("Could not load Settings: {error}"));
                return;
            }
        };
        config.editor.explorer_open_extensions = extensions;
        if let Err(error) = storage.save_config(&config) {
            self.set_settings_error(format!("Could not save Settings: {error}"));
            return;
        }
        self.replace_storage_config(config);
        if let Some(state) = self.settings_state.as_mut() {
            state.file_extensions_editor = None;
            state.status = "Saved Explorer file suffixes".to_string();
        }
        self.notify_status("Saved Explorer file suffixes");
    }

    pub(in crate::session) fn handle_settings_weather_location_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() {
            return;
        }
        match &key.key {
            InputKey::Escape => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.weather_location_editor = None;
                    state.status = "Ready".to_string();
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
                    editor.error = Some(
                        "Only English letters, numbers and common address punctuation are allowed"
                            .to_string(),
                    );
                } else if editor.value.len() >= WEATHER_LOCATION_MAX_LEN {
                    editor.error = Some(format!(
                        "Weather location is limited to {WEATHER_LOCATION_MAX_LEN} characters"
                    ));
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
            "Confirm weather location",
            format!(
                "Save {value:?}? Weather uses text search, so the match may be inaccurate or return no results."
            ),
            ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("save", "Save")
                    .with_shortcut(InputKey::Char('s'))
                    .with_follow_up(ShellCommand::SettingsWeatherLocationConfirmed),
                ShellNotificationAction::new("cancel", "Cancel")
                    .with_shortcut(InputKey::Escape)
                    .cancel(),
            ],
        )
        .with_key(SETTINGS_WEATHER_LOCATION_NOTIFICATION_KEY);
        self.notify_modal_with_options(notification);
    }

    pub(in crate::session) fn save_settings_weather_location(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error("Administrator permission is required");
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
            self.set_settings_error("Storage unavailable");
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(format!("Could not load Settings: {error}"));
                return;
            }
        };
        config.weather_location = (!value.is_empty()).then_some(value);
        if let Err(error) = storage.save_config(&config) {
            self.set_settings_error(format!("Could not save Settings: {error}"));
            return;
        }
        self.replace_storage_config(config);
        if let Some(state) = self.settings_state.as_mut() {
            state.weather_location_editor = None;
            state.status = "Saved weather location".to_string();
        }
        self.notify_status("Saved weather location");
    }

    pub(in crate::session) fn handle_settings_color_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() {
            return;
        }
        match &key.key {
            InputKey::Escape => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.color_editor = None;
                    state.status = "Ready".to_string();
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
            Err(error) => {
                if let Some(state) = self.settings_state.as_mut()
                    && let Some(color_editor) = state.color_editor.as_mut()
                {
                    color_editor.error = Some(error.to_string());
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
        if self.save_settings_appearance(appearance, picker_label(editor.kind))
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
            self.set_settings_error("Administrator permission is required");
            return;
        }
        let notification = ShellNotification::modal(
            "Restore defaults",
            format!(
                "Restore all {} settings to their defaults?",
                category.label()
            ),
            ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("restore", "Restore")
                    .with_shortcut(InputKey::Char('r'))
                    .with_follow_up(ShellCommand::SettingsRestoreDefaultsConfirmed),
                ShellNotificationAction::new("cancel", "Cancel")
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
                "Appearance defaults",
            );
            return;
        }
        if !self.can_change_global_settings() {
            self.set_settings_error("Administrator permission is required");
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error("Storage unavailable");
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(format!("Could not load Settings: {error}"));
                return;
            }
        };
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
        if let Err(error) = storage.save_config(&config) {
            self.set_settings_error(format!("Could not restore defaults: {error}"));
            return;
        }
        self.replace_storage_config(config);
        if let Some(state) = self.settings_state.as_mut() {
            state.status = format!("Restored {} defaults", category.label());
        }
        self.notify_status(format!("Restored {} defaults", category.label()));
    }

    pub(in crate::session) fn set_settings_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        if let Some(state) = self.settings_state.as_mut() {
            state.status = format!("Error: {message}");
        }
        self.notify_status(format!("Settings error: {message}"));
    }

    pub fn to_settings_view_model(&self) -> Option<ui::SettingsViewModel> {
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
            let options = settings_picker_options(picker);
            ui::SettingsPickerViewModel {
                kind: picker.kind,
                title: picker_title(picker.kind).to_string(),
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
                    title: format!("Custom {}", picker_label(editor.kind)),
                    value: editor.value.clone(),
                    error: editor.error.clone(),
                });
        let weather_location_editor = state.weather_location_editor.as_ref().map(|editor| {
            ui::SettingsWeatherLocationEditorViewModel {
                value: editor.value.clone(),
                error: editor.error.clone(),
            }
        });
        let file_extensions_editor = state.file_extensions_editor.as_ref().map(|editor| {
            ui::SettingsFileExtensionsEditorViewModel {
                value: editor.value.clone(),
                error: editor.error.clone(),
            }
        });
        let time_sync_server_editor = state.time_sync_server_editor.as_ref().map(|editor| {
            ui::SettingsTimeSyncServerEditorViewModel {
                value: editor.value.clone(),
                error: editor.error.clone(),
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
                empty_message: self.settings_update_state.status.clone(),
                confirmation: self.settings_update_state.confirmation_open.then(|| {
                    ui::SettingsUpdateConfirmationViewModel {
                        title: if replacement {
                            "Replace with GitHub version".to_string()
                        } else {
                            "Install update".to_string()
                        },
                        body: if replacement {
                            "This is a non-standard, dirty, ahead, diverged, or unknown build. TundraUX will download the exact checked GitHub commit, compile it locally, replace both programs and the Default theme, then restart immediately. If startup fails, the previous local version will be restored.".to_string()
                        } else {
                            "TundraUX will download the exact checked GitHub commit, compile it locally, replace both programs and the Default theme, then restart immediately. If startup fails, the previous local version will be restored.".to_string()
                        },
                        confirm_label: if replacement {
                            "Replace and restart".to_string()
                        } else {
                            "Update and restart".to_string()
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
                self.settings_update_state.status.clone()
            } else {
                state.status.clone()
            },
            locked_message: (!global_enabled && state.category != ui::SettingsCategory::Appearance)
                .then_some("Locked: administrator permission is required".to_string()),
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
        .unwrap_or_else(|| "unknown".to_string());
    let local_state = if identity.dirty { "dirty" } else { "clean" };
    let (remote_value, remote_description) = update
        .check_result
        .as_ref()
        .map(|result| {
            let checked = update
                .checked_at
                .map(|value| value.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "unknown time".to_string());
            (
                format!(
                    "{} @ {}",
                    result.default_branch,
                    short_sha(&result.head_sha)
                ),
                format!(
                    "Full SHA: {}. Checked {checked}. {}",
                    result.head_sha,
                    update_relation_label(&result.relation)
                ),
            )
        })
        .unwrap_or_else(|| {
            (
                "Not checked".to_string(),
                "Open this page or choose Check again to query GitHub.".to_string(),
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
        "Already up to date"
    } else if identity_requires_replacement {
        "Replace with GitHub version"
    } else {
        "Start update"
    };
    vec![
        Card::new(
            "Installed build",
            vec![Item::new(
                Field::InstalledVersion,
                "Version",
                format!("{} ({})", identity.package_version, short_sha(&local_sha)),
                format!("Full SHA: {local_sha}. Build state: {local_state}."),
                Kind::ReadOnly,
            )],
        ),
        Card::new(
            "GitHub default branch",
            vec![Item::new(
                Field::RemoteVersion,
                "Latest commit",
                remote_value,
                remote_description,
                Kind::ReadOnly,
            )],
        ),
        Card::new(
            "Actions",
            vec![
                Item::new(
                    Field::CheckUpdates,
                    "Check again",
                    if update.busy { "Working…" } else { "Check GitHub" },
                    "Refresh the default branch, commit relation, and commit messages.",
                    Kind::Action,
                )
                .enabled(supported && !update.busy),
                Item::new(
                    Field::StartUpdate,
                    start_label,
                    if admin { "Confirm once" } else { "Administrator only" },
                    "Download, compile, replace, and restart automatically. Rust is never installed automatically.",
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

fn update_relation_label(relation: &app::update::UpdateRelation) -> String {
    match relation {
        app::update::UpdateRelation::Identical => "Already up to date".to_string(),
        app::update::UpdateRelation::Behind { remote_ahead } => {
            format!("Behind GitHub by {remote_ahead} commit(s)")
        }
        app::update::UpdateRelation::Ahead { local_ahead } => {
            format!("Local build is ahead by {local_ahead} commit(s)")
        }
        app::update::UpdateRelation::Diverged {
            remote_ahead,
            local_ahead,
        } => format!(
            "Builds diverged: GitHub has {remote_ahead} new commit(s), local has {local_ahead}"
        ),
        app::update::UpdateRelation::Unknown => "The local commit is unknown to GitHub".to_string(),
    }
}

pub(in crate::session) fn settings_cards(
    state: &SettingsState,
    config: &storage::StorageConfig,
    appearance: &storage::AppearanceConfig,
    global_enabled: bool,
    asset_theme_id: &str,
    image_icons_supported: bool,
) -> Vec<ui::SettingsCardViewModel> {
    use ui::{
        SettingsCardViewModel as Card, SettingsControlKind as Kind, SettingsField as Field,
        SettingsItemViewModel as Item,
    };
    let toggle = |field, label, value: bool, description, enabled| {
        Item::new(
            field,
            label,
            if value { "On" } else { "Off" },
            description,
            Kind::Toggle,
        )
        .enabled(enabled)
    };
    let reset = |enabled| {
        Item::new(
            Field::RestoreDefaults,
            "Restore defaults",
            "Confirm",
            "Restore every setting in this category.",
            Kind::Action,
        )
        .enabled(enabled)
    };
    let motion_enabled = !appearance.motion_preference.reduced();
    match state.category {
        ui::SettingsCategory::Appearance => vec![
            Card::new(
                "Theme",
                vec![
                    Item::new(
                        Field::Theme,
                        "Theme",
                        if asset_theme_id == ui::DEFAULT_THEME_ID {
                            match (appearance.icon_display_mode, image_icons_supported) {
                                (storage::IconDisplayMode::Image, true) => {
                                    "Default theme / Image icons"
                                }
                                _ => "Default theme / ASCII icons",
                            }
                        } else {
                            asset_theme_id
                        },
                        if asset_theme_id == ui::DEFAULT_THEME_ID {
                            "Open Default theme options to choose ASCII or image icons."
                        } else {
                            "Icon mode switching is available only for the Default asset theme."
                        },
                        Kind::Picker,
                    )
                    .enabled(asset_theme_id == ui::DEFAULT_THEME_ID),
                ],
            ),
            Card::new(
                "Visual style",
                vec![
                    Item::new(
                        Field::BorderShape,
                        "Border shape",
                        match appearance.border_shape {
                            storage::BorderShape::Rounded => "Rounded",
                            storage::BorderShape::Square => "Square",
                        },
                        "Choose rounded or square card borders.",
                        Kind::Cycle,
                    ),
                    Item::new(
                        Field::BorderColor,
                        "Border color",
                        appearance.border_color.to_string(),
                        "Choose a standard color or enter #RRGGBB.",
                        Kind::Palette,
                    ),
                    Item::new(
                        Field::AccentColor,
                        "Accent color",
                        appearance.accent_color.to_string(),
                        "Used for selection and focus; must differ from the border.",
                        Kind::Palette,
                    ),
                ],
            ),
            Card::new(
                "Animation",
                vec![
                    Item::new(
                        Field::MotionPreference,
                        "Motion",
                        match appearance.motion_preference {
                            storage::MotionPreference::Full => "Full",
                            storage::MotionPreference::Reduced => "Reduced",
                        },
                        "Use Reduced to disable interface transitions while preserving essential refreshes.",
                        Kind::Cycle,
                    ),
                    Item::new(
                        Field::AnimationSpeed,
                        "Animation speed",
                        format!("{}%", appearance.normalized_animation_speed_percent()),
                        "Adjust transition speed from 50% to 200% in 25% steps.",
                        Kind::Stepper,
                    )
                    .enabled(motion_enabled),
                    Item::new(
                        Field::ResetAnimationSpeed,
                        "Restore speed default",
                        "Reset",
                        "Restore animation speed to the 100% default.",
                        Kind::Action,
                    )
                    .enabled(motion_enabled),
                ],
            ),
            Card::new("Reset", vec![reset(true)]),
        ],
        ui::SettingsCategory::RegionTime => vec![
            Card::new(
                "Language and timezone",
                vec![
                    Item::new(
                        Field::Language,
                        "Language",
                        language_label(&config.language),
                        "Choose from the extensible language catalogue.",
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::Timezone,
                        "City / timezone",
                        timezone_label(&config.timezone),
                        "Search by city, region or timezone identifier.",
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::WeatherLocation,
                        "Weather location",
                        config
                            .weather_location
                            .as_deref()
                            .unwrap_or("Same as timezone"),
                        "Enter a detailed English city or address used only by Weathr.",
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                "Time synchronization",
                vec![
                    Item::new(
                        Field::TimeSyncSource,
                        "Time source",
                        time_sync_source_label(config.time_sync.source),
                        "Use a network time server or the operating system clock.",
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::TimeSyncServer,
                        "Synchronization server",
                        config
                            .time_sync
                            .server_url
                            .as_deref()
                            .unwrap_or("Automatic default servers"),
                        "Set an HTTP(S) server; it must synchronize successfully before saving.",
                        Kind::Picker,
                    )
                    .enabled(
                        global_enabled
                            && config.time_sync.source == storage::TimeSyncSource::NetworkServer,
                    ),
                ],
            ),
            Card::new("Reset", vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::System => vec![
            Card::new(
                "Storage pressure",
                vec![
                    Item::new(
                        Field::SystemLowAvailable,
                        "Low available",
                        format!("{} GiB", config.system_status.low_available_gib),
                        "Low pressure is reported when either the available-space or percentage threshold is reached.",
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SystemLowPercentage,
                        "Low percentage",
                        format!("{}%", config.system_status.low_percentage),
                        "Low pressure is reported when either the absolute or percentage threshold is reached.",
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SystemCriticalAvailable,
                        "Critical available",
                        format!("{} GiB", config.system_status.critical_available_gib),
                        "Critical pressure is reported when either the available-space or percentage threshold is reached.",
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SystemCriticalPercentage,
                        "Critical percentage",
                        format!("{}%", config.system_status.critical_percentage),
                        "Critical pressure is reported when either the absolute or percentage threshold is reached.",
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new("Reset", vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::FileExplorer => vec![
            Card::new(
                "Display",
                vec![
                    toggle(
                        Field::ShowHidden,
                        "Show hidden files",
                        config.explorer.show_hidden,
                        "Display hidden files in Explorer.",
                        global_enabled,
                    ),
                    toggle(
                        Field::ShowSystem,
                        "Show system files",
                        config.explorer.show_system,
                        "Display operating-system files.",
                        global_enabled,
                    ),
                    toggle(
                        Field::ShowExtensions,
                        "Show file extensions",
                        config.explorer.show_extensions,
                        "Show filename extensions.",
                        global_enabled,
                    ),
                    toggle(
                        Field::FoldersFirst,
                        "Folders first",
                        config.explorer.folders_first,
                        "Group directories before files.",
                        global_enabled,
                    ),
                    toggle(
                        Field::ShowSidebar,
                        "Show Quick Access",
                        config.explorer.show_sidebar,
                        "Show the Quick Access sidebar.",
                        global_enabled,
                    ),
                ],
            ),
            Card::new(
                "Sorting & format",
                vec![
                    toggle(
                        Field::CaseSensitiveSort,
                        "Case-sensitive sort",
                        config.explorer.case_sensitive_sort,
                        "Treat letter case as significant while sorting.",
                        global_enabled,
                    ),
                    Item::new(
                        Field::SizeFormat,
                        "Size format",
                        size_format_label(config.explorer.size_format),
                        "Choose human-readable binary sizes or exact bytes.",
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::DateZone,
                        "Date timezone",
                        date_zone_label(config.explorer.date_zone),
                        "Use the configured timezone or UTC for file dates.",
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SortField,
                        "Default sort field",
                        sort_field_label(config.explorer.sort_field),
                        "Choose the default Explorer sort column.",
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SortDirection,
                        "Default direction",
                        sort_direction_label(config.explorer.sort_direction),
                        "Choose ascending or descending order.",
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                "Safety",
                vec![
                    toggle(
                        Field::ConfirmDelete,
                        "Confirm delete",
                        config.explorer.confirm_delete,
                        "Ask before moving items to Trash.",
                        global_enabled,
                    ),
                    toggle(
                        Field::ConfirmNameConflicts,
                        "Confirm name conflicts",
                        config.explorer.confirm_name_conflicts,
                        "Ask how to resolve duplicate names.",
                        global_enabled,
                    ),
                ],
            ),
            Card::new("Reset", vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::Editor => vec![
            Card::new(
                "Explorer file opening",
                vec![
                    Item::new(
                        Field::ExplorerOpenExtensions,
                        "Open in Editor",
                        editor_extensions_summary(&config.editor.explorer_open_extensions),
                        "Choose filename suffixes that Explorer opens in the built-in Editor.",
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                "Cursor acceleration",
                vec![
                    toggle(
                        Field::CursorAcceleration,
                        "Cursor acceleration",
                        config.editor.cursor_acceleration_enabled,
                        "Accelerate repeated arrow-key movement.",
                        global_enabled,
                    ),
                    Item::new(
                        Field::CursorDelay,
                        "Start delay",
                        format!("{} ms", config.editor.cursor_acceleration_delay_ms),
                        "Delay before acceleration begins.",
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::CursorRamp,
                        "Ramp to maximum",
                        format!("{} ms", config.editor.cursor_acceleration_ramp_ms),
                        "Time taken to reach the maximum step.",
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::CursorHorizontalStep,
                        "Horizontal maximum",
                        format!("{} cells", config.editor.cursor_horizontal_max_step),
                        "Maximum horizontal movement per repeat.",
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::CursorVerticalStep,
                        "Vertical maximum",
                        format!("{} lines", config.editor.cursor_vertical_max_step),
                        "Maximum vertical movement per repeat.",
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new("Reset", vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::Update => Vec::new(),
    }
}

pub(in crate::session) fn settings_picker_options(
    picker: &SettingsPickerState,
) -> Vec<ui::SettingsPickerOptionViewModel> {
    let query = picker.query.trim().to_ascii_lowercase();
    match picker.kind {
        ui::SettingsPickerKind::Theme => vec![ui::SettingsPickerOptionViewModel::new(
            "Default theme",
            "Built-in asset theme",
        )],
        ui::SettingsPickerKind::DefaultThemeIcons => vec![
            ui::SettingsPickerOptionViewModel::new(
                "ASCII icons",
                "Always use text-based asset icons",
            ),
            ui::SettingsPickerOptionViewModel::new(
                "Image icons",
                "Requires Kitty, Sixel, or iTerm2 image support",
            )
            .enabled(picker.image_icons_supported),
        ],
        ui::SettingsPickerKind::AnimationSpeed => (storage::MIN_ANIMATION_SPEED_PERCENT
            ..=storage::MAX_ANIMATION_SPEED_PERCENT)
            .step_by(usize::from(storage::ANIMATION_SPEED_STEP_PERCENT))
            .map(|speed| {
                let detail = match speed.cmp(&storage::DEFAULT_ANIMATION_SPEED_PERCENT) {
                    std::cmp::Ordering::Less => "Slower than default",
                    std::cmp::Ordering::Equal => "Default",
                    std::cmp::Ordering::Greater => "Faster than default",
                };
                ui::SettingsPickerOptionViewModel::new(format!("{speed}%"), detail)
            })
            .collect(),
        ui::SettingsPickerKind::Language => app::setup_language_options()
            .into_iter()
            .filter(|option| {
                query.is_empty()
                    || option.code.to_ascii_lowercase().contains(&query)
                    || option.label.to_ascii_lowercase().contains(&query)
            })
            .map(|option| ui::SettingsPickerOptionViewModel::new(option.label, option.code))
            .collect(),
        ui::SettingsPickerKind::Timezone => {
            app::setup_timezone_options()
                .into_iter()
                .filter(|option| {
                    query.is_empty()
                        || option.id.to_ascii_lowercase().contains(&query)
                        || option.label.to_ascii_lowercase().contains(&query)
                        || option.description.to_ascii_lowercase().contains(&query)
                })
                .map(|option| {
                    ui::SettingsPickerOptionViewModel::new(option.label, option.description)
                        .timezone(option.id, option.longitude, option.latitude)
                })
                .collect()
        }
        ui::SettingsPickerKind::BorderColor | ui::SettingsPickerKind::AccentColor => {
            let mut options = ui::setup_standard_color_options()
                .iter()
                .map(|option| ui::SettingsPickerOptionViewModel::new(option.label, option.value))
                .collect::<Vec<_>>();
            options.push(ui::SettingsPickerOptionViewModel::new(
                "Custom color…",
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

pub(in crate::session) fn picker_title(kind: ui::SettingsPickerKind) -> &'static str {
    match kind {
        ui::SettingsPickerKind::Theme => "Choose theme",
        ui::SettingsPickerKind::DefaultThemeIcons => "Default theme",
        ui::SettingsPickerKind::AnimationSpeed => "Choose animation speed",
        ui::SettingsPickerKind::Language => "Choose language",
        ui::SettingsPickerKind::Timezone => "Choose city and timezone",
        ui::SettingsPickerKind::BorderColor => "Choose border color",
        ui::SettingsPickerKind::AccentColor => "Choose accent color",
    }
}

pub(in crate::session) fn picker_label(kind: ui::SettingsPickerKind) -> &'static str {
    match kind {
        ui::SettingsPickerKind::Theme => "Theme",
        ui::SettingsPickerKind::DefaultThemeIcons => "Default theme icon mode",
        ui::SettingsPickerKind::AnimationSpeed => "Animation speed",
        ui::SettingsPickerKind::BorderColor => "Border color",
        ui::SettingsPickerKind::AccentColor => "Accent color",
        ui::SettingsPickerKind::Language => "Language",
        ui::SettingsPickerKind::Timezone => "Timezone",
    }
}

pub(in crate::session) fn language_label(code: &str) -> String {
    app::setup_language_options()
        .into_iter()
        .find(|option| option.code == code)
        .map(|option| format!("{} ({})", option.label, option.code))
        .unwrap_or_else(|| code.to_string())
}

pub(in crate::session) fn timezone_label(id: &str) -> String {
    app::setup_timezone_options()
        .into_iter()
        .find(|option| option.id == id)
        .map(|option| format!("{} ({})", option.label, option.id))
        .unwrap_or_else(|| id.to_string())
}

pub(in crate::session) fn time_sync_source_label(source: storage::TimeSyncSource) -> &'static str {
    match source {
        storage::TimeSyncSource::NetworkServer => "Network server",
        storage::TimeSyncSource::OperatingSystem => "Operating system",
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

pub(in crate::session) fn settings_field_label(field: ui::SettingsField) -> &'static str {
    match field {
        ui::SettingsField::Theme => "Theme",
        ui::SettingsField::ShowHidden => "Show hidden files",
        ui::SettingsField::ShowSystem => "Show system files",
        ui::SettingsField::ShowExtensions => "Show file extensions",
        ui::SettingsField::FoldersFirst => "Folders first",
        ui::SettingsField::ShowSidebar => "Quick Access",
        ui::SettingsField::CaseSensitiveSort => "Case-sensitive sort",
        ui::SettingsField::SizeFormat => "Size format",
        ui::SettingsField::DateZone => "Date timezone",
        ui::SettingsField::SortField => "Sort field",
        ui::SettingsField::SortDirection => "Sort direction",
        ui::SettingsField::ConfirmDelete => "Delete confirmation",
        ui::SettingsField::ConfirmNameConflicts => "Conflict confirmation",
        ui::SettingsField::ExplorerOpenExtensions => "Explorer file suffixes",
        ui::SettingsField::CursorAcceleration => "Cursor acceleration",
        ui::SettingsField::CursorDelay => "Cursor delay",
        ui::SettingsField::CursorRamp => "Cursor ramp",
        ui::SettingsField::CursorHorizontalStep => "Horizontal maximum",
        ui::SettingsField::CursorVerticalStep => "Vertical maximum",
        ui::SettingsField::BorderShape => "Border shape",
        ui::SettingsField::BorderColor => "Border color",
        ui::SettingsField::AccentColor => "Accent color",
        ui::SettingsField::MotionPreference => "Motion",
        ui::SettingsField::AnimationSpeed => "Animation speed",
        ui::SettingsField::ResetAnimationSpeed => "Animation speed default",
        ui::SettingsField::Language => "Language",
        ui::SettingsField::Timezone => "Timezone",
        ui::SettingsField::TimeSyncSource => "Time source",
        ui::SettingsField::TimeSyncServer => "Time synchronization server",
        ui::SettingsField::WeatherLocation => "Weather location",
        ui::SettingsField::SystemLowAvailable => "Low available",
        ui::SettingsField::SystemLowPercentage => "Low percentage",
        ui::SettingsField::SystemCriticalAvailable => "Critical available",
        ui::SettingsField::SystemCriticalPercentage => "Critical percentage",
        ui::SettingsField::RestoreDefaults => "Defaults",
        ui::SettingsField::InstalledVersion => "Installed version",
        ui::SettingsField::RemoteVersion => "GitHub version",
        ui::SettingsField::CheckUpdates => "Check updates",
        ui::SettingsField::StartUpdate => "Start update",
    }
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
) -> Result<Vec<String>, String> {
    let mut extensions = Vec::new();
    for raw in value.split(|character: char| {
        character == ',' || character == ';' || character.is_ascii_whitespace()
    }) {
        if raw.is_empty() {
            continue;
        }
        let Some(extension) = storage::normalize_editor_explorer_open_extension(raw) else {
            return Err(format!(
                "Invalid suffix {raw:?}; use values such as .md, .rs or .d.ts"
            ));
        };
        if extensions.contains(&extension) {
            continue;
        }
        if extensions.len() >= storage::MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS {
            return Err(format!(
                "At most {} suffixes are allowed",
                storage::MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS
            ));
        }
        extensions.push(extension);
    }
    Ok(extensions)
}

#[cfg(test)]
mod update_tests {
    use super::*;

    fn checked_update_state(relation: app::update::UpdateRelation) -> SettingsUpdateState {
        SettingsUpdateState {
            check_result: Some(app::update::UpdateCheckResult {
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
            status: "Checked".to_string(),
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
        return "System default".to_string();
    }
    if extensions.len() <= 4 {
        return format_editor_explorer_open_extensions(extensions);
    }
    format!(
        "{}, +{} more",
        format_editor_explorer_open_extensions(&extensions[..3]),
        extensions.len() - 3
    )
}

pub(in crate::session) fn size_format_label(value: storage::ExplorerSizeFormat) -> &'static str {
    match value {
        storage::ExplorerSizeFormat::HumanBinary => "Human binary",
        storage::ExplorerSizeFormat::Bytes => "Bytes",
    }
}

pub(in crate::session) fn date_zone_label(value: storage::ExplorerDateZone) -> &'static str {
    match value {
        storage::ExplorerDateZone::ConfiguredTimezone => "Configured timezone",
        storage::ExplorerDateZone::Utc => "UTC",
    }
}

pub(in crate::session) fn sort_field_label(value: storage::ExplorerSortField) -> &'static str {
    match value {
        storage::ExplorerSortField::Name => "Name",
        storage::ExplorerSortField::Type => "Type",
        storage::ExplorerSortField::Size => "Size",
        storage::ExplorerSortField::Modified => "Modified",
    }
}

pub(in crate::session) fn sort_direction_label(
    value: storage::ExplorerSortDirection,
) -> &'static str {
    match value {
        storage::ExplorerSortDirection::Ascending => "Ascending",
        storage::ExplorerSortDirection::Descending => "Descending",
    }
}
