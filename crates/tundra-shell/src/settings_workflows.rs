const SETTINGS_RESTORE_NOTIFICATION_KEY: &str = "settings.restore-defaults";
const SETTINGS_WEATHER_LOCATION_NOTIFICATION_KEY: &str = "settings.weather-location";
const WEATHER_LOCATION_MAX_LEN: usize = 120;

const APPEARANCE_SETTINGS_FIELDS: &[tundra_ui::SettingsField] = &[
    tundra_ui::SettingsField::BorderShape,
    tundra_ui::SettingsField::BorderColor,
    tundra_ui::SettingsField::AccentColor,
    tundra_ui::SettingsField::RestoreDefaults,
];
const REGION_SETTINGS_FIELDS: &[tundra_ui::SettingsField] = &[
    tundra_ui::SettingsField::Language,
    tundra_ui::SettingsField::Timezone,
    tundra_ui::SettingsField::WeatherLocation,
    tundra_ui::SettingsField::RestoreDefaults,
];
const EXPLORER_SETTINGS_FIELDS: &[tundra_ui::SettingsField] = &[
    tundra_ui::SettingsField::ShowHidden,
    tundra_ui::SettingsField::ShowSystem,
    tundra_ui::SettingsField::ShowExtensions,
    tundra_ui::SettingsField::FoldersFirst,
    tundra_ui::SettingsField::ShowSidebar,
    tundra_ui::SettingsField::CaseSensitiveSort,
    tundra_ui::SettingsField::SizeFormat,
    tundra_ui::SettingsField::DateZone,
    tundra_ui::SettingsField::SortField,
    tundra_ui::SettingsField::SortDirection,
    tundra_ui::SettingsField::ConfirmDelete,
    tundra_ui::SettingsField::ConfirmNameConflicts,
    tundra_ui::SettingsField::RestoreDefaults,
];
const EDITOR_SETTINGS_FIELDS: &[tundra_ui::SettingsField] = &[
    tundra_ui::SettingsField::CursorAcceleration,
    tundra_ui::SettingsField::CursorDelay,
    tundra_ui::SettingsField::CursorRamp,
    tundra_ui::SettingsField::CursorHorizontalStep,
    tundra_ui::SettingsField::CursorVerticalStep,
    tundra_ui::SettingsField::RestoreDefaults,
];

impl ShellState {
    fn open_settings(&mut self) {
        if self.is_strict_guest() {
            self.notify_status("Guest access is read-only");
            return;
        }
        let Some(actor) = self.auth_session.clone() else {
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
        let appearance = match users
            .list_accessible_users(&actor)
            .and_then(|users| {
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

        self.settings_state = Some(SettingsState {
            category: tundra_ui::SettingsCategory::Appearance,
            selected_field: tundra_ui::SettingsField::BorderShape,
            focus: SettingsFocus::Categories,
            config,
            appearance,
            status: "Ready".to_string(),
            scroll_offset: 0,
            picker: None,
            color_editor: None,
            weather_location_editor: None,
        });
        if self.active_screen() != ShellScreen::Settings {
            self.screen_stack.push(ShellScreen::Settings);
        }
        self.focused_component = ShellComponent::Settings;
        self.error_message = None;
        self.notify_status("Settings");
        self.refresh_hit_map();
    }

    fn close_settings(&mut self) {
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

    fn can_change_global_settings(&self) -> bool {
        PermissionService::new(self.debug_policy)
            .authorize(
                self.auth_session.as_ref(),
                PermissionAction::ChangeSettings,
                Some("change_settings"),
            )
            .allowed
    }

    fn handle_settings_key(&mut self, key: &KeyInput) {
        if self.settings_state.is_none() {
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
            (InputKey::Enter | InputKey::Character(' '), Some(SettingsFocus::Categories)) => {
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
                self.adjust_selected_setting(-1)
            }
            (InputKey::Right, Some(SettingsFocus::Fields)) => {
                self.adjust_selected_setting(1)
            }
            (InputKey::Enter | InputKey::Character(' '), Some(SettingsFocus::Fields)) => {
                self.activate_selected_setting()
            }
            _ => {}
        }
        self.refresh_hit_map();
    }

    fn handle_settings_pointer(&mut self, input: MouseInput) {
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
                self.scroll_settings(if direction == ScrollDirection::Up { -3 } else { 3 });
            }
            self.refresh_hit_map();
            return;
        }
        let MouseInput::Down {
            button: PointerButton::Left,
            coordinates,
            ..
        } = input
        else {
            return;
        };
        let Some(model) = self.to_settings_view_model() else {
            return;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let app_area = match tundra_ui::compute_shell_layout(area) {
            tundra_ui::ShellLayout::Compact(compact) => compact,
            tundra_ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = tundra_ui::settings_layout(app_area, &model);
        match tundra_ui::settings_hit_test(&layout, coordinates) {
            Some(tundra_ui::SettingsHitTarget::Category(category)) => {
                self.select_settings_category(category);
            }
            Some(tundra_ui::SettingsHitTarget::Field(field)) => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.focus = SettingsFocus::Fields;
                    state.selected_field = field;
                }
                self.activate_selected_setting();
            }
            Some(tundra_ui::SettingsHitTarget::PickerOption(index)) => {
                if let Some(state) = self.settings_state.as_mut()
                    && let Some(picker) = state.picker.as_mut()
                {
                    picker.selected_index = index;
                }
                self.apply_settings_picker_selection();
            }
            Some(tundra_ui::SettingsHitTarget::ColorEditor)
            | Some(tundra_ui::SettingsHitTarget::WeatherLocationEditor)
            | None => {}
        }
        self.refresh_hit_map();
    }

    fn select_settings_category_delta(&mut self, delta: isize) {
        let Some(current) = self.settings_state.as_ref().map(|state| state.category) else {
            return;
        };
        let index = tundra_ui::SettingsCategory::ALL
            .iter()
            .position(|category| *category == current)
            .unwrap_or(0) as isize;
        let maximum = tundra_ui::SettingsCategory::ALL.len().saturating_sub(1) as isize;
        let next = (index + delta).clamp(0, maximum) as usize;
        self.select_settings_category(tundra_ui::SettingsCategory::ALL[next]);
    }

    fn select_settings_category(&mut self, category: tundra_ui::SettingsCategory) {
        if let Some(state) = self.settings_state.as_mut() {
            state.category = category;
            state.selected_field = settings_fields(category)[0];
            state.focus = SettingsFocus::Categories;
            state.scroll_offset = 0;
            state.picker = None;
            state.color_editor = None;
            state.weather_location_editor = None;
            state.status = "Ready".to_string();
        }
        self.notify_status(format!("Settings: {}", category.label()));
    }

    fn select_settings_field_delta(&mut self, delta: isize) {
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

    fn select_settings_field_at(&mut self, index: usize) {
        let Some(state) = self.settings_state.as_mut() else {
            return;
        };
        let fields = settings_fields(state.category);
        state.selected_field = fields[index.min(fields.len().saturating_sub(1))];
        state.scroll_offset = (index as u16).saturating_sub(6);
    }

    fn scroll_settings(&mut self, delta: i16) {
        if let Some(state) = self.settings_state.as_mut() {
            state.scroll_offset = if delta < 0 {
                state.scroll_offset.saturating_sub(delta.unsigned_abs())
            } else {
                state.scroll_offset.saturating_add(delta as u16).min(200)
            };
        }
    }

    fn activate_selected_setting(&mut self) {
        let Some(field) = self.settings_state.as_ref().map(|state| state.selected_field) else {
            return;
        };
        match field {
            tundra_ui::SettingsField::BorderColor => {
                self.open_settings_picker(tundra_ui::SettingsPickerKind::BorderColor)
            }
            tundra_ui::SettingsField::AccentColor => {
                self.open_settings_picker(tundra_ui::SettingsPickerKind::AccentColor)
            }
            tundra_ui::SettingsField::Language => {
                self.open_settings_picker(tundra_ui::SettingsPickerKind::Language)
            }
            tundra_ui::SettingsField::Timezone => {
                self.open_settings_picker(tundra_ui::SettingsPickerKind::Timezone)
            }
            tundra_ui::SettingsField::WeatherLocation => self.open_settings_weather_location(),
            tundra_ui::SettingsField::RestoreDefaults => self.request_settings_restore_defaults(),
            _ => self.adjust_selected_setting(1),
        }
    }

    fn adjust_selected_setting(&mut self, direction: i8) {
        let Some(field) = self.settings_state.as_ref().map(|state| state.selected_field) else {
            return;
        };
        if matches!(
            field,
            tundra_ui::SettingsField::BorderColor
                | tundra_ui::SettingsField::AccentColor
                | tundra_ui::SettingsField::Language
                | tundra_ui::SettingsField::Timezone
                | tundra_ui::SettingsField::WeatherLocation
        ) {
            self.activate_selected_setting();
            return;
        }
        if field == tundra_ui::SettingsField::RestoreDefaults {
            self.request_settings_restore_defaults();
            return;
        }
        if field == tundra_ui::SettingsField::BorderShape {
            let Some(mut appearance) = self
                .settings_state
                .as_ref()
                .map(|state| state.appearance.clone())
            else {
                return;
            };
            appearance.border_shape = match appearance.border_shape {
                tundra_storage::BorderShape::Rounded => tundra_storage::BorderShape::Square,
                tundra_storage::BorderShape::Square => tundra_storage::BorderShape::Rounded,
            };
            self.save_settings_appearance(appearance, "Border shape");
            return;
        }
        self.save_global_setting(field, direction);
    }

    fn save_global_setting(&mut self, field: tundra_ui::SettingsField, direction: i8) {
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
            tundra_ui::SettingsField::ShowHidden => {
                config.explorer.show_hidden = !config.explorer.show_hidden
            }
            tundra_ui::SettingsField::ShowSystem => {
                config.explorer.show_system = !config.explorer.show_system
            }
            tundra_ui::SettingsField::ShowExtensions => {
                config.explorer.show_extensions = !config.explorer.show_extensions
            }
            tundra_ui::SettingsField::FoldersFirst => {
                config.explorer.folders_first = !config.explorer.folders_first
            }
            tundra_ui::SettingsField::ShowSidebar => {
                config.explorer.show_sidebar = !config.explorer.show_sidebar
            }
            tundra_ui::SettingsField::CaseSensitiveSort => {
                config.explorer.case_sensitive_sort = !config.explorer.case_sensitive_sort
            }
            tundra_ui::SettingsField::SizeFormat => {
                config.explorer.size_format = match config.explorer.size_format {
                    tundra_storage::ExplorerSizeFormat::HumanBinary => {
                        tundra_storage::ExplorerSizeFormat::Bytes
                    }
                    tundra_storage::ExplorerSizeFormat::Bytes => {
                        tundra_storage::ExplorerSizeFormat::HumanBinary
                    }
                }
            }
            tundra_ui::SettingsField::DateZone => {
                config.explorer.date_zone = match config.explorer.date_zone {
                    tundra_storage::ExplorerDateZone::ConfiguredTimezone => {
                        tundra_storage::ExplorerDateZone::Utc
                    }
                    tundra_storage::ExplorerDateZone::Utc => {
                        tundra_storage::ExplorerDateZone::ConfiguredTimezone
                    }
                }
            }
            tundra_ui::SettingsField::SortField => {
                config.explorer.sort_field = cycle_explorer_sort_field(
                    config.explorer.sort_field,
                    if increase { 1 } else { -1 },
                )
            }
            tundra_ui::SettingsField::SortDirection => {
                config.explorer.sort_direction = match config.explorer.sort_direction {
                    tundra_storage::ExplorerSortDirection::Ascending => {
                        tundra_storage::ExplorerSortDirection::Descending
                    }
                    tundra_storage::ExplorerSortDirection::Descending => {
                        tundra_storage::ExplorerSortDirection::Ascending
                    }
                }
            }
            tundra_ui::SettingsField::ConfirmDelete => {
                config.explorer.confirm_delete = !config.explorer.confirm_delete
            }
            tundra_ui::SettingsField::ConfirmNameConflicts => {
                config.explorer.confirm_name_conflicts = !config.explorer.confirm_name_conflicts
            }
            tundra_ui::SettingsField::CursorAcceleration => {
                config.editor.cursor_acceleration_enabled =
                    !config.editor.cursor_acceleration_enabled
            }
            tundra_ui::SettingsField::CursorDelay => {
                config.editor.cursor_acceleration_delay_ms = adjust_u32_setting(
                    config.editor.cursor_acceleration_delay_ms,
                    EDITOR_CURSOR_TIME_STEP_MS,
                    increase,
                )
            }
            tundra_ui::SettingsField::CursorRamp => {
                config.editor.cursor_acceleration_ramp_ms = adjust_u32_setting(
                    config.editor.cursor_acceleration_ramp_ms,
                    EDITOR_CURSOR_TIME_STEP_MS,
                    increase,
                )
            }
            tundra_ui::SettingsField::CursorHorizontalStep => {
                config.editor.cursor_horizontal_max_step =
                    adjust_u8_setting(config.editor.cursor_horizontal_max_step, increase)
            }
            tundra_ui::SettingsField::CursorVerticalStep => {
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
        self.editor_config = config.editor;
        if let Some(state) = self.settings_state.as_mut() {
            state.config = config;
            state.status = format!("Saved {}", settings_field_label(field));
        }
        self.notify_status(format!("Saved {}", settings_field_label(field)));
    }

    fn save_settings_appearance(
        &mut self,
        appearance: tundra_storage::AppearanceConfig,
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
        let Some(actor) = self.auth_session.clone() else {
            self.set_settings_error("Login required");
            return false;
        };
        let users = UserService::with_debug_policy(storage, self.debug_policy);
        match users.update_user_appearance(&actor, &actor.username, appearance) {
            Ok(account) => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.appearance = account.appearance;
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

    fn open_settings_picker(&mut self, kind: tundra_ui::SettingsPickerKind) {
        if !matches!(
            kind,
            tundra_ui::SettingsPickerKind::BorderColor
                | tundra_ui::SettingsPickerKind::AccentColor
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
            state.status = "Choose a value".to_string();
        }
    }

    fn settings_picker_initial_index(&self, kind: tundra_ui::SettingsPickerKind) -> usize {
        let Some(state) = self.settings_state.as_ref() else {
            return 0;
        };
        match kind {
            tundra_ui::SettingsPickerKind::Language => tundra_ui::setup_language_options()
                .iter()
                .position(|option| option.code == state.config.language)
                .unwrap_or(0),
            tundra_ui::SettingsPickerKind::Timezone => tundra_ui::setup_timezone_options()
                .iter()
                .position(|option| option.id == state.config.timezone)
                .unwrap_or(0),
            tundra_ui::SettingsPickerKind::BorderColor => color_picker_initial_index(
                state.appearance.border_color,
            ),
            tundra_ui::SettingsPickerKind::AccentColor => color_picker_initial_index(
                state.appearance.accent_color,
            ),
        }
    }

    fn handle_settings_picker_key(&mut self, key: &KeyInput) {
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
                        tundra_ui::SettingsPickerKind::Language
                            | tundra_ui::SettingsPickerKind::Timezone
                    )
                {
                    picker.query.pop();
                    picker.selected_index = 0;
                    picker.window_start = 0;
                }
            }
            InputKey::Character(character)
                if !character.is_control()
                    && self
                        .settings_state
                        .as_ref()
                        .and_then(|state| state.picker.as_ref())
                        .is_some_and(|picker| {
                            matches!(
                                picker.kind,
                                tundra_ui::SettingsPickerKind::Language
                                    | tundra_ui::SettingsPickerKind::Timezone
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

    fn select_settings_picker_delta(&mut self, delta: isize) {
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

    fn select_settings_picker_at(&mut self, index: usize) {
        let count = self
            .settings_state
            .as_ref()
            .and_then(|state| state.picker.as_ref())
            .map(settings_picker_options)
            .map(|options| options.len())
            .unwrap_or(0);
        if let Some(state) = self.settings_state.as_mut()
            && let Some(picker) = state.picker.as_mut()
            && count > 0
        {
            picker.selected_index = index.min(count - 1);
            let visible = settings_picker_visible_rows(self.terminal_size.1);
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

    fn apply_settings_picker_selection(&mut self) {
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
            tundra_ui::SettingsPickerKind::Language => {
                self.save_region_picker_value(Some(option.detail), None)
            }
            tundra_ui::SettingsPickerKind::Timezone => {
                self.save_region_picker_value(None, option.timezone_id)
            }
            tundra_ui::SettingsPickerKind::BorderColor
            | tundra_ui::SettingsPickerKind::AccentColor => {
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
                let Ok(color) = option.detail.parse::<tundra_storage::BorderColor>() else {
                    self.set_settings_error("Invalid color option");
                    return;
                };
                let Some(mut appearance) = self
                    .settings_state
                    .as_ref()
                    .map(|state| state.appearance.clone())
                else {
                    return;
                };
                match picker.kind {
                    tundra_ui::SettingsPickerKind::BorderColor => {
                        appearance.border_color = color
                    }
                    tundra_ui::SettingsPickerKind::AccentColor => {
                        appearance.accent_color = color
                    }
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

    fn save_region_picker_value(&mut self, language: Option<String>, timezone: Option<String>) {
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
        if timezone.is_some() {
            self.set_clock_timezone(timezone);
        }
        if let Some(state) = self.settings_state.as_mut() {
            state.config = config;
            state.picker = None;
            state.status = "Saved region and time".to_string();
        }
        self.notify_status("Saved region and time");
    }

    fn open_settings_weather_location(&mut self) {
        if !self.can_change_global_settings() {
            self.set_settings_error("Administrator permission is required");
            return;
        }
        if let Some(state) = self.settings_state.as_mut() {
            state.weather_location_editor = Some(SettingsWeatherLocationEditorState {
                value: state.config.weather_location.clone().unwrap_or_default(),
                error: None,
            });
            state.picker = None;
            state.color_editor = None;
            state.status = "Enter an English weather location".to_string();
        }
    }

    fn handle_settings_weather_location_key(&mut self, key: &KeyInput) {
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
            InputKey::Character(character) => {
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

    fn request_settings_weather_location_confirmation(&mut self) {
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
            tundra_ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("save", "Save")
                    .with_shortcut(InputKey::Character('s'))
                    .with_follow_up(ShellCommand::SettingsWeatherLocationConfirmed),
                ShellNotificationAction::new("cancel", "Cancel")
                    .with_shortcut(InputKey::Escape)
                    .cancel(),
            ],
        )
        .with_key(SETTINGS_WEATHER_LOCATION_NOTIFICATION_KEY);
        self.notify_modal_with_options(notification);
    }

    fn save_settings_weather_location(&mut self) {
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
        if let Some(state) = self.settings_state.as_mut() {
            state.config = config;
            state.weather_location_editor = None;
            state.status = "Saved weather location".to_string();
        }
        self.notify_status("Saved weather location");
    }

    fn handle_settings_color_key(&mut self, key: &KeyInput) {
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
            InputKey::Character(character)
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

    fn apply_settings_custom_color(&mut self) {
        let Some(editor) = self
            .settings_state
            .as_ref()
            .and_then(|state| state.color_editor.as_ref())
            .cloned()
        else {
            return;
        };
        let color = match editor.value.parse::<tundra_storage::BorderColor>() {
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
        let Some(mut appearance) = self
            .settings_state
            .as_ref()
            .map(|state| state.appearance.clone())
        else {
            return;
        };
        match editor.kind {
            tundra_ui::SettingsPickerKind::BorderColor => appearance.border_color = color,
            tundra_ui::SettingsPickerKind::AccentColor => appearance.accent_color = color,
            _ => return,
        }
        if self.save_settings_appearance(appearance, picker_label(editor.kind))
            && let Some(state) = self.settings_state.as_mut()
        {
            state.color_editor = None;
        }
    }

    fn request_settings_restore_defaults(&mut self) {
        let Some(category) = self.settings_state.as_ref().map(|state| state.category) else {
            return;
        };
        if category != tundra_ui::SettingsCategory::Appearance
            && !self.can_change_global_settings()
        {
            self.set_settings_error("Administrator permission is required");
            return;
        }
        let notification = ShellNotification::modal(
            "Restore defaults",
            format!("Restore all {} settings to their defaults?", category.label()),
            tundra_ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("restore", "Restore")
                    .with_shortcut(InputKey::Character('r'))
                    .with_follow_up(ShellCommand::SettingsRestoreDefaultsConfirmed),
                ShellNotificationAction::new("cancel", "Cancel")
                    .with_shortcut(InputKey::Escape)
                    .cancel(),
            ],
        )
        .with_key(SETTINGS_RESTORE_NOTIFICATION_KEY);
        self.notify_modal_with_options(notification);
    }

    fn restore_settings_defaults(&mut self) {
        let Some(category) = self.settings_state.as_ref().map(|state| state.category) else {
            return;
        };
        if category == tundra_ui::SettingsCategory::Appearance {
            self.save_settings_appearance(
                tundra_storage::AppearanceConfig::default(),
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
        let defaults = tundra_storage::StorageConfig::default();
        match category {
            tundra_ui::SettingsCategory::RegionTime => {
                config.language = defaults.language;
                config.timezone = defaults.timezone;
                config.weather_location = defaults.weather_location;
            }
            tundra_ui::SettingsCategory::FileExplorer => config.explorer = defaults.explorer,
            tundra_ui::SettingsCategory::Editor => config.editor = defaults.editor,
            tundra_ui::SettingsCategory::Appearance => unreachable!(),
        }
        if let Err(error) = storage.save_config(&config) {
            self.set_settings_error(format!("Could not restore defaults: {error}"));
            return;
        }
        if category == tundra_ui::SettingsCategory::RegionTime {
            self.set_clock_timezone(Some(config.timezone.clone()));
        }
        if category == tundra_ui::SettingsCategory::Editor {
            self.editor_config = config.editor;
        }
        if let Some(state) = self.settings_state.as_mut() {
            state.config = config;
            state.status = format!("Restored {} defaults", category.label());
        }
        self.notify_status(format!("Restored {} defaults", category.label()));
    }

    fn set_settings_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        if let Some(state) = self.settings_state.as_mut() {
            state.status = format!("Error: {message}");
        }
        self.notify_status(format!("Settings error: {message}"));
    }

    pub fn to_settings_view_model(&self) -> Option<tundra_ui::SettingsViewModel> {
        let state = self.settings_state.as_ref()?;
        let global_enabled = self.can_change_global_settings();
        let cards = settings_cards(state, global_enabled);
        let appearance_preview = (state.category == tundra_ui::SettingsCategory::Appearance)
            .then_some(tundra_ui::SettingsAppearancePreview {
                border_shape: match state.appearance.border_shape {
                    tundra_storage::BorderShape::Rounded => tundra_ui::BorderShape::Rounded,
                    tundra_storage::BorderShape::Square => tundra_ui::BorderShape::Square,
                },
                border_color: ui_theme_color(state.appearance.border_color),
                accent_color: ui_theme_color(state.appearance.accent_color),
            });
        let picker = state.picker.as_ref().map(|picker| {
            let options = settings_picker_options(picker);
            tundra_ui::SettingsPickerViewModel {
                kind: picker.kind,
                title: picker_title(picker.kind).to_string(),
                query: picker.query.clone(),
                selected_index: picker.selected_index.min(options.len().saturating_sub(1)),
                window_start: picker.window_start,
                searchable: matches!(
                    picker.kind,
                    tundra_ui::SettingsPickerKind::Language
                        | tundra_ui::SettingsPickerKind::Timezone
                ),
                options,
            }
        });
        let color_editor = state.color_editor.as_ref().map(|editor| {
            tundra_ui::SettingsColorEditorViewModel {
                title: format!("Custom {}", picker_label(editor.kind)),
                value: editor.value.clone(),
                error: editor.error.clone(),
            }
        });
        let weather_location_editor = state.weather_location_editor.as_ref().map(|editor| {
            tundra_ui::SettingsWeatherLocationEditorViewModel {
                value: editor.value.clone(),
                error: editor.error.clone(),
            }
        });
        Some(tundra_ui::SettingsViewModel {
            selected_category: state.category,
            selected_field: state.selected_field,
            cards,
            appearance_preview,
            status: state.status.clone(),
            locked_message: (!global_enabled
                && state.category != tundra_ui::SettingsCategory::Appearance)
                .then_some("Locked: administrator permission is required".to_string()),
            scroll_offset: state.scroll_offset,
            picker,
            color_editor,
            weather_location_editor,
        })
    }
}

fn settings_fields(category: tundra_ui::SettingsCategory) -> &'static [tundra_ui::SettingsField] {
    match category {
        tundra_ui::SettingsCategory::Appearance => APPEARANCE_SETTINGS_FIELDS,
        tundra_ui::SettingsCategory::RegionTime => REGION_SETTINGS_FIELDS,
        tundra_ui::SettingsCategory::FileExplorer => EXPLORER_SETTINGS_FIELDS,
        tundra_ui::SettingsCategory::Editor => EDITOR_SETTINGS_FIELDS,
    }
}

fn settings_cards(
    state: &SettingsState,
    global_enabled: bool,
) -> Vec<tundra_ui::SettingsCardViewModel> {
    use tundra_ui::{
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
        tundra_ui::SettingsCategory::Appearance => vec![
            Card::new(
                "Visual style",
                vec![
                    Item::new(
                        Field::BorderShape,
                        "Border shape",
                        match state.appearance.border_shape {
                            tundra_storage::BorderShape::Rounded => "Rounded",
                            tundra_storage::BorderShape::Square => "Square",
                        },
                        "Choose rounded or square card borders.",
                        Kind::Cycle,
                    ),
                    Item::new(
                        Field::BorderColor,
                        "Border color",
                        state.appearance.border_color.to_string(),
                        "Choose a standard color or enter #RRGGBB.",
                        Kind::Palette,
                    ),
                    Item::new(
                        Field::AccentColor,
                        "Accent color",
                        state.appearance.accent_color.to_string(),
                        "Used for selection and focus; must differ from the border.",
                        Kind::Palette,
                    ),
                ],
            ),
            Card::new("Reset", vec![reset(true)]),
        ],
        tundra_ui::SettingsCategory::RegionTime => vec![
            Card::new(
                "Language and timezone",
                vec![
                    Item::new(
                        Field::Language,
                        "Language",
                        language_label(&state.config.language),
                        "Choose from the extensible language catalogue.",
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::Timezone,
                        "City / timezone",
                        timezone_label(&state.config.timezone),
                        "Search by city, region or timezone identifier.",
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::WeatherLocation,
                        "Weather location",
                        state
                            .config
                            .weather_location
                            .as_deref()
                            .unwrap_or("Same as timezone"),
                        "Enter a detailed English city or address used only by Weathr.",
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new("Reset", vec![reset(global_enabled)]),
        ],
        tundra_ui::SettingsCategory::FileExplorer => vec![
            Card::new(
                "Display",
                vec![
                    toggle(Field::ShowHidden, "Show hidden files", state.config.explorer.show_hidden, "Display hidden files in Explorer.", global_enabled),
                    toggle(Field::ShowSystem, "Show system files", state.config.explorer.show_system, "Display operating-system files.", global_enabled),
                    toggle(Field::ShowExtensions, "Show file extensions", state.config.explorer.show_extensions, "Show filename extensions.", global_enabled),
                    toggle(Field::FoldersFirst, "Folders first", state.config.explorer.folders_first, "Group directories before files.", global_enabled),
                    toggle(Field::ShowSidebar, "Show Quick Access", state.config.explorer.show_sidebar, "Show the Quick Access sidebar.", global_enabled),
                ],
            ),
            Card::new(
                "Sorting & format",
                vec![
                    toggle(Field::CaseSensitiveSort, "Case-sensitive sort", state.config.explorer.case_sensitive_sort, "Treat letter case as significant while sorting.", global_enabled),
                    Item::new(Field::SizeFormat, "Size format", size_format_label(state.config.explorer.size_format), "Choose human-readable binary sizes or exact bytes.", Kind::Cycle).enabled(global_enabled),
                    Item::new(Field::DateZone, "Date timezone", date_zone_label(state.config.explorer.date_zone), "Use the configured timezone or UTC for file dates.", Kind::Cycle).enabled(global_enabled),
                    Item::new(Field::SortField, "Default sort field", sort_field_label(state.config.explorer.sort_field), "Choose the default Explorer sort column.", Kind::Cycle).enabled(global_enabled),
                    Item::new(Field::SortDirection, "Default direction", sort_direction_label(state.config.explorer.sort_direction), "Choose ascending or descending order.", Kind::Cycle).enabled(global_enabled),
                ],
            ),
            Card::new(
                "Safety",
                vec![
                    toggle(Field::ConfirmDelete, "Confirm delete", state.config.explorer.confirm_delete, "Ask before moving items to Trash.", global_enabled),
                    toggle(Field::ConfirmNameConflicts, "Confirm name conflicts", state.config.explorer.confirm_name_conflicts, "Ask how to resolve duplicate names.", global_enabled),
                ],
            ),
            Card::new("Reset", vec![reset(global_enabled)]),
        ],
        tundra_ui::SettingsCategory::Editor => vec![
            Card::new(
                "Cursor acceleration",
                vec![
                    toggle(Field::CursorAcceleration, "Cursor acceleration", state.config.editor.cursor_acceleration_enabled, "Accelerate repeated arrow-key movement.", global_enabled),
                    Item::new(Field::CursorDelay, "Start delay", format!("{} ms", state.config.editor.cursor_acceleration_delay_ms), "Delay before acceleration begins.", Kind::Stepper).enabled(global_enabled),
                    Item::new(Field::CursorRamp, "Ramp to maximum", format!("{} ms", state.config.editor.cursor_acceleration_ramp_ms), "Time taken to reach the maximum step.", Kind::Stepper).enabled(global_enabled),
                    Item::new(Field::CursorHorizontalStep, "Horizontal maximum", format!("{} cells", state.config.editor.cursor_horizontal_max_step), "Maximum horizontal movement per repeat.", Kind::Stepper).enabled(global_enabled),
                    Item::new(Field::CursorVerticalStep, "Vertical maximum", format!("{} lines", state.config.editor.cursor_vertical_max_step), "Maximum vertical movement per repeat.", Kind::Stepper).enabled(global_enabled),
                ],
            ),
            Card::new("Reset", vec![reset(global_enabled)]),
        ],
    }
}

fn settings_picker_options(
    picker: &SettingsPickerState,
) -> Vec<tundra_ui::SettingsPickerOptionViewModel> {
    let query = picker.query.trim().to_ascii_lowercase();
    match picker.kind {
        tundra_ui::SettingsPickerKind::Language => tundra_ui::setup_language_options()
            .into_iter()
            .filter(|option| {
                query.is_empty()
                    || option.code.to_ascii_lowercase().contains(&query)
                    || option.label.to_ascii_lowercase().contains(&query)
            })
            .map(|option| {
                tundra_ui::SettingsPickerOptionViewModel::new(option.label, option.code)
            })
            .collect(),
        tundra_ui::SettingsPickerKind::Timezone => tundra_ui::setup_timezone_options()
            .into_iter()
            .filter(|option| {
                query.is_empty()
                    || option.id.to_ascii_lowercase().contains(&query)
                    || option.label.to_ascii_lowercase().contains(&query)
                    || option.description.to_ascii_lowercase().contains(&query)
            })
            .map(|option| {
                tundra_ui::SettingsPickerOptionViewModel::new(
                    option.label,
                    option.description,
                )
                .timezone(option.id, option.longitude, option.latitude)
            })
            .collect(),
        tundra_ui::SettingsPickerKind::BorderColor
        | tundra_ui::SettingsPickerKind::AccentColor => {
            let mut options = tundra_ui::setup_standard_color_options()
                .iter()
                .map(|option| {
                    tundra_ui::SettingsPickerOptionViewModel::new(option.label, option.value)
                })
                .collect::<Vec<_>>();
            options.push(tundra_ui::SettingsPickerOptionViewModel::new(
                "Custom color…",
                "#RRGGBB",
            ));
            options
        }
    }
}

fn color_picker_initial_index(color: tundra_storage::BorderColor) -> usize {
    tundra_ui::setup_standard_color_options()
        .iter()
        .position(|option| option.value == color.to_string())
        .unwrap_or_else(|| tundra_ui::setup_standard_color_options().len())
}

fn settings_picker_visible_rows(terminal_height: u16) -> usize {
    usize::from(terminal_height.saturating_sub(10).clamp(4, 18))
}

fn picker_title(kind: tundra_ui::SettingsPickerKind) -> &'static str {
    match kind {
        tundra_ui::SettingsPickerKind::Language => "Choose language",
        tundra_ui::SettingsPickerKind::Timezone => "Choose city and timezone",
        tundra_ui::SettingsPickerKind::BorderColor => "Choose border color",
        tundra_ui::SettingsPickerKind::AccentColor => "Choose accent color",
    }
}

fn picker_label(kind: tundra_ui::SettingsPickerKind) -> &'static str {
    match kind {
        tundra_ui::SettingsPickerKind::BorderColor => "Border color",
        tundra_ui::SettingsPickerKind::AccentColor => "Accent color",
        tundra_ui::SettingsPickerKind::Language => "Language",
        tundra_ui::SettingsPickerKind::Timezone => "Timezone",
    }
}

fn language_label(code: &str) -> String {
    tundra_ui::setup_language_options()
        .into_iter()
        .find(|option| option.code == code)
        .map(|option| format!("{} ({})", option.label, option.code))
        .unwrap_or_else(|| code.to_string())
}

fn timezone_label(id: &str) -> String {
    tundra_ui::setup_timezone_options()
        .into_iter()
        .find(|option| option.id == id)
        .map(|option| format!("{} ({})", option.label, option.id))
        .unwrap_or_else(|| id.to_string())
}

fn cycle_explorer_sort_field(
    value: tundra_storage::ExplorerSortField,
    delta: isize,
) -> tundra_storage::ExplorerSortField {
    let values = [
        tundra_storage::ExplorerSortField::Name,
        tundra_storage::ExplorerSortField::Type,
        tundra_storage::ExplorerSortField::Size,
        tundra_storage::ExplorerSortField::Modified,
    ];
    let index = values.iter().position(|item| *item == value).unwrap_or(0) as isize;
    values[(index + delta).clamp(0, values.len().saturating_sub(1) as isize) as usize]
}

fn settings_field_label(field: tundra_ui::SettingsField) -> &'static str {
    match field {
        tundra_ui::SettingsField::ShowHidden => "Show hidden files",
        tundra_ui::SettingsField::ShowSystem => "Show system files",
        tundra_ui::SettingsField::ShowExtensions => "Show file extensions",
        tundra_ui::SettingsField::FoldersFirst => "Folders first",
        tundra_ui::SettingsField::ShowSidebar => "Quick Access",
        tundra_ui::SettingsField::CaseSensitiveSort => "Case-sensitive sort",
        tundra_ui::SettingsField::SizeFormat => "Size format",
        tundra_ui::SettingsField::DateZone => "Date timezone",
        tundra_ui::SettingsField::SortField => "Sort field",
        tundra_ui::SettingsField::SortDirection => "Sort direction",
        tundra_ui::SettingsField::ConfirmDelete => "Delete confirmation",
        tundra_ui::SettingsField::ConfirmNameConflicts => "Conflict confirmation",
        tundra_ui::SettingsField::CursorAcceleration => "Cursor acceleration",
        tundra_ui::SettingsField::CursorDelay => "Cursor delay",
        tundra_ui::SettingsField::CursorRamp => "Cursor ramp",
        tundra_ui::SettingsField::CursorHorizontalStep => "Horizontal maximum",
        tundra_ui::SettingsField::CursorVerticalStep => "Vertical maximum",
        tundra_ui::SettingsField::BorderShape => "Border shape",
        tundra_ui::SettingsField::BorderColor => "Border color",
        tundra_ui::SettingsField::AccentColor => "Accent color",
        tundra_ui::SettingsField::Language => "Language",
        tundra_ui::SettingsField::Timezone => "Timezone",
        tundra_ui::SettingsField::WeatherLocation => "Weather location",
        tundra_ui::SettingsField::RestoreDefaults => "Defaults",
    }
}

fn is_weather_location_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, ' ' | ',' | '.' | '-' | '\'' | '/' | '(' | ')')
}

fn size_format_label(value: tundra_storage::ExplorerSizeFormat) -> &'static str {
    match value {
        tundra_storage::ExplorerSizeFormat::HumanBinary => "Human binary",
        tundra_storage::ExplorerSizeFormat::Bytes => "Bytes",
    }
}

fn date_zone_label(value: tundra_storage::ExplorerDateZone) -> &'static str {
    match value {
        tundra_storage::ExplorerDateZone::ConfiguredTimezone => "Configured timezone",
        tundra_storage::ExplorerDateZone::Utc => "UTC",
    }
}

fn sort_field_label(value: tundra_storage::ExplorerSortField) -> &'static str {
    match value {
        tundra_storage::ExplorerSortField::Name => "Name",
        tundra_storage::ExplorerSortField::Type => "Type",
        tundra_storage::ExplorerSortField::Size => "Size",
        tundra_storage::ExplorerSortField::Modified => "Modified",
    }
}

fn sort_direction_label(value: tundra_storage::ExplorerSortDirection) -> &'static str {
    match value {
        tundra_storage::ExplorerSortDirection::Ascending => "Ascending",
        tundra_storage::ExplorerSortDirection::Descending => "Descending",
    }
}
