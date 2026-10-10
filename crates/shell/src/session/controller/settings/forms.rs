use super::*;
use crate::session::*;

impl ShellSession {
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
}
