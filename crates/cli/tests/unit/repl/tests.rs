use super::*;
use rustyline::hint::Hint;

#[test]
fn suggestions_continue_through_debug_config_help_and_log_arguments() {
    for (line, expected) in [
        ("/debug scr", "screen-keyboard"),
        ("/debug view-ui-style ", "2"),
        ("/debug clear-logs --type sn", "snapshots"),
        ("/debug clear-logs runtime --y", "--yes"),
        ("/config set mo", "motion"),
        ("/config set motion r", "reduced"),
        ("/config reset border-s", "border-shape"),
        ("/help debug scr", "screen-keyboard"),
        ("/logs query --sou", "--source"),
        ("/logs query --source l", "linux"),
        ("/logs export --out", "--output"),
        ("/packages sources add --signed", "--signed-by"),
    ] {
        assert!(
            command_completions(line, line.len())
                .1
                .contains(&expected.to_string()),
            "{line}"
        );
    }
    assert!(
        !command_completions("/config set ", 12)
            .1
            .contains(&"theme".into())
    );
    for line in ["/debug", "/debug ", "/debug test-", "/config set motion"] {
        let hint = command_hint(line, line.len()).unwrap();
        assert!(hint.display().starts_with("\nSuggestions: "), "{line}");
        assert!(hint.completion().is_none());
    }
    let hint = command_hint("/debug scr", 10).unwrap();
    assert_eq!(hint.display(), "een-keyboard");
    assert_eq!(hint.completion(), Some("een-keyboard"));
}

#[test]
fn typos_suggest_full_corrected_commands_without_inserting_hint_labels() {
    for (line, corrected) in [
        ("hell", "help"),
        ("/hell", "/help"),
        ("/hlep", "/help"),
        ("/debug scren-keyboard", "/debug screen-keyboard"),
        ("/config set motoin", "/config set motion"),
    ] {
        let hint = command_hint(line, line.len()).unwrap();
        assert_eq!(hint.display(), format!("\nDid you mean: {corrected}"));
        assert_eq!(hint.completion(), None);
    }
    assert_eq!(command_completions("/hell", 5), (0, vec!["/help".into()]));
    for line in [
        "",
        "help",
        "/usr/bin/hell",
        "/config set address 'hell",
        "/debug 中文",
        "hell\n",
    ] {
        assert!(command_hint(line, line.len()).is_none(), "{line}");
    }
    assert!(command_hint("/debug scr", 8).is_none());
    assert!(command_completions("/debug 中", 9).1.is_empty());
}

#[test]
fn hint_foreground_is_gray_and_resets_afterward() {
    let display = PromptDisplay {
        accent: "\x1b[95m".into(),
    };
    assert_eq!(display.highlight_hint("lp"), "\x1b[90mlp\x1b[39m");
    assert_eq!(
        display.highlight_hint("\nDid you mean: /help"),
        "\x1b[90m\nDid you mean: /help\x1b[39m"
    );
}

#[test]
fn system_suggestions_match_the_platform_and_use_general_typo_matching() {
    for (os, expected, absent) in [
        ("windows", "ipconfig", "systemctl"),
        ("linux", "systemctl", "ipconfig"),
        ("macos", "diskutil", "systemctl"),
    ] {
        let commands = completion::system_words(os, &[]);
        assert!(commands.contains(&expected.to_string()));
        assert!(!commands.contains(&absent.to_string()));
        assert!(commands.contains(&"help".into()));
    }
    for (line, corrected) in [
        ("gti", "git"),
        ("gitt", "git"),
        ("gxx", "git"),
        ("git statsu", "git status"),
        ("cargo bulid", "cargo build"),
        ("/debug doctro", "/debug doctor"),
        ("/config get timeznoe", "/config get timezone"),
    ] {
        let hint = command_hint(line, line.len()).unwrap();
        assert!(
            hint.display().contains(corrected),
            "{line}: {}",
            hint.display()
        );
        assert!(hint.completion().is_none());
    }
    assert_eq!(command_completions("hel", 3).1, ["help"]);
    assert_eq!(command_completions("git stat", 8).1, ["status"]);
    assert!(is_system_help("help"));
    assert!(is_system_help("help cd"));
    assert!(!is_system_help("/help"));
}

#[test]
fn ux_completion_covers_formal_operations_and_keeps_system_commands_intact() {
    assert_eq!(
        command_completions("/serv", 5),
        (0, vec!["/services".into()])
    );
    let (start, values) = command_completions("/services sta", 13);
    assert_eq!(start, 10);
    assert_eq!(values, ["start"]);
    assert!(
        command_completions("/logs f", 7)
            .1
            .contains(&"follow".into())
    );
    assert_eq!(command_completions("/packages sources e", 19).1, ["enable"]);
    assert!(command_completions("services sta", 12).1.is_empty());
    assert!(command_completions("/usr/bin/ls", 11).1.is_empty());
    let line = "/users primary-group demo --pri";
    assert!(
        command_completions(line, line.len())
            .1
            .contains(&"--primary-group".into())
    );
}

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
        "debug screen-keyboard",
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
