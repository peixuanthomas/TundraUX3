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
            id: "platform",
            label: "Platform".to_string(),
            status: CheckStatus::Pass,
            message: "macOS".to_string(),
        },
    ];
    replace_terminal_environment_check(&mut checks, terminal);

    assert_eq!(
        checks.iter().filter(|check| check.id == "terminal").count(),
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
