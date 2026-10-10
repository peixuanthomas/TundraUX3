use super::*;
use crate::session::*;

impl ShellSession {
    pub fn to_settings_view_model(&self) -> Option<ui::SettingsViewModel> {
        let _language = i18n::enter_snapshot(self.language.clone());
        let state = self.settings_state.as_ref()?;
        let config = self.app.storage_config();
        let appearance = self.app.active_appearance()?;
        let global_enabled = self.can_change_global_settings();
        let identity = app::update::current_build_identity();
        let unavailable_reason = self.system_settings_unavailable_reason(state.category);
        let cards = if let Some(reason) = unavailable_reason.as_deref() {
            system_device_cards(state.category, reason)
        } else if state.category == ui::SettingsCategory::Update {
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
                summary_title: check
                    .filter(|result| result.release.is_some())
                    .map(|_| i18n::tr!("settings-release-latest")),
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
                        body: if check.is_some_and(|value| value.release.is_some()) {
                            i18n::tr!("settings-release-install-body")
                        } else if replacement {
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
            status: if let Some(reason) = unavailable_reason.as_ref() {
                reason.clone()
            } else if state.category == ui::SettingsCategory::Update {
                self.settings_update_state.status.render_current()
            } else {
                state.status.render_current()
            },
            locked_message: (!global_enabled
                && state.category != ui::SettingsCategory::Appearance
                && !state.category.is_system_device())
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

pub(super) fn update_settings_cards(
    identity: &app::update::BuildIdentity,
    update: &SettingsUpdateState,
    supported: bool,
    admin: bool,
) -> Vec<ui::SettingsCardViewModel> {
    use ui::{
        SettingsCardViewModel as Card, SettingsControlKind as Kind, SettingsField as Field,
        SettingsItemViewModel as Item,
    };
    let release_mode =
        cfg!(target_os = "linux") && update.mode == storage::LinuxUpdateMode::Release;
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
                    relation = checked_update_label(result).render_current()
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
    let mut cards = vec![
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
            if release_mode {
                i18n::tr!("settings-release-latest")
            } else if cfg!(target_os = "linux") {
                "GitHub master".into()
            } else {
                i18n::tr!("settings-github-default-branch")
            },
            vec![Item::new(
                Field::RemoteVersion,
                if release_mode {
                    i18n::tr!("settings-version")
                } else {
                    i18n::tr!("settings-latest-commit")
                },
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
                    if cfg!(target_os = "linux") {
                        i18n::tr!("settings-check-mode-description")
                    } else {
                        i18n::tr!("settings-check-description")
                    },
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
    ];
    if cfg!(target_os = "linux") {
        cards.insert(
            0,
            Card::new(
                i18n::tr!("settings-update-mode"),
                vec![
                    Item::new(
                        Field::UpdateMode,
                        i18n::tr!("settings-update-mode"),
                        match update.mode {
                            storage::LinuxUpdateMode::Release => {
                                i18n::tr!("settings-update-mode-release")
                            }
                            storage::LinuxUpdateMode::Beta => {
                                i18n::tr!("settings-update-mode-beta")
                            }
                        },
                        i18n::tr!("settings-update-mode-help"),
                        Kind::Action,
                    )
                    .enabled(admin && !update.busy),
                ],
            ),
        );
    }
    cards
}

pub(super) fn short_sha(value: &str) -> String {
    value.chars().take(7).collect()
}

pub(super) fn checked_update_label(
    result: &app::update::UpdateCheckResult,
) -> i18n::LocalizedMessage {
    if result.release.is_some() {
        match result.relation {
            app::update::UpdateRelation::Identical => i18n::msg!("settings-up-to-date"),
            app::update::UpdateRelation::Behind { .. } => i18n::msg!("settings-release-available"),
            _ => i18n::msg!("settings-release-different"),
        }
    } else {
        update_relation_label(&result.relation)
    }
}

pub(super) fn update_relation_label(
    relation: &app::update::UpdateRelation,
) -> i18n::LocalizedMessage {
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
                "AutoAdmin (AA)",
                vec![
                    Item::new(
                        Field::AutoAdmin,
                        i18n::tr!("aa-policy"),
                        crate::session::controller::auto_admin::policy_label(config.auto_admin),
                        i18n::tr!("aa-policy-help"),
                        Kind::Cycle,
                    )
                    .enabled(global_enabled),
                ],
            ),
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
        ui::SettingsCategory::Update
        | ui::SettingsCategory::Sound
        | ui::SettingsCategory::Display
        | ui::SettingsCategory::Wifi
        | ui::SettingsCategory::Bluetooth => Vec::new(),
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

pub(super) fn settings_color_label(color: storage::BorderColor) -> String {
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

pub(super) fn adjust_u16_setting(value: u16, increase: bool, minimum: u16, maximum: u16) -> u16 {
    if increase {
        value.saturating_add(1).min(maximum)
    } else {
        value.saturating_sub(1).max(minimum)
    }
}

pub(super) fn adjust_u8_setting_in_range(
    value: u8,
    increase: bool,
    minimum: u8,
    maximum: u8,
) -> u8 {
    if increase {
        value.saturating_add(1).min(maximum)
    } else {
        value.saturating_sub(1).max(minimum)
    }
}

pub(super) fn settings_saved_field(field: ui::SettingsField) -> i18n::LocalizedMessage {
    i18n::msg!("settings-saved-field", field = format!("{field:?}"))
}

pub(super) fn settings_saved_picker(kind: ui::SettingsPickerKind) -> i18n::LocalizedMessage {
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

pub(super) fn settings_scroll_offset(current: u16, delta: i16, maximum: u16) -> u16 {
    let next = if delta < 0 {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as u16)
    };
    next.min(maximum)
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
