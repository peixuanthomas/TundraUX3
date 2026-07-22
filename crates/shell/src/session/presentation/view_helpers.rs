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
            | ShellScreen::Editor
            | ShellScreen::Settings
            | ShellScreen::UserManagement
            | ShellScreen::Diagnostics
            | ShellScreen::Clock
    )
}

pub(in crate::session) fn diagnostics_status_to_ui(
    status: app::diagnostics::DiagnosticStatus,
) -> ui::DiagnosticsStatus {
    match status {
        app::diagnostics::DiagnosticStatus::Pass => ui::DiagnosticsStatus::Pass,
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
        watchdog::RecoveryOutcome::Pending => "Pending".to_string(),
        watchdog::RecoveryOutcome::Recovered(_) => "Recovered".to_string(),
        watchdog::RecoveryOutcome::RecoveredWithWarnings(_) => {
            "Recovered with warnings".to_string()
        }
        watchdog::RecoveryOutcome::ManualActionRequired(_) => "Manual action required".to_string(),
        watchdog::RecoveryOutcome::Unrecoverable(_) => "Unrecoverable".to_string(),
    }
}

pub(in crate::session) fn diagnostics_public_check_summary(
    check: &app::diagnostics::DiagnosticCheck,
) -> String {
    use app::diagnostics::{DiagnosticCategory, DiagnosticStatus};

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

pub(in crate::session) fn explorer_context_menu_view_model(
    anchor: CellPosition,
    selected_count: usize,
    clipboard_available: bool,
    is_trash: bool,
    trash_has_items: bool,
    focused_index: usize,
    can_manage_launcher: bool,
    launcher_eligible_count: usize,
) -> ui::ExplorerOverlayViewModel {
    let item = |id: &str, label: &str, enabled: bool, dangerous: bool| {
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
        let mut items = vec![item("open", "Open", selected_count == 1, false)];
        if can_manage_launcher && launcher_eligible_count > 0 {
            let mut add_to_launcher = item("add-to-launcher", "Add to Launcher", true, false);
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
    ui::ExplorerOverlayViewModel::ContextMenu(ui::ExplorerContextMenuViewModel {
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

pub(in crate::session) fn explorer_sort_menu_view_model(
    anchor: CellPosition,
    selected: ui::ExplorerSortColumn,
    focused_index: usize,
) -> ui::ExplorerOverlayViewModel {
    let items = ui::ExplorerSortColumn::ALL
        .into_iter()
        .map(|column| ui::ExplorerContextMenuItemViewModel {
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
    ui::ExplorerOverlayViewModel::ContextMenu(ui::ExplorerContextMenuViewModel {
        x: anchor.0,
        y: anchor.1,
        title: "Sort by".to_string(),
        items,
        selected_index: Some(focused_index.min(ui::ExplorerSortColumn::ALL.len() - 1)),
    })
}

pub(in crate::session) fn explorer_options_view_model(
    state: &ExplorerState,
    focused_index: usize,
    enabled: bool,
) -> ui::ExplorerOverlayViewModel {
    let toggle = |id: &str, label: &str, value: bool| ui::ExplorerOptionViewModel {
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
        ui::ExplorerOptionViewModel {
            id: "size-format".to_string(),
            label: "Size format".to_string(),
            value: match state.size_format {
                storage::ExplorerSizeFormat::HumanBinary => "Human binary",
                storage::ExplorerSizeFormat::Bytes => "Bytes",
            }
            .to_string(),
            enabled,
            selected: false,
            focused: false,
        },
        ui::ExplorerOptionViewModel {
            id: "date-zone".to_string(),
            label: "Date zone".to_string(),
            value: match state.date_zone {
                storage::ExplorerDateZone::ConfiguredTimezone => "Configured",
                storage::ExplorerDateZone::Utc => "UTC",
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
    ui::ExplorerOverlayViewModel::Options(ui::ExplorerOptionsViewModel {
        title: "Advanced options".to_string(),
        options,
        close_label: "Close".to_string(),
    })
}

pub(in crate::session) fn explorer_properties_view_model(
    state: &ExplorerState,
    configured_timezone: Option<&str>,
) -> ui::ExplorerOverlayViewModel {
    let Some(entry) = state.selected_entry() else {
        return ui::ExplorerOverlayViewModel::Properties(ui::ExplorerPropertiesViewModel {
            title: "Properties".to_string(),
            properties: vec![ui::ExplorerPropertyViewModel {
                label: "Selection".to_string(),
                value: "No item selected".to_string(),
            }],
            close_label: "Close".to_string(),
        });
    };
    let mut properties = vec![
        ui::ExplorerPropertyViewModel {
            label: "Name".to_string(),
            value: entry.name.clone(),
        },
        ui::ExplorerPropertyViewModel {
            label: "Path".to_string(),
            value: entry.path.display().to_string(),
        },
        ui::ExplorerPropertyViewModel {
            label: "Type".to_string(),
            value: entry.type_label.clone(),
        },
        ui::ExplorerPropertyViewModel {
            label: "Size".to_string(),
            value: if entry.kind == app::explorer::ExplorerEntryKind::Directory {
                "--".to_string()
            } else {
                explorer_size_label(entry.size, state.size_format)
            },
        },
        ui::ExplorerPropertyViewModel {
            label: "Modified".to_string(),
            value: entry
                .modified
                .map(|modified| {
                    explorer_system_time_label(modified, state.date_zone, configured_timezone)
                })
                .unwrap_or_else(|| "Unknown".to_string()),
        },
        ui::ExplorerPropertyViewModel {
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
        properties.push(ui::ExplorerPropertyViewModel {
            label: "Open policy".to_string(),
            value: reason.to_string(),
        });
    }
    ui::ExplorerOverlayViewModel::Properties(ui::ExplorerPropertiesViewModel {
        title: format!("Properties: {}", entry.name),
        properties,
        close_label: "Close".to_string(),
    })
}

pub(in crate::session) fn explorer_attribute_labels(attributes: &FileAttributes) -> Vec<String> {
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

pub(in crate::session) fn explorer_input_prompt(mode: ExplorerInputMode) -> &'static str {
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

pub(in crate::session) fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .ok()
        .and_then(|millis| u64::try_from(millis).ok())
        .unwrap_or(0)
}

pub(in crate::session) fn format_core_error(error: &CoreError) -> String {
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

pub(in crate::session) fn login_error_message(
    error: &CoreError,
    password_hint: Option<&str>,
) -> String {
    if matches!(error, CoreError::InvalidCredentials)
        && let Some(hint) = password_hint.map(str::trim).filter(|hint| !hint.is_empty())
    {
        return format!("Password hint: {hint}");
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
    label: &str,
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
        ui::ShellEntry::new("Explorer", "Browse files"),
        ui::ShellEntry::new("Launcher", "Open apps and commands"),
        ui::ShellEntry::new("Editor", "Edit text files"),
        ui::ShellEntry::new("Settings", "Adjust TundraUX"),
        ui::ShellEntry::new("Diagnostics", "Inspect system readiness"),
    ]
}

pub(in crate::session) fn terminal_flag_labels(flags: ShellTerminalFlags) -> Vec<String> {
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
    startup.storage_manager.is_some()
        && !startup.auth_bootstrap_required
        && !startup.login_users.is_empty()
}

pub(in crate::session) fn startup_lockscreen_launch_options(
    startup: &ShellStartupState,
    terminal_size_requirement: ShellTerminalSizeRequirement,
) -> weathr::LaunchOptions {
    let mut options = weathr::LaunchOptions {
        load_config_file: false,
        prefer_config_location: false,
        minimum_terminal_size: Some(terminal_size_requirement.as_terminal_size()),
        ..weathr::LaunchOptions::default()
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

    if let Some(timezone) = app::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == config.timezone)
    {
        options.location_override = Some(weathr::LaunchLocation {
            latitude: timezone.latitude,
            longitude: timezone.longitude,
            city: Some(timezone.label),
        });
    }

    options
}

pub(in crate::session) fn platform_capability_summary(
    kind: PlatformKind,
    capabilities: &PlatformCapabilities,
) -> String {
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

pub(in crate::session) fn build_mode_label() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}
