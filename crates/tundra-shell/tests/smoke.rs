use tundra_shell::{
    ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE, HomeModeOverride, ShellArgError,
    ShellLaunchConfig, ShellTerminalMode, banner_lines, parse_shell_args, render_static_banner,
    run_fullscreen_once_without_animation, run_not_fullscreen,
    run_not_fullscreen_without_animation, run_without_animation, startup_lines,
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
    let lines = banner_lines();

    assert_eq!(lines.len(), 10);
    assert!(lines[0].starts_with("ooooooooooooo"));
    assert!(lines[1].contains(r#".dP""Y88b"#));
    assert!(lines[3].contains(r#"`888""8P"#));
    assert!(lines[6].contains(r#"d888b    `Y888""8o"#));
    assert!(lines[6].contains(r#"`8bd88P'"#));
}

#[test]
fn static_banner_renders_all_logo_lines() {
    let mut output = Vec::new();

    render_static_banner(&mut output).expect("banner should render");

    let output = String::from_utf8(output).expect("banner should be utf8");
    assert!(output.contains("ooooooooooooo"));
    assert!(output.contains("`Y8bod88P\""));
    assert_eq!(output.lines().count(), banner_lines().len());
}

#[test]
fn shell_can_enter_smoke_loop_without_animation() {
    let mut output = Vec::new();

    run_without_animation(&mut output).expect("shell should run without animation");

    let output = String::from_utf8(output).expect("shell output should be utf8");
    assert!(output.contains("ooooooooooooo"));
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

    run_fullscreen_once_without_animation(&mut output).expect("fullscreen render should complete");

    let output = String::from_utf8(output).expect("fullscreen output should be utf8");
    assert!(output.starts_with(ENTER_FULLSCREEN_SEQUENCE));
    assert!(output.contains("Entering smoke loop"));
    assert!(output.ends_with(EXIT_FULLSCREEN_SEQUENCE));
}

#[test]
fn notfullscreen_mode_does_not_write_alternate_screen_sequences() {
    let mut output = Vec::new();

    run_not_fullscreen_without_animation(&mut output)
        .expect("notfullscreen render should complete");

    let output = String::from_utf8(output).expect("notfullscreen output should be utf8");
    assert!(!output.contains(ENTER_FULLSCREEN_SEQUENCE));
    assert!(!output.contains(EXIT_FULLSCREEN_SEQUENCE));
    assert!(output.contains("Entering smoke loop"));
}

#[test]
fn notfullscreen_accepts_debug_config_without_alternate_screen_sequences() {
    let mut output = Vec::new();

    run_not_fullscreen(
        &mut output,
        ShellLaunchConfig {
            terminal_mode: ShellTerminalMode::NotFullscreen,
            home_mode_override: HomeModeOverride::Debug,
        },
    )
    .expect("notfullscreen debug render should complete");

    let output = String::from_utf8(output).expect("notfullscreen output should be utf8");
    assert!(!output.contains(ENTER_FULLSCREEN_SEQUENCE));
    assert!(!output.contains(EXIT_FULLSCREEN_SEQUENCE));
    assert!(output.contains("TundraUX3 shell - Phase 0 smoke"));
}
