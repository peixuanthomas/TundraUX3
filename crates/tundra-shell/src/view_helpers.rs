fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn clock_display_label(display: tundra_weathr::network_clock::ClockDisplay) -> String {
    format!(
        "{} {:02}:{:02}",
        display.date,
        display.time.hour(),
        display.time.minute()
    )
}

fn clock_button_active_for_screen(screen: ShellScreen) -> bool {
    matches!(
        screen,
        ShellScreen::Home
            | ShellScreen::Explorer
            | ShellScreen::Launcher
            | ShellScreen::Editor
            | ShellScreen::Settings
            | ShellScreen::UserManagement
            | ShellScreen::Diagnostics
            | ShellScreen::Clock
    )
}

fn diagnostics_status_to_ui(
    status: tundra_apps::diagnostics::DiagnosticStatus,
) -> tundra_ui::DiagnosticsStatus {
    match status {
        tundra_apps::diagnostics::DiagnosticStatus::Pass => tundra_ui::DiagnosticsStatus::Pass,
        tundra_apps::diagnostics::DiagnosticStatus::Warning => {
            tundra_ui::DiagnosticsStatus::Warning
        }
        tundra_apps::diagnostics::DiagnosticStatus::Fail => tundra_ui::DiagnosticsStatus::Fail,
    }
}

fn diagnostics_incident_severity_to_ui(
    severity: tundra_watchdog::IncidentSeverity,
) -> tundra_ui::DiagnosticsStatus {
    match severity {
        tundra_watchdog::IncidentSeverity::Warning => tundra_ui::DiagnosticsStatus::Warning,
        tundra_watchdog::IncidentSeverity::Error | tundra_watchdog::IncidentSeverity::Critical => {
            tundra_ui::DiagnosticsStatus::Fail
        }
    }
}

fn diagnostics_recovery_label(recovery: &tundra_watchdog::RecoveryOutcome) -> String {
    match recovery {
        tundra_watchdog::RecoveryOutcome::Pending => "Pending".to_string(),
        tundra_watchdog::RecoveryOutcome::Recovered(_) => "Recovered".to_string(),
        tundra_watchdog::RecoveryOutcome::RecoveredWithWarnings(_) => {
            "Recovered with warnings".to_string()
        }
        tundra_watchdog::RecoveryOutcome::ManualActionRequired(_) => {
            "Manual action required".to_string()
        }
        tundra_watchdog::RecoveryOutcome::Unrecoverable(_) => "Unrecoverable".to_string(),
    }
}

fn diagnostics_public_check_summary(check: &tundra_apps::diagnostics::DiagnosticCheck) -> String {
    use tundra_apps::diagnostics::{DiagnosticCategory, DiagnosticStatus};

    match (check.category, check.status) {
        (DiagnosticCategory::Environment, DiagnosticStatus::Pass) => {
            "Environment check passed".to_string()
        }
        (DiagnosticCategory::Environment, DiagnosticStatus::Warning) => {
            "Environment check needs review".to_string()
        }
        (DiagnosticCategory::Environment, DiagnosticStatus::Fail) => {
            "Environment check failed".to_string()
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Pass) => {
            "Application path is accessible".to_string()
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Warning) => {
            "Application path needs attention".to_string()
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Fail) => {
            "Application path check failed".to_string()
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Pass) => {
            "Storage document is healthy".to_string()
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Warning) => {
            "Storage document needs attention".to_string()
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Fail) => {
            "Storage document check failed".to_string()
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Pass) => {
            "Required asset is available".to_string()
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Warning) => {
            "Required asset needs attention".to_string()
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Fail) => {
            "Required asset check failed".to_string()
        }
    }
}

fn startup_clock_timezone_id(startup: &ShellStartupState) -> Option<String> {
    startup
        .storage_manager
        .as_ref()
        .and_then(|storage| storage.load_config().ok())
        .map(|config| config.timezone)
        .or_else(|| Some("UTC".to_string()))
}

fn explorer_system_time_label(
    value: SystemTime,
    zone: tundra_storage::ExplorerDateZone,
    configured_timezone: Option<&str>,
) -> String {
    let utc = DateTime::<Utc>::from(value);
    match zone {
        tundra_storage::ExplorerDateZone::Utc => utc.format("%Y-%m-%d %H:%M").to_string(),
        tundra_storage::ExplorerDateZone::ConfiguredTimezone => configured_timezone
            .and_then(|timezone| timezone.parse::<chrono_tz::Tz>().ok())
            .map(|timezone| {
                utc.with_timezone(&timezone)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| utc.format("%Y-%m-%d %H:%M").to_string()),
    }
}

fn explorer_size_label(size: u64, format: tundra_storage::ExplorerSizeFormat) -> String {
    if format == tundra_storage::ExplorerSizeFormat::Bytes {
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

fn explorer_display_name(
    entry: &tundra_apps::explorer::ExplorerEntry,
    show_extensions: bool,
) -> String {
    if show_extensions || entry.kind != tundra_apps::explorer::ExplorerEntryKind::File {
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

fn explorer_breadcrumb_view_models(
    path: &std::path::Path,
    state: &ExplorerState,
) -> Vec<tundra_ui::ExplorerBreadcrumbViewModel> {
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
            let mut model = tundra_ui::ExplorerBreadcrumbViewModel::new(
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

fn explorer_context_menu_view_model(
    anchor: CellPosition,
    selected_count: usize,
    clipboard_available: bool,
    is_trash: bool,
    trash_has_items: bool,
    focused_index: usize,
    can_manage_launcher: bool,
    launcher_eligible_count: usize,
) -> tundra_ui::ExplorerOverlayViewModel {
    let item = |id: &str, label: &str, enabled: bool, dangerous: bool| {
        tundra_ui::ExplorerContextMenuItemViewModel {
            id: id.to_string(),
            label: label.to_string(),
            shortcut: None,
            enabled,
            dangerous,
        }
    };
    let items = if is_trash && selected_count > 0 {
        vec![
            item("restore", "Restore", selected_count == 1, false),
            item("properties", "Properties", selected_count == 1, false),
        ]
    } else if is_trash {
        vec![
            item("refresh", "Refresh", true, false),
            item("dump-trash", "Dump Trash", trash_has_items, true),
            item("sort", "Sort", true, false),
            item("options", "Advanced options", true, false),
        ]
    } else if selected_count > 0 {
        let mut items = vec![
            item("open", "Open", selected_count == 1, false),
        ];
        if can_manage_launcher && launcher_eligible_count > 0 {
            let mut add_to_launcher = item(
                "add-to-launcher",
                "Add to Launcher",
                true,
                false,
            );
            add_to_launcher.shortcut = Some("A".to_string());
            items.push(add_to_launcher);
        }
        items.extend([
            item("cut", "Cut", true, false),
            item("copy", "Copy", true, false),
            item("rename", "Rename", selected_count == 1, false),
            item("delete", "Delete", true, true),
            item("properties", "Properties", selected_count == 1, false),
        ]);
        items
    } else {
        vec![
            item("new-folder", "New folder", true, false),
            item("new-text", "New text file", true, false),
            item("paste", "Paste", clipboard_available, false),
            item("select-all", "Select all", true, false),
            item("refresh", "Refresh", true, false),
            item("sort", "Sort", true, false),
            item("options", "Advanced options", true, false),
        ]
    };
    let selected_index = (!items.is_empty()).then_some(focused_index.min(items.len() - 1));
    tundra_ui::ExplorerOverlayViewModel::ContextMenu(tundra_ui::ExplorerContextMenuViewModel {
        x: anchor.0,
        y: anchor.1,
        title: if selected_count > 0 {
            "Selection".to_string()
        } else {
            "Explorer".to_string()
        },
        items,
        selected_index,
    })
}

fn explorer_sort_menu_view_model(
    anchor: CellPosition,
    selected: tundra_ui::ExplorerSortColumn,
    focused_index: usize,
) -> tundra_ui::ExplorerOverlayViewModel {
    let items = tundra_ui::ExplorerSortColumn::ALL
        .into_iter()
        .map(|column| tundra_ui::ExplorerContextMenuItemViewModel {
            id: format!("sort-{}", column.label().to_ascii_lowercase()),
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
    tundra_ui::ExplorerOverlayViewModel::ContextMenu(tundra_ui::ExplorerContextMenuViewModel {
        x: anchor.0,
        y: anchor.1,
        title: "Sort by".to_string(),
        items,
        selected_index: Some(focused_index.min(tundra_ui::ExplorerSortColumn::ALL.len() - 1)),
    })
}

fn explorer_options_view_model(
    state: &ExplorerState,
    focused_index: usize,
    enabled: bool,
) -> tundra_ui::ExplorerOverlayViewModel {
    let toggle = |id: &str, label: &str, value: bool| tundra_ui::ExplorerOptionViewModel {
        id: id.to_string(),
        label: label.to_string(),
        value: if value { "On" } else { "Off" }.to_string(),
        enabled,
        selected: value,
        focused: false,
    };
    let mut options = vec![
        toggle("hidden", "Show hidden files", state.show_hidden),
        toggle("system", "Show system files", state.show_system),
        toggle("extensions", "Show file extensions", state.show_extensions),
        toggle("folders-first", "Folders first", state.folders_first),
        toggle(
            "case-sensitive",
            "Case-sensitive sort",
            state.case_sensitive_sort,
        ),
        tundra_ui::ExplorerOptionViewModel {
            id: "size-format".to_string(),
            label: "Size format".to_string(),
            value: match state.size_format {
                tundra_storage::ExplorerSizeFormat::HumanBinary => "Human binary",
                tundra_storage::ExplorerSizeFormat::Bytes => "Bytes",
            }
            .to_string(),
            enabled,
            selected: false,
            focused: false,
        },
        tundra_ui::ExplorerOptionViewModel {
            id: "date-zone".to_string(),
            label: "Date zone".to_string(),
            value: match state.date_zone {
                tundra_storage::ExplorerDateZone::ConfiguredTimezone => "Configured",
                tundra_storage::ExplorerDateZone::Utc => "UTC",
            }
            .to_string(),
            enabled,
            selected: false,
            focused: false,
        },
        toggle("confirm-delete", "Confirm delete", state.confirm_delete),
        toggle(
            "confirm-conflicts",
            "Confirm name conflicts",
            state.confirm_name_conflicts,
        ),
        toggle("sidebar", "Show quick access", state.show_sidebar),
    ];
    let option_count = options.len();
    if let Some(option) = options.get_mut(focused_index.min(option_count.saturating_sub(1))) {
        option.focused = true;
    }
    tundra_ui::ExplorerOverlayViewModel::Options(tundra_ui::ExplorerOptionsViewModel {
        title: "Advanced options".to_string(),
        options,
        close_label: "Close".to_string(),
    })
}

fn explorer_properties_view_model(
    state: &ExplorerState,
    configured_timezone: Option<&str>,
) -> tundra_ui::ExplorerOverlayViewModel {
    let Some(entry) = state.selected_entry() else {
        return tundra_ui::ExplorerOverlayViewModel::Properties(
            tundra_ui::ExplorerPropertiesViewModel {
                title: "Properties".to_string(),
                properties: vec![tundra_ui::ExplorerPropertyViewModel {
                    label: "Selection".to_string(),
                    value: "No item selected".to_string(),
                }],
                close_label: "Close".to_string(),
            },
        );
    };
    let mut properties = vec![
        tundra_ui::ExplorerPropertyViewModel {
            label: "Name".to_string(),
            value: entry.name.clone(),
        },
        tundra_ui::ExplorerPropertyViewModel {
            label: "Path".to_string(),
            value: entry.path.display().to_string(),
        },
        tundra_ui::ExplorerPropertyViewModel {
            label: "Type".to_string(),
            value: entry.type_label.clone(),
        },
        tundra_ui::ExplorerPropertyViewModel {
            label: "Size".to_string(),
            value: if entry.kind == tundra_apps::explorer::ExplorerEntryKind::Directory {
                "--".to_string()
            } else {
                explorer_size_label(entry.size, state.size_format)
            },
        },
        tundra_ui::ExplorerPropertyViewModel {
            label: "Modified".to_string(),
            value: entry
                .modified
                .map(|modified| {
                    explorer_system_time_label(modified, state.date_zone, configured_timezone)
                })
                .unwrap_or_else(|| "Unknown".to_string()),
        },
        tundra_ui::ExplorerPropertyViewModel {
            label: "Attributes".to_string(),
            value: {
                let labels = explorer_attribute_labels(&entry.attributes);
                if labels.is_empty() {
                    "None".to_string()
                } else {
                    labels.join(", ")
                }
            },
        },
    ];
    if let Some(reason) = entry.open_policy.reason() {
        properties.push(tundra_ui::ExplorerPropertyViewModel {
            label: "Open policy".to_string(),
            value: reason.to_string(),
        });
    }
    tundra_ui::ExplorerOverlayViewModel::Properties(tundra_ui::ExplorerPropertiesViewModel {
        title: format!("Properties: {}", entry.name),
        properties,
        close_label: "Close".to_string(),
    })
}

fn explorer_attribute_labels(attributes: &FileAttributes) -> Vec<String> {
    let mut labels = Vec::new();
    if attributes.readonly {
        labels.push("readonly".to_string());
    }
    if attributes.hidden {
        labels.push("hidden".to_string());
    }
    if attributes.system {
        labels.push("system".to_string());
    }
    if attributes.archive {
        labels.push("archive".to_string());
    }
    if attributes.symlink {
        labels.push("symlink".to_string());
    }
    if attributes.junction {
        labels.push("junction".to_string());
    }
    if attributes.reparse_point {
        labels.push("reparse".to_string());
    }
    if attributes.shortcut {
        labels.push("shortcut".to_string());
    }
    labels
}

fn explorer_input_prompt(mode: ExplorerInputMode) -> &'static str {
    match mode {
        ExplorerInputMode::Browse => "Explorer",
        ExplorerInputMode::Address => "Absolute path",
        ExplorerInputMode::Search => "Search",
        ExplorerInputMode::NewFolder => "New folder name",
        ExplorerInputMode::NewTextFile => "New text file name",
        ExplorerInputMode::Rename => "Rename to",
        ExplorerInputMode::RestoreDestination => "Restore destination directory",
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .ok()
        .and_then(|millis| u64::try_from(millis).ok())
        .unwrap_or(0)
}

fn format_core_error(error: &CoreError) -> String {
    match error {
        CoreError::InvalidCredentials => "Invalid username or password".to_string(),
        CoreError::AccountDisabled => "Account disabled".to_string(),
        CoreError::AccountLocked { .. } => "Account locked".to_string(),
        CoreError::BootstrapAlreadyExists => "Admin already exists".to_string(),
        CoreError::BootstrapRequired => "Create the first admin account".to_string(),
        CoreError::DuplicateUsername => "Username already exists".to_string(),
        CoreError::InvalidUsername => "Invalid username".to_string(),
        CoreError::InvalidUserInfo(reason) => format!("Invalid user info: {reason}"),
        CoreError::InvalidPassword(reason) => format!("Invalid password: {reason}"),
        CoreError::LastPrivilegedUserRequired => {
            "At least one enabled admin is required".to_string()
        }
        CoreError::PermissionDenied { reason, .. } => format!("Permission denied: {reason}"),
        CoreError::UserNotFound => "User not found".to_string(),
        other => other.to_string(),
    }
}

fn login_error_message(error: &CoreError, password_hint: Option<&str>) -> String {
    if matches!(error, CoreError::InvalidCredentials)
        && let Some(hint) = password_hint.map(str::trim).filter(|hint| !hint.is_empty())
    {
        return format!("Password hint: {hint}");
    }

    format_core_error(error)
}

fn to_ui_user_management_field(field: UserManagementFormField) -> tundra_ui::UserManagementField {
    match field {
        UserManagementFormField::Username => tundra_ui::UserManagementField::Username,
        UserManagementFormField::DisplayName => tundra_ui::UserManagementField::DisplayName,
        UserManagementFormField::Role => tundra_ui::UserManagementField::Role,
        UserManagementFormField::Password => tundra_ui::UserManagementField::Password,
        UserManagementFormField::Submit => tundra_ui::UserManagementField::Submit,
        UserManagementFormField::Cancel => tundra_ui::UserManagementField::Cancel,
    }
}

fn from_ui_user_management_field(field: tundra_ui::UserManagementField) -> UserManagementFormField {
    match field {
        tundra_ui::UserManagementField::Username => UserManagementFormField::Username,
        tundra_ui::UserManagementField::DisplayName => UserManagementFormField::DisplayName,
        tundra_ui::UserManagementField::Role => UserManagementFormField::Role,
        tundra_ui::UserManagementField::Password => UserManagementFormField::Password,
        tundra_ui::UserManagementField::Submit => UserManagementFormField::Submit,
        tundra_ui::UserManagementField::Cancel => UserManagementFormField::Cancel,
    }
}

fn user_management_action_model(
    action: tundra_ui::UserManagementAction,
    label: &str,
    shortcut: Option<char>,
    enabled: bool,
    disabled_reason: Option<String>,
    dangerous: bool,
) -> tundra_ui::UserManagementActionViewModel {
    tundra_ui::UserManagementActionViewModel {
        action,
        label: label.to_string(),
        shortcut,
        enabled,
        disabled_reason: (!enabled).then_some(disabled_reason).flatten(),
        dangerous,
    }
}

fn user_is_locked(user: &UserAccount) -> bool {
    user.locked_until_epoch_ms
        .is_some_and(|locked_until| locked_until > unix_millis())
}

fn user_home_entries() -> Vec<tundra_ui::ShellEntry> {
    vec![
        tundra_ui::ShellEntry::new("Explorer", "Browse files"),
        tundra_ui::ShellEntry::new("Launcher", "Open apps and commands"),
        tundra_ui::ShellEntry::new("Editor", "Edit text files"),
        tundra_ui::ShellEntry::new("Settings", "Adjust TundraUX"),
        tundra_ui::ShellEntry::new("Diagnostics", "Inspect system readiness"),
    ]
}

fn terminal_flag_labels(flags: ShellTerminalFlags) -> Vec<String> {
    let mut labels = Vec::new();

    if flags.raw_mode {
        labels.push("raw mode: enabled".to_string());
    }
    if flags.alternate_screen {
        labels.push("alternate screen: enabled".to_string());
    }
    if flags.mouse_capture {
        labels.push("mouse capture: enabled".to_string());
    }
    if flags.cursor_restore_enabled {
        labels.push("cursor restore: enabled".to_string());
    }

    labels
}

fn resolved_home_mode(
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
            .unwrap_or_else(|| ShellState::legacy_default_home_mode(launch_config)),
    };

    if requested_mode == ShellHomeMode::Debug && !startup.debug_policy.allows_debug() {
        ShellHomeMode::User
    } else {
        requested_mode
    }
}

fn should_show_startup_lockscreen(startup: &ShellStartupState) -> bool {
    startup.storage_manager.is_some()
        && !startup.auth_bootstrap_required
        && !startup.login_users.is_empty()
}

fn startup_lockscreen_launch_options(
    startup: &ShellStartupState,
    terminal_size_requirement: ShellTerminalSizeRequirement,
) -> tundra_weathr::LaunchOptions {
    let mut options = tundra_weathr::LaunchOptions {
        load_config_file: false,
        prefer_config_location: false,
        minimum_terminal_size: Some(terminal_size_requirement.as_terminal_size()),
        ..tundra_weathr::LaunchOptions::default()
    };
    let Some(config) = startup
        .storage_manager
        .as_ref()
        .and_then(|storage| storage.load_config().ok())
    else {
        return options;
    };

    options.timezone_id = Some(config.timezone.clone());
    options.location_query = config.weather_location.clone();

    if let Some(timezone) = tundra_ui::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == config.timezone)
    {
        options.location_override = Some(tundra_weathr::LaunchLocation {
            latitude: timezone.latitude,
            longitude: timezone.longitude,
            city: Some(timezone.label),
        });
    }

    options
}

fn platform_capability_summary(kind: PlatformKind, capabilities: &PlatformCapabilities) -> String {
    let (mut supported, mut best_effort, mut unsupported) = (0, 0, 0);

    for (_, status) in capabilities.checks() {
        match status {
            CapabilityStatus::Supported => supported += 1,
            CapabilityStatus::BestEffort => best_effort += 1,
            CapabilityStatus::Unsupported => unsupported += 1,
        }
    }

    format!(
        "{}: {supported} supported, {best_effort} best-effort, {unsupported} unsupported",
        kind.as_str()
    )
}

fn build_mode_label() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}
