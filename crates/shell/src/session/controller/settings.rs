use super::super::*;
pub(in crate::session) const SETTINGS_RESTORE_NOTIFICATION_KEY: &str = "settings.restore-defaults";
pub(in crate::session) const SETTINGS_WEATHER_LOCATION_NOTIFICATION_KEY: &str =
    "settings.weather-location";
pub(in crate::session) const WEATHER_LOCATION_MAX_LEN: usize = 120;
pub(in crate::session) const EDITOR_EXTENSIONS_INPUT_MAX_LEN: usize = 1_024;

pub(in crate::session) const APPEARANCE_SETTINGS_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::BorderShape,
    ui::SettingsField::BorderColor,
    ui::SettingsField::AccentColor,
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
            selected_field: ui::SettingsField::BorderShape,
            focus: SettingsFocus::Categories,
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

    pub(in crate::session) fn handle_settings_key(
        &mut self,
        key: &KeyInput,
        platform: &dyn Platform,
    ) {
        if self.settings_state.is_none() {
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

        let focus = self.settings_state.as_ref().map(|state| state.focus);
        match (&key.key, focus) {
            (InputKey::Escape, _) => self.close_settings(),
            (InputKey::Tab, _) | (InputKey::BackTab, _) => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.focus = match state.focus {
                        SettingsFocus::Categories => SettingsFocus::Fields,
                        SettingsFocus::Fields => SettingsFocus::Categories,
                    };
                }
            }
            (InputKey::Left | InputKey::Up, Some(SettingsFocus::Categories)) => {
                self.select_settings_category_delta(-1)
            }
            (InputKey::Right | InputKey::Down, Some(SettingsFocus::Categories)) => {
                self.select_settings_category_delta(1)
            }
            (InputKey::Enter | InputKey::Char(' '), Some(SettingsFocus::Categories)) => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.focus = SettingsFocus::Fields;
                }
            }
            (InputKey::Up, Some(SettingsFocus::Fields)) => self.select_settings_field_delta(-1),
            (InputKey::Down, Some(SettingsFocus::Fields)) => self.select_settings_field_delta(1),
            (InputKey::Home, Some(SettingsFocus::Fields)) => self.select_settings_field_at(0),
            (InputKey::End, Some(SettingsFocus::Fields)) => {
                let last = self
                    .settings_state
                    .as_ref()
                    .map(|state| settings_fields(state.category).len().saturating_sub(1))
                    .unwrap_or(0);
                self.select_settings_field_at(last);
            }
            (InputKey::PageUp, Some(SettingsFocus::Fields)) => self.scroll_settings(-6),
            (InputKey::PageDown, Some(SettingsFocus::Fields)) => self.scroll_settings(6),
            (InputKey::Left, Some(SettingsFocus::Fields)) => {
                self.adjust_selected_setting(-1, platform)
            }
            (InputKey::Right, Some(SettingsFocus::Fields)) => {
                self.adjust_selected_setting(1, platform)
            }
            (InputKey::Enter | InputKey::Char(' '), Some(SettingsFocus::Fields)) => {
                self.activate_selected_setting(platform)
            }
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
            Some(ui::SettingsHitTarget::Category(category)) => {
                self.select_settings_category(category);
            }
            Some(ui::SettingsHitTarget::Field(field)) => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.focus = SettingsFocus::Fields;
                    state.selected_field = field;
                }
                self.activate_selected_setting(platform);
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
        let maximum = ui::SettingsCategory::ALL.len().saturating_sub(1) as isize;
        let next = (index + delta).clamp(0, maximum) as usize;
        self.select_settings_category(ui::SettingsCategory::ALL[next]);
    }

    pub(in crate::session) fn select_settings_category(&mut self, category: ui::SettingsCategory) {
        if let Some(state) = self.settings_state.as_mut() {
            state.category = category;
            state.selected_field = settings_fields(category)[0];
            state.focus = SettingsFocus::Categories;
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
                state.scroll_offset.saturating_add(delta as u16).min(200)
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
            ui::SettingsField::RestoreDefaults => self.request_settings_restore_defaults(),
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
            ui::SettingsField::BorderColor
                | ui::SettingsField::AccentColor
                | ui::SettingsField::Language
                | ui::SettingsField::Timezone
                | ui::SettingsField::WeatherLocation
                | ui::SettingsField::ExplorerOpenExtensions
                | ui::SettingsField::TimeSyncServer
        ) {
            self.activate_selected_setting(platform);
            return;
        }
        if field == ui::SettingsField::RestoreDefaults {
            self.request_settings_restore_defaults();
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

    pub(in crate::session) fn open_settings_picker(&mut self, kind: ui::SettingsPickerKind) {
        if !matches!(
            kind,
            ui::SettingsPickerKind::BorderColor | ui::SettingsPickerKind::AccentColor
        ) && !self.can_change_global_settings()
        {
            self.set_settings_error("Administrator permission is required");
            return;
        }
        let selected_index = self.settings_picker_initial_index(kind);
        if let Some(state) = self.settings_state.as_mut() {
            state.picker = Some(SettingsPickerState {
                kind,
                query: String::new(),
                selected_index,
                window_start: selected_index.saturating_sub(4),
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
                if let Some(state) = self.settings_state.as_mut() {
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
        match picker.kind {
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
            ui::SettingsCategory::FileExplorer => config.explorer = defaults.explorer,
            ui::SettingsCategory::Editor => config.editor = defaults.editor,
            ui::SettingsCategory::Appearance => unreachable!(),
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
        let cards = settings_cards(state, config, appearance, global_enabled);
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
        Some(ui::SettingsViewModel {
            selected_category: state.category,
            selected_field: state.selected_field,
            cards,
            appearance_preview,
            status: state.status.clone(),
            locked_message: (!global_enabled && state.category != ui::SettingsCategory::Appearance)
                .then_some("Locked: administrator permission is required".to_string()),
            scroll_offset: state.scroll_offset,
            picker,
            color_editor,
            weather_location_editor,
            file_extensions_editor,
            time_sync_server_editor,
        })
    }
}

pub(in crate::session) fn settings_fields(
    category: ui::SettingsCategory,
) -> &'static [ui::SettingsField] {
    match category {
        ui::SettingsCategory::Appearance => APPEARANCE_SETTINGS_FIELDS,
        ui::SettingsCategory::RegionTime => REGION_SETTINGS_FIELDS,
        ui::SettingsCategory::FileExplorer => EXPLORER_SETTINGS_FIELDS,
        ui::SettingsCategory::Editor => EDITOR_SETTINGS_FIELDS,
    }
}

pub(in crate::session) fn settings_cards(
    state: &SettingsState,
    config: &storage::StorageConfig,
    appearance: &storage::AppearanceConfig,
    global_enabled: bool,
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
    match state.category {
        ui::SettingsCategory::Appearance => vec![
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
    }
}

pub(in crate::session) fn settings_picker_options(
    picker: &SettingsPickerState,
) -> Vec<ui::SettingsPickerOptionViewModel> {
    let query = picker.query.trim().to_ascii_lowercase();
    match picker.kind {
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

pub(in crate::session) fn settings_picker_visible_rows(terminal_height: u16) -> usize {
    usize::from(terminal_height.saturating_sub(10).clamp(4, 18))
}

pub(in crate::session) fn picker_title(kind: ui::SettingsPickerKind) -> &'static str {
    match kind {
        ui::SettingsPickerKind::Language => "Choose language",
        ui::SettingsPickerKind::Timezone => "Choose city and timezone",
        ui::SettingsPickerKind::BorderColor => "Choose border color",
        ui::SettingsPickerKind::AccentColor => "Choose accent color",
    }
}

pub(in crate::session) fn picker_label(kind: ui::SettingsPickerKind) -> &'static str {
    match kind {
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

pub(in crate::session) fn settings_field_label(field: ui::SettingsField) -> &'static str {
    match field {
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
        ui::SettingsField::Language => "Language",
        ui::SettingsField::Timezone => "Timezone",
        ui::SettingsField::TimeSyncSource => "Time source",
        ui::SettingsField::TimeSyncServer => "Time synchronization server",
        ui::SettingsField::WeatherLocation => "Weather location",
        ui::SettingsField::RestoreDefaults => "Defaults",
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
