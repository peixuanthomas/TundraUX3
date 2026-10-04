use super::*;

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
    let worker = std::thread::spawn(move || {
        if worker_job.wait_for_approval().is_ok() {
            done.send("executed").unwrap();
        }
    });
    assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
    job.decide(true);
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        "executed"
    );
    job.decide(true);
    worker.join().unwrap();
    assert!(result.try_recv().is_err());
}

#[test]
fn held_approval_enter_does_not_answer_the_childs_next_question() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (job, rx) = job(storage::AutoAdminPolicy::Manual);
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        approve_selected: true,
        ..Default::default()
    };
    state.apply_input(InputEvent::Key(KeyInput::new(InputKey::Enter)));
    assert_eq!(job.phase(), RUNNING);
    state.apply_input(InputEvent::Key(KeyInput::new(InputKey::Enter).repeated()));
    assert!(rx.try_recv().is_err());
    state.apply_input(InputEvent::Key(KeyInput::new(InputKey::Char('n'))));
    state.apply_input(InputEvent::Key(KeyInput::new(InputKey::Enter)));
    assert_eq!(rx.try_iter().count(), 2);
}

#[test]
fn approval_click_requires_its_own_matching_release_and_resize_cancels_it() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (job, _rx) = job(storage::AutoAdminPolicy::Manual);
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    let area = ui::auto_admin_layout(Rect::new(0, 0, 120, 40), true).buttons[0];
    let point = (area.x, area.y);
    state.handle_auto_admin_input(&InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(job.phase(), WAITING);
    state.handle_auto_admin_input(&InputEvent::mouse_down(PointerButton::Left, point));
    state.handle_auto_admin_input(&InputEvent::Resize {
        width: 120,
        height: 40,
    });
    state.handle_auto_admin_input(&InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(job.phase(), WAITING);
    state.handle_auto_admin_input(&InputEvent::mouse_down(PointerButton::Left, point));
    state.handle_auto_admin_input(&InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(job.phase(), RUNNING);
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

#[test]
fn modal_captures_ctrl_c_escape_and_page_shortcuts_and_reopens_with_f12() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
    state.auto_admin = AutoAdminState {
        job: Some(job),
        visible: true,
        ..Default::default()
    };
    let old_screen = state.active_screen();
    for input in [
        InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('c'),
            InputModifiers::new(true, false, false),
        )),
        InputEvent::Key(KeyInput::new(InputKey::Escape)),
        InputEvent::Key(KeyInput::new(InputKey::Char('r'))),
    ] {
        assert_eq!(state.apply_input(input), ShellAction::Redraw);
    }
    assert_eq!(state.active_screen(), old_screen);
    assert!(state.last_key_event.is_none());
    assert_eq!(rx.try_iter().count(), 3);
    state.apply_input(InputEvent::Key(KeyInput::new(InputKey::F(12))));
    assert!(!state.auto_admin_visible());
    assert!(state.auto_admin_running());
    state.apply_input(InputEvent::Key(KeyInput::new(InputKey::F(12))));
    assert!(state.auto_admin_visible());
}

#[test]
fn finished_terminal_keeps_keyboard_scrollback_available() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (80, 24),
        ShellHomeMode::User,
    );
    let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
    for index in 0..60 {
        job.emit(&OperationEvent::Output {
            text: format!("Output line {index}"),
        });
    }
    job.finish(Ok("Done".into()));
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    state.apply_input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::PageUp,
        InputModifiers::SHIFT,
    )));
    assert!(job.0.display.lock().unwrap().parser.screen().scrollback() > 0);
    state.apply_input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::PageDown,
        InputModifiers::SHIFT,
    )));
    assert_eq!(
        job.0.display.lock().unwrap().parser.screen().scrollback(),
        0
    );
    assert!(rx.try_recv().is_err());
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(!state.auto_admin_visible());
}

#[test]
fn hiding_pending_approval_denies_it_and_reopening_cannot_approve_it() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (80, 24),
        ShellHomeMode::User,
    );
    let (tx, _rx) = mpsc::channel();
    let job = state
        .begin_auto_admin("Remove demo".into(), true, tx)
        .unwrap();
    assert_eq!(job.phase(), WAITING);
    assert!(!state.auto_admin.approve_selected);
    let (tx, _rx) = mpsc::channel();
    assert!(
        state
            .begin_auto_admin("Remove other".into(), true, tx)
            .is_none()
    );
    assert_eq!(state.auto_admin.job.as_ref().unwrap(), &job);
    state.apply_input(InputEvent::from_key_label("F12"));
    assert!(job.wait_for_approval().is_err());
    state.apply_input(InputEvent::from_key_label("F12"));
    state.apply_input(InputEvent::from_key_label("Right"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(job.phase(), DENIED);
}

#[test]
fn requests_read_the_saved_policy_and_fail_closed_when_config_cannot_be_read() {
    let root = std::env::temp_dir().join(format!(
        "tundra-auto-admin-policy-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let paths = platform::build_windows_app_paths(
        root.join("roaming"),
        root.join("local"),
        root.join("temp"),
    )
    .unwrap();
    let storage = StorageManager::open(paths).unwrap().manager;
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    state.storage_manager = Some(storage.clone());
    for (policy, expected) in [
        (storage::AutoAdminPolicy::Manual, WAITING),
        (storage::AutoAdminPolicy::Automatic, RUNNING),
        (storage::AutoAdminPolicy::Deny, DENIED),
    ] {
        let mut config = storage.load_config().unwrap();
        config.auto_admin = policy;
        storage.save_config(&config).unwrap();
        let (tx, _rx) = mpsc::channel();
        let job = state
            .begin_auto_admin("System operation".into(), true, tx)
            .unwrap();
        assert_eq!(job.phase(), expected);
        state.stop_auto_admin();
    }
    std::fs::remove_file(&storage.layout().config_path).unwrap();
    let (tx, _rx) = mpsc::channel();
    let job = state
        .begin_auto_admin("System operation".into(), true, tx)
        .unwrap();
    assert_eq!(job.phase(), DENIED);
    assert!(job.wait_for_approval().is_err());
    platform::cleanup_temp_path(&root).unwrap();
}
