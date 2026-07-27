use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::{env, fs};

use platform::{
    AppPaths, CapabilityStatus, CheckStatus, EnvironmentCheck, PathCheck, Platform, PlatformKind,
};
use storage::StorageManager;

use crate::path_report::{write_path_templates, write_resolved_paths};

pub(crate) fn run_doctor<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    asset_root: Option<&Path>,
) -> i32 {
    run_doctor_with_terminal_graphics_probe(
        platform,
        stdout,
        stderr,
        asset_root,
        &SystemTerminalGraphicsProbe,
    )
}

fn run_doctor_with_terminal_graphics_probe<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    asset_root: Option<&Path>,
    graphics_probe: &dyn TerminalGraphicsProbe,
) -> i32 {
    let terminal_check = terminal_environment_check_from_probe(platform.kind(), graphics_probe);
    let _ = writeln!(stdout, "TundraUX3 doctor");
    let _ = writeln!(stdout, "Platform kind: {}", platform.kind().as_str());
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "Path templates:");
    write_path_templates(stdout);

    match platform::run_doctor_with(platform) {
        Ok(report) => {
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Resolved paths:");
            write_resolved_paths(stdout, &report.app_paths);
            let mut environment_checks = report.environment_checks.clone();
            replace_terminal_environment_check(&mut environment_checks, terminal_check.clone());
            environment_checks.extend(linux_environment_checks(
                platform.kind(),
                &SystemDoctorProbe,
            ));
            write_doctor_checks(stdout, &environment_checks, &report.path_checks);

            let storage_check = run_storage_check(&report.app_paths);
            write_storage_check(stdout, &storage_check);
            let asset_theme_id = asset_theme_id_from_storage(storage_check.theme_id.as_deref());
            let asset_check = run_asset_check(asset_root, &asset_theme_id);
            write_asset_check(stdout, &asset_check);

            if report.has_failures()
                || environment_checks_have_failures(&environment_checks)
                || storage_check.status == CheckStatus::Fail
            {
                let _ = writeln!(stderr, "Doctor result: FAIL");
                1
            } else {
                let _ = writeln!(stdout, "Doctor result: PASS");
                0
            }
        }
        Err(error) => {
            write_fallback_doctor_checks(stdout, platform, &terminal_check, &error);
            let asset_check = run_asset_check(asset_root, ascii_assets::DEFAULT_THEME_ID);
            write_asset_check(stdout, &asset_check);
            let _ = writeln!(stderr, "Doctor result: FAIL");
            1
        }
    }
}

trait TerminalGraphicsProbe {
    fn detect(&self) -> Result<Option<String>, String>;
}

struct SystemTerminalGraphicsProbe;

impl TerminalGraphicsProbe for SystemTerminalGraphicsProbe {
    fn detect(&self) -> Result<Option<String>, String> {
        shell::detect_terminal_graphics_protocol().map(|protocol| protocol.map(ToOwned::to_owned))
    }
}

fn terminal_environment_check_from_probe(
    kind: PlatformKind,
    probe: &dyn TerminalGraphicsProbe,
) -> EnvironmentCheck {
    let wt_session = env::var("WT_SESSION").ok();
    match probe.detect() {
        Ok(protocol) => platform::terminal_environment_check_with_graphics_protocol(
            kind,
            wt_session.as_deref(),
            protocol.as_deref(),
        ),
        Err(error) => {
            let mut check = platform::terminal_environment_check_with_graphics_protocol(
                kind,
                wt_session.as_deref(),
                None,
            );
            check.message = format!(
                "Terminal graphics capability probe failed: {error}; {}",
                check.message
            );
            check
        }
    }
}

fn replace_terminal_environment_check(
    checks: &mut Vec<EnvironmentCheck>,
    terminal_check: EnvironmentCheck,
) {
    if let Some(check) = checks.iter_mut().find(|check| is_terminal_check(check)) {
        *check = terminal_check;
    } else {
        checks.push(terminal_check);
    }
}

fn environment_checks_have_failures(checks: &[EnvironmentCheck]) -> bool {
    checks.iter().any(|check| check.status == CheckStatus::Fail)
}

/// Read-only view of the parts of a Linux desktop session that matter to the
/// desktop integrations.  Keeping this behind a small interface makes the
/// doctor output deterministic in tests and, more importantly, avoids
/// starting a D-Bus service or opening a graphical session merely to diagnose
/// it.
trait LinuxDoctorProbe {
    fn env_var(&self, name: &str) -> Option<String>;
    fn command_exists(&self, command: &str) -> bool;
    fn path_exists(&self, path: &str) -> bool;
    fn logind_poweroff_state(&self) -> Option<Result<String, String>> {
        None
    }
    fn session_bus_reachable(&self) -> Option<bool> {
        None
    }
    fn session_service_available(&self, _name: &str) -> Option<bool> {
        None
    }
    fn clipboard_backend_available(&self) -> Option<bool> {
        None
    }
}

struct SystemDoctorProbe;

impl LinuxDoctorProbe for SystemDoctorProbe {
    fn env_var(&self, name: &str) -> Option<String> {
        env::var(name).ok().filter(|value| !value.trim().is_empty())
    }

    fn command_exists(&self, command: &str) -> bool {
        let Some(path) = self.env_var("PATH") else {
            return false;
        };

        env::split_paths(&path).any(|directory| {
            let candidate = directory.join(command);
            fs::metadata(candidate)
                .map(|metadata| {
                    metadata.is_file() && {
                        #[cfg(unix)]
                        {
                            metadata.permissions().mode() & 0o111 != 0
                        }
                        #[cfg(not(unix))]
                        {
                            true
                        }
                    }
                })
                .unwrap_or(false)
        })
    }

    fn path_exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn logind_poweroff_state(&self) -> Option<Result<String, String>> {
        #[cfg(target_os = "linux")]
        {
            Some(query_logind_poweroff_state())
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    fn session_bus_reachable(&self) -> Option<bool> {
        #[cfg(target_os = "linux")]
        {
            Some(zbus::blocking::Connection::session().is_ok())
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    fn session_service_available(&self, name: &str) -> Option<bool> {
        #[cfg(target_os = "linux")]
        {
            Some(session_dbus_name_available(name).unwrap_or(false))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            None
        }
    }

    fn clipboard_backend_available(&self) -> Option<bool> {
        #[cfg(target_os = "linux")]
        {
            Some(arboard::Clipboard::new().is_ok())
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn query_logind_poweroff_state() -> Result<String, String> {
    let connection = zbus::blocking::Connection::system().map_err(|error| error.to_string())?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(|error| error.to_string())?;
    proxy
        .call("CanPowerOff", &())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn session_dbus_name_available(name: &str) -> Result<bool, String> {
    let connection = zbus::blocking::Connection::session().map_err(|error| error.to_string())?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .map_err(|error| error.to_string())?;
    let has_owner: bool = proxy
        .call("NameHasOwner", &(name,))
        .map_err(|error| error.to_string())?;
    if has_owner {
        return Ok(true);
    }
    let activatable: Vec<String> = proxy
        .call("ListActivatableNames", &())
        .map_err(|error| error.to_string())?;
    Ok(activatable.iter().any(|candidate| candidate == name))
}

fn linux_environment_checks(
    kind: PlatformKind,
    probe: &dyn LinuxDoctorProbe,
) -> Vec<EnvironmentCheck> {
    if kind != PlatformKind::Linux {
        return Vec::new();
    }

    vec![
        linux_architecture_check(),
        command_check(
            probe,
            "xdg-open",
            "xdg-open",
            CheckStatus::Fail,
            "install xdg-utils (for example: sudo apt install xdg-utils)",
        ),
        command_check(
            probe,
            "gio",
            "gio",
            CheckStatus::Fail,
            "install GLib command-line tools (for example: sudo apt install libglib2.0-bin)",
        ),
        logind_check(probe),
        session_dbus_check(probe),
        portal_check(probe),
        clipboard_check(probe),
        notification_check(probe),
        polkit_check(probe),
    ]
}

fn portal_check(probe: &dyn LinuxDoctorProbe) -> EnvironmentCheck {
    if let Some(available) = probe.session_service_available("org.freedesktop.portal.Desktop") {
        return if available {
            EnvironmentCheck {
                label: "Desktop portal".to_string(),
                status: CheckStatus::Pass,
                message: "the xdg-desktop-portal service is running or D-Bus activatable"
                    .to_string(),
            }
        } else {
            EnvironmentCheck {
                label: "Desktop portal".to_string(),
                status: CheckStatus::Warning,
                message: "org.freedesktop.portal.Desktop is neither running nor D-Bus activatable; install the portal and the GNOME/KDE backend".to_string(),
            }
        };
    }
    let installed = probe.command_exists("xdg-desktop-portal")
        || probe.path_exists("/usr/libexec/xdg-desktop-portal")
        || probe.path_exists("/usr/lib/xdg-desktop-portal")
        || probe.path_exists("/usr/share/xdg-desktop-portal/portals");
    let session_bus = probe.env_var("DBUS_SESSION_BUS_ADDRESS").is_some();
    if installed && session_bus {
        EnvironmentCheck {
            label: "Desktop portal".to_string(),
            status: CheckStatus::Pass,
            message:
                "xdg-desktop-portal is installed and can be activated through the session D-Bus"
                    .to_string(),
        }
    } else {
        let reason = match (installed, session_bus) {
            (false, _) => "xdg-desktop-portal was not detected",
            (true, false) => "xdg-desktop-portal is installed but session D-Bus is unavailable",
            (true, true) => unreachable!(),
        };
        EnvironmentCheck {
            label: "Desktop portal".to_string(),
            status: CheckStatus::Warning,
            message: format!(
                "{reason}; install/enable xdg-desktop-portal in the GNOME/KDE user session"
            ),
        }
    }
}

fn linux_architecture_check() -> EnvironmentCheck {
    if env::consts::ARCH == "x86_64" {
        EnvironmentCheck {
            label: "Linux architecture".to_string(),
            status: CheckStatus::Pass,
            message: "x86_64 is supported".to_string(),
        }
    } else {
        EnvironmentCheck {
            label: "Linux architecture".to_string(),
            status: CheckStatus::Fail,
            message: format!(
                "{} is not an M0 release architecture; use an x86_64 build",
                env::consts::ARCH
            ),
        }
    }
}

fn command_check(
    probe: &dyn LinuxDoctorProbe,
    label: &str,
    command: &str,
    missing_status: CheckStatus,
    remediation: &str,
) -> EnvironmentCheck {
    if probe.command_exists(command) {
        EnvironmentCheck {
            label: format!("Linux command: {label}"),
            status: CheckStatus::Pass,
            message: format!("{command} is available"),
        }
    } else {
        EnvironmentCheck {
            label: format!("Linux command: {label}"),
            status: missing_status,
            message: format!("{command} was not found in PATH; {remediation}"),
        }
    }
}

fn logind_check(probe: &dyn LinuxDoctorProbe) -> EnvironmentCheck {
    if let Some(result) = probe.logind_poweroff_state() {
        return match result {
            Ok(state) if matches!(state.as_str(), "yes" | "challenge") => EnvironmentCheck {
                label: "systemd-logind".to_string(),
                status: CheckStatus::Pass,
                message: format!(
                    "live CanPowerOff returned {state}; interactive Power off is available through logind"
                ),
            },
            Ok(state) => EnvironmentCheck {
                label: "systemd-logind".to_string(),
                status: CheckStatus::Warning,
                message: format!(
                    "live CanPowerOff returned {state}; check the active logind session and polkit policy"
                ),
            },
            Err(error) => EnvironmentCheck {
                label: "systemd-logind".to_string(),
                status: CheckStatus::Warning,
                message: format!("could not query logind CanPowerOff on the system D-Bus: {error}"),
            },
        };
    }
    let systemd_running = probe.path_exists("/run/systemd/system");
    let system_bus = probe.path_exists("/run/dbus/system_bus_socket");
    if systemd_running && system_bus {
        EnvironmentCheck {
            label: "systemd-logind".to_string(),
            status: CheckStatus::Warning,
            message: "systemd and the system D-Bus socket exist, but this build could not issue a live CanPowerOff probe".to_string(),
        }
    } else {
        EnvironmentCheck {
            label: "systemd-logind".to_string(),
            status: CheckStatus::Warning,
            message: "systemd-logind or its system D-Bus socket was not detected; run inside a systemd user session to enable Power off".to_string(),
        }
    }
}

fn session_dbus_check(probe: &dyn LinuxDoctorProbe) -> EnvironmentCheck {
    let reachable = probe
        .session_bus_reachable()
        .unwrap_or_else(|| probe.env_var("DBUS_SESSION_BUS_ADDRESS").is_some());
    if reachable {
        EnvironmentCheck {
            label: "Session D-Bus".to_string(),
            status: CheckStatus::Pass,
            message: "a live session D-Bus connection is available for desktop services"
                .to_string(),
        }
    } else {
        EnvironmentCheck {
            label: "Session D-Bus".to_string(),
            status: CheckStatus::Warning,
            message: "DBUS_SESSION_BUS_ADDRESS is unset; start TundraUX3 from your GNOME/KDE login session (or configure a session D-Bus) for notifications and portal integration".to_string(),
        }
    }
}

fn clipboard_check(probe: &dyn LinuxDoctorProbe) -> EnvironmentCheck {
    let x11_available = probe.env_var("DISPLAY").is_some();
    let wayland_available = probe.env_var("WAYLAND_DISPLAY").is_some()
        || probe
            .env_var("XDG_SESSION_TYPE")
            .is_some_and(|value| value.eq_ignore_ascii_case("wayland"));

    if let Some(available) = probe.clipboard_backend_available() {
        return if available {
            EnvironmentCheck {
                label: "Linux clipboard".to_string(),
                status: CheckStatus::Pass,
                message: match (wayland_available, x11_available) {
                    (true, true) => {
                        "connected to a clipboard backend; Wayland and X11/XWayland endpoints are present"
                    }
                    (true, false) => {
                        "connected to the compositor clipboard through the Wayland data-control backend"
                    }
                    (false, true) => "connected to the X11 clipboard backend",
                    (false, false) => "connected to a Linux clipboard backend",
                }
                .to_string(),
            }
        } else {
            EnvironmentCheck {
                label: "Linux clipboard".to_string(),
                status: CheckStatus::Warning,
                message: "the clipboard backend could not establish a live Wayland or X11 connection; enable compositor data-control or XWayland (Bracketed Paste remains available)".to_string(),
            }
        };
    }

    match (wayland_available, x11_available) {
        (_, true) => EnvironmentCheck {
            label: "Linux clipboard".to_string(),
            status: CheckStatus::Pass,
            message: "X11/XWayland clipboard fallback is available".to_string(),
        },
        (true, false) => EnvironmentCheck {
            label: "Linux clipboard".to_string(),
            status: CheckStatus::Warning,
            message: "native Wayland session detected without XWayland; clipboard requires compositor data-control support (enable XWayland or use a compositor with ext-data-control/wlr-data-control)".to_string(),
        },
        (false, false) => EnvironmentCheck {
            label: "Linux clipboard".to_string(),
            status: CheckStatus::Warning,
            message: "no Wayland or X11 display was detected; start from a graphical session to enable clipboard integration (Bracketed Paste remains available in the editor)".to_string(),
        },
    }
}

fn notification_check(probe: &dyn LinuxDoctorProbe) -> EnvironmentCheck {
    let available = probe
        .session_service_available("org.freedesktop.Notifications")
        .unwrap_or_else(|| probe.env_var("DBUS_SESSION_BUS_ADDRESS").is_some());
    if available {
        EnvironmentCheck {
            label: "Desktop notifications".to_string(),
            status: CheckStatus::Pass,
            message: "org.freedesktop.Notifications is running or D-Bus activatable; stderr and watchdog reports remain durable fallbacks".to_string(),
        }
    } else {
        EnvironmentCheck {
            label: "Desktop notifications".to_string(),
            status: CheckStatus::Warning,
            message: "no session D-Bus was detected; install/enable xdg-desktop-portal or a notification daemon in the graphical session; stderr and watchdog reports will be used".to_string(),
        }
    }
}

fn polkit_check(probe: &dyn LinuxDoctorProbe) -> EnvironmentCheck {
    if !probe.command_exists("pkcheck") {
        return EnvironmentCheck {
            label: "polkit".to_string(),
            status: CheckStatus::Warning,
            message: "pkcheck was not found; install and enable polkit to authorize interactive Power off requests".to_string(),
        };
    }
    match probe.logind_poweroff_state() {
        Some(Ok(state)) if matches!(state.as_str(), "yes" | "challenge") => EnvironmentCheck {
            label: "polkit".to_string(),
            status: CheckStatus::Pass,
            message: format!(
                "pkcheck is executable and logind CanPowerOff returned {state}; no sudo path is used"
            ),
        },
        Some(Ok(state)) => EnvironmentCheck {
            label: "polkit".to_string(),
            status: CheckStatus::Warning,
            message: format!(
                "pkcheck is executable, but logind CanPowerOff returned {state}; check the desktop polkit agent and policy"
            ),
        },
        Some(Err(error)) => EnvironmentCheck {
            label: "polkit".to_string(),
            status: CheckStatus::Warning,
            message: format!(
                "pkcheck is executable, but logind authorization could not be queried: {error}"
            ),
        },
        None => EnvironmentCheck {
            label: "polkit".to_string(),
            status: CheckStatus::Warning,
            message: "pkcheck is executable, but this build could not verify the live desktop authorization agent".to_string(),
        },
    }
}

fn write_doctor_checks(
    output: &mut impl Write,
    environment_checks: &[EnvironmentCheck],
    path_checks: &[PathCheck],
) {
    let _ = writeln!(output);
    let _ = writeln!(output, "Checks:");

    let _ = writeln!(output);
    let _ = writeln!(output, "Platform checks:");
    for check in environment_checks
        .iter()
        .filter(|check| is_platform_check(check))
    {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Terminal check:");
    for check in environment_checks
        .iter()
        .filter(|check| is_terminal_check(check))
    {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Capability checks:");
    for check in environment_checks
        .iter()
        .filter(|check| is_capability_check(check))
    {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Path checks:");
    for check in path_checks {
        write_path_check(output, check);
    }
}

fn write_storage_check(output: &mut impl Write, check: &StorageCheck) {
    let _ = writeln!(output);
    let _ = writeln!(output, "Storage checks:");
    let _ = writeln!(
        output,
        "[{}] {}: {}",
        check.status.as_str(),
        check.label,
        check.message
    );
}

fn write_asset_check(output: &mut impl Write, check: &AsciiAssetCheck) {
    let _ = writeln!(output);
    let _ = writeln!(output, "Asset checks:");
    let _ = writeln!(
        output,
        "[{}] Required ASCII assets (theme {}): {}",
        check.status.as_str(),
        check.theme_id,
        check.message
    );
    for detail in &check.details {
        let _ = writeln!(output, "  {detail}");
    }
}

fn write_environment_check(output: &mut impl Write, check: &EnvironmentCheck) {
    let _ = writeln!(
        output,
        "[{}] {}: {}",
        check.status.as_str(),
        check.label,
        check.message
    );
}

fn write_path_check(output: &mut impl Write, check: &PathCheck) {
    let _ = writeln!(
        output,
        "[{}] {}: {} - {}",
        check.status.as_str(),
        check.label,
        check.path.display(),
        check.message
    );
}

fn write_fallback_doctor_checks(
    output: &mut impl Write,
    platform: &dyn Platform,
    terminal_check: &EnvironmentCheck,
    error: &platform::PlatformError,
) {
    let capability_checks = fallback_capability_checks(platform);

    let _ = writeln!(output);
    let _ = writeln!(output, "Checks:");

    let _ = writeln!(output);
    let _ = writeln!(output, "Terminal check:");
    write_environment_check(output, terminal_check);

    let _ = writeln!(output);
    let _ = writeln!(output, "Capability checks:");
    for check in &capability_checks {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Path checks:");
    let _ = writeln!(output, "[FAIL] App paths: {error}");
}

fn fallback_capability_checks(platform: &dyn Platform) -> Vec<EnvironmentCheck> {
    platform
        .capabilities()
        .checks()
        .into_iter()
        .map(|(name, status)| EnvironmentCheck {
            label: format!("Capability: {name}"),
            status: check_status_for_capability(status),
            message: status.as_str().to_string(),
        })
        .collect()
}

fn check_status_for_capability(status: CapabilityStatus) -> CheckStatus {
    match status {
        CapabilityStatus::Supported => CheckStatus::Pass,
        CapabilityStatus::BestEffort => CheckStatus::Warning,
        CapabilityStatus::Unsupported => CheckStatus::Warning,
    }
}

fn is_platform_check(check: &EnvironmentCheck) -> bool {
    !is_terminal_check(check) && !is_capability_check(check)
}

fn is_terminal_check(check: &EnvironmentCheck) -> bool {
    check.label == "Terminal"
}

fn is_capability_check(check: &EnvironmentCheck) -> bool {
    check.label.starts_with("Capability: ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageCheck {
    label: &'static str,
    status: CheckStatus,
    message: String,
    theme_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AsciiAssetCheck {
    status: CheckStatus,
    theme_id: String,
    message: String,
    details: Vec<String>,
}

fn run_storage_check(paths: &AppPaths) -> StorageCheck {
    match StorageManager::open(paths.clone()) {
        Ok(opened) => {
            let theme_id = opened.manager.load_config().ok().map(|config| config.theme);
            if opened.report.warnings.is_empty() && opened.report.migrated_files.is_empty() {
                StorageCheck {
                    label: "Storage bootstrap",
                    status: CheckStatus::Pass,
                    message: "storage initialized and loaded cleanly".to_string(),
                    theme_id,
                }
            } else {
                StorageCheck {
                    label: "Storage bootstrap",
                    status: CheckStatus::Warning,
                    message: storage_warning_message(&opened.report),
                    theme_id,
                }
            }
        }
        Err(error) => StorageCheck {
            label: "Storage bootstrap",
            status: CheckStatus::Fail,
            message: error.to_string(),
            theme_id: None,
        },
    }
}

fn run_asset_check(asset_root: Option<&Path>, theme_id: &str) -> AsciiAssetCheck {
    let theme_id = normalized_asset_theme_id(theme_id);
    let root = match asset_root {
        Some(root) => Ok(root.to_path_buf()),
        None => ascii_assets::asset_root_from_env_or_current_exe(),
    };

    let root = match root {
        Ok(root) => root,
        Err(error) => {
            return AsciiAssetCheck {
                status: CheckStatus::Warning,
                theme_id,
                message: format!("could not resolve asset root: {error}"),
                details: Vec::new(),
            };
        }
    };

    let report = ascii_assets::check_required_assets(&root, &theme_id);
    if report.is_ok() {
        return AsciiAssetCheck {
            status: CheckStatus::Pass,
            theme_id,
            message: format!(
                "{} assets present and valid at {}",
                report.checks.len(),
                root.display()
            ),
            details: Vec::new(),
        };
    }

    let missing = report.missing_assets();
    let unreadable = report.unreadable_assets();
    let invalid = report.invalid_assets();
    let mut details = Vec::new();
    for check in &missing {
        details.push(format!("missing: {} ({})", check.key, check.path.display()));
    }
    for check in &unreadable {
        details.push(format!(
            "unreadable: {} ({})",
            check.key,
            check.path.display()
        ));
    }
    for check in &invalid {
        details.push(format!(
            "invalid: {} ({}) - {}",
            check.key,
            check.path.display(),
            check.message
        ));
    }

    AsciiAssetCheck {
        status: CheckStatus::Warning,
        theme_id,
        message: format!(
            "{}; {}; {} at {}",
            asset_count_message(missing.len(), "missing"),
            asset_count_message(unreadable.len(), "unreadable"),
            asset_count_message(invalid.len(), "invalid"),
            root.display()
        ),
        details,
    }
}

fn asset_theme_id_from_storage(theme_id: Option<&str>) -> String {
    normalized_asset_theme_id(theme_id.unwrap_or(ascii_assets::DEFAULT_THEME_ID))
}

fn normalized_asset_theme_id(theme_id: &str) -> String {
    match theme_id.trim() {
        "" | "dark" | "light" => ascii_assets::DEFAULT_THEME_ID.to_string(),
        other => other.to_string(),
    }
}

fn asset_count_message(count: usize, label: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{count} {label} asset{suffix}")
}

fn storage_warning_message(report: &storage::StorageLoadReport) -> String {
    let mut warnings = report.warnings.clone();
    if !report.migrated_files.is_empty() {
        warnings.push(format!(
            "migrated {} storage files",
            report.migrated_files.len()
        ));
    }

    if warnings.is_empty() {
        "storage initialized with warnings".to_string()
    } else {
        format!("storage initialized with warnings: {}", warnings.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    #[derive(Default)]
    struct TestProbe {
        environment: HashMap<&'static str, &'static str>,
        commands: HashSet<&'static str>,
        paths: HashSet<&'static str>,
        logind_state: Option<&'static str>,
        session_bus_reachable: Option<bool>,
        session_services: HashSet<&'static str>,
        clipboard_backend_available: Option<bool>,
    }

    impl LinuxDoctorProbe for TestProbe {
        fn env_var(&self, name: &str) -> Option<String> {
            self.environment.get(name).map(|value| (*value).to_string())
        }

        fn command_exists(&self, command: &str) -> bool {
            self.commands.contains(command)
        }

        fn path_exists(&self, path: &str) -> bool {
            self.paths.contains(path)
        }

        fn logind_poweroff_state(&self) -> Option<Result<String, String>> {
            self.logind_state.map(|state| Ok(state.to_string()))
        }

        fn session_bus_reachable(&self) -> Option<bool> {
            self.session_bus_reachable
        }

        fn session_service_available(&self, name: &str) -> Option<bool> {
            self.session_bus_reachable
                .map(|_| self.session_services.contains(name))
        }

        fn clipboard_backend_available(&self) -> Option<bool> {
            self.clipboard_backend_available
        }
    }

    struct FixedTerminalGraphicsProbe {
        result: Result<Option<String>, String>,
    }

    impl TerminalGraphicsProbe for FixedTerminalGraphicsProbe {
        fn detect(&self) -> Result<Option<String>, String> {
            self.result.clone()
        }
    }

    fn check<'a>(checks: &'a [EnvironmentCheck], label: &str) -> &'a EnvironmentCheck {
        checks
            .iter()
            .find(|check| check.label == label)
            .unwrap_or_else(|| panic!("missing check {label}"))
    }

    #[test]
    fn linux_doctor_reports_ready_desktop_dependencies_from_injected_probe() {
        let probe = TestProbe {
            environment: HashMap::from([
                ("PATH", "/test/bin"),
                ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
                ("DISPLAY", ":0"),
                ("TERM", "xterm-kitty"),
            ]),
            commands: HashSet::from(["xdg-open", "gio", "pkcheck"]),
            paths: HashSet::from([
                "/run/systemd/system",
                "/run/dbus/system_bus_socket",
                "/usr/libexec/xdg-desktop-portal",
            ]),
            logind_state: Some("challenge"),
            session_bus_reachable: Some(true),
            session_services: HashSet::from([
                "org.freedesktop.portal.Desktop",
                "org.freedesktop.Notifications",
            ]),
            clipboard_backend_available: Some(true),
        };

        let checks = linux_environment_checks(PlatformKind::Linux, &probe);

        assert_eq!(
            check(&checks, "Linux command: xdg-open").status,
            CheckStatus::Pass
        );
        assert_eq!(check(&checks, "systemd-logind").status, CheckStatus::Pass);
        assert_eq!(check(&checks, "Desktop portal").status, CheckStatus::Pass);
        assert_eq!(check(&checks, "Linux clipboard").status, CheckStatus::Pass);
        assert!(
            checks
                .iter()
                .all(|check| check.label != "Terminal image protocol"),
            "terminal graphics support must come from the live protocol probe"
        );
    }

    #[test]
    fn linux_doctor_explains_wayland_clipboard_and_missing_runtime_services() {
        let probe = TestProbe {
            environment: HashMap::from([("WAYLAND_DISPLAY", "wayland-0")]),
            ..TestProbe::default()
        };

        let checks = linux_environment_checks(PlatformKind::Linux, &probe);

        let clipboard = check(&checks, "Linux clipboard");
        assert_eq!(clipboard.status, CheckStatus::Warning);
        assert!(clipboard.message.contains("data-control"));
        assert!(
            check(&checks, "Linux command: xdg-open")
                .message
                .contains("xdg-utils")
        );
        assert!(
            check(&checks, "Session D-Bus")
                .message
                .contains("DBUS_SESSION_BUS_ADDRESS")
        );
        assert!(
            check(&checks, "Desktop portal")
                .message
                .contains("xdg-desktop-portal")
        );
        assert!(
            checks
                .iter()
                .all(|check| check.label != "Terminal image protocol")
        );
    }

    #[test]
    fn doctor_terminal_check_uses_the_live_graphics_protocol_probe() {
        let probe = FixedTerminalGraphicsProbe {
            result: Ok(Some("Sixel".to_string())),
        };

        let terminal = terminal_environment_check_from_probe(PlatformKind::Macos, &probe);

        assert_eq!(terminal.status, CheckStatus::Pass);
        assert!(terminal.message.contains("Sixel graphics protocol"));

        let mut checks = vec![
            platform::terminal_environment_check_with_graphics_protocol(
                PlatformKind::Macos,
                None,
                None,
            ),
            EnvironmentCheck {
                label: "Platform".to_string(),
                status: CheckStatus::Pass,
                message: "macOS".to_string(),
            },
        ];
        replace_terminal_environment_check(&mut checks, terminal);

        assert_eq!(
            checks
                .iter()
                .filter(|check| check.label == "Terminal")
                .count(),
            1
        );
        assert_eq!(check(&checks, "Terminal").status, CheckStatus::Pass);
    }

    #[test]
    fn doctor_terminal_check_warns_without_a_protocol_or_when_the_probe_fails() {
        let text_only = terminal_environment_check_from_probe(
            PlatformKind::Macos,
            &FixedTerminalGraphicsProbe { result: Ok(None) },
        );
        assert_eq!(text_only.status, CheckStatus::Warning);
        assert!(text_only.message.contains("text-only"));

        let failed = terminal_environment_check_from_probe(
            PlatformKind::Macos,
            &FixedTerminalGraphicsProbe {
                result: Err("query timeout".to_string()),
            },
        );
        assert_eq!(failed.status, CheckStatus::Warning);
        assert!(failed.message.contains("probe failed"));
        assert!(failed.message.contains("query timeout"));
    }

    #[test]
    fn doctor_execution_prints_the_probed_terminal_result() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let probe = FixedTerminalGraphicsProbe {
            result: Ok(Some("Kitty".to_string())),
        };

        let exit_code = run_doctor_with_terminal_graphics_probe(
            &platform::mock::UnsupportedPlatform,
            &mut stdout,
            &mut stderr,
            Some(Path::new(ascii_assets::CANONICAL_ASSETS_DIR)),
            &probe,
        );

        assert_eq!(exit_code, 1, "unsupported app paths still fail doctor");
        let stdout = String::from_utf8(stdout).expect("doctor output should be UTF-8");
        assert!(stdout.contains("[PASS] Terminal: Kitty graphics protocol detected"));
        assert_eq!(stdout.matches("] Terminal:").count(), 1);
        assert!(!stdout.contains("Terminal image protocol"));
        assert!(
            String::from_utf8(stderr)
                .expect("doctor error should be UTF-8")
                .contains("Doctor result: FAIL")
        );
    }

    #[test]
    fn linux_doctor_checks_do_not_change_other_platform_output() {
        let probe = TestProbe::default();
        assert!(linux_environment_checks(PlatformKind::Windows, &probe).is_empty());
        assert!(linux_environment_checks(PlatformKind::Macos, &probe).is_empty());
    }

    #[test]
    fn linux_fail_checks_are_release_blocking() {
        let probe = TestProbe::default();
        let checks = linux_environment_checks(PlatformKind::Linux, &probe);

        assert!(
            environment_checks_have_failures(&checks),
            "missing required Linux commands must make doctor fail"
        );
    }
}
