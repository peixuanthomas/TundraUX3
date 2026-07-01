use tundra_shell::{
    ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE, ShellLaunchMode, banner_lines,
    parse_shell_args, render_static_banner, run_fullscreen_once_without_animation,
    run_not_fullscreen_without_animation, run_without_animation, startup_lines,
};

#[test]
fn startup_lines_state_phase_zero_boundaries() {
    let lines = startup_lines();

    assert!(lines.iter().any(|line| line.contains("TundraUX3 shell")));
    assert!(lines.iter().any(|line| line.contains("Phase 0")));
    assert!(lines.iter().any(|line| line.contains("Windows 11")));
    assert!(lines.iter().any(|line| line.contains("Windows Terminal")));
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
fn shell_can_enter_placeholder_main_loop_without_animation() {
    let mut output = Vec::new();

    run_without_animation(&mut output).expect("shell should run without animation");

    let output = String::from_utf8(output).expect("shell output should be utf8");
    assert!(output.contains("ooooooooooooo"));
    assert!(output.contains("TundraUX3 shell - Phase 0 smoke"));
    assert!(output.contains("Entering main loop placeholder"));
}

#[test]
fn shell_defaults_to_fullscreen_mode() {
    assert_eq!(
        parse_shell_args(std::iter::empty::<&str>()).expect("empty args should parse"),
        ShellLaunchMode::Fullscreen
    );
}

#[test]
fn shell_can_be_started_without_fullscreen_explicitly() {
    assert_eq!(
        parse_shell_args(["-notfullscreen"]).expect("flag should parse"),
        ShellLaunchMode::NotFullscreen
    );
}

#[test]
fn fullscreen_mode_enters_and_exits_alternate_screen() {
    let mut output = Vec::new();

    run_fullscreen_once_without_animation(&mut output).expect("fullscreen render should complete");

    let output = String::from_utf8(output).expect("fullscreen output should be utf8");
    assert!(output.starts_with(ENTER_FULLSCREEN_SEQUENCE));
    assert!(output.contains("Entering main loop placeholder"));
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
    assert!(output.contains("Entering main loop placeholder"));
}
