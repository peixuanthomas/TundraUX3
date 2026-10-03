use super::*;

#[test]
fn compact_management_routes_mouse_to_the_visible_page() {
    let mut state = state();
    state.terminal_size = (60, 18);
    state.refresh_hit_map();
    assert_eq!(
        state.hit_map.target_at((10, 8)),
        Some(ShellComponent::Management)
    );
}

#[test]
fn held_enter_does_not_confirm_an_action() {
    let mut state = state();
    state
        .management_state
        .snapshot
        .actions
        .push(ManagementAction {
            id: "recover".into(),
            label: "Reconnect".into(),
            confirm: true,
            ..Default::default()
        });
    state.activate_management_action(0);
    state.handle_management_key(&KeyInput::new(InputKey::Enter).repeated());
    assert!(state.management_state.form.is_some());
    assert!(state.management_state.operation_job.is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn switching_applications_keeps_background_questions_and_operation() {
    let mut state = state();
    let (job, _) = state.management_job();
    state.management_state.operation_job = Some(job.clone());
    state.open_management(ManagementKind::Packages);
    assert_eq!(state.management_state.kind, Some(ManagementKind::Packages));
    job.0
        .events
        .lock()
        .unwrap()
        .push_back(OperationEvent::Question {
            id: "confirm".into(),
            prompt: "Continue?".into(),
            choices: vec!["Continue".into(), "Cancel".into()],
            secret: false,
        });
    state.poll_management();
    assert!(state.management_state.form.is_none());
    assert!(
        state.management_background[&ManagementKind::Services]
            .form
            .is_some()
    );
    state.open_management(ManagementKind::Services);
    assert!(state.management_state.operation_job.is_some());
    assert_eq!(
        state.management_state.form.as_ref().unwrap().fields[0].value,
        "Cancel"
    );
}

#[test]
fn terminal_ctrl_c_is_routed_to_task_and_configuration_cannot_be_dismissed() {
    let mut state = state();
    state.management_state.terminal_mode = true;
    let key = KeyInput::from_label("Ctrl+C");
    assert!(matches!(
        state.route_key_input(&key).1,
        ShellCommand::ManagementKey(_)
    ));
    state.management_state.form = Some(ManagementEditor {
        message_scroll: 0,
        title: "Configuration".into(),
        message: String::new(),
        fields: Vec::new(),
        selected: 0,
        purpose: FormPurpose::Answer("package-config-1".into()),
    });
    state.management_state.terminal_mode = false;
    state.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert!(state.management_state.form.is_some());
}

#[test]
fn changed_process_identity_invalidates_pressed_action() {
    let mut state = state();
    state.management_state.snapshot.rows.push(ManagementRow {
        id: "42".into(),
        identity: BTreeMap::from([("start_time_ticks".into(), "100".into())]),
        actions: vec![ManagementAction {
            id: "kill".into(),
            label: "Kill".into(),
            confirm: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let point = (layout.actions[0].x, layout.actions[0].y);
    let before = state.management_button_at(point);
    state.management_state.snapshot.rows[0]
        .identity
        .insert("start_time_ticks".into(), "200".into());
    assert_ne!(before, state.management_button_at(point));
}

fn state() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LocalAccount,
            session_id: "management-test".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            role: UserRole::User,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    state.screen_stack.push(ShellScreen::Management);
    state.management_state.kind = Some(ManagementKind::Services);
    state.management_state.query = Some(ManagementQuery::new(ManagementKind::Services));
    state
}

#[test]
fn text_fields_take_priority_over_actions_and_keep_secrets_out_of_view_model() {
    let mut state = state();
    state.management_state.form = Some(ManagementEditor {
        message_scroll: 0,
        title: "Authorization".into(),
        message: String::new(),
        fields: vec![ManagementField {
            id: "password".into(),
            secret: true,
            ..Default::default()
        }],
        selected: 0,
        purpose: FormPurpose::Answer("sudo-password".into()),
    });
    for key in [InputKey::Char('r'), InputKey::Space, InputKey::Char('1')] {
        state.handle_management_key(&KeyInput::new(key));
    }
    assert_eq!(
        state.management_state.form.as_ref().unwrap().fields[0].value,
        "r 1"
    );
    assert_eq!(
        state.to_management_view_model().form.unwrap().fields[0].value,
        "•••"
    );
    assert!(state.management_state.query_job.is_none());
}

#[test]
fn choice_fields_reject_free_text_and_paste() {
    let mut state = state();
    state.management_state.form = Some(ManagementEditor {
        message_scroll: 0,
        title: "Choice".into(),
        message: String::new(),
        fields: vec![ManagementField {
            id: "answer".into(),
            value: "No".into(),
            choices: vec!["No".into(), "Yes".into()],
            ..Default::default()
        }],
        selected: 0,
        purpose: FormPurpose::Answer("confirmation".into()),
    });
    state.handle_management_paste("anything");
    state.handle_management_key(&KeyInput::new(InputKey::Char('x')));
    assert_eq!(
        state.management_state.form.as_ref().unwrap().fields[0].value,
        "No"
    );
    state.handle_management_key(&KeyInput::new(InputKey::Right));
    assert_eq!(
        state.management_state.form.as_ref().unwrap().fields[0].value,
        "Yes"
    );
}

#[test]
fn result_snapshot_and_outcome_survive_completion() {
    let mut state = state();
    let (job, _) = state.management_job();
    job.0.events.lock().unwrap().extend([
        OperationEvent::Snapshot {
            snapshot: ManagementSnapshot {
                backend: "authorized-inspection".into(),
                ..Default::default()
            },
        },
        OperationEvent::Completed {
            message: "Inspection complete".into(),
        },
    ]);
    state.management_state.operation_job = Some(job);
    state.poll_management();
    assert_eq!(
        state.management_state.snapshot.backend,
        "authorized-inspection"
    );
    assert_eq!(state.management_state.status, "Inspection complete");
    assert!(state.management_state.query_job.is_none());
}

#[test]
fn action_capture_requires_matching_release() {
    let mut state = state();
    state
        .management_state
        .snapshot
        .actions
        .push(ManagementAction {
            id: "start".into(),
            label: "Start".into(),
            confirm: true,
            ..Default::default()
        });
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let point = ui::Point::new(layout.actions[0].x, layout.actions[0].y);
    let down = InputEvent::Mouse(MouseInput {
        position: point,
        kind: ui::MouseEventKind::Down(PointerButton::Left),
        modifiers: ui::KeyModifiers::NONE,
    });
    let now = Instant::now();
    assert!(state.prepare_button_input(down, now).is_none());
    assert!(state.management_state.form.is_none());
    let released = state.prepare_button_input(
        InputEvent::Mouse(MouseInput {
            position: point,
            kind: ui::MouseEventKind::Up(PointerButton::Left),
            modifiers: ui::KeyModifiers::NONE,
        }),
        now + Duration::from_millis(50),
    );
    let Some(InputEvent::Mouse(mouse)) = released else {
        panic!("expected captured release");
    };
    state.handle_management_pointer(mouse);
    assert!(state.management_state.form.is_some());
}

#[test]
fn home_management_entries_are_linux_only_and_guest_is_blocked() {
    let mut state = state();
    let entries = state.user_home_entries();
    for kind in [
        ManagementKind::Services,
        ManagementKind::Processes,
        ManagementKind::Packages,
        ManagementKind::Network,
        ManagementKind::Disks,
    ] {
        assert_eq!(
            entries
                .iter()
                .any(|entry| entry.icon_identity() == kind.id()),
            cfg!(target_os = "linux")
        );
    }
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LocalAccount,
            session_id: "guest".into(),
            user_id: "guest".into(),
            username: "guest".into(),
            role: UserRole::Guest,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    state.screen_stack = vec![ShellScreen::Home];
    state.open_management(ManagementKind::Packages);
    assert_eq!(state.active_screen(), ShellScreen::Home);
}
