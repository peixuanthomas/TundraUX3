use super::*;

#[test]
fn quote_preserves_shell_expansion_and_escapes_systemd_syntax() {
    assert_eq!(
        quote("echo \"$HOME\"; printf '%s' a\\b\n"),
        "\"echo \\\"$HOME\\\"; printf '%%s' a\\\\b\\n\""
    );
    assert!(result_command("safe", 1000).starts_with(":/bin/sh -c "));
    assert!(result_command("safe", 1000).contains("$SERVICE_RESULT"));
}

#[test]
fn timer_names_reject_patterns_paths_and_templates() {
    for name in ["../a.timer", "*.timer", "-a.timer", "a@.timer", "a.service"] {
        assert!(!valid_timer(name));
    }
    assert!(valid_timer("example@user.timer"));
}

#[test]
fn native_calendar_and_generated_definitions_are_validated_without_installing() {
    let directory = tempfile::tempdir().unwrap();
    let account = actor(unsafe { libc::getuid() }).unwrap();
    let values = BTreeMap::from([
        ("name".into(), "fixture".into()),
        ("command".into(), "printf '%s\\n' \"$HOME\"".into()),
        (
            "working_directory".into(),
            directory.path().display().to_string(),
        ),
        ("schedule".into(), "daily".into()),
        ("user".into(), account.username),
    ]);
    let definition = definition(&values, unsafe { libc::getuid() }).unwrap();
    assert!(definition.timer.contains("Persistent=false"));
    assert!(definition.service.contains("ExecStart=:/bin/sh"));
    assert!(
        validate(&definition, &AtomicBool::new(false))
            .unwrap()
            .contains("Next elapse")
    );
    let bad = Definition {
        schedule: "invalid schedule".into(),
        ..definition
    };
    assert!(validate(&bad, &AtomicBool::new(false)).is_err());
}
