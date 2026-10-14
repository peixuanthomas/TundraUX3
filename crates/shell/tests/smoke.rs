use std::io::{self, Write};

use shell::{
    ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE, ShellArgError, ShellLaunchCommand,
    banner_lines, parse_shell_args, render_static_banner,
};

#[test]
fn static_banner_renders_the_complete_logo_and_resets_color() {
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
fn shell_accepts_information_flags_and_rejects_other_arguments() {
    assert_eq!(
        parse_shell_args(std::iter::empty::<&str>()),
        Ok(ShellLaunchCommand::Run)
    );
    for argument in ["-h", "--help"] {
        assert_eq!(parse_shell_args([argument]), Ok(ShellLaunchCommand::Help));
    }
    for argument in ["-V", "--version"] {
        assert_eq!(
            parse_shell_args([argument]),
            Ok(ShellLaunchCommand::Version)
        );
    }

    for argument in [
        "-notfullscreen",
        "-debug",
        "-editor",
        "--unknown",
        "document.md",
    ] {
        let error =
            parse_shell_args([argument]).expect_err("unsupported argument must be rejected");
        assert_eq!(
            error,
            ShellArgError::ArgumentNotAllowed(argument.to_string())
        );
        assert_eq!(
            error.to_string(),
            format!("unsupported tundra-shell argument: {argument}; use --help for usage")
        );
    }

    assert_eq!(
        parse_shell_args(["-debug", "-editor"]),
        Err(ShellArgError::ArgumentNotAllowed("-debug".to_string()))
    );

    for flag in ["--help", "-h", "--version", "-V"] {
        for extra in ["extra", "--help", "--version"] {
            assert_eq!(
                parse_shell_args([flag, extra]),
                Err(ShellArgError::ArgumentNotAllowed(extra.to_string()))
            );
        }
    }
}

#[test]
fn fullscreen_mode_enters_and_restores_the_terminal() {
    let mut output = Vec::new();

    platform::with_terminal_fullscreen(&mut output, |output| writeln!(output, "content"))
        .expect("fullscreen render should complete");

    let output = String::from_utf8(output).expect("fullscreen output should be utf8");
    assert_eq!(
        output,
        format!("{ENTER_FULLSCREEN_SEQUENCE}content\n{EXIT_FULLSCREEN_SEQUENCE}")
    );

    let mut failed_output = Vec::new();
    let error = platform::with_terminal_fullscreen(&mut failed_output, |output| {
        writeln!(output, "partial content")?;
        Err::<(), _>(io::Error::other("body failed"))
    })
    .expect_err("body failure should be returned");

    assert_eq!(error.kind(), io::ErrorKind::Other);
    let failed_output = String::from_utf8(failed_output).expect("fullscreen output should be utf8");
    assert_eq!(
        failed_output,
        format!("{ENTER_FULLSCREEN_SEQUENCE}partial content\n{EXIT_FULLSCREEN_SEQUENCE}")
    );
}
