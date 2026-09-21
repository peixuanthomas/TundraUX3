use super::*;

#[test]
fn prompt_rejects_untrusted_environment_values() {
    assert_eq!(
        prompt_for_username(Some("user\u{1b}[31m")),
        "tundra >> ".to_string()
    );
    assert_eq!(
        prompt_for_username(Some(" user.name ")),
        "user.name >> ".to_string()
    );
}

#[test]
fn repl_parser_keeps_shell_escapes_out_of_cli_parsing() {
    assert_eq!(
        shlex::split("config set address 'New York'"),
        Some(vec![
            "config".to_string(),
            "set".to_string(),
            "address".to_string(),
            "New York".to_string(),
        ])
    );
    assert_eq!("/dir".strip_prefix('/'), Some("dir"));
}

#[test]
fn exit_and_reset_words_are_exact() {
    assert!(is_exit_line("exit"));
    assert!(!is_exit_line(" exit"));
    assert!(!is_exit_line("exit "));
    assert!(is_reset_confirmation("RESET"));
    assert!(!is_reset_confirmation("reset"));
    assert!(!is_reset_confirmation("RESET "));
}

#[test]
fn system_command_returns_its_exit_code() {
    let command = if cfg!(windows) { "exit /B 7" } else { "exit 7" };
    assert_eq!(run_system_command(command), 7);
}
