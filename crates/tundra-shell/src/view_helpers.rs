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
            | ShellScreen::UserManagement
            | ShellScreen::Clock
    )
}

fn startup_clock_timezone_id(startup: &ShellStartupState) -> Option<String> {
    startup
        .storage_manager
        .as_ref()
        .and_then(|storage| storage.load_config().ok())
        .map(|config| config.timezone)
        .or_else(|| Some("UTC".to_string()))
}

fn system_time_label(value: SystemTime) -> String {
    value
        .duration_since(UNIX_EPOCH)
        .map(|duration| format!("unix:{}", duration.as_secs()))
        .unwrap_or_else(|_| "unknown".to_string())
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
        ExplorerInputMode::Search => "Search",
        ExplorerInputMode::NewFolder => "New folder name",
        ExplorerInputMode::NewTextFile => "New text file name",
        ExplorerInputMode::Rename => "Rename to",
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
    match launch_config.home_mode_override {
        HomeModeOverride::Debug => ShellHomeMode::Debug,
        HomeModeOverride::BuildDefault => startup
            .restored_session
            .as_ref()
            .map(|session| session.display_mode)
            .or(startup.app_config.home_mode)
            .unwrap_or_else(|| ShellState::legacy_default_home_mode(launch_config)),
    }
}

fn should_show_startup_lockscreen(startup: &ShellStartupState) -> bool {
    startup.storage_manager.is_some()
        && !startup.auth_bootstrap_required
        && !startup.login_users.is_empty()
}

fn startup_lockscreen_launch_options(startup: &ShellStartupState) -> tundra_weathr::LaunchOptions {
    let Some(config) = startup
        .storage_manager
        .as_ref()
        .and_then(|storage| storage.load_config().ok())
    else {
        return tundra_weathr::LaunchOptions::default();
    };

    let mut options = tundra_weathr::LaunchOptions {
        timezone_id: Some(config.timezone.clone()),
        ..tundra_weathr::LaunchOptions::default()
    };

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
