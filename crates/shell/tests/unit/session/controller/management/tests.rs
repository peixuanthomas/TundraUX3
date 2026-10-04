use super::*;

fn shortcut_action_state(kind: ManagementKind, id: &str) -> ShellSession {
    let mut session = state();
    session.management_state.kind = Some(kind);
    session.management_state.query = Some(ManagementQuery::new(kind));
    session.management_state.snapshot.rows.push(ManagementRow {
        id: "mock-selected-item".into(),
        identity: BTreeMap::from([("identity".into(), "mock".into())]),
        actions: vec![ManagementAction {
            id: id.into(),
            label: id.into(),
            confirm: true,
            privileged: true,
            fields: vec![ManagementField {
                id: "option".into(),
                label: "Option".into(),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    });
    session
}

fn click_management_point(session: &mut ShellSession, point: (u16, u16)) {
    let now = Instant::now();
    assert!(
        session
            .prepare_button_input(InputEvent::mouse_down(PointerButton::Left, point), now)
            .is_none()
    );
    let released = session.prepare_button_input(
        InputEvent::mouse_up(PointerButton::Left, point),
        now + Duration::from_millis(30),
    );
    let Some(InputEvent::Mouse(mouse)) = released else {
        panic!("button must capture matching release")
    };
    session.handle_management_pointer(mouse);
}

#[test]
fn clearing_management_search_and_refreshing_restores_the_full_list() {
    for refresh_with_key in [false, true] {
        let mut session = shortcut_action_state(ManagementKind::Packages, "install");
        session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
        session.management_state.filter_input = "bash".into();
        session.apply_management_filter();
        assert_eq!(
            session.management_state.query.as_ref().unwrap().filter,
            "bash"
        );

        let (search_job, _) = session.management_job();
        *search_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
            rows: vec![ManagementRow {
                id: "bash".into(),
                ..Default::default()
            }],
            ..Default::default()
        }));
        session.management_state.query_job = Some(search_job);
        session.poll_management();
        assert_eq!(session.management_state.snapshot.rows.len(), 1);

        session.management_state.query.as_mut().unwrap().target = Some("bash".into());
        session.management_state.list_scroll_explicit = true;
        session.management_touch_control(ui::ManagementControl::Search);
        for _ in 0..4 {
            session.handle_management_key(&KeyInput::new(InputKey::Backspace));
        }
        assert!(session.management_state.filter_input.is_empty());
        if refresh_with_key {
            session.handle_management_key(&KeyInput::new(InputKey::F(5)));
        } else {
            let layout = ui::management_layout(
                session.management_main(),
                &session.to_management_view_model(),
            );
            let area = layout
                .controls
                .iter()
                .find(|(control, _)| *control == ui::ManagementControl::Refresh)
                .unwrap()
                .1;
            click_management_point(&mut session, (area.x, area.y));
        }
        let query = session.management_state.query.as_ref().unwrap();
        assert!(query.filter.is_empty());
        assert!(query.target.is_none());
        assert!(!session.management_state.list_scroll_explicit);

        let (full_job, _) = session.management_job();
        *full_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
            rows: ["bash", "coreutils", "curl"]
                .into_iter()
                .map(|id| ManagementRow {
                    id: id.into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }));
        session.management_state.query_job = Some(full_job);
        session.poll_management();
        assert_eq!(session.management_state.snapshot.rows.len(), 3);
    }
}

#[test]
fn refreshing_management_discards_pending_results_even_without_a_worker() {
    let mut session = shortcut_action_state(ManagementKind::Packages, "install");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.management_state.filter_input = "bash".into();
    session.apply_management_filter();
    let (old_job, _) = session.management_job();
    session.management_state.query_job = Some(old_job.clone());
    session.management_state.filter_input.clear();
    session.management_touch_control(ui::ManagementControl::Refresh);
    assert!(session.management_state.query_job.is_none());

    *old_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "old-search-result".into(),
        ..Default::default()
    }));
    session.poll_management();
    assert_ne!(
        session.management_state.snapshot.backend,
        "old-search-result"
    );

    let (current_job, _) = session.management_job();
    *current_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "full-list".into(),
        ..Default::default()
    }));
    session.management_state.query_job = Some(current_job);
    session.poll_management();
    assert_eq!(session.management_state.snapshot.backend, "full-list");
    session.poll_management();
    assert_eq!(session.management_state.snapshot.backend, "full-list");
}

#[test]
fn management_refresh_updates_drafts_but_retains_an_unchanged_detail_target() {
    let mut session = shortcut_action_state(ManagementKind::Packages, "install");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.management_state.filter_input = "bash".into();
    session.apply_management_filter();
    session.management_state.query.as_mut().unwrap().target = Some("bash".into());
    session.management_touch_control(ui::ManagementControl::Refresh);
    assert_eq!(
        session
            .management_state
            .query
            .as_ref()
            .unwrap()
            .target
            .as_deref(),
        Some("bash")
    );
    session.management_state.filter_input = "curl".into();
    session.management_touch_control(ui::ManagementControl::Refresh);
    let query = session.management_state.query.as_ref().unwrap();
    assert_eq!(query.filter, "curl");
    assert!(query.target.is_none());
}

#[test]
fn management_action_shortcuts_match_clicks_and_preserve_fields_and_identity() {
    for (kind, ids) in [
        (
            ManagementKind::Services,
            vec![
                "start",
                "stop",
                "restart",
                "enable",
                "disable",
                "view_logs",
                "set_view",
            ],
        ),
        (
            ManagementKind::Processes,
            vec!["term", "kill", "stop", "cont", "nice", "set_view"],
        ),
        (
            ManagementKind::Packages,
            vec![
                "install",
                "remove",
                "upgrade",
                "upgrade_all",
                "refresh",
                "scope_search",
                "scope_installed",
                "scope_updates",
            ],
        ),
        (
            ManagementKind::Network,
            vec![
                "inspect_network",
                "configure",
                "wifi-connect",
                "wifi-disconnect",
                "wifi-forget",
                "forget_saved_wifi",
                "check",
            ],
        ),
        (
            ManagementKind::Disks,
            vec!["mount", "unmount", "scan", "open_directory"],
        ),
    ] {
        for id in ids {
            let mut keyed = shortcut_action_state(kind, id);
            let mut clicked = shortcut_action_state(kind, id);
            let (hint, key) = management_action_shortcut(Some(kind), id)
                .expect("each current action has a shortcut");
            assert!(
                keyed.to_management_view_model().actions[0]
                    .0
                    .starts_with(&format!("[{hint}]"))
            );
            keyed.handle_management_key(&KeyInput::new(key));
            let layout = ui::management_layout(
                clicked.management_main(),
                &clicked.to_management_view_model(),
            );
            click_management_point(&mut clicked, (layout.actions[0].x, layout.actions[0].y));
            assert_eq!(
                keyed.management_state.form, clicked.management_state.form,
                "{kind:?}/{id}"
            );
            assert!(
                keyed.management_state.form.is_some(),
                "{id} still collects operation fields before AutoAdmin approval"
            );
            assert!(
                keyed.management_state.operation_job.is_none(),
                "no real Linux operation in shortcut test"
            );
        }
    }
}

#[test]
fn management_toolbar_shortcuts_match_clicks() {
    for (control, key) in [
        (
            ui::ManagementControl::Search,
            KeyInput::new(InputKey::Char('s')),
        ),
        (
            ui::ManagementControl::Search,
            KeyInput::new(InputKey::Char('/')),
        ),
        (
            ui::ManagementControl::Refresh,
            KeyInput::new(InputKey::Char('r')),
        ),
        (
            ui::ManagementControl::Refresh,
            KeyInput::new(InputKey::F(5)),
        ),
        (
            ui::ManagementControl::ApplySearch,
            KeyInput::with_modifiers(InputKey::Enter, ui::KeyModifiers::CTRL),
        ),
        (
            ui::ManagementControl::ClearSearch,
            KeyInput::with_modifiers(InputKey::Char('u'), ui::KeyModifiers::CTRL),
        ),
        (
            ui::ManagementControl::Details,
            KeyInput::new(InputKey::F(4)),
        ),
        (
            ui::ManagementControl::Terminal,
            KeyInput::with_modifiers(InputKey::Char('t'), ui::KeyModifiers::CTRL),
        ),
    ] {
        let mut keyed = shortcut_action_state(ManagementKind::Services, "start");
        let mut clicked = shortcut_action_state(ManagementKind::Services, "start");
        keyed.management_state.filter_input = "needle".into();
        clicked.management_state.filter_input = "needle".into();
        keyed.handle_management_key(&key);
        let layout = ui::management_layout(
            clicked.management_main(),
            &clicked.to_management_view_model(),
        );
        let area = layout
            .controls
            .iter()
            .find(|(id, _)| *id == control)
            .unwrap()
            .1;
        click_management_point(&mut clicked, (area.x, area.y));
        assert_eq!(
            keyed.management_state.query, clicked.management_state.query,
            "{control:?}"
        );
        assert_eq!(
            keyed.management_state.filter_input,
            clicked.management_state.filter_input
        );
        assert_eq!(
            keyed.management_state.filtering,
            clicked.management_state.filtering
        );
        assert_eq!(
            keyed.management_state.details_only,
            clicked.management_state.details_only
        );
        assert_eq!(
            keyed.management_state.terminal_mode,
            clicked.management_state.terminal_mode
        );
    }
}

#[test]
fn management_shortcuts_cannot_bypass_disabled_actions_or_unrelated_modifiers() {
    let mut session = shortcut_action_state(ManagementKind::Processes, "kill");
    session.management_state.snapshot.rows[0].actions[0].disabled_reason =
        Some("Protected process".into());
    for key in [InputKey::Char('k'), InputKey::Char('1')] {
        session.handle_management_key(&KeyInput::new(key));
        assert!(session.management_state.form.is_none());
        assert_eq!(session.management_state.status, "Protected process");
    }
    session.management_state.snapshot.rows[0].actions[0].disabled_reason = None;
    for modifiers in [
        ui::KeyModifiers::CTRL,
        ui::KeyModifiers::ALT,
        ui::KeyModifiers {
            super_key: true,
            ..Default::default()
        },
        ui::KeyModifiers {
            meta: true,
            ..Default::default()
        },
    ] {
        for key in [
            InputKey::Char('k'),
            InputKey::Char('s'),
            InputKey::Char('1'),
            InputKey::F(5),
        ] {
            session.handle_management_key(&KeyInput::with_modifiers(key, modifiers));
            assert!(session.management_state.form.is_none());
            assert!(!session.management_state.filtering);
            assert!(session.management_state.query_job.is_none());
        }
    }
    let (job, _) = session.management_job();
    session.management_state.operation_job = Some(job);
    assert!(!session.to_management_view_model().actions[0].1);
    session.handle_management_key(&KeyInput::new(InputKey::Char('k')));
    assert!(session.management_state.form.is_none());
    session.handle_management_key(&KeyInput::new(InputKey::F(10)));
    assert!(
        matches!(session.management_state.form.as_ref().unwrap().purpose, FormPurpose::Action(ref action, _) if action.id == "cancel_operation")
    );
}

#[test]
fn management_text_input_and_shortcut_repeats_do_not_trigger_actions() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.handle_management_key(&KeyInput::new(InputKey::Char('s')));
    session.handle_management_key(&KeyInput::new(InputKey::Char('s')).repeated());
    assert!(
        session.management_state.filter_input.is_empty(),
        "held Search does not type into its new field"
    );
    for c in "srak1".chars() {
        session.handle_management_key(&KeyInput::new(InputKey::Char(c)));
    }
    assert_eq!(session.management_state.filter_input, "srak1");
    assert!(session.management_state.form.is_none());
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    session.management_state.snapshot.rows[0].actions[0]
        .fields
        .push(ManagementField {
            id: "text".into(),
            ..Default::default()
        });
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')).repeated());
    assert!(
        session.management_state.form.as_ref().unwrap().fields[0]
            .value
            .is_empty()
    );
    for c in "srak1".chars() {
        session.handle_management_key(&KeyInput::new(InputKey::Char(c)));
    }
    assert_eq!(
        session.management_state.form.as_ref().unwrap().fields[0].value,
        "srak1"
    );
    assert!(session.management_state.operation_job.is_none());
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Char('t'),
        ui::KeyModifiers::CTRL,
    ));
    assert!(session.management_state.terminal_mode);
    session.handle_management_key(
        &KeyInput::with_modifiers(InputKey::Char('t'), ui::KeyModifiers::CTRL).repeated(),
    );
    assert!(
        session.management_state.terminal_mode,
        "held Output does not toggle again"
    );
}

#[test]
fn management_choice_enter_and_form_control_enter_preserve_input_and_required_checks() {
    let mut session = state();
    let (job, responses) = session.management_job();
    session.management_state.operation_job = Some(job);
    session.management_state.form = Some(ManagementEditor {
        title: "Mock question".into(),
        message: String::new(),
        message_scroll: 0,
        selected: 0,
        purpose: FormPurpose::Answer("mock-question".into()),
        fields: vec![ManagementField {
            id: "answer".into(),
            value: "No".into(),
            choices: vec!["No".into(), "Yes".into()],
            required: true,
            ..Default::default()
        }],
    });
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    assert_eq!(session.management_state.choice_field, Some(0));
    session.handle_management_key(&KeyInput::new(InputKey::End));
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    session.handle_management_key(&KeyInput::new(InputKey::Enter).repeated());
    assert!(session.management_state.form.is_some());
    assert!(responses.try_recv().is_err());
    assert_eq!(
        session.management_state.form.as_ref().unwrap().fields[0].value,
        "Yes"
    );
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Enter,
        ui::KeyModifiers::CTRL,
    ));
    assert!(
        matches!(responses.try_recv().unwrap(), OperationInput::Answer { id, value } if id == "mock-question" && value == "Yes")
    );
    assert!(session.management_state.form.is_none());

    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.management_state.snapshot.rows[0].actions[0]
        .fields
        .push(ManagementField {
            id: "required".into(),
            required: true,
            ..Default::default()
        });
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Enter,
        ui::KeyModifiers::CTRL,
    ));
    assert!(session.management_state.form.is_some());
    assert!(session.management_state.operation_job.is_none());
    assert_eq!(
        session.management_state.status,
        i18n::tr!("management-required")
    );
}

#[test]
fn duplicate_action_mnemonics_use_first_target_and_keep_number_access() {
    let mut session = shortcut_action_state(ManagementKind::Disks, "scan");
    session
        .management_state
        .snapshot
        .actions
        .push(ManagementAction {
            id: "scan".into(),
            confirm: true,
            ..Default::default()
        });
    let model = session.to_management_view_model();
    assert!(model.actions[0].0.starts_with("[A]"));
    assert!(!model.actions[1].0.starts_with("[A]"));
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    assert!(matches!(
        session.management_state.form.as_ref().unwrap().purpose,
        FormPurpose::Action(_, Some(_))
    ));
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    session.handle_management_key(&KeyInput::new(InputKey::Char('2')));
    assert!(matches!(
        session.management_state.form.as_ref().unwrap().purpose,
        FormPurpose::Action(_, None)
    ));
}

#[test]
fn management_paging_keys_follow_buttons_without_activating_actions() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.terminal_size = (60, 18);
    session.management_state.snapshot.rows[0].actions = (0..8)
        .map(|index| ManagementAction {
            id: format!("mock-{index}"),
            confirm: true,
            ..Default::default()
        })
        .collect();
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    assert!(!layout.action_next.is_empty());
    let mut clicked = shortcut_action_state(ManagementKind::Services, "start");
    clicked.terminal_size = session.terminal_size;
    clicked.management_state.snapshot = session.management_state.snapshot.clone();
    click_management_point(&mut clicked, (layout.action_next.x, layout.action_next.y));
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Right,
        ui::KeyModifiers::ALT,
    ));
    assert_eq!(
        session.management_state.action_scroll,
        clicked.management_state.action_scroll
    );
    assert_eq!(session.management_state.action_scroll, Some(1));
    assert!(session.management_state.form.is_none());
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Left,
        ui::KeyModifiers::ALT,
    ));
    assert_eq!(session.management_state.action_scroll, Some(0));
}

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
fn terminal_button_reopens_auto_admin_instead_of_a_second_input_surface() {
    let mut state = state();
    let (responses, _inputs) = mpsc::channel();
    let job = state
        .begin_auto_admin("Operation".into(), false, responses)
        .unwrap();
    state.management_state.auto_admin_job = Some(job.clone());
    state.close_auto_admin();
    assert!(!state.auto_admin_visible());
    state.management_touch_control(ui::ManagementControl::Terminal);
    assert!(state.auto_admin_visible());
    assert!(!state.management_state.terminal_mode);
    assert!(job.running());
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
fn escape_returns_management_to_launcher_without_cancelling_task() {
    let mut state = state();
    state.screen_stack = vec![
        ShellScreen::Home,
        ShellScreen::Launcher,
        ShellScreen::Management,
    ];
    let (job, _) = state.management_job();
    state.management_state.operation_job = Some(job);
    state.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    assert_eq!(state.focused_component, ShellComponent::Launcher);
    assert!(state.management_state.operation_job.is_some());
}

#[test]
fn compact_management_uses_the_reserved_shell_content_area() {
    let mut state = state();
    state.terminal_size = (49, 11);
    assert_eq!(state.management_main(), Rect::new(0, 1, 49, 10));
    assert!(state.management_button_at((48, 0)).is_none());
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
fn management_shortcuts_route_to_the_foreground_popup_and_escape_closes_it() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Management),
        anchor: (5, 5),
    });
    for key in [
        KeyInput::new(InputKey::Char('s')),
        KeyInput::new(InputKey::Char('a')),
        KeyInput::new(InputKey::F(5)),
        KeyInput::with_modifiers(InputKey::Enter, ui::KeyModifiers::CTRL),
    ] {
        assert_eq!(
            session.route_key_input(&key).0,
            RoutedTarget::Popup(ShellComponent::ContextMenu)
        );
        session.apply_input(InputEvent::Key(key));
        assert!(session.management_state.form.is_none());
        assert!(!session.management_state.filtering);
        assert!(session.management_state.query_job.is_none());
    }
    session.apply_input(InputEvent::from_key_label("Esc"));
    assert!(session.active_popup.is_none());
    assert_eq!(session.active_screen(), ShellScreen::Management);
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
