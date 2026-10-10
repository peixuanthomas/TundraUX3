use super::*;

#[path = "stop_tests.rs"]
mod stop_tests;

fn job(policy: storage::AutoAdminPolicy) -> (AutoAdminJob, mpsc::Receiver<OperationInput>) {
    let (tx, rx) = mpsc::channel();
    (
        AutoAdminJob::new("Remove demo package".into(), policy, tx),
        rx,
    )
}

#[test]
fn a_new_confirmation_focuses_approve_and_keyboard_denial_still_cancels() {
    for deny_key in [
        InputKey::Tab,
        InputKey::BackTab,
        InputKey::Right,
        InputKey::Left,
        InputKey::Escape,
    ] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        let (tx, rx) = mpsc::channel();
        let job = state
            .begin_auto_admin("Enable demo".into(), true, tx)
            .unwrap();
        assert!(state.auto_admin_view().unwrap().approve_selected);
        state.apply_input(InputEvent::key(deny_key.clone()));
        if deny_key != InputKey::Escape {
            assert!(!state.auto_admin_view().unwrap().approve_selected);
            state.apply_input(InputEvent::key(InputKey::Enter));
        }
        assert_eq!(job.phase(), DENIED);
        assert!(job.wait_for_approval().is_err());
        assert!(
            rx.try_iter()
                .all(|input| matches!(input, OperationInput::Resize { .. }))
        );
        assert!(state.auto_admin_view().is_none());
    }
}

#[test]
fn shared_back_cancels_aa_confirmation_on_release_without_leaving_page() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.enter_screen(ShellScreen::Launcher);
    state.enter_screen(ShellScreen::UserManagement);
    let (tx, rx) = mpsc::channel();
    let job = state
        .begin_auto_admin("Enable demo".into(), true, tx)
        .unwrap();
    state.refresh_hit_map();
    let area = state.frame_layout.unwrap().back_button.unwrap();
    let point = (area.x, area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(state.auto_admin_visible());
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, (0, 10)));
    assert!(state.auto_admin_visible());
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert_eq!(job.phase(), WAITING);
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert!(!state.auto_admin_visible());
    assert_eq!(job.phase(), DENIED);
    assert!(job.wait_for_approval().is_err());
    assert!(
        rx.try_iter()
            .all(|input| matches!(input, OperationInput::Resize { .. }))
    );
    assert_eq!(
        state.screen_stack(),
        &[
            ShellScreen::Home,
            ShellScreen::Launcher,
            ShellScreen::UserManagement
        ]
    );
}

#[test]
fn deny_click_closes_confirmation_without_running_the_operation() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (job, rx) = job(storage::AutoAdminPolicy::Manual);
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    let area = ui::auto_admin_layout(Rect::new(0, 0, 120, 40), &state.auto_admin_view().unwrap())
        .buttons[1];
    let point = (area.x, area.y);
    state.handle_auto_admin_input(&InputEvent::mouse_down(PointerButton::Left, point));
    assert!(state.auto_admin_visible());
    assert_eq!(job.phase(), WAITING);
    state.handle_auto_admin_input(&InputEvent::mouse_up(PointerButton::Left, point));
    assert!(state.auto_admin_view().is_none());
    assert_eq!(job.phase(), DENIED);
    assert!(job.wait_for_approval().is_err());
    assert!(
        rx.try_iter()
            .all(|input| matches!(input, OperationInput::Resize { .. }))
    );
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
    let group = default_editor_watchdog()
        .unwrap()
        .task_group("auto-admin-approval-test");
    let worker = spawn_task(
        &group,
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
fn legacy_terminal_enter_burst_after_approval_does_not_reach_the_child() {
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
    let start = Instant::now();
    // Ordinary VT terminals report every CR/LF, including repeats, as Press.
    for millis in [0, 1, 50, 500, 550, 1000] {
        let input = crossterm_event_to_input(crossterm::event::Event::Key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
        ));
        state.apply_input_at(input, start + Duration::from_millis(millis));
    }
    assert_eq!(job.phase(), RUNNING);
    assert!(
        rx.try_recv().is_err(),
        "approval Enter must not become terminal input"
    );
    state.apply_input_at(
        InputEvent::key(InputKey::Char('y')),
        start + Duration::from_millis(1001),
    );
    state.apply_input_at(
        InputEvent::key(InputKey::Enter),
        start + Duration::from_millis(1002),
    );
    let bytes = rx
        .try_iter()
        .flat_map(|input| match input {
            OperationInput::Terminal { bytes } => bytes,
            _ => panic!("unexpected input"),
        })
        .collect::<Vec<_>>();
    assert_eq!(bytes, b"y\r");
}

#[test]
fn deliberate_enter_after_release_or_a_quiet_interval_reaches_the_terminal() {
    for with_release in [false, true] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        let (job, rx) = job(storage::AutoAdminPolicy::Manual);
        state.auto_admin = AutoAdminState {
            job: Some(job),
            visible: true,
            approve_selected: true,
            ..Default::default()
        };
        let at = Instant::now();
        state.apply_input_at(InputEvent::key(InputKey::Enter), at);
        let next = if with_release {
            state.apply_input_at(
                InputEvent::Key(KeyInput::with_phase(
                    InputKey::Enter,
                    InputModifiers::NONE,
                    InputPhase::Release,
                )),
                at + Duration::from_millis(1),
            );
            at + Duration::from_millis(2)
        } else {
            at + Duration::from_secs(1)
        };
        state.apply_input_at(InputEvent::key(InputKey::Enter), next);
        assert!(
            matches!(rx.try_recv().unwrap(), OperationInput::Terminal { bytes } if bytes == b"\r")
        );
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn question_submission_enter_cannot_answer_the_next_question_or_enter_the_terminal() {
    for next_question in [false, true] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
        state.auto_admin = AutoAdminState {
            job: Some(job.clone()),
            visible: true,
            ..Default::default()
        };
        let question = OperationEvent::Question {
            id: "password".into(),
            prompt: "Password:".into(),
            choices: vec![],
            secret: true,
        };
        job.emit(&question);
        state.apply_input(InputEvent::Paste("test-secret".into()));
        let at = Instant::now();
        state.apply_input_at(InputEvent::key(InputKey::Enter), at);
        assert!(
            matches!(rx.try_recv().unwrap(), OperationInput::Answer { value, .. } if value == "test-secret")
        );
        if next_question {
            job.emit(&question);
        }
        state.apply_input_at(
            InputEvent::key(InputKey::Enter),
            at + Duration::from_millis(20),
        );
        state.apply_input_at(
            InputEvent::Key(KeyInput::new(InputKey::Enter).repeated()),
            at + Duration::from_millis(50),
        );
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn confirmation_navigation_and_space_activation_stay_out_of_terminal() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (job, rx) = job(storage::AutoAdminPolicy::Manual);
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    for (key, selected) in [
        (InputKey::Tab, true),
        (InputKey::BackTab, false),
        (InputKey::Right, true),
    ] {
        state.apply_input(InputEvent::key(key));
        assert_eq!(state.auto_admin.approve_selected, selected);
    }
    let at = Instant::now();
    state.apply_input_at(InputEvent::key(InputKey::Space), at);
    state.apply_input_at(
        InputEvent::key(InputKey::Space),
        at + Duration::from_millis(20),
    );
    assert_eq!(job.phase(), RUNNING);
    assert!(rx.try_recv().is_err());
    assert_eq!(state.auto_admin.button_focus, None);
}

#[test]
fn running_buttons_are_keyboard_accessible_without_stealing_terminal_tab() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    state.apply_input(InputEvent::key(InputKey::Tab));
    assert!(matches!(rx.try_recv().unwrap(), OperationInput::Terminal { bytes } if bytes == b"\t"));
    state.apply_input(InputEvent::key(InputKey::F(6)));
    assert_eq!(state.auto_admin_view().unwrap().button_focus, Some(0));
    state.apply_input(InputEvent::key(InputKey::Enter));
    state.apply_input(InputEvent::key(InputKey::Tab));
    state.apply_input(InputEvent::key(InputKey::Space));
    state.apply_input(InputEvent::key(InputKey::Right));
    state.apply_input(InputEvent::key(InputKey::Enter));
    let bytes = rx
        .try_iter()
        .flat_map(|input| match input {
            OperationInput::Terminal { bytes } => bytes,
            _ => panic!("unexpected input"),
        })
        .collect::<Vec<_>>();
    assert_eq!(bytes, b"yn\r");
    state.apply_input(InputEvent::key(InputKey::BackTab));
    assert_eq!(state.auto_admin.button_focus, Some(1));
    state.apply_input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Tab,
        InputModifiers::SHIFT,
    )));
    assert_eq!(state.auto_admin.button_focus, Some(0));
    state.apply_input(InputEvent::key(InputKey::Escape));
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert!(rx.try_recv().is_err());
    assert_eq!(state.auto_admin.button_focus, None);
    state.apply_input(InputEvent::key(InputKey::F(6)));
    state.apply_input(InputEvent::key(InputKey::Left));
    let last = if cfg!(target_os = "linux") { 3 } else { 2 };
    assert_eq!(state.auto_admin.button_focus, Some(last));
    state.apply_input(InputEvent::key(InputKey::Right));
    assert_eq!(state.auto_admin.button_focus, Some(0));
    state.apply_input(InputEvent::key(InputKey::BackTab));
    assert_eq!(state.auto_admin.button_focus, Some(last));
    if cfg!(target_os = "linux") {
        state.apply_input(InputEvent::key(InputKey::Left));
    }
    assert_eq!(state.auto_admin.button_focus, Some(2));
    state.apply_input(InputEvent::key(InputKey::Enter));
    assert!(state.auto_admin_visible());
    assert_eq!(job.phase(), RUNNING);
    assert!(matches!(rx.try_recv().unwrap(), OperationInput::Terminal { bytes } if bytes == b"\r"));
    assert!(rx.try_recv().is_err());
    job.finish(Ok("Done".into()));
    state.apply_input(InputEvent::key(InputKey::Space));
    assert!(!state.auto_admin_visible());
}

#[test]
fn closing_auto_admin_consumes_the_action_key_before_it_can_reach_the_page() {
    for key in [
        InputKey::Enter,
        InputKey::Space,
        InputKey::Escape,
        InputKey::F(12),
    ] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
        job.finish(Ok("Done".into()));
        state.auto_admin = AutoAdminState {
            job: Some(job),
            visible: true,
            ..Default::default()
        };
        let at = Instant::now();
        assert!(state.handle_auto_admin_input_at(&InputEvent::key(key.clone()), at));
        assert!(!state.auto_admin_visible());
        for millis in [1, 50, 500, 600] {
            assert!(state.handle_auto_admin_input_at(
                &InputEvent::key(key.clone()),
                at + Duration::from_millis(millis)
            ));
        }
        assert!(rx.try_recv().is_err());
        assert!(state.handle_auto_admin_input_at(
            &InputEvent::Key(KeyInput::with_phase(
                key.clone(),
                InputModifiers::NONE,
                InputPhase::Release,
            )),
            at + Duration::from_millis(601)
        ));
        // F12 intentionally opens a finished result again after the key is released.
        if key != InputKey::F(12) {
            assert!(!state.handle_auto_admin_input_at(
                &InputEvent::key(key),
                at + Duration::from_millis(602)
            ));
        }
    }
}

#[test]
fn running_task_cannot_be_closed_or_hidden_before_it_finishes() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    let old_screen = state.active_screen();

    state.close_auto_admin();
    assert!(state.auto_admin_visible());
    // An unknown button action cannot send an active task to the background.
    state.activate_auto_admin_button(&job, 4);
    assert!(state.auto_admin_visible());
    state.apply_input(InputEvent::key(InputKey::F(12)));
    state.apply_input(InputEvent::Key(KeyInput::new(InputKey::F(12)).repeated()));
    state.apply_input(InputEvent::Key(KeyInput::with_phase(
        InputKey::F(12),
        InputModifiers::NONE,
        InputPhase::Release,
    )));
    state.apply_input(InputEvent::key(InputKey::F(12)));
    assert!(state.auto_admin_visible());
    assert!(rx.try_recv().is_err(), "F12 must not reach the child");

    for input in [
        InputEvent::mouse_down(PointerButton::Left, (0, 0)),
        InputEvent::mouse_up(PointerButton::Left, (0, 0)),
        InputEvent::FocusLost,
        InputEvent::FocusGained,
        InputEvent::key(InputKey::Escape),
    ] {
        assert_eq!(state.apply_input(input), ShellAction::Redraw);
        assert!(state.auto_admin_visible());
        assert_eq!(job.phase(), RUNNING);
        assert_eq!(state.active_screen(), old_screen);
    }
    assert!(
        matches!(rx.try_recv().unwrap(), OperationInput::Terminal { bytes } if bytes == b"\x1b")
    );
    assert!(rx.try_recv().is_err());

    job.finish(Ok("Done".into()));
    state.close_auto_admin();
    assert!(!state.auto_admin_visible());
}

#[test]
fn completed_failed_and_disconnected_tasks_can_be_closed() {
    for event in [
        OperationEvent::Completed {
            message: "Done".into(),
        },
        OperationEvent::Failed {
            message: "Fixture failed".into(),
        },
        OperationEvent::Disconnected {
            message: "Fixture disconnected".into(),
        },
    ] {
        for key in [
            InputKey::Enter,
            InputKey::Space,
            InputKey::Escape,
            InputKey::F(12),
        ] {
            let mut state = ShellSession::new_for_home_mode(
                ShellLaunchConfig::default(),
                (120, 40),
                ShellHomeMode::User,
            );
            let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
            state.auto_admin = AutoAdminState {
                job: Some(job.clone()),
                visible: true,
                ..Default::default()
            };
            job.emit(&event);
            assert!(state.auto_admin_visible());
            assert_eq!(job.phase(), FINISHED);
            assert!(state.auto_admin_view().unwrap().finished);
            state.apply_input(InputEvent::key(key));
            assert!(!state.auto_admin_visible());
            assert!(rx.try_recv().is_err());
        }

        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
        state.auto_admin = AutoAdminState {
            job: Some(job.clone()),
            visible: true,
            ..Default::default()
        };
        job.emit(&event);
        let button =
            ui::auto_admin_layout(Rect::new(0, 0, 120, 40), &state.auto_admin_view().unwrap())
                .buttons[0];
        let point = (button.x, button.y);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        assert!(state.auto_admin_visible());
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        assert!(!state.auto_admin_visible());
        assert!(rx.try_recv().is_err());
    }
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
    let area = ui::auto_admin_layout(Rect::new(0, 0, 120, 40), &state.auto_admin_view().unwrap())
        .buttons[0];
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
fn modal_captures_ctrl_c_escape_and_page_shortcuts_and_stays_visible_with_f12() {
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
    assert!(state.auto_admin_visible());
    assert!(state.auto_admin_running());
    state.apply_input(InputEvent::Key(KeyInput::new(InputKey::F(12))));
    assert!(
        state.auto_admin_visible(),
        "holding F12 must keep the active task in the foreground"
    );
    state.apply_input(InputEvent::Key(KeyInput::with_phase(
        InputKey::F(12),
        InputModifiers::NONE,
        InputPhase::Release,
    )));
    state.apply_input(InputEvent::Key(KeyInput::new(InputKey::F(12))));
    assert!(state.auto_admin_visible());
    assert!(rx.try_recv().is_err(), "F12 must not reach the child");
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
fn closing_pending_approval_denies_it_and_reopening_cannot_approve_it() {
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
    assert!(state.auto_admin.approve_selected);
    let (tx, _rx) = mpsc::channel();
    assert!(
        state
            .begin_auto_admin("Remove other".into(), true, tx)
            .is_none()
    );
    assert_eq!(state.auto_admin.job.as_ref().unwrap(), &job);
    state.apply_input(InputEvent::from_key_label("F12"));
    assert!(job.wait_for_approval().is_err());
    state.apply_input(InputEvent::Key(KeyInput::with_phase(
        InputKey::F(12),
        InputModifiers::NONE,
        InputPhase::Release,
    )));
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

#[test]
fn confirmation_scroll_stops_at_the_last_description_line() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (tx, _rx) = mpsc::channel();
    state
        .begin_auto_admin(
            (0..30)
                .map(|i| format!("Affected item {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
            true,
            tx,
        )
        .unwrap();
    for _ in 0..30 {
        state.apply_input(InputEvent::key(InputKey::Down));
    }
    let view = state.auto_admin_view().unwrap();
    let layout = ui::auto_admin_layout(Rect::new(0, 0, 120, 40), &view);
    assert_eq!(usize::from(view.scroll + layout.description.height), 30);
    assert!(view.confirming);
}
