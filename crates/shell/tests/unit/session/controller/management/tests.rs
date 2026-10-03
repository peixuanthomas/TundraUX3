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
fn launcher_management_entries_are_linux_only_and_guest_is_blocked() {
    let mut state = state();
    let entries = state.built_in_launcher_applications();
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
                .any(|entry| entry.id == format!("builtin.{}", kind.id())),
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

#[test]
fn management_scrollbar_drag_preserves_viewport_and_cancel_stops_capture() {
    let mut state = state();
    state.management_state.snapshot.columns = vec!["Name".into()];
    state.management_state.snapshot.rows = (0..100)
        .map(|index| ManagementRow {
            id: index.to_string(),
            cells: vec![format!("row-{index}")],
            ..Default::default()
        })
        .collect();
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let bar = *layout
        .scrollbars
        .iter()
        .find(|bar| bar.target == ui::ManagementScrollTarget::Rows)
        .unwrap();
    let pointer = |position: (u16, u16), kind| MouseInput {
        position: ui::Point::new(position.0, position.1),
        kind,
        modifiers: ui::KeyModifiers::NONE,
    };
    state.handle_management_pointer(pointer(
        (bar.thumb.x, bar.thumb.y),
        ui::MouseEventKind::Down(PointerButton::Left),
    ));
    state.handle_management_pointer(pointer(
        (bar.track.x, bar.track.bottom()),
        ui::MouseEventKind::Drag(PointerButton::Left),
    ));
    assert_eq!(state.management_state.scroll, 100 - layout.list_capacity);
    state.poll_management();
    assert_eq!(state.management_state.scroll, 100 - layout.list_capacity);
    state.cancel_management_pointer_gesture();
    assert!(!state.management_pointer_drag_active());
}

#[test]
fn touch_choice_list_selects_without_cycling_or_submitting() {
    let mut state = state();
    state.management_state.form = Some(ManagementEditor {
        title: "Choice".into(),
        message: String::new(),
        message_scroll: 0,
        fields: vec![ManagementField {
            id: "answer".into(),
            label: "Action".into(),
            value: "No".into(),
            choices: vec!["No".into(), "Yes".into()],
            ..Default::default()
        }],
        selected: 0,
        purpose: FormPurpose::Answer("question".into()),
    });
    let pointer = |area: Rect| MouseInput {
        position: ui::Point::new(area.x, area.y),
        kind: ui::MouseEventKind::Click(PointerButton::Left),
        modifiers: ui::KeyModifiers::NONE,
    };
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    state.handle_management_pointer(pointer(layout.fields[0].1));
    assert_eq!(
        state.management_state.form.as_ref().unwrap().fields[0].value,
        "No"
    );
    assert_eq!(state.management_state.choice_field, Some(0));
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let choice = layout
        .choice_rows
        .iter()
        .find(|(index, _)| *index == 1)
        .unwrap()
        .1;
    state.handle_management_pointer(pointer(choice));
    assert_eq!(
        state.management_state.form.as_ref().unwrap().fields[0].value,
        "Yes"
    );
    assert!(state.management_state.choice_field.is_none());
    assert!(state.management_state.form.is_some());
}

#[test]
fn touch_back_returns_management_to_launcher_without_cancelling_task() {
    let mut state = state();
    state.screen_stack = vec![
        ShellScreen::Home,
        ShellScreen::Launcher,
        ShellScreen::Management,
    ];
    let (job, _) = state.management_job();
    state.management_state.operation_job = Some(job);
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let area = layout
        .controls
        .iter()
        .find(|(control, _)| *control == ui::ManagementControl::Back)
        .unwrap()
        .1;
    state.handle_management_pointer(MouseInput {
        position: ui::Point::new(area.x, area.y),
        kind: ui::MouseEventKind::Click(PointerButton::Left),
        modifiers: ui::KeyModifiers::NONE,
    });
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    assert_eq!(state.focused_component, ShellComponent::Launcher);
    assert!(state.management_state.operation_job.is_some());
}

#[test]
fn management_touch_buttons_are_hidden_by_global_overlays() {
    let mut state = state();
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let point = (layout.controls[0].1.x, layout.controls[0].1.y);
    assert!(state.management_button_at(point).is_some());
    state.time_sync_dialog_visible = true;
    assert!(state.management_button_at(point).is_none());
    state.time_sync_dialog_visible = false;
    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Management),
        anchor: point,
    });
    assert!(state.management_button_at(point).is_none());
}

#[test]
fn global_notification_releases_management_scroll_capture() {
    let mut state = state();
    state.management_state.scrollbar_grab = Some((ui::ManagementScrollTarget::Rows, 0));
    state.notify_modal(
        "Notice",
        "Stop dragging",
        ui::NotificationTone::Info,
        vec![],
    );
    assert!(state.management_state.scrollbar_grab.is_none());
}

#[test]
fn terminal_events_close_the_choice_list_and_release_form_drag() {
    for event in [
        OperationEvent::Completed {
            message: "Completed".into(),
        },
        OperationEvent::Failed {
            message: "Failed".into(),
        },
        OperationEvent::Disconnected {
            message: "Disconnected".into(),
        },
    ] {
        let mut state = state();
        state.management_state.form = Some(ManagementEditor {
            title: "Question".into(),
            message: String::new(),
            message_scroll: 0,
            fields: vec![ManagementField {
                id: "answer".into(),
                choices: vec!["No".into(), "Yes".into()],
                ..Default::default()
            }],
            selected: 0,
            purpose: FormPurpose::Answer("question".into()),
        });
        state.management_state.choice_field = Some(0);
        state.management_state.choice_scroll = 3;
        state.management_state.choice_columns = 8;
        state.management_state.form_field_scroll = Some(4);
        state.management_state.scrollbar_grab = Some((ui::ManagementScrollTarget::Choices, 0));
        state.management_state.received_snapshot = true;
        let (job, _) = state.management_job();
        job.0.events.lock().unwrap().push_back(event);
        state.management_state.operation_job = Some(job);
        state.poll_management();
        assert!(state.management_state.form.is_none());
        assert!(state.management_state.choice_field.is_none());
        assert!(state.management_state.form_field_scroll.is_none());
        assert_eq!(state.management_state.choice_scroll, 0);
        assert_eq!(state.management_state.choice_columns, 0);
        assert!(!state.management_pointer_drag_active());
        assert!(!state.handle_management_choice_key(&KeyInput::new(InputKey::Right)));
    }
}

#[test]
fn compact_action_paging_makes_each_middle_action_touch_reachable() {
    let mut state = state();
    state.terminal_size = (60, 18);
    state.management_state.snapshot.actions = (0..6)
        .map(|index| ManagementAction {
            id: format!("action-{index}"),
            label: format!("Action {index}"),
            confirm: true,
            ..Default::default()
        })
        .collect();
    let tap = |area: Rect| MouseInput {
        position: ui::Point::new(area.x, area.y),
        kind: ui::MouseEventKind::Click(PointerButton::Left),
        modifiers: ui::KeyModifiers::NONE,
    };
    for index in 0..6 {
        let layout =
            ui::management_layout(state.management_main(), &state.to_management_view_model());
        assert_eq!(layout.actions.len(), 1);
        assert_eq!(layout.action_start, index);
        assert!(layout.action_next.width > 0 && layout.action_next.height > 0);
        if index < 5 {
            state.handle_management_pointer(tap(layout.action_next));
        }
    }
    for _ in 0..3 {
        let layout =
            ui::management_layout(state.management_main(), &state.to_management_view_model());
        state.handle_management_pointer(tap(layout.action_previous));
    }
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    assert_eq!(layout.action_start, 2);
    state.handle_management_pointer(tap(layout.actions[0]));
    assert!(
        matches!(&state.management_state.form.as_ref().unwrap().purpose,FormPurpose::Action(action,_) if action.id=="action-2")
    );
}
