use tundra_cli::{CliCommand, CliError, parse_args, run};

#[test]
fn no_args_dispatches_help() {
    assert_eq!(parse_args(std::iter::empty::<&str>()), Ok(CliCommand::Help));
}

#[test]
fn doctor_arg_dispatches_doctor() {
    assert_eq!(parse_args(["doctor"]), Ok(CliCommand::Doctor));
}

#[test]
fn paths_arg_dispatches_paths() {
    assert_eq!(parse_args(["paths"]), Ok(CliCommand::Paths));
}

#[test]
fn explain_arg_dispatches_explain() {
    assert_eq!(parse_args(["explain"]), Ok(CliCommand::Explain));
}

#[test]
fn unknown_arg_is_an_error() {
    assert_eq!(
        parse_args(["repair"]),
        Err(CliError::UnknownCommand("repair".to_string()))
    );
}

#[test]
fn extra_arg_is_an_error() {
    assert_eq!(
        parse_args(["doctor", "--json"]),
        Err(CliError::UnexpectedArgument("--json".to_string()))
    );
}

#[test]
fn help_command_writes_usage_to_stdout() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["help"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("help output should be utf8");
    assert!(stdout.contains("Usage: tundra-cli <doctor|explain|paths>"));
}

#[test]
fn explain_command_prints_startup_and_boundary_notes() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["explain"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("explain output should be utf8");
    assert!(stdout.contains("Startup flow"));
    assert!(stdout.contains("tundra-cli"));
    assert!(stdout.contains("Kernel boundary"));
    assert!(stdout.contains("UI boundary"));
    assert!(stdout.contains("tundra-platform"));
    assert!(stdout.contains("tundra-shell"));
}

#[test]
fn unknown_command_exits_two_and_writes_error_to_stderr() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["repair"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 2);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("error output should be utf8");
    assert!(stderr.contains("ERROR: unknown command: repair"));
    assert!(stderr.contains("Usage: tundra-cli <doctor|explain|paths>"));
}
