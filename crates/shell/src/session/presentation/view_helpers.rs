use super::super::*;
pub(in crate::session) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

pub(in crate::session) fn clock_display_label(display: time::ClockDisplay) -> String {
    format!(
        "{} {:02}:{:02}",
        display.date,
        display.time.hour(),
        display.time.minute()
    )
}

pub(in crate::session) fn clock_button_active_for_screen(screen: ShellScreen) -> bool {
    matches!(
        screen,
        ShellScreen::Home
            | ShellScreen::Explorer
            | ShellScreen::Launcher
            | ShellScreen::CommandLine
            | ShellScreen::Editor
            | ShellScreen::Settings
            | ShellScreen::SystemStatus
            | ShellScreen::UserManagement
            | ShellScreen::Logs
            | ShellScreen::Diagnostics
            | ShellScreen::Clock
    )
}

pub(in crate::session) fn diagnostics_status_to_ui(
    status: app::diagnostics::DiagnosticStatus,
) -> ui::DiagnosticsStatus {
    match status {
        app::diagnostics::DiagnosticStatus::Pass => ui::DiagnosticsStatus::Pass,
        app::diagnostics::DiagnosticStatus::Unsupported => ui::DiagnosticsStatus::Unsupported,
        app::diagnostics::DiagnosticStatus::Warning => ui::DiagnosticsStatus::Warning,
        app::diagnostics::DiagnosticStatus::Fail => ui::DiagnosticsStatus::Fail,
    }
}

pub(in crate::session) fn diagnostics_incident_severity_to_ui(
    severity: watchdog::IncidentSeverity,
) -> ui::DiagnosticsStatus {
    match severity {
        watchdog::IncidentSeverity::Warning => ui::DiagnosticsStatus::Warning,
        watchdog::IncidentSeverity::Error | watchdog::IncidentSeverity::Critical => {
            ui::DiagnosticsStatus::Fail
        }
    }
}

pub(in crate::session) fn diagnostics_recovery_label(
    recovery: &watchdog::RecoveryOutcome,
) -> String {
    match recovery {
        watchdog::RecoveryOutcome::Pending => i18n::tr!("shell-pending"),
        watchdog::RecoveryOutcome::Recovered(_) => i18n::tr!("shell-recovered"),
        watchdog::RecoveryOutcome::RecoveredWithWarnings(_) => {
            i18n::tr!("shell-recovered-with-warnings")
        }
        watchdog::RecoveryOutcome::ManualActionRequired(_) => {
            i18n::tr!("shell-manual-action-required")
        }
        watchdog::RecoveryOutcome::Unrecoverable(_) => i18n::tr!("shell-unrecoverable"),
    }
}

pub(in crate::session) fn diagnostics_public_check_summary(
    check: &app::diagnostics::DiagnosticCheck,
) -> String {
    use app::diagnostics::{DiagnosticCategory, DiagnosticStatus};

    match (check.category, check.status) {
        (DiagnosticCategory::Environment, DiagnosticStatus::Pass) => {
            i18n::tr!("shell-environment-check-passed")
        }
        (DiagnosticCategory::Environment, DiagnosticStatus::Unsupported) => {
            i18n::tr!("shell-environment-capability-is-unsupported")
        }
        (DiagnosticCategory::Environment, DiagnosticStatus::Warning) => {
            i18n::tr!("shell-environment-check-needs-review")
        }
        (DiagnosticCategory::Environment, DiagnosticStatus::Fail) => {
            i18n::tr!("shell-environment-check-failed")
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Pass) => {
            i18n::tr!("shell-application-path-is-accessible")
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Unsupported) => {
            i18n::tr!("shell-application-path-capability-is-unsupported")
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Warning) => {
            i18n::tr!("shell-application-path-needs-attention")
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Fail) => {
            i18n::tr!("shell-application-path-check-failed")
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Pass) => {
            i18n::tr!("shell-storage-document-is-healthy")
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Unsupported) => {
            i18n::tr!("shell-storage-capability-is-unsupported")
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Warning) => {
            i18n::tr!("shell-storage-document-needs-attention")
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Fail) => {
            i18n::tr!("shell-storage-document-check-failed")
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Pass) => {
            i18n::tr!("shell-required-asset-is-available")
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Unsupported) => {
            i18n::tr!("shell-required-asset-capability-is-unsupported")
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Warning) => {
            i18n::tr!("shell-required-asset-needs-attention")
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Fail) => {
            i18n::tr!("shell-required-asset-check-failed")
        }
    }
}

pub(in crate::session) fn explorer_system_time_label(
    value: SystemTime,
    zone: storage::ExplorerDateZone,
    configured_timezone: Option<&str>,
) -> String {
    let utc = DateTime::<Utc>::from(value);
    match zone {
        storage::ExplorerDateZone::Utc => utc.format("%Y-%m-%d %H:%M").to_string(),
        storage::ExplorerDateZone::ConfiguredTimezone => configured_timezone
            .and_then(|timezone| timezone.parse::<chrono_tz::Tz>().ok())
            .map(|timezone| {
                utc.with_timezone(&timezone)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| utc.format("%Y-%m-%d %H:%M").to_string()),
    }
}

pub(in crate::session) fn explorer_size_label(
    size: u64,
    format: storage::ExplorerSizeFormat,
) -> String {
    if format == storage::ExplorerSizeFormat::Bytes {
        return format!("{size} B");
    }
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = size as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", size, UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(in crate::session) fn explorer_display_name(
    entry: &app::explorer::ExplorerEntry,
    show_extensions: bool,
) -> String {
    if show_extensions || entry.kind != app::explorer::ExplorerEntryKind::File {
        return entry.name.clone();
    }
    entry
        .path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| entry.name.clone())
}

pub(in crate::session) fn explorer_breadcrumb_view_models(
    path: &std::path::Path,
    state: &ExplorerState,
) -> Vec<ui::ExplorerBreadcrumbViewModel> {
    let mut ancestors = path.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    ancestors
        .into_iter()
        .enumerate()
        .map(|(index, ancestor)| {
            let label = ancestor
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .filter(|label| !label.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| ancestor.display().to_string());
            let mut model = ui::ExplorerBreadcrumbViewModel::new(
                format!("breadcrumb-{index}"),
                label,
                ancestor.display().to_string(),
            );
            model.drop_target = state
                .drag
                .as_ref()
                .and_then(|drag| drag.target.as_ref())
                .is_some_and(|target| target == ancestor);
            model
        })
        .collect()
}

pub(in crate::session) struct ExplorerContextMenuInput {
    pub(in crate::session) anchor: CellPosition,
    pub(in crate::session) selected_count: usize,
    pub(in crate::session) clipboard_available: bool,
    pub(in crate::session) is_trash: bool,
    pub(in crate::session) trash_has_items: bool,
    pub(in crate::session) focused_index: usize,
    pub(in crate::session) can_manage_launcher: bool,
    pub(in crate::session) launcher_eligible_count: usize,
}

pub(in crate::session) fn explorer_context_menu_view_model(
    input: ExplorerContextMenuInput,
) -> ui::ExplorerOverlayViewModel {
    let ExplorerContextMenuInput {
        anchor,
        selected_count,
        clipboard_available,
        is_trash,
        trash_has_items,
        focused_index,
        can_manage_launcher,
        launcher_eligible_count,
    } = input;
    let item = |id: &str, label: String, enabled: bool, dangerous: bool| {
        ui::ExplorerContextMenuItemViewModel {
            id: id.to_string(),
            label: label.to_string(),
            shortcut: None,
            enabled,
            dangerous,
        }
    };
    let items = if is_trash && selected_count > 0 {
        vec![
            item(
                "restore",
                i18n::tr!("shell-restore"),
                selected_count == 1,
                false,
            ),
            item(
                "properties",
                i18n::tr!("shell-properties"),
                selected_count == 1,
                false,
            ),
        ]
    } else if is_trash {
        vec![
            item("refresh", i18n::tr!("shell-refresh"), true, false),
            item(
                "dump-trash",
                i18n::tr!("shell-dump-trash"),
                trash_has_items,
                true,
            ),
            item("sort", i18n::tr!("shell-sort"), true, false),
            item("options", i18n::tr!("shell-advanced-options"), true, false),
        ]
    } else if selected_count > 0 {
        let mut items = vec![item(
            "open",
            i18n::tr!("shell-open"),
            selected_count == 1,
            false,
        )];
        if can_manage_launcher && launcher_eligible_count > 0 {
            let mut add_to_launcher = item(
                "add-to-launcher",
                i18n::tr!("shell-add-to-launcher"),
                true,
                false,
            );
            add_to_launcher.shortcut = Some("A".to_string());
            items.push(add_to_launcher);
        }
        items.extend([
            item("cut", i18n::tr!("shell-cut"), true, false),
            item("copy", i18n::tr!("shell-copy"), true, false),
            item(
                "rename",
                i18n::tr!("shell-rename"),
                selected_count == 1,
                false,
            ),
            item("delete", i18n::tr!("shell-delete"), true, true),
            item(
                "properties",
                i18n::tr!("shell-properties"),
                selected_count == 1,
                false,
            ),
        ]);
        items
    } else {
        vec![
            item("new-folder", i18n::tr!("shell-new-folder"), true, false),
            item("new-text", i18n::tr!("shell-new-text-file"), true, false),
            item(
                "paste",
                i18n::tr!("shell-paste"),
                clipboard_available,
                false,
            ),
            item("select-all", i18n::tr!("shell-select-all"), true, false),
            item("refresh", i18n::tr!("shell-refresh"), true, false),
            item("sort", i18n::tr!("shell-sort"), true, false),
            item("options", i18n::tr!("shell-advanced-options"), true, false),
        ]
    };
    let selected_index = (!items.is_empty()).then_some(focused_index.min(items.len() - 1));
    ui::ExplorerOverlayViewModel::ContextMenu(ui::ExplorerContextMenuViewModel {
        x: anchor.0,
        y: anchor.1,
        title: if selected_count > 0 {
            i18n::tr!("shell-selection")
        } else {
            i18n::tr!("shell-explorer")
        },
        items,
        selected_index,
    })
}

pub(in crate::session) fn explorer_sort_menu_view_model(
    anchor: CellPosition,
    selected: ui::ExplorerSortColumn,
    focused_index: usize,
) -> ui::ExplorerOverlayViewModel {
    let items = ui::ExplorerSortColumn::ALL
        .into_iter()
        .map(|column| ui::ExplorerContextMenuItemViewModel {
            id: match column {
                ui::ExplorerSortColumn::Name => "sort-name",
                ui::ExplorerSortColumn::Type => "sort-type",
                ui::ExplorerSortColumn::Size => "sort-size",
                ui::ExplorerSortColumn::Modified => "sort-modified",
            }
            .to_string(),
            label: if column == selected {
                format!("* {}", column.label())
            } else {
                format!("  {}", column.label())
            },
            shortcut: None,
            enabled: true,
            dangerous: false,
        })
        .collect();
    ui::ExplorerOverlayViewModel::ContextMenu(ui::ExplorerContextMenuViewModel {
        x: anchor.0,
        y: anchor.1,
        title: i18n::tr!("shell-sort-by"),
        items,
        selected_index: Some(focused_index.min(ui::ExplorerSortColumn::ALL.len() - 1)),
    })
}

pub(in crate::session) fn explorer_options_view_model(
    state: &ExplorerState,
    focused_index: usize,
    enabled: bool,
) -> ui::ExplorerOverlayViewModel {
    let toggle = |id: &str, label: String, value: bool| ui::ExplorerOptionViewModel {
        id: id.to_string(),
        label: label.to_string(),
        value: if value {
            i18n::tr!("shell-on")
        } else {
            i18n::tr!("shell-off")
        }
        .to_string(),
        enabled,
        selected: value,
        focused: false,
    };
    let mut options = vec![
        toggle(
            "hidden",
            i18n::tr!("shell-show-hidden-files"),
            state.show_hidden,
        ),
        toggle(
            "system",
            i18n::tr!("shell-show-system-files"),
            state.show_system,
        ),
        toggle(
            "extensions",
            i18n::tr!("shell-show-file-extensions"),
            state.show_extensions,
        ),
        toggle(
            "folders-first",
            i18n::tr!("shell-folders-first"),
            state.folders_first,
        ),
        toggle(
            "case-sensitive",
            i18n::tr!("shell-case-sensitive-sort"),
            state.case_sensitive_sort,
        ),
        ui::ExplorerOptionViewModel {
            id: "size-format".to_string(),
            label: i18n::tr!("shell-size-format"),
            value: match state.size_format {
                storage::ExplorerSizeFormat::HumanBinary => i18n::tr!("shell-human-binary"),
                storage::ExplorerSizeFormat::Bytes => i18n::tr!("shell-bytes"),
            }
            .to_string(),
            enabled,
            selected: false,
            focused: false,
        },
        ui::ExplorerOptionViewModel {
            id: "date-zone".to_string(),
            label: i18n::tr!("shell-date-zone"),
            value: match state.date_zone {
                storage::ExplorerDateZone::ConfiguredTimezone => i18n::tr!("shell-configured"),
                storage::ExplorerDateZone::Utc => "UTC".to_string(),
            }
            .to_string(),
            enabled,
            selected: false,
            focused: false,
        },
        toggle(
            "confirm-delete",
            i18n::tr!("shell-confirm-delete"),
            state.confirm_delete,
        ),
        toggle(
            "confirm-conflicts",
            i18n::tr!("shell-confirm-name-conflicts"),
            state.confirm_name_conflicts,
        ),
        toggle(
            "sidebar",
            i18n::tr!("shell-show-quick-access"),
            state.show_sidebar,
        ),
    ];
    let option_count = options.len();
    if let Some(option) = options.get_mut(focused_index.min(option_count.saturating_sub(1))) {
        option.focused = true;
    }
    ui::ExplorerOverlayViewModel::Options(ui::ExplorerOptionsViewModel {
        title: i18n::tr!("shell-advanced-options"),
        options,
        close_label: i18n::tr!("shell-close"),
    })
}

pub(in crate::session) fn explorer_properties_view_model(
    state: &ExplorerState,
    configured_timezone: Option<&str>,
) -> ui::ExplorerOverlayViewModel {
    let Some(entry) = state.selected_entry() else {
        return ui::ExplorerOverlayViewModel::Properties(ui::ExplorerPropertiesViewModel {
            title: i18n::tr!("shell-properties"),
            properties: vec![ui::ExplorerPropertyViewModel {
                label: i18n::tr!("shell-selection"),
                value: i18n::tr!("shell-no-item-selected"),
            }],
            close_label: i18n::tr!("shell-close"),
        });
    };
    let mut properties = vec![
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-name"),
            value: entry.name.clone(),
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-path"),
            value: entry.path.display().to_string(),
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-type"),
            value: entry.localized_type_label().render_current(),
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-size"),
            value: if entry.kind == app::explorer::ExplorerEntryKind::Directory {
                "--".to_string()
            } else {
                explorer_size_label(entry.size, state.size_format)
            },
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-modified"),
            value: entry
                .modified
                .map(|modified| {
                    explorer_system_time_label(modified, state.date_zone, configured_timezone)
                })
                .unwrap_or_else(|| i18n::tr!("shell-unknown")),
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-attributes"),
            value: {
                let labels = explorer_attribute_labels(&entry.attributes);
                if labels.is_empty() {
                    i18n::tr!("shell-none")
                } else {
                    labels.join(", ")
                }
            },
        },
    ];
    if let Some(reason) = entry.open_policy.reason() {
        properties.push(ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-open-policy"),
            value: reason.to_string(),
        });
    }
    ui::ExplorerOverlayViewModel::Properties(ui::ExplorerPropertiesViewModel {
        title: i18n::tr!("shell-properties-arg1", arg1 = &entry.name),
        properties,
        close_label: i18n::tr!("shell-close"),
    })
}

pub(in crate::session) fn explorer_attribute_labels(attributes: &FileAttributes) -> Vec<String> {
    let mut labels = Vec::new();
    if attributes.readonly {
        labels.push(i18n::tr!("shell-readonly"));
    }
    if attributes.hidden {
        labels.push(i18n::tr!("shell-hidden"));
    }
    if attributes.system {
        labels.push(i18n::tr!("shell-system"));
    }
    if attributes.archive {
        labels.push(i18n::tr!("shell-archive"));
    }
    if attributes.symlink {
        labels.push(i18n::tr!("shell-symlink"));
    }
    if attributes.junction {
        labels.push(i18n::tr!("shell-junction"));
    }
    if attributes.reparse_point {
        labels.push(i18n::tr!("shell-reparse"));
    }
    if attributes.shortcut {
        labels.push(i18n::tr!("shell-shortcut"));
    }
    labels
}

pub(in crate::session) fn explorer_input_prompt(mode: ExplorerInputMode) -> String {
    match mode {
        ExplorerInputMode::Browse => i18n::tr!("shell-explorer"),
        ExplorerInputMode::Address => i18n::tr!("shell-absolute-path"),
        ExplorerInputMode::Search => i18n::tr!("shell-search"),
        ExplorerInputMode::NewFolder => i18n::tr!("shell-new-folder-name"),
        ExplorerInputMode::NewTextFile => i18n::tr!("shell-new-text-file-name"),
        ExplorerInputMode::Rename => i18n::tr!("shell-rename-to"),
        ExplorerInputMode::RestoreDestination => i18n::tr!("shell-restore-destination-directory"),
    }
}

pub(in crate::session) fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .ok()
        .and_then(|millis| u64::try_from(millis).ok())
        .unwrap_or(0)
}

pub(in crate::session) fn format_core_error(error: &CoreError) -> i18n::LocalizedText {
    match error {
        CoreError::InvalidCredentials => {
            i18n::LocalizedText::from(i18n::msg!("shell-invalid-username-or-password"))
        }
        CoreError::AccountDisabled => {
            i18n::LocalizedText::from(i18n::msg!("shell-account-disabled"))
        }
        CoreError::AccountLocked { .. } => {
            i18n::LocalizedText::from(i18n::msg!("shell-account-locked"))
        }
        CoreError::BootstrapAlreadyExists => {
            i18n::LocalizedText::from(i18n::msg!("shell-admin-already-exists"))
        }
        CoreError::BootstrapRequired => {
            i18n::LocalizedText::from(i18n::msg!("shell-create-the-first-admin-account"))
        }
        CoreError::DuplicateUsername => {
            i18n::LocalizedText::from(i18n::msg!("shell-username-already-exists"))
        }
        CoreError::InvalidUsername => {
            i18n::LocalizedText::from(i18n::msg!("shell-invalid-username"))
        }
        CoreError::InvalidUserInfo(reason) => i18n::LocalizedText::from(i18n::msg!(
            "shell-invalid-user-info-reason",
            reason = reason.to_string()
        )),
        CoreError::InvalidPassword(reason) => i18n::LocalizedText::from(i18n::msg!(
            "shell-invalid-password-reason",
            reason = reason.to_string()
        )),
        CoreError::LastPrivilegedUserRequired => {
            i18n::LocalizedText::from(i18n::msg!("shell-at-least-one-enabled-admin-is-required"))
        }
        CoreError::PermissionDenied { reason, .. } => i18n::LocalizedText::from(i18n::msg!(
            "shell-permission-denied-reason",
            reason = reason.to_string()
        )),
        CoreError::UserNotFound => i18n::LocalizedText::from(i18n::msg!("shell-user-not-found")),
        other => other.to_string().into(),
    }
}

pub(in crate::session) fn login_error_message(
    error: &CoreError,
    password_hint: Option<&str>,
) -> i18n::LocalizedText {
    if matches!(error, CoreError::InvalidCredentials)
        && let Some(hint) = password_hint.map(str::trim).filter(|hint| !hint.is_empty())
    {
        return i18n::LocalizedText::from(i18n::msg!("shell-password-hint-hint", hint = hint));
    }

    format_core_error(error)
}

pub(in crate::session) fn to_ui_user_management_field(
    field: UserManagementFormField,
) -> ui::UserManagementField {
    match field {
        UserManagementFormField::Username => ui::UserManagementField::Username,
        UserManagementFormField::DisplayName => ui::UserManagementField::DisplayName,
        UserManagementFormField::Role => ui::UserManagementField::Role,
        UserManagementFormField::Password => ui::UserManagementField::Password,
        UserManagementFormField::Submit => ui::UserManagementField::Submit,
        UserManagementFormField::Cancel => ui::UserManagementField::Cancel,
    }
}

pub(in crate::session) fn from_ui_user_management_field(
    field: ui::UserManagementField,
) -> UserManagementFormField {
    match field {
        ui::UserManagementField::Username => UserManagementFormField::Username,
        ui::UserManagementField::DisplayName => UserManagementFormField::DisplayName,
        ui::UserManagementField::Role => UserManagementFormField::Role,
        ui::UserManagementField::Password => UserManagementFormField::Password,
        ui::UserManagementField::Submit => UserManagementFormField::Submit,
        ui::UserManagementField::Cancel => UserManagementFormField::Cancel,
    }
}

pub(in crate::session) fn user_management_action_model(
    action: ui::UserManagementAction,
    label: String,
    shortcut: Option<char>,
    enabled: bool,
    disabled_reason: Option<String>,
    dangerous: bool,
) -> ui::UserManagementActionViewModel {
    ui::UserManagementActionViewModel {
        action,
        label: label.to_string(),
        shortcut,
        enabled,
        disabled_reason: (!enabled).then_some(disabled_reason).flatten(),
        dangerous,
    }
}

pub(in crate::session) fn user_is_locked(user: &UserAccount) -> bool {
    user.locked_until_epoch_ms
        .is_some_and(|locked_until| locked_until > unix_millis())
}

pub(in crate::session) fn user_home_entries() -> Vec<ui::ShellEntry> {
    vec![
        ui::ShellEntry::new(i18n::tr!("shell-explorer"), i18n::tr!("shell-browse-files"))
            .with_icon_key("explorer"),
        ui::ShellEntry::new(
            i18n::tr!("shell-launcher"),
            i18n::tr!("shell-open-apps-and-commands"),
        )
        .with_icon_key("launcher"),
        ui::ShellEntry::new(
            i18n::tr!("shell-settings"),
            i18n::tr!("shell-adjust-tundraux"),
        )
        .with_icon_key("settings"),
        ui::ShellEntry::new(
            i18n::tr!("shell-system-status"),
            i18n::tr!("shell-view-storage-and-network-health"),
        )
        .with_icon_key("system_status"),
        ui::ShellEntry::new(
            i18n::tr!("shell-logs"),
            i18n::tr!("shell-view-runtime-logs-and-incidents"),
        )
        .with_icon_key("logs"),
    ]
}

pub(in crate::session) fn terminal_flag_labels(flags: ShellTerminalFlags) -> Vec<String> {
    let mut labels = Vec::new();

    if flags.raw_mode {
        labels.push(i18n::tr!("shell-raw-mode-enabled"));
    }
    if flags.alternate_screen {
        labels.push(i18n::tr!("shell-alternate-screen-enabled"));
    }
    if flags.mouse_capture {
        labels.push(i18n::tr!("shell-mouse-capture-enabled"));
    }
    if flags.cursor_restore_enabled {
        labels.push(i18n::tr!("shell-cursor-restore-enabled"));
    }

    labels
}

pub(in crate::session) fn resolved_home_mode(
    launch_config: ShellLaunchConfig,
    startup: &ShellStartupState,
) -> ShellHomeMode {
    let requested_mode = match launch_config.home_mode_override {
        HomeModeOverride::Debug => ShellHomeMode::Debug,
        HomeModeOverride::BuildDefault => startup
            .restored_session
            .as_ref()
            .map(|session| session.display_mode)
            .or(startup.app_config.home_mode)
            .unwrap_or_else(|| ShellSession::legacy_default_home_mode(launch_config)),
    };

    if requested_mode == ShellHomeMode::Debug && !startup.debug_policy.allows_debug() {
        ShellHomeMode::User
    } else {
        requested_mode
    }
}

pub(in crate::session) fn should_show_startup_lockscreen(startup: &ShellStartupState) -> bool {
    startup.system_auth_session.is_none()
        && startup.storage_manager.is_some()
        && !startup.auth_bootstrap_required
        && !startup.login_users.is_empty()
}

pub(in crate::session) fn platform_capability_summary(
    kind: PlatformKind,
    capabilities: &PlatformCapabilities,
) -> i18n::LocalizedText {
    let (mut supported, mut best_effort, mut unsupported) = (0, 0, 0);

    for (_, status) in capabilities.checks() {
        match status {
            CapabilityStatus::Supported => supported += 1,
            CapabilityStatus::BestEffort => best_effort += 1,
            CapabilityStatus::Unsupported => unsupported += 1,
        }
    }

    i18n::msg!(
        "shell-arg1-supported-supported-best-effort-best-effort-unsupported-unsupported",
        arg1 = kind.as_str(),
        supported = supported,
        best_effort = best_effort,
        unsupported = unsupported
    )
    .into()
}

pub(in crate::session) fn build_mode_label() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

pub(in crate::session) fn component_message(component: ShellComponent) -> i18n::LocalizedMessage {
    match component {
        ShellComponent::CompactHome => i18n::msg!("shell-component-compact-home"),
        ShellComponent::TopBar => i18n::msg!("shell-component-top-bar"),
        ShellComponent::Home => i18n::msg!("shell-component-home"),
        ShellComponent::ClockButton => i18n::msg!("shell-component-clock-button"),
        ShellComponent::Clock => i18n::msg!("shell-component-clock"),
        ShellComponent::ClockNewButton => i18n::msg!("shell-component-clock-new-button"),
        ShellComponent::ClockEntryList => i18n::msg!("shell-component-clock-entry-list"),
        ShellComponent::ClockCreateDialog => i18n::msg!("shell-component-clock-create-dialog"),
        ShellComponent::ClockCreateInput => i18n::msg!("shell-component-clock-create-input"),
        ShellComponent::ClockCreateAlarmButton => {
            i18n::msg!("shell-component-clock-create-alarm-button")
        }
        ShellComponent::ClockCreateCountdownButton => {
            i18n::msg!("shell-component-clock-create-countdown-button")
        }
        ShellComponent::Diagnostics => i18n::msg!("shell-component-diagnostics"),
        ShellComponent::Logs => i18n::msg!("shell-component-logs"),
        ShellComponent::SystemStatus => i18n::msg!("shell-component-system-status"),
        ShellComponent::DiagnosticsRepairDialog => {
            i18n::msg!("shell-component-diagnostics-repair-dialog")
        }
        ShellComponent::LoginUserList => i18n::msg!("shell-component-login-user-list"),
        ShellComponent::LoginUsername => i18n::msg!("shell-component-login-username"),
        ShellComponent::LoginPassword => i18n::msg!("shell-component-login-password"),
        ShellComponent::LoginPasswordVisibility => {
            i18n::msg!("shell-component-login-password-visibility")
        }
        ShellComponent::HomeLogout => i18n::msg!("shell-component-home-logout"),
        ShellComponent::SetupLanguage => i18n::msg!("shell-component-setup-language"),
        ShellComponent::SetupTimezone => i18n::msg!("shell-component-setup-timezone"),
        ShellComponent::SetupAdminUsername => i18n::msg!("shell-component-setup-admin-username"),
        ShellComponent::SetupAdminPassword => i18n::msg!("shell-component-setup-admin-password"),
        ShellComponent::SetupAdminPasswordConfirm => {
            i18n::msg!("shell-component-setup-admin-password-confirm")
        }
        ShellComponent::SetupAdminHint => i18n::msg!("shell-component-setup-admin-hint"),
        ShellComponent::SetupSubmit => i18n::msg!("shell-component-setup-submit"),
        ShellComponent::SetupAppearanceShape => {
            i18n::msg!("shell-component-setup-appearance-shape")
        }
        ShellComponent::SetupAppearanceThemeColor => {
            i18n::msg!("shell-component-setup-appearance-theme-color")
        }
        ShellComponent::SetupAppearanceThemeCustom => {
            i18n::msg!("shell-component-setup-appearance-theme-custom")
        }
        ShellComponent::SetupAppearanceAccentColor => {
            i18n::msg!("shell-component-setup-appearance-accent-color")
        }
        ShellComponent::SetupAppearanceAccentCustom => {
            i18n::msg!("shell-component-setup-appearance-accent-custom")
        }
        ShellComponent::SetupAppearanceSubmit => {
            i18n::msg!("shell-component-setup-appearance-submit")
        }
        ShellComponent::SetupCustomColorDialog => {
            i18n::msg!("shell-component-setup-custom-color-dialog")
        }
        ShellComponent::BootstrapUsername => i18n::msg!("shell-component-bootstrap-username"),
        ShellComponent::BootstrapPassword => i18n::msg!("shell-component-bootstrap-password"),
        ShellComponent::Explorer => i18n::msg!("shell-component-explorer"),
        ShellComponent::Launcher => i18n::msg!("shell-component-launcher"),
        ShellComponent::CommandLine => i18n::msg!("shell-component-command-line"),
        ShellComponent::Editor => i18n::msg!("shell-component-editor"),
        ShellComponent::Settings => i18n::msg!("shell-component-settings"),
        ShellComponent::UserManagement => i18n::msg!("shell-component-user-management"),
        ShellComponent::StatusBar => i18n::msg!("shell-component-status-bar"),
        ShellComponent::ExitDialog => i18n::msg!("shell-component-exit-dialog"),
        ShellComponent::TimeSyncDialog => i18n::msg!("shell-component-time-sync-dialog"),
        ShellComponent::NotificationDialog => i18n::msg!("shell-component-notification-dialog"),
        ShellComponent::ContextMenu => i18n::msg!("shell-component-context-menu"),
    }
}
