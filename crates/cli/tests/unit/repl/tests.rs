use super::*;

#[test]
fn status_metadata_is_added_only_to_the_rendered_primary_prompt() {
    let display = PromptDisplay {
        accent: "\x1b[95m".to_string(),
    };
    let prompt = "○ user@C:\\Documents >> ";
    assert_eq!(
        display.highlight_prompt(prompt, true),
        "○\x1b[777;0z \x1b[95muser@C:\\Documents\x1b[39m >> "
    );
    assert_eq!(display.highlight_prompt(prompt, false), prompt);
    let reset = "Type RESET to erase TundraUX3 data, or press Enter to cancel: ";
    assert_eq!(display.highlight_prompt(reset, true), reset);
}

#[test]
fn prompt_color_accepts_only_foreground_sequences() {
    for sequence in ["\x1b[95m", "\x1b[38;5;200m", "\x1b[38;2;12;34;56m"] {
        assert_eq!(validated_foreground(sequence).as_deref(), Some(sequence));
    }
    for sequence in ["\x1b[2J", "\x1b[0m", "\x1b[38;2;256;0;0m", "\x1b[95m\n"] {
        assert_eq!(validated_foreground(sequence), None);
    }
}

#[test]
fn prompt_rejects_untrusted_environment_values() {
    assert_eq!(
        prompt_for_username(Some("user\u{1b}[31m"), None),
        "tundra@? >> ".to_string()
    );
    assert_eq!(
        prompt_for_username(Some(" user.name "), None),
        "user.name@? >> ".to_string()
    );
}

#[test]
fn prompt_shows_the_absolute_directory_without_terminal_controls() {
    for path in ["/home/user/space and 中文", r"C:\Users\user\My Documents"] {
        assert_eq!(
            prompt_for_username(Some("user"), Some(std::path::Path::new(path))),
            format!("user@{path} >> "),
        );
    }
    assert_eq!(
        prompt_for_username(
            Some("user"),
            Some(std::path::Path::new("/tmp/a\n\u{1b}[31m"))
        ),
        "user@/tmp/a\\n\\u{1b}[31m >> ",
    );
}

#[test]
fn prompt_uses_the_system_session_directory() {
    let mut session = SystemCommandSession::new().unwrap();
    let root = session
        .current_dir()
        .ancestors()
        .last()
        .unwrap()
        .to_path_buf();
    let command = if cfg!(windows) {
        format!("cd /d \"{}\"", root.display())
    } else {
        "cd /".to_string()
    };
    let result = session.run(&command).unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.state_error.is_none());
    let prompt = repl_prompt(false, Some(&session));
    assert!(
        prompt.ends_with(&format!("@{} >> ", root.display())),
        "{prompt}"
    );
}

#[test]
fn repl_requires_a_prefix_for_ux_commands_and_keeps_system_paths_intact() {
    assert_eq!(
        ux_command_input("/config set address 'New York'"),
        Some("config set address 'New York'")
    );
    assert_eq!(
        shlex::split(ux_command_input("/config set address 'New York'").unwrap()),
        Some(vec![
            "config".to_string(),
            "set".to_string(),
            "address".to_string(),
            "New York".to_string(),
        ])
    );
    assert_eq!(ux_command_input("/"), Some(""));
    assert_eq!(ux_command_input("/echo example"), Some("echo example"));
    for line in [
        "dir",
        "config get",
        "/usr/bin/printf '%s' hello",
        "'/usr/bin/printf' hello",
        "echo \"$HOME\" | cat",
    ] {
        assert_eq!(ux_command_input(line), None, "{line}");
    }
}

#[test]
fn ux_hints_recognize_invalid_subcommands_without_guessing_unknown_names() {
    for line in [
        "help",
        "config get",
        "config invalid",
        "debug invalid",
        "new",
    ] {
        assert!(is_likely_ux_command(line), "{line}");
    }
    for line in ["echo hello", "tundra-unknown", "config '", ""] {
        assert!(!is_likely_ux_command(line), "{line}");
    }
    assert_eq!(visible_command("echo \u{1b}[31m\n"), "echo \\u{1b}[31m\\n");
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
    assert_eq!(run_system_command(&mut None, command, false), 7);
}
