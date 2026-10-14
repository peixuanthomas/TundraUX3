use std::process::{Command, Stdio};

#[test]
fn help_and_version_work_without_startup_or_writable_watchdog_paths() {
    // A file cannot contain directories, even when the test runs as root.
    let blocked = std::env::current_exe().unwrap().join("unavailable");
    for flag in ["--help", "-h", "--version", "-V"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tundra-shell"));
        command.arg(flag).stdin(Stdio::null());
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
        let output = command.output().expect("run Shell information command");
        assert_eq!(output.status.code(), Some(0), "{flag}: {output:?}");
        assert!(output.stderr.is_empty(), "{flag}: {output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        if matches!(flag, "--version" | "-V") {
            assert_eq!(
                text,
                format!("tundra-shell {}\n", env!("CARGO_PKG_VERSION"))
            );
        } else {
            assert!(text.contains("Usage: tundra-shell [OPTIONS]"), "{text}");
            assert!(
                text.contains("--help") && text.contains("--version"),
                "{text}"
            );
        }
        assert!(!text.contains('\x1b'), "must not enter terminal UI: {text}");
    }
}
