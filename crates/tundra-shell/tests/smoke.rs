use std::io::Write;

use tundra_shell::{
    ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE, HomeModeOverride, ShellArgError,
    ShellLaunchConfig, ShellTerminalMode, banner_lines, parse_shell_args, render_static_banner,
    startup_lines,
};

#[test]
fn startup_lines_state_phase_zero_boundaries() {
    let lines = startup_lines();
    let output = lines.join("\n");

    assert!(lines.iter().any(|line| line.contains("TundraUX3 shell")));
    assert!(lines.iter().any(|line| line.contains("Phase 0")));
    assert!(lines.iter().any(|line| line.contains("Supported")));
    assert!(
        lines
            .iter()
            .any(|line| line.to_ascii_lowercase().contains("terminal"))
    );
    assert!(!output.contains("Windows 11"));
    assert!(!output.contains("Windows Terminal"));
}

#[test]
fn banner_contains_requested_tundraux3_logo() {
    let lines = banner_lines().expect("banner asset should load");

    assert!(!lines.is_empty());
    assert!(lines.iter().all(|line| line.is_ascii()));
    assert!(lines.iter().any(|line| !line.trim().is_empty()));
}

#[test]
fn static_banner_renders_all_logo_lines() {
    let mut output = Vec::new();
    let expected_lines = banner_lines().expect("banner asset should load");

    render_static_banner(&mut output).expect("banner should render");

    let output = String::from_utf8(output).expect("banner should be utf8");
    let actual_lines = output.lines().map(str::to_string).collect::<Vec<String>>();
    assert_eq!(actual_lines, expected_lines);
}

#[test]
fn shell_can_enter_smoke_loop_without_animation() {
    let mut output = Vec::new();
    let first_banner_line = first_non_blank_banner_line();

    render_static_banner(&mut output).expect("static banner should render");
    for line in startup_lines() {
        writeln!(output, "{line}").expect("startup line should render");
    }
    writeln!(output, "Entering smoke loop").expect("smoke marker should render");

    let output = String::from_utf8(output).expect("shell output should be utf8");
    assert!(output.contains(&first_banner_line));
    assert!(output.contains("TundraUX3 shell - Phase 0 smoke"));
    assert!(output.contains("Entering smoke loop"));
}

#[test]
fn shell_default_config_uses_fullscreen_and_build_default_home() {
    assert_eq!(
        parse_shell_args(std::iter::empty::<&str>()).expect("empty args should parse"),
        ShellLaunchConfig {
            terminal_mode: ShellTerminalMode::Fullscreen,
            home_mode_override: HomeModeOverride::BuildDefault,
        }
    );
}

#[test]
fn shell_can_be_started_without_fullscreen_explicitly() {
    assert_eq!(
        parse_shell_args(["-notfullscreen"]).expect("flag should parse"),
        ShellLaunchConfig {
            terminal_mode: ShellTerminalMode::NotFullscreen,
            home_mode_override: HomeModeOverride::BuildDefault,
        }
    );
}

#[test]
fn debug_flag_forces_debug_home() {
    assert_eq!(
        parse_shell_args(["-debug"]).expect("debug flag should parse"),
        ShellLaunchConfig {
            terminal_mode: ShellTerminalMode::Fullscreen,
            home_mode_override: HomeModeOverride::Debug,
        }
    );
}

#[test]
fn notfullscreen_and_debug_can_be_combined() {
    let expected = ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::NotFullscreen,
        home_mode_override: HomeModeOverride::Debug,
    };

    assert_eq!(
        parse_shell_args(["-notfullscreen", "-debug"]).expect("flags should parse"),
        expected
    );
    assert_eq!(
        parse_shell_args(["-debug", "-notfullscreen"]).expect("flags should parse in either order"),
        expected
    );
}

#[test]
fn duplicate_debug_flag_is_an_error() {
    let error = parse_shell_args(["-debug", "-debug"]).expect_err("duplicate flag should fail");

    assert_eq!(
        error,
        ShellArgError::DuplicateArgument("-debug".to_string())
    );
    assert_eq!(error.to_string(), "duplicate argument: -debug");
}

#[test]
fn duplicate_notfullscreen_flag_is_an_error() {
    let error = parse_shell_args(["-notfullscreen", "-notfullscreen"])
        .expect_err("duplicate flag should fail");

    assert_eq!(
        error,
        ShellArgError::DuplicateArgument("-notfullscreen".to_string())
    );
    assert_eq!(error.to_string(), "duplicate argument: -notfullscreen");
}

#[test]
fn unknown_flag_is_an_error() {
    let error = parse_shell_args(["-surprise"]).expect_err("unknown flag should fail");

    assert_eq!(
        error,
        ShellArgError::UnknownArgument("-surprise".to_string())
    );
    assert_eq!(error.to_string(), "unknown argument: -surprise");
}

#[test]
fn fullscreen_mode_enters_and_exits_alternate_screen() {
    let mut output = Vec::new();

    tundra_platform::with_terminal_fullscreen(&mut output, |output| {
        render_static_banner(output)?;
        writeln!(output, "Entering smoke loop")
    })
    .expect("fullscreen render should complete");

    let output = String::from_utf8(output).expect("fullscreen output should be utf8");
    assert!(output.starts_with(ENTER_FULLSCREEN_SEQUENCE));
    assert!(output.contains("Entering smoke loop"));
    assert!(output.ends_with(EXIT_FULLSCREEN_SEQUENCE));
}

#[test]
fn notfullscreen_mode_does_not_write_alternate_screen_sequences() {
    let mut output = Vec::new();

    render_static_banner(&mut output).expect("notfullscreen render should complete");
    writeln!(output, "Entering smoke loop").expect("smoke marker should render");

    let output = String::from_utf8(output).expect("notfullscreen output should be utf8");
    assert!(!output.contains(ENTER_FULLSCREEN_SEQUENCE));
    assert!(!output.contains(EXIT_FULLSCREEN_SEQUENCE));
    assert!(output.contains("Entering smoke loop"));
}

fn first_non_blank_banner_line() -> String {
    banner_lines()
        .expect("banner asset should load")
        .into_iter()
        .find(|line| !line.trim().is_empty())
        .expect("banner asset should contain visible content")
}
