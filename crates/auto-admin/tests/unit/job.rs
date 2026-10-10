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
}
