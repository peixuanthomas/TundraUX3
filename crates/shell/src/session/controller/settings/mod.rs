mod background;
pub(in crate::session) mod devices;
mod forms;
pub(in crate::session) mod tasks;
mod view;
use crate::session::*;
use devices::*;
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
    ui::SettingsField::AutoAdmin,
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
    #[cfg(target_os = "linux")]
    ui::SettingsField::UpdateMode,
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
        let appearance = match users.current_user_appearance(&actor) {
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

        if self.settings_update_state.mode != config.linux_update_mode {
            self.settings_update_state = SettingsUpdateState {
                mode: config.linux_update_mode,
                ..Default::default()
            };
        }
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
        self.enter_screen(ShellScreen::Settings);
        self.error_message = None;
        self.notify_status(i18n::msg!("settings-title"));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_settings(&mut self) {
        self.settings_state = None;
        self.return_from_screen(ShellScreen::Settings);
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
        if self.settings_state.is_none() || key.phase == InputPhase::Release {
            return;
        }
        let editing_text = self.settings_state.as_ref().is_some_and(|state| {
            state.time_sync_server_editor.is_some()
                || state.file_extensions_editor.is_some()
                || state.weather_location_editor.is_some()
                || state.color_editor.is_some()
                || state.picker.as_ref().is_some_and(|picker| {
                    matches!(
                        picker.kind,
                        ui::SettingsPickerKind::Language | ui::SettingsPickerKind::Timezone
                    )
                })
        });
        if key.phase == InputPhase::Repeat
            && (matches!(key.key, InputKey::Enter | InputKey::Escape)
                || (key.key == InputKey::Char(' ') && !editing_text))
        {
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
            InputKey::Tab if key.modifiers.shift => self.select_settings_category_delta(-1),
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
        let app_area = match self.shell_layout_for(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = ui::settings_layout(app_area, &model);
        match ui::settings_hit_test(&layout, coordinates) {
            Some(ui::SettingsHitTarget::OverlayApply) => {
                self.handle_settings_key(&KeyInput::new(InputKey::Enter), platform)
            }
            Some(ui::SettingsHitTarget::OverlayCancel) => {
                self.handle_settings_key(&KeyInput::new(InputKey::Escape), platform)
            }
            Some(ui::SettingsHitTarget::AdjustField(field, delta)) => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.selected_field = field;
                }
                self.adjust_selected_setting(delta, platform);
            }
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
        }
        if let Some(layout) = self.current_settings_layout()
            && let Some(row) = layout.selected_field_row
            && let Some(state) = self.settings_state.as_mut()
        {
            state.scroll_offset = if row < layout.scroll_offset {
                row
            } else if row >= layout.scroll_offset.saturating_add(layout.detail.height) {
                row.saturating_add(1).saturating_sub(layout.detail.height)
            } else {
                layout.scroll_offset
            }
            .min(layout.max_scroll_offset);
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
        let main = match self.shell_layout_for(terminal) {
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
        if self.block_unavailable_system_setting() {
            return;
        }
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
            ui::SettingsField::UpdateMode => self.switch_update_mode(),
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
        if self.block_unavailable_system_setting() {
            return;
        }
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
                | ui::SettingsField::UpdateMode
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
            ui::SettingsField::AutoAdmin => {
                use storage::AutoAdminPolicy::*;
                config.auto_admin = match (config.auto_admin, increase) {
                    (Manual, true) | (Deny, false) => Automatic,
                    (Automatic, true) | (Manual, false) => Deny,
                    _ => Manual,
                };
            }
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

    pub(in crate::session) fn request_settings_restore_defaults(&mut self) {
        let Some(category) = self.settings_state.as_ref().map(|state| state.category) else {
            return;
        };
        if category.is_system_device() {
            return;
        }
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
        if category.is_system_device() {
            return;
        }
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
            ui::SettingsCategory::System => {
                config.system_status = defaults.system_status;
                config.auto_admin = defaults.auto_admin;
            }
            ui::SettingsCategory::FileExplorer => config.explorer = defaults.explorer,
            ui::SettingsCategory::Editor => config.editor = defaults.editor,
            ui::SettingsCategory::Appearance => unreachable!(),
            ui::SettingsCategory::Update
            | ui::SettingsCategory::Sound
            | ui::SettingsCategory::Display
            | ui::SettingsCategory::Wifi
            | ui::SettingsCategory::Bluetooth => return,
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
}

pub(in crate::session) fn settings_fields(
    category: ui::SettingsCategory,
) -> &'static [ui::SettingsField] {
    match category {
        ui::SettingsCategory::Appearance => APPEARANCE_SETTINGS_FIELDS,
        ui::SettingsCategory::RegionTime => REGION_SETTINGS_FIELDS,
        ui::SettingsCategory::System => SYSTEM_SETTINGS_FIELDS,
        ui::SettingsCategory::Sound => SOUND_FIELDS,
        ui::SettingsCategory::Display => DISPLAY_FIELDS,
        ui::SettingsCategory::Wifi => WIFI_FIELDS,
        ui::SettingsCategory::Bluetooth => BLUETOOTH_FIELDS,
        ui::SettingsCategory::FileExplorer => EXPLORER_SETTINGS_FIELDS,
        ui::SettingsCategory::Editor => EDITOR_SETTINGS_FIELDS,
        ui::SettingsCategory::Update => UPDATE_SETTINGS_FIELDS,
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/settings/update_tests.rs"]
mod update_tests;

use view::*;
