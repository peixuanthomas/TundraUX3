use super::*;

#[test]
fn stop_requests_bypass_queued_terminal_input_without_losing_other_packets() {
    let mut input = b"{\"request\":\"terminal\",\"bytes\":[120]}\n{\"request\":\"terminate\"}\n{\"request\":\"kill\"}\n{\"request\":\"resize\"".to_vec();
    assert_eq!(take_stop_request(&mut input), Some(false));
    assert_eq!(take_stop_request(&mut input), Some(true));
    assert_eq!(take_stop_request(&mut input), None);
    assert_eq!(
        input,
        b"{\"request\":\"terminal\",\"bytes\":[120]}\n{\"request\":\"resize\""
    );
}

#[test]
#[ignore = "subprocess fixture used only by helper_stop_reaches_a_blocked_worker"]
fn helper_stop_process_fixture() {
    let Some(directory) = std::env::var_os("TUNDRA_AA_STOP_FIXTURE") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let listener = UnixListener::bind(directory.join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    serve_operation(
        listener,
        OperationEvent::Started {
            kind: ManagementKind::Packages,
            action: "test-sleep".into(),
            target: None,
        },
        &directory,
        |io, _| {
            let mut child = std::process::Command::new("/bin/sh")
                .args(["-c", "trap '' TERM; echo ready; exec sleep 30"])
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let mut line = String::new();
            std::io::BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            io.emit(OperationEvent::Output { text: line });
            // Intentionally block without reading OperationInput. The independent
            // helper control loop must still deliver TERM, and later KILL.
            let status = child.wait().unwrap();
            Err(ManagementError::Failed(format!("Fixture exited: {status}")))
        },
    )
    .unwrap();
}

#[test]
fn helper_stop_reaches_a_blocked_worker() {
    use std::process::{Command, Stdio};
    let directory = std::env::temp_dir().join(format!(
        "aa-stop-helper-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "management::helper::tests::helper_stop_process_fixture",
            "--nocapture",
        ])
        .env("TUNDRA_AA_STOP_FIXTURE", &directory)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            if let Ok(stream) = UnixStream::connect(directory.join("control.sock")) {
                break stream;
            }
            assert!(
                Instant::now() < deadline,
                "helper did not create the control socket"
            );
            assert!(child.try_wait().unwrap().is_none(), "fixture exited early");
            std::thread::sleep(Duration::from_millis(10));
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            let record: OperationRecord = serde_json::from_str(&line).unwrap();
            if matches!(record.event, OperationEvent::Output { ref text } if text.trim() == "ready")
            {
                break;
            }
        }
        // KILL alone is rejected even over the authorized channel.
        stream.write_all(b"{\"request\":\"kill\"}\n").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(child.try_wait().unwrap().is_none());
        stream.write_all(b"{\"request\":\"terminate\"}\n").unwrap();
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            child.try_wait().unwrap().is_none(),
            "TERM must not escalate automatically"
        );
        stream.write_all(b"{\"request\":\"kill\"}\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                use std::os::unix::process::ExitStatusExt;
                assert_eq!(status.signal(), Some(libc::SIGKILL));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "force-stop never reached the blocked worker"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }));
    if child.try_wait().unwrap().is_none() {
        if let Ok(mut tree) = super::super::termination::ProcessTree::child(child.id()) {
            let _ = tree.signal(true);
        }
        let _ = child.kill();
    }
    let _ = child.wait();
    fs::remove_dir_all(directory).unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[test]
fn elevated_reconnect_is_recognized_without_an_operation_command() {
    assert!(requires_authorized_attach(std::path::Path::new(
        "/run/tundraux3-management/1000/operation/control.sock"
    )));
    assert!(!requires_authorized_attach(std::path::Path::new(
        "/tmp/tundraux3-management-1000/operation/control.sock"
    )));
    assert!(!requires_authorized_attach(std::path::Path::new(
        "/run/tundraux3-management-other/1000/operation/control.sock"
    )));
}

#[test]
fn attach_rejects_paths_outside_the_actors_fixed_socket_namespace() {
    for path in [
        "/tmp/operation/control.sock",
        "/run/tundraux3-management/1001/operation/control.sock",
        "/run/tundraux3-management/1000/../1001/control.sock",
        "/run/tundraux3-management/1000/operation/private/control.sock",
        "/run/tundraux3-management/1000/operation/other.sock",
    ] {
        assert!(check_attach_path(std::path::Path::new(path), 1000).is_err());
    }
}

#[test]
fn reconnect_metadata_contains_only_kind_and_rejects_untrusted_files() {
    let directory = std::env::temp_dir().join(format!(
        "tundra-operation-metadata-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let socket = directory.join("control.sock");
    write_operation_metadata(&directory, ManagementKind::Packages).unwrap();
    let path = directory.join("metadata.json");
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"kind":"packages"}"#);
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o644);
    assert_eq!(operation_kind(&socket), Some(ManagementKind::Packages));
    assert!(write_operation_metadata(&directory, ManagementKind::Network).is_err());
    assert_eq!(operation_kind(&directory.join("other.sock")), None);

    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(operation_kind(&socket), None);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(operation_kind(&socket), None);
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_file(&path).unwrap();
    let other = directory.join("other.json");
    fs::write(&other, r#"{"kind":"network"}"#).unwrap();
    std::os::unix::fs::symlink("other.json", &path).unwrap();
    assert_eq!(operation_kind(&socket), None);
    fs::remove_file(&path).unwrap();
    fs::remove_file(&other).unwrap();
    fs::remove_dir(&directory).unwrap();
}

fn question(id: &str) -> OperationEvent {
    OperationEvent::Question {
        id: id.into(),
        prompt: "Choose a response".into(),
        choices: vec!["Keep".into(), "Restore".into()],
        secret: false,
    }
}

#[test]
fn historical_questions_are_not_replayed_and_current_question_is_sent_once_with_a_new_sequence() {
    let mut replay = VecDeque::new();
    let mut count = 0;
    let mut pending = None;
    let mut sequence = 0;
    record_event(
        OperationEvent::Output {
            text: "started".into(),
        },
        &mut sequence,
        &mut replay,
        &mut count,
        &mut pending,
    )
    .unwrap();
    record_event(
        question("first"),
        &mut sequence,
        &mut replay,
        &mut count,
        &mut pending,
    )
    .unwrap();
    pending = None; // The first question was answered.
    record_event(
        question("current"),
        &mut sequence,
        &mut replay,
        &mut count,
        &mut pending,
    )
    .unwrap();
    let before = sequence;
    let outgoing = reconnect_outgoing(&replay, &pending, &mut sequence, None).unwrap();
    let records = outgoing
        .iter()
        .map(|bytes| serde_json::from_slice::<OperationRecord>(bytes).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert!(matches!(&records[0].event, OperationEvent::Output { text } if text == "started"));
    assert!(matches!(&records[1].event, OperationEvent::Question { id, .. } if id == "current"));
    assert!(records[1].sequence > before);
    record_event(
        OperationEvent::Completed {
            message: "done".into(),
        },
        &mut sequence,
        &mut replay,
        &mut count,
        &mut pending,
    )
    .unwrap();
    assert!(pending.is_none());
    assert!(
        reconnect_outgoing(&replay, &pending, &mut sequence, None)
            .unwrap()
            .iter()
            .all(|bytes| !matches!(
                serde_json::from_slice::<OperationRecord>(bytes)
                    .unwrap()
                    .event,
                OperationEvent::Question { .. }
            ))
    );
}

#[test]
fn task_metadata_survives_output_history_truncation() {
    let started = encode_record(
        1,
        OperationEvent::Started {
            kind: ManagementKind::Network,
            action: "configure".into(),
            target: Some("eth0".into()),
        },
    )
    .unwrap();
    let mut replay = VecDeque::new();
    let mut total = 0;
    append_replay(&mut replay, &mut total, vec![b'x'; MAX_REPLAY + 1]);
    assert!(replay.is_empty());
    let mut sequence = 2;
    let reconnect = reconnect_outgoing(&replay, &None, &mut sequence, Some(&started)).unwrap();
    let record: OperationRecord = serde_json::from_slice(&reconnect[0]).unwrap();
    assert_eq!(record.sequence, 1);
    assert!(
        matches!(record.event, OperationEvent::Started { kind: ManagementKind::Network, action, target } if action == "configure" && target.as_deref() == Some("eth0"))
    );
}

#[test]
fn incorrect_and_queued_answers_preserve_or_clear_only_the_matching_question() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let cancelled = AtomicBool::new(false);
    let mut pending = Some(PendingQuestion {
        id: "current".into(),
        event: question("current"),
    });
    assert!(deliver_input(
        OperationInput::Answer {
            id: "old".into(),
            value: "Keep".into()
        },
        &sender,
        &cancelled,
        &mut pending
    ));
    assert!(pending.is_some());
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    sender
        .send(OperationInput::Resize {
            columns: 80,
            rows: 24,
        })
        .unwrap();
    assert!(!deliver_input(
        OperationInput::Answer {
            id: "current".into(),
            value: "Keep".into()
        },
        &sender,
        &cancelled,
        &mut pending
    ));
    assert!(pending.is_some());
    receiver.recv().unwrap();
    assert!(deliver_input(
        OperationInput::Answer {
            id: "current".into(),
            value: "Keep".into()
        },
        &sender,
        &cancelled,
        &mut pending
    ));
    assert!(pending.is_none());
    assert!(
        matches!(receiver.recv().unwrap(), OperationInput::Answer { id, value } if id == "current" && value == "Keep")
    );
}

#[test]
fn cancellation_is_immediate_even_when_the_interaction_queue_is_full() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    sender
        .send(OperationInput::Resize {
            columns: 80,
            rows: 24,
        })
        .unwrap();
    let cancelled = AtomicBool::new(false);
    assert!(deliver_input(
        OperationInput::Cancel,
        &sender,
        &cancelled,
        &mut None
    ));
    assert!(cancelled.load(Ordering::Relaxed));
}

#[test]
fn only_network_changes_stage_a_stable_recovery_binary() {
    let mut command = ManagementCommand {
        kind: ManagementKind::Network,
        action: "configure".into(),
        target: None,
        values: Default::default(),
        identity: Default::default(),
    };
    assert!(needs_stable_recovery_binary(&command));
    for action in ["wifi-connect", "wifi-disconnect", "wifi-forget"] {
        command.action = action.into();
        assert!(needs_stable_recovery_binary(&command));
    }
    for action in ["check", "inspect_network", "forget_saved_wifi"] {
        command.action = action.into();
        assert!(!needs_stable_recovery_binary(&command));
    }
    command.action = "configure".into();
    for kind in [
        ManagementKind::Services,
        ManagementKind::Processes,
        ManagementKind::Packages,
        ManagementKind::Disks,
    ] {
        command.kind = kind;
        assert!(!needs_stable_recovery_binary(&command));
    }
}
