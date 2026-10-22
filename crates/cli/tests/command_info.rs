use std::process::{Command, Stdio};

#[test]
fn help_and_version_work_without_startup_or_writable_watchdog_paths() {
    // A file cannot contain directories, even when the test runs as root.
    let blocked = std::env::current_exe().unwrap().join("unavailable");
    let cases: &[&[&str]] = &[
        &[],
        &["help"],
        &["--help"],
        &["-h"],
        &["--version"],
        &["-V"],
        &["help", "config"],
        &["config", "set", "-h"],
        &["launcher", "--help"],
        &["logs", "export", "--help"],
        &["services", "--help"],
        &["operations", "--help"],
        &["firewall", "allow", "--help"],
        &["ssh", "confirm", "--help"],
        &["backups", "restore", "--help"],
        &["scheduled-tasks", "create", "--help"],
        &["ui"],
        &["ui", "--help"],
        &["help", "ui", "text"],
        &["ui", "click", "--help"],
        &["debug"],
        &["debug", "asset"],
        &["debug", "clear-logs"],
        &["debug", "view-ui-style"],
        &["debug", "test-watchdog-panic", "--help"],
    ];
    for args in cases {
        assert!(
            cli::parse_args(args.iter().copied())
                .unwrap()
                .is_help_or_version()
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_tundra-cli"));
        command.args(*args).stdin(Stdio::null());
        for key in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
            "TMPDIR",
            "TMP",
            "TEMP",
            "APPDATA",
            "LOCALAPPDATA",
        ] {
            command.env(key, &blocked);
        }
        let output = command.output().expect("run CLI information command");
        assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        if matches!(*args, ["--version"] | ["-V"]) {
            assert_eq!(text, format!("tundra-cli {}\n", env!("CARGO_PKG_VERSION")));
        } else {
            assert!(text.contains("Usage:"), "{args:?}: {text}");
        }
        assert!(!text.contains('\x1b'), "must not enter terminal UI: {text}");
    }
    for args in [
        &["new"][..],
        &["doctor"],
        &["debug", "test-watchdog-panic"],
        &["repl"],
        &["config", "get"],
    ] {
        assert!(
            !cli::parse_args(args.iter().copied())
                .unwrap()
                .is_help_or_version()
        );
    }
}
