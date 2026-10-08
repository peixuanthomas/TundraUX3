use super::*;
use serde_json::json;

fn query() -> LogQuery {
    LogQuery {
        source: LogSource::Linux,
        ..Default::default()
    }
}
fn journal(transport: &str, pri: u8, time: i64) -> String {
    json!({"__CURSOR": format!("s=cursor;t={time}"), "__REALTIME_TIMESTAMP": time.to_string(), "_TRANSPORT": transport, "PRIORITY": pri.to_string(), "_SYSTEMD_UNIT": "example.service", "_KERNEL_DEVICE": "+pci:0000:01:00.0", "_PID": "45", "MESSAGE": "driver reported an error"}).to_string()
}
fn good(stdout: &str) -> Capture {
    Capture {
        stdout: stdout.as_bytes().to_vec(),
        success: true,
        ..Default::default()
    }
}

#[test]
fn boot_and_invocation_filters_are_passed_before_record_limit() {
    let mut query = query();
    query.systemd_boot = Some("-1".into());
    query.systemd_invocation = Some("1234567890abcdef1234567890abcdef".into());
    let result = query_with(&query, &AtomicBool::new(false), |program, args| {
        assert_eq!(program, "journalctl");
        assert!(args.contains(&"--boot=-1".into()));
        assert!(args.contains(&"_SYSTEMD_INVOCATION_ID=1234567890abcdef1234567890abcdef".into()));
        assert!(args.contains(&"INVOCATION_ID=1234567890abcdef1234567890abcdef".into()));
        assert!(args.contains(&"+".into()));
        good("")
    });
    assert_eq!(result.state, LogSourceState::Ready);
    query.systemd_boot = Some("--all".into());
    let result = query_with(&query, &AtomicBool::new(false), |_, _| {
        panic!("invalid filter must not spawn")
    });
    assert_eq!(result.state, LogSourceState::Unavailable);
}

#[test]
fn invocation_alternatives_keep_service_boot_time_and_user_constraints() {
    for scope in ["system", "user"] {
        let query = LogQuery {
            systemd_unit: Some("example.service".into()),
            systemd_scope: Some(scope.into()),
            systemd_boot: Some("-1".into()),
            systemd_invocation: Some("1234567890abcdef1234567890abcdef".into()),
            since: DateTime::from_timestamp_micros(1_000_000),
            until: DateTime::from_timestamp_micros(2_000_000),
            module: Some("linux.kernel".into()),
            ..query()
        };
        let result = query_with(&query, &AtomicBool::new(false), |program, args| {
            assert_eq!(program, "journalctl");
            for option in ["--boot=-1", "--since=@1.000000", "--until=@2.000000"] {
                assert_eq!(args.iter().filter(|arg| *arg == option).count(), 1);
            }
            assert!(args.contains(&format!(
                "--{}unit=example.service",
                if scope == "user" { "user-" } else { "" }
            )));
            let branches: Vec<_> = args
                .iter()
                .filter(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .collect::<Vec<_>>()
                .split(|arg| *arg == "+")
                .map(<[_]>::to_vec)
                .collect();
            assert_eq!(branches.len(), 2);
            for branch in &branches {
                assert!(branch.contains(&"_TRANSPORT=kernel"));
                if scope == "user" {
                    assert!(branch.contains(&format!("_UID={}", current_uid()).as_str()));
                }
            }
            assert!(
                branches[0].contains(&"_SYSTEMD_INVOCATION_ID=1234567890abcdef1234567890abcdef")
            );
            assert!(branches[1].contains(&"INVOCATION_ID=1234567890abcdef1234567890abcdef"));
            good("")
        });
        assert_eq!(result.state, LogSourceState::Ready);
    }
}

#[test]
fn invocation_filter_keeps_manager_records_with_a_different_origin_invocation() {
    let id = "1234567890abcdef1234567890abcdef";
    let other_id = "abcdef1234567890abcdef1234567890";
    let mut record: Value = serde_json::from_str(&journal("stdout", 3, 1_000_000)).unwrap();
    record["_SYSTEMD_INVOCATION_ID"] = Value::String(other_id.into());
    record["INVOCATION_ID"] = Value::String(id.into());
    let query = LogQuery {
        systemd_unit: Some("example.service".into()),
        systemd_invocation: Some(id.into()),
        ..query()
    };
    let result = parse_journal(serde_json::to_string(&record).unwrap().as_bytes(), &query);
    assert_eq!(result.events.len(), 1);
    assert!(
        result.events[0]
            .native_source
            .as_deref()
            .unwrap()
            .contains(&format!("invocation={id}"))
    );
    record["INVOCATION_ID"] = Value::String(other_id.into());
    assert!(
        parse_journal(serde_json::to_string(&record).unwrap().as_bytes(), &query)
            .events
            .is_empty()
    );
}

#[test]
fn journal_uses_origin_and_native_priority_not_message_words() {
    let records = format!(
        "{}\n{}",
        journal("kernel", 4, 1_000_000),
        journal("stdout", 6, 2_000_000)
    );
    let result = parse_journal(records.as_bytes(), &query());
    assert_eq!(result.state, LogSourceState::Ready);
    let service = &result.events[0];
    assert_eq!(service.context.module, "linux.service");
    assert_eq!(service.level, LogLevel::Info);
    assert_eq!(service.native_priority, Some(6));
    assert_eq!(
        service.native_source.as_deref(),
        Some("journal; unit=example.service")
    );
    assert_eq!(service.event_id, "journal:s=cursor;t=2000000");
    assert_eq!(service.process_id, 45);
    let kernel = &result.events[1];
    assert_eq!(kernel.context.module, "linux.kernel");
    assert_eq!(kernel.level, LogLevel::Warning);
    assert!(kernel.native_source.as_deref().unwrap().contains("pci:"));
    for event in result.events {
        assert_eq!(event.context.run_id, None);
        assert_eq!(event.context.task_id, None);
        assert_eq!(event.context.operation_id, None);
        assert_eq!(event.context.owner_id, None);
        assert_eq!(event.incident_id, None);
    }
}

#[test]
fn journal_maps_all_eight_priorities_and_filters_before_limit() {
    let data = (0..8)
        .map(|p| journal("kernel", p, 1_000_000 + i64::from(p)))
        .collect::<Vec<_>>()
        .join("\n");
    let result = parse_journal(data.as_bytes(), &query());
    let mut levels = result.events.iter().map(|e| e.level).collect::<Vec<_>>();
    levels.reverse();
    assert_eq!(
        levels,
        vec![
            LogLevel::Critical,
            LogLevel::Critical,
            LogLevel::Critical,
            LogLevel::Error,
            LogLevel::Warning,
            LogLevel::Info,
            LogLevel::Info,
            LogLevel::Debug
        ]
    );
    let filtered = LogQuery {
        min_level: Some(LogLevel::Warning),
        since: DateTime::from_timestamp_micros(1_000_002),
        until: DateTime::from_timestamp_micros(1_000_003),
        module: Some("linux.kernel".into()),
        limit: 1,
        ..query()
    };
    let result = parse_journal(data.as_bytes(), &filtered);
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].native_priority, Some(3));
    assert!(result.truncated);
}

#[test]
fn mixed_corrupt_and_binary_messages_are_skipped() {
    let data = format!(
        "{}\nnot json\n{{\"MESSAGE\":[0,255]}}\n{{",
        journal("kernel", 3, 1)
    );
    let result = parse_journal(data.as_bytes(), &query());
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.damaged_records, 3);
    assert_eq!(result.state, LogSourceState::Partial);
}

#[test]
fn messages_omit_declared_sensitive_payloads_and_terminal_controls() {
    for text in [
        "password=secrets",
        "Authorization: Bearer abc",
        "token:abc",
        "clipboard: body",
        "file content: private",
        "response body: private",
        "Cookie: session=abc",
    ] {
        assert_eq!(clean(text), "[sensitive message omitted]");
    }
    assert!(!clean("a\x1b[2J\n\u{202e}b").contains(['\x1b', '\n', '\u{202e}']));
    assert_eq!(clean(&"a".repeat(MAX_TEXT + 1)).len(), MAX_TEXT);
    assert!(!clean("connect https://user:opaque@host/path failed").contains("opaque"));
}

#[test]
fn unavailable_journal_falls_back_with_explicit_service_gap() {
    let mut programs = vec![];
    let result = query_with(&query(), &AtomicBool::new(false), |program, _| {
        programs.push(program.to_owned());
        if program == "journalctl" {
            Capture {
                stop: Some(LogSourceState::Unavailable),
                ..Default::default()
            }
        } else {
            good(r#"{"dmesg":[{"pri":4,"time":"12.345","msg":"device failed"}]}"#)
        }
    });
    assert_eq!(programs, ["journalctl", "dmesg"]);
    assert_eq!(result.state, LogSourceState::Partial);
    assert_eq!(result.events[0].context.module, "linux.kernel");
    assert_eq!(result.events[0].level, LogLevel::Warning);
    assert!(
        result.events[0]
            .timestamp_note
            .as_deref()
            .unwrap()
            .contains("12.345 seconds since boot")
    );
    assert!(
        result
            .notices
            .iter()
            .any(|n| n.contains("not external services"))
    );
}

#[test]
fn denied_journal_does_not_silently_fallback() {
    let result = query_with(&query(), &AtomicBool::new(false), |program, _| {
        assert_eq!(program, "journalctl");
        Capture {
            stderr: b"Permission denied".to_vec(),
            ..Default::default()
        }
    });
    assert_eq!(result.state, LogSourceState::PermissionDenied);
    assert!(result.events.is_empty());
}

#[test]
fn successful_journal_with_restricted_access_is_partial() {
    let result = query_with(&query(), &AtomicBool::new(false), |_, _| Capture {
        stderr: b"Hint: You are currently not seeing messages from other users and the system."
            .to_vec(),
        ..good(&journal("stdout", 4, 3))
    });
    assert_eq!(result.state, LogSourceState::Partial);
    assert_eq!(result.events.len(), 1);
}

#[test]
fn old_dmesg_uses_raw_format_and_kernel_facility() {
    let mut count = 0;
    let result = query_with(&query(), &AtomicBool::new(false), |_, args| {
        count += 1;
        match count {
            1 => Capture {
                stop: Some(LogSourceState::Unavailable),
                ..Default::default()
            },
            2 => Capture {
                stderr: b"unrecognized option '--json'".to_vec(),
                ..Default::default()
            },
            _ => {
                assert!(args.iter().any(|arg| arg == "--raw"));
                good("<3>[ 12.450000] device error\n<14>[ 12.46] userspace injected")
            }
        }
    });
    assert_eq!(count, 3);
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].level, LogLevel::Error);
    assert_eq!(result.damaged_records, 1);
}

#[test]
fn dmesg_permission_and_cancellation_remain_explicit() {
    for expected in [LogSourceState::PermissionDenied, LogSourceState::Cancelled] {
        let result = query_with(&query(), &AtomicBool::new(false), |program, _| Capture {
            stop: Some(if program == "journalctl" {
                LogSourceState::Unavailable
            } else {
                expected
            }),
            ..Default::default()
        });
        assert_eq!(result.state, expected);
    }
}

#[test]
fn dmesg_wallclock_filters_do_not_match_capture_time() {
    let result = parse_dmesg(
        br#"{"dmesg":[{"pri":4,"time":"12.345","msg":"device failed"}]}"#,
        &LogQuery {
            since: Some(Utc::now() - chrono::Duration::days(1)),
            ..query()
        },
        false,
    );
    assert!(result.events.is_empty());
    assert_eq!(result.state, LogSourceState::Partial);
    assert!(result.notices.iter().any(|n| n.contains("boot-relative")));
}

#[test]
fn dmesg_invalid_json_or_monotonic_time_is_reported() {
    for data in [r#"{"dmesg":[{"pri":4,"time":"NaN","msg":"bad"}]}"#, "{"] {
        let result = parse_dmesg(data.as_bytes(), &query(), false);
        assert!(result.events.is_empty());
        assert_eq!(result.state, LogSourceState::Partial);
        assert_eq!(result.damaged_records, 1);
    }
}

#[test]
fn ux_correlation_does_not_fabricate_linux_matches_or_spawn() {
    let query = LogQuery {
        run_id: Some("ux-run".into()),
        ..query()
    };
    let result = query_with(&query, &AtomicBool::new(false), |_, _| {
        panic!("must not spawn")
    });
    assert!(result.events.is_empty());
    assert_eq!(result.state, LogSourceState::Ready);
}

#[test]
fn query_arguments_are_separate_and_bounded() {
    query_with(
        &LogQuery {
            min_level: Some(LogLevel::Error),
            module: Some("linux.kernel".into()),
            limit: usize::MAX,
            ..query()
        },
        &AtomicBool::new(false),
        |program, args| {
            assert_eq!(program, "journalctl");
            assert!(args.iter().any(|a| a == "--lines=10001"));
            assert!(args.iter().any(|a| a == "--priority=0..3"));
            assert!(args.iter().any(|a| a == "_TRANSPORT=kernel"));
            good("")
        },
    );
}

#[test]
fn service_log_filters_reach_journal_before_its_limit_and_never_use_dmesg() {
    for scope in ["system", "user"] {
        let query = LogQuery {
            systemd_unit: Some("example.service".into()),
            systemd_scope: Some(scope.into()),
            ..query()
        };
        let mut calls = 0;
        let result = query_with(&query, &AtomicBool::new(false), |program, args| {
            calls += 1;
            assert_eq!(program, "journalctl");
            let prefix = if scope == "system" {
                "--unit"
            } else {
                "--user-unit"
            };
            assert!(args.contains(&format!("{prefix}=example.service")));
            if scope == "user" {
                assert!(args.contains(&format!("_UID={}", current_uid())));
            }
            Capture {
                stop: Some(LogSourceState::Unavailable),
                ..Default::default()
            }
        });
        assert_eq!(calls, 1);
        assert_eq!(result.state, LogSourceState::Unavailable);
        assert!(result.events.is_empty());
        assert!(
            result
                .notices
                .iter()
                .any(|notice| notice.contains("dmesg cannot"))
        );
    }
}

#[test]
fn invalid_service_log_names_and_scopes_do_not_spawn() {
    for (unit, scope) in [
        ("*.service", "system"),
        ("../test.service", "system"),
        ("test.service", "other"),
        ("test.service;id", "user"),
    ] {
        let result = query_with(
            &LogQuery {
                systemd_unit: Some(unit.into()),
                systemd_scope: Some(scope.into()),
                ..query()
            },
            &AtomicBool::new(false),
            |_, _| panic!("invalid filter must not spawn"),
        );
        assert_eq!(result.state, LogSourceState::Unavailable);
    }
}

#[test]
fn exact_service_parser_keeps_only_selected_unit_and_current_user() {
    let system_query = LogQuery {
        systemd_unit: Some("example.service".into()),
        systemd_scope: Some("system".into()),
        ..query()
    };
    let good_record = journal("stdout", 6, 1_000_000);
    let other_record = good_record.replace("example.service", "other.service");
    let result = parse_journal(
        format!("{good_record}\n{other_record}").as_bytes(),
        &system_query,
    );
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.damaged_records, 0);
    let user_query = LogQuery {
        systemd_scope: Some("user".into()),
        ..system_query
    };
    let mut record: Value = serde_json::from_str(&good_record).unwrap();
    record["_SYSTEMD_USER_UNIT"] = Value::String("example.service".into());
    record["_UID"] = Value::String(current_uid().to_string());
    let matching = serde_json::to_string(&record).unwrap();
    record["_UID"] = Value::String(current_uid().wrapping_add(1).to_string());
    let other_user = serde_json::to_string(&record).unwrap();
    let result = parse_journal(
        format!("{matching}\n{other_user}\n{other_record}").as_bytes(),
        &user_query,
    );
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.damaged_records, 0);
}

#[cfg(unix)]
#[test]
fn capture_timeout_and_cancellation_stop_hanging_children() {
    use std::time::{Duration, Instant};
    // The shell is a fixture only; production programs never run through it.
    let start = Instant::now();
    let cancelled = AtomicBool::new(false);
    let output = capture(
        "/bin/sh",
        &["-c".into(), "exec sleep 10".into()],
        &cancelled,
        start + Duration::from_millis(80),
        4096,
    );
    assert_eq!(output.stop, Some(LogSourceState::Partial));
    assert!(start.elapsed() < Duration::from_secs(2));
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(40));
            cancelled.store(true, Ordering::Relaxed);
        });
        let output = capture(
            "/bin/sh",
            &["-c".into(), "exec sleep 10".into()],
            &cancelled,
            Instant::now() + Duration::from_secs(2),
            4096,
        );
        assert_eq!(output.stop, Some(LogSourceState::Cancelled));
    });
}

#[cfg(unix)]
#[test]
fn capture_drains_both_pipes_and_bounds_noisy_output() {
    use std::time::{Duration, Instant};
    let cancelled = AtomicBool::new(false);
    let output = capture(
        "/bin/sh",
        &["-c".into(), "printf 'out'; printf 'err' >&2".into()],
        &cancelled,
        Instant::now() + Duration::from_secs(1),
        4096,
    );
    assert!(output.success);
    assert_eq!(output.stdout, b"out");
    assert_eq!(output.stderr, b"err");
    let output = capture(
        "/bin/sh",
        &[
            "-c".into(),
            "while :; do printf '0123456789'; printf '9876543210' >&2; done".into(),
        ],
        &cancelled,
        Instant::now() + Duration::from_secs(1),
        4096,
    );
    assert!(output.limited);
    assert_eq!(output.stdout.len() + output.stderr.len(), 4096);
    assert_eq!(output.stop, Some(LogSourceState::Partial));
}

#[test]
fn dmesg_returns_newest_matches_before_applying_limit() {
    let result = parse_dmesg(br#"{"dmesg":[{"pri":3,"time":1.0,"msg":"first"},{"pri":4,"time":2.0,"msg":"second"},{"pri":3,"time":3.0,"msg":"third"}]}"#, &LogQuery { limit: 2, ..query() }, false);
    assert_eq!(
        result
            .events
            .iter()
            .map(|event| event.message.as_str())
            .collect::<Vec<_>>(),
        ["third", "second"]
    );
    assert!(result.truncated);
}
