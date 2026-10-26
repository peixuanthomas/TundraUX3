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
    let mut unicode_values = values.clone();
    unicode_values.insert("name".into(), "每日维护 %s".into());
    unicode_values.insert("persistent".into(), "true".into());
    let unicode = super::definition(&unicode_values, unsafe { libc::getuid() }).unwrap();
    assert!(unicode.name.is_ascii());
    assert!(unicode.service.contains("Description=每日维护 %%s"));
    assert!(unicode.timer.contains("Persistent=true"));
    validate(&unicode, &AtomicBool::new(false)).unwrap();
    let preview = preview_plan(
        &unicode_values,
        unsafe { libc::getuid() },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(preview.contains("每日维护 %s"));
    assert!(!preview.contains("[schedule-review]"));
    assert_eq!(
        preview
            .lines()
            .filter(|line| line
                .trim_start()
                .starts_with(|character: char| character.is_ascii_digit()))
            .count(),
        5
    );
    unicode_values.insert("schedule".into(), "2000-01-01 00:00:00".into());
    let past = super::definition(&unicode_values, unsafe { libc::getuid() }).unwrap();
    assert!(validate(&past, &AtomicBool::new(false)).is_err());
}
