use std::io::Write;
use std::process::Command;

use shell::{
    ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE, ShellArgError, banner_lines,
    parse_shell_args, render_static_banner, startup_lines,
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
    assert!(output.starts_with("\x1B[97m"));
    assert!(output.ends_with("\x1B[0m"));
    let visible_output = output
        .strip_prefix("\x1B[97m")
        .and_then(|output| output.strip_suffix("\x1B[0m"))
        .expect("static banner should wrap its output in white and reset ANSI sequences");
    let actual_lines = visible_output
        .lines()
        .map(str::to_string)
        .collect::<Vec<String>>();
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
fn shell_accepts_only_an_empty_argument_list() {
    assert_eq!(parse_shell_args(std::iter::empty::<&str>()), Ok(()));
}

#[test]
fn former_flags_help_and_positional_arguments_are_all_rejected() {
    for argument in [
        "-notfullscreen",
        "-debug",
        "-editor",
        "--help",
        "document.md",
    ] {
        let error = parse_shell_args([argument]).expect_err("every argument must be rejected");
        assert_eq!(
            error,
            ShellArgError::ArgumentNotAllowed(argument.to_string())
        );
        assert_eq!(
            error.to_string(),
            format!("tundra-shell does not accept arguments: {argument}")
        );
    }
}

#[test]
fn multiple_arguments_are_rejected_at_the_process_boundary() {
    assert_eq!(
        parse_shell_args(["-debug", "-editor"]),
        Err(ShellArgError::ArgumentNotAllowed("-debug".to_string()))
    );
}

#[test]
fn shell_binary_rejects_arguments_before_starting_the_ui() {
    let output = Command::new(env!("CARGO_BIN_EXE_tundra-shell"))
        .arg("--help")
        .output()
        .expect("run tundra-shell with a prohibited argument");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("argument error should be utf8");
    assert!(stderr.contains("tundra-shell does not accept arguments: --help"));
}

#[test]
fn fullscreen_mode_enters_and_exits_alternate_screen() {
    let mut output = Vec::new();

    platform::with_terminal_fullscreen(&mut output, |output| {
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
fn static_banner_does_not_write_alternate_screen_sequences() {
    let mut output = Vec::new();

    render_static_banner(&mut output).expect("static banner should render");
    writeln!(output, "Entering smoke loop").expect("smoke marker should render");

    let output = String::from_utf8(output).expect("static banner output should be utf8");
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
