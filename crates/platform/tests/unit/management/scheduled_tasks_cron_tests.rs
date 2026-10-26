use super::*;
use chrono::TimeZone;

#[test]
fn editing_and_deleting_one_entry_preserve_other_lines_and_pause_state() {
    let original = "# keep\r\nCRON_TZ=Asia/Shanghai\r\n# TUNDRAUX3-PAUSED 0 9 * * 1-5 echo old\\%value\r\n@hourly echo untouched\r\n";
    let entry = entries(original, false, "demo").remove(0);
    let values = BTreeMap::from([
        ("schedule".into(), "30 10 * * 1-5".into()),
        ("command".into(), "echo 新任务\\%".into()),
    ]);
    let changed = change_entry(original, &entry, &values, false, false).unwrap();
    assert_eq!(
        changed,
        "# keep\r\nCRON_TZ=Asia/Shanghai\r\n# TUNDRAUX3-PAUSED 30 10 * * 1-5 echo 新任务\\%\r\n@hourly echo untouched\r\n"
    );
    assert_eq!(
        change_entry(original, &entry, &values, true, false).unwrap(),
        "# keep\r\nCRON_TZ=Asia/Shanghai\r\n@hourly echo untouched\r\n"
    );
    let bad = BTreeMap::from([("command".into(), "echo a\necho b".into())]);
    assert!(change_entry(original, &entry, &bad, false, false).is_err());
}

#[test]
fn multiple_lines_in_one_source_are_checked_and_changed_together() {
    let text = "# keep\r\nSHELL=/bin/sh\r\n@daily echo first\r\n@hourly echo untouched\r\n*/5 * * * * echo second\r\n";
    let all = rows(text, "user:demo", false, "demo", "version");
    let mut commands = [0, 2].map(|index| ManagementCommand {
        kind: ManagementKind::ScheduledTasks,
        action: "pause".into(),
        target: Some(all[index].id.clone()),
        identity: all[index].identity.clone(),
        values: BTreeMap::new(),
    });
    let selected =
        selected_entries(&commands, text, "demo", "crontab", "user:demo", "version").unwrap();
    let indices = selected.iter().map(|entry| entry.line).collect::<Vec<_>>();
    let paused = changed_text(text, &indices, true).unwrap();
    assert_eq!(
        paused,
        "# keep\r\nSHELL=/bin/sh\r\n# TUNDRAUX3-PAUSED @daily echo first\r\n@hourly echo untouched\r\n# TUNDRAUX3-PAUSED */5 * * * * echo second\r\n"
    );
    assert_eq!(changed_text(&paused, &indices, false).unwrap(), text);
    commands[1]
        .identity
        .insert("version".into(), "stale".into());
    assert!(matches!(
        selected_entries(&commands, text, "demo", "crontab", "user:demo", "version"),
        Err(ManagementError::Conflict(_))
    ));
}

#[test]
fn chinese_cron_labels_keep_native_task_identity_and_schedule() {
    let snapshot = i18n::LanguageSnapshotData {
        code: "zh-CN".into(),
        generation: 1,
        current: vec![
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ascii-assets/assets/locales/zh-CN/modules/management-scheduled-tasks.ftl"
            ))
            .into(),
        ],
        english: vec![],
    }
    .restore()
    .unwrap();
    let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
    let rows = rows(
        "SHELL=/bin/sh\n# TUNDRAUX3-PAUSED @daily echo unchanged\n",
        "user:demo",
        false,
        "demo",
        "original-version",
    );
    let row = &rows[0];
    assert_eq!(row.cells[4], "已暂停");
    assert_eq!(row.cells[6], "已暂停");
    assert_eq!(row.actions[0].label, "暂停");
    assert_eq!(row.actions[0].id, "pause");
    assert_eq!(row.identity["source"], "crontab");
    assert_eq!(row.identity["version"], "original-version");
    assert_eq!(row.identity["user"], "demo");
    assert_eq!(row.cells[3], "@daily");
    assert!(
        row.detail
            .iter()
            .any(|(label, value)| label == "环境变量" && value == "SHELL=/bin/sh")
    );
}

#[test]
fn parsing_preserves_environment_owner_aliases_and_paused_lines() {
    let text = "# comment\nPATH = '/custom/bin:/usr/bin'\nCRON_TZ=Asia/Shanghai\n0 3 * * MON root echo one\n# TUNDRAUX3-PAUSED @daily nobody echo two\n";
    let entries = entries(text, true, "");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].user, "root");
    assert_eq!(entries[0].environment["PATH"], "/custom/bin:/usr/bin");
    assert_eq!(entries[0].command, "echo one");
    assert!(entries[1].paused);
}

#[test]
fn pause_resume_changes_only_selected_line_and_detects_changed_source() {
    let text = "# comment\r\nSHELL=/bin/bash\r\n*/5 * * * * echo hello\r\n\n";
    let paused = changed_text(text, &[2], true).unwrap();
    assert_eq!(changed_text(&paused, &[2], false).unwrap(), text);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cron");
    fs::write(&path, text).unwrap();
    let version = file_version(&path, text).unwrap();
    fs::write(&path, "concurrent edit\n").unwrap();
    assert!(matches!(
        replace_file(&path, text, &version, &paused),
        Err(ManagementError::Conflict(_))
    ));
    assert_eq!(fs::read_to_string(path).unwrap(), "concurrent edit\n");
}

#[test]
fn next_trigger_uses_five_fields_aliases_dom_or_dow_and_cron_timezone() {
    let mut entry = entries(
        "CRON_TZ=Asia/Shanghai\n0 8 * * * echo hello\n",
        false,
        "demo",
    )
    .remove(0);
    let now = Utc.with_ymd_and_hms(2026, 10, 14, 23, 0, 0).unwrap();
    assert_eq!(next_run(&entry, now), "2026-10-15T08:00:00+08:00");
    entry.schedule = "@hourly".into();
    assert_eq!(next_run(&entry, now), "2026-10-15T08:00:00+08:00");
    entry.schedule = "0 8 1 * THU".into();
    assert_eq!(next_run(&entry, now), "2026-10-15T08:00:00+08:00");
    for pattern in ["0 8 L * *", "0 8 * * 1#2", "@reboot", "~ * * * *"] {
        entry.schedule = pattern.into();
        assert!(next_run(&entry, now).to_lowercase().contains("cannot"));
    }
}

#[test]
fn cron_percent_uses_stdin_even_inside_shell_quotes() {
    assert_eq!(
        split_percent("cat%first%second\\%literal"),
        ("cat".into(), Some("first\nsecond%literal\n".into()))
    );
    assert_eq!(
        split_percent("printf '\\%s' a"),
        ("printf '%s' a".into(), None)
    );
    assert_eq!(
        split_percent("echo 'x%y'"),
        ("echo 'x".into(), Some("y'\n".into()))
    );
    assert_eq!(split_percent("cat%"), ("cat".into(), Some("\n".into())));
}

#[test]
fn manual_execution_arguments_keep_identity_environment_directory_and_stdin() {
    let account = actor(unsafe { libc::getuid() }).unwrap();
    let entry = entries(
        "SHELL=/bin/sh\nPATH=/usr/bin:/bin\n* * * * * cat%line one%line two\n",
        false,
        &account.username,
    )
    .remove(0);
    let args = run_arguments(
        &entry,
        &account,
        "cron-safe",
        "fixture",
        "EXTRA_MANAGER_VALUE=must-not-leak\n",
    )
    .unwrap();
    assert!(
        args.iter()
            .any(|arg| arg.starts_with("--property=UnsetEnvironment=")
                && arg.contains("EXTRA_MANAGER_VALUE"))
    );
    assert!(args.contains(&format!("--uid={}", account.uid)));
    assert!(args.contains(&format!("--working-directory={}", account.home.display())));
    assert!(args.contains(&format!("--setenv=LOGNAME={}", account.username)));
    assert!(
        args.iter()
            .find(|arg| arg.starts_with("--property=ExecStopPost="))
            .unwrap()
            .contains("'%s")
    );
    let output = Command::new("/bin/sh")
        .args(["-c", args.last().unwrap()])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"line one\nline two\n");
}
