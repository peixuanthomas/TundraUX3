use super::*;
use ui::KeyModifiers as InputModifiers;
fn job(policy: storage::AutoAdminPolicy) -> (AutoAdminJob, mpsc::Receiver<OperationInput>) {
    let (tx, rx) = mpsc::channel();
    (
        AutoAdminJob::new("Remove demo package".into(), policy, tx),
        rx,
    )
}

#[test]
fn structured_questions_edit_at_the_cursor_and_keep_passwords_masked() {
    for secret in [false, true] {
        let (job, responses) = job(storage::AutoAdminPolicy::Automatic);
        job.emit(&OperationEvent::Question {
            id: "answer".into(),
            prompt: "Input".into(),
            choices: vec![],
            secret,
        });
        job.paste("甲乙");
        job.key(&KeyInput::new(InputKey::Left));
        job.paste("文");
        let model = job.view_model(false, None, 0);
        assert_eq!(
            model.input.as_deref(),
            Some(if secret { "> •••" } else { "> 甲文乙" })
        );
        assert_eq!(
            model
                .input_editing
                .input("answer", if secret { "•••" } else { "甲文乙" })
                .cursor(),
            2
        );
        job.key(&KeyInput::new(InputKey::Enter));
        assert!(
            matches!(responses.recv_timeout(Duration::from_secs(1)).unwrap(),
            OperationInput::Answer { value, .. } if value == "甲文乙")
        );
    }
}

#[test]
fn history_keeps_output_and_result_without_secret_input_or_snapshot_contents() {
    use platform::task_history::{HistoryStore, TaskRecorder, TaskStatus, TaskSummary};
    let root =
        platform::create_temp_dir(&std::env::temp_dir().join("TundraUX3"), "aa-history-test")
            .unwrap();
    let store = HistoryStore::new(&root);
    let recorder = TaskRecorder::new(
        store.clone(),
        TaskSummary::new("task".into(), "auto-admin".into(), "services".into()),
    )
    .unwrap();
    let id = recorder.id().to_owned();
    let (job, _) = job(storage::AutoAdminPolicy::Automatic);
    job.record_history(recorder, "services".into());
    job.emit(&OperationEvent::Question {
        id: "password".into(),
        prompt: "Secret prompt".into(),
        choices: vec![],
        secret: true,
    });
    job.paste("private-password");
    job.emit(&OperationEvent::Snapshot {
        snapshot: platform::management::ManagementSnapshot {
            notices: vec!["protected configuration contents".into()],
            ..Default::default()
        },
    });
    job.emit(&OperationEvent::Output {
        text: "safe output".into(),
    });
    job.finish(Ok("done".into()));
    let (bytes, _) = store.output(&id, 0, 4096).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("safe output"));
    assert!(!text.contains("private-password"));
    assert!(!text.contains("protected configuration"));
    assert!(!text.contains("Secret prompt"));
    assert_eq!(store.read(&id).unwrap().status, TaskStatus::Succeeded);
    drop(job);
    platform::cleanup_temp_path(&root).unwrap();
}

#[test]
fn approval_is_required_before_worker_runs_and_denial_never_runs_it() {
    let (manual, _) = job(storage::AutoAdminPolicy::Manual);
    assert_eq!(manual.phase(), WAITING);
    manual.key(&KeyInput::new(InputKey::Char('y')));
    assert_eq!(manual.phase(), WAITING);
    manual.decide(false);
    assert!(manual.wait_for_approval().is_err());
    manual.decide(true);
    assert!(manual.wait_for_approval().is_err());
    let (automatic, _) = job(storage::AutoAdminPolicy::Automatic);
    assert!(automatic.wait_for_approval().is_ok());
    let (denied, _) = job(storage::AutoAdminPolicy::Deny);
    assert!(denied.wait_for_approval().is_err());
}

#[test]
fn worker_waits_for_explicit_approval_and_runs_once() {
    let (job, _inputs) = job(storage::AutoAdminPolicy::Manual);
    let worker_job = job.clone();
    let (done, result) = mpsc::channel();
    let fixture = crate::tasks::tests::TaskFixture::new();
    let worker = spawn_task(
        &fixture.group,
        TaskId::from_static("approval"),
        Arc::new(i18n::LanguageSnapshot::embedded(0)),
        Some(&job),
        move || {
            let result = worker_job.run_approved(std::convert::identity, || {
                done.send("executed").unwrap();
                Ok(())
            });
            worker_job.finish_result(&result, |()| "Done".into());
        },
    )
    .unwrap();
    assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
    job.decide(true);
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        "executed"
    );
    job.decide(true);
    worker.join().unwrap();
    assert!(result.try_recv().is_err());
    assert_eq!(job.phase(), FINISHED);
    assert_eq!(job.0.display.lock().unwrap().status, "Done");
}

#[test]
fn terminal_accepts_yes_no_navigation_paste_and_enter_without_answer_translation() {
    let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
    for key in [
        InputKey::Char('y'),
        InputKey::Enter,
        InputKey::Up,
        InputKey::Escape,
    ] {
        job.key(&KeyInput::new(key));
    }
    job.paste("中文");
    let bytes = rx
        .try_iter()
        .flat_map(|input| match input {
            OperationInput::Terminal { bytes } => bytes,
            _ => panic!("unexpected input"),
        })
        .collect::<Vec<_>>();
    assert_eq!(bytes, b"y\r\x1b[A\x1b\xe4\xb8\xad\xe6\x96\x87");
    job.key(&KeyInput::with_phase(
        InputKey::Char('n'),
        InputModifiers::NONE,
        InputPhase::Release,
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn password_is_never_added_to_terminal_output_or_debug_text() {
    let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
    job.emit(&OperationEvent::Question {
        id: "sudo-password".into(),
        prompt: "Password:".into(),
        choices: vec![],
        secret: true,
    });
    job.paste("test-secret");
    assert!(job.view_model(false, None, 0).input_sensitive);
    assert!(
        !job.0
            .display
            .lock()
            .unwrap()
            .parser
            .screen()
            .contents()
            .contains("test-secret")
    );
    assert!(!format!("{job:?}").contains("test-secret"));
    job.key(&KeyInput::new(InputKey::Enter));
    assert!(
        matches!(rx.try_recv().unwrap(), OperationInput::Answer { id, value } if id == "sudo-password" && value == "test-secret")
    );
    assert!(job.0.display.lock().unwrap().question.is_none());
    assert!(!job.view_model(false, None, 0).input_sensitive);
}
