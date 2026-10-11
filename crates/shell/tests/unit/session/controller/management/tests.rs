use super::*;

#[test]
fn same_kind_operations_finish_without_replacing_each_others_snapshot() {
    let mut session = state();
    let first = session.active_page_id();
    let (first_job, _) = session.management_job();
    session.register_management_job(
        &first_job,
        TaskDomain::ManagementOperation,
        TaskClass::Operation,
    );
    session.pages.management.management_state.operation_job = Some(first_job.clone());
    session.enter_new_page(ShellScreen::Management);
    let second = session.active_page_id();
    session.pages.management.management_state.kind = Some(ManagementKind::Services);
    let (second_job, _) = session.management_job();
    session.register_management_job(
        &second_job,
        TaskDomain::ManagementOperation,
        TaskClass::Operation,
    );
    session.pages.management.management_state.operation_job = Some(second_job.clone());
    for (job, label) in [
        (&first_job, "first operation"),
        (&second_job, "second operation"),
    ] {
        job.0.events.lock().unwrap().extend([
            OperationEvent::Snapshot {
                snapshot: ManagementSnapshot {
                    backend: label.into(),
                    ..Default::default()
                },
            },
            OperationEvent::Completed {
                message: label.into(),
            },
        ]);
    }
    session.poll_management();
    assert_eq!(session.active_page_id(), second);
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "second operation"
    );
    assert_eq!(
        session.pages.management.management_state.outcome.as_deref(),
        Some("second operation")
    );
    let hidden = &session
        .pages
        .management
        .get(first)
        .unwrap()
        .management_state;
    assert_eq!(hidden.snapshot.backend, "first operation");
    assert_eq!(hidden.outcome.as_deref(), Some("first operation"));
    assert!(hidden.operation_job.is_none());
    assert!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
}

#[test]
fn same_kind_queries_deliver_to_their_own_covered_page() {
    let mut session = state();
    session.pages.management.management_state.kind = Some(ManagementKind::Packages);
    let first = session.active_page_id();
    let (first_job, _) = session.management_job();
    session.register_management_job(&first_job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(first_job.clone());
    session.enter_new_page(ShellScreen::Management);
    let second = session.active_page_id();
    session.pages.management.management_state.kind = Some(ManagementKind::Packages);
    let (second_job, _) = session.management_job();
    session.register_management_job(&second_job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(second_job.clone());
    *first_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "first query".into(),
        ..Default::default()
    }));
    *second_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "second query".into(),
        ..Default::default()
    }));
    session.poll_management();
    assert_eq!(session.active_page_id(), second);
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "second query"
    );
    assert_eq!(
        session
            .pages
            .management
            .get(first)
            .unwrap()
            .management_state
            .snapshot
            .backend,
        "first query"
    );
    session.close_page_instance(second);
    assert_eq!(session.active_page_id(), first);
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "first query"
    );
}

#[test]
fn closed_management_query_is_cancelled_and_cannot_write_into_a_new_page() {
    let mut session = state();
    let first = session.active_page_id();
    let (job, _) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(job.clone());
    session.close_page_instance(first);
    assert!(job.0.cancelled.load(Ordering::Relaxed));
    session.enter_new_page(ShellScreen::Management);
    *job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "late old result".into(),
        ..Default::default()
    }));
    session.poll_management();
    assert!(
        session
            .pages
            .management
            .management_state
            .snapshot
            .backend
            .is_empty()
    );
    assert_eq!(
        session.tasks.owner(TaskDomain::ManagementQuery, job.0.id),
        None
    );
}

#[test]
fn detached_management_operation_does_not_keep_or_update_its_page() {
    let mut session = state();
    let first = session.active_page_id();
    let (job, _) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session.pages.management.management_state.operation_job = Some(job.clone());
    session.tasks.transfer_operations(first);
    session.detach_management_page(first);
    session.close_page_instance(first);
    assert!(session.pages.management.get(first).is_none());
    assert!(!job.0.cancelled.load(Ordering::Relaxed));
    session.enter_new_page(ShellScreen::Management);
    session.pages.management.management_state.status = "new page".into();
    job.0.events.lock().unwrap().extend([
        OperationEvent::Snapshot {
            snapshot: ManagementSnapshot {
                backend: "old operation".into(),
                ..Default::default()
            },
        },
        OperationEvent::Completed {
            message: "finished old operation".into(),
        },
    ]);
    session.poll_management();
    assert_eq!(session.pages.management.management_state.status, "new page");
    assert!(
        session
            .pages
            .management
            .management_state
            .snapshot
            .backend
            .is_empty()
    );
    assert!(session.detached_management_operations.is_empty());
    assert_eq!(
        session
            .tasks
            .owner(TaskDomain::ManagementOperation, job.0.id),
        None
    );
}

#[test]
fn account_state_translations_preserve_names_and_native_output() {
    let users = Some(ManagementKind::Users);
    assert_eq!(
        management_detail_value(users, "Source", "remote"),
        i18n::tr!("management-account-source-remote")
    );
    assert_eq!(
        management_detail_value(users, "Password locked", "true"),
        i18n::tr!("management-value-true")
    );
    assert_eq!(
        management_detail_value(
            users,
            "Groups",
            "unknown (select the account to query full membership)"
        ),
        i18n::tr!("management-value-unknown-select-the-account-to-query-full-membership")
    );
    for (key, value) in [
        ("Groups", "remote"),
        ("Home", "/home/local"),
        ("Account expiry", "2030-01-01"),
    ] {
        assert_eq!(management_detail_value(users, key, value), value);
    }
    assert_eq!(
        management_detail_value(Some(ManagementKind::Services), "Source", "remote"),
        "remote"
    );
}

fn shortcut_action_state(kind: ManagementKind, id: &str) -> ShellSession {
    let mut session = state();
    session.pages.management.management_state.kind = Some(kind);
    session.pages.management.management_state.query = Some(ManagementQuery::new(kind));
    session
        .pages
        .management
        .management_state
        .snapshot
        .rows
        .push(ManagementRow {
            id: "mock-selected-item".into(),
            identity: BTreeMap::from([("identity".into(), "mock".into())]),
            actions: vec![ManagementAction {
                id: id.into(),
                label: id.into(),
                primary: true,
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

#[test]
fn package_buttons_stay_in_place_and_disabled_actions_do_not_execute() {
    for id in [
        "remove",
        "install",
        "upgrade",
        "pacman_install",
        "pacman_upgrade",
    ] {
        let mut session = shortcut_action_state(ManagementKind::Packages, id);
        let actions = session.management_actions();
        assert_eq!(
            actions
                .iter()
                .take(3)
                .map(|(action, _)| action.id.as_str())
                .collect::<Vec<_>>(),
            ["refresh", "scope_installed", "more_actions"]
        );
        assert_eq!(actions.len(), 7);
        let model = session.to_management_view_model();
        assert_eq!(model.detail_action_start, Some(0));
        for (index, expected) in ["remove", "install", "upgrade", "scope_search"]
            .into_iter()
            .enumerate()
        {
            let action = &actions[index + 3].0;
            assert!(action.id == expected || action.id == format!("pacman_{expected}"));
            assert_eq!(
                model.actions[index + 3].1,
                action.id == id || expected == "scope_search"
            );
            if !model.actions[index + 3].1 {
                session.activate_management_action(index + 3);
                assert!(session.pages.management.management_state.form.is_none());
                assert!(
                    session
                        .pages
                        .management
                        .management_state
                        .draft_job
                        .is_none()
                );
                assert!(
                    session
                        .pages
                        .management
                        .management_state
                        .operation_job
                        .is_none()
                );
            }
        }
    }
}

#[test]
fn package_detail_buttons_use_the_same_target_for_pointer_and_keyboard() {
    for by_touch in [false, true] {
        let mut session = shortcut_action_state(ManagementKind::Packages, "install");
        session.terminal_size = (120, 30);
        let model = session.to_management_view_model();
        let layout = ui::management_layout(session.management_main(), &model);
        let install = layout.actions[4];
        assert_eq!(install.intersection(layout.detail_actions_panel), install);
        if by_touch {
            click_management_point(&mut session, (install.x, install.y));
        } else {
            session.handle_management_key(&KeyInput::new(InputKey::Char('5')));
        }
        assert!(
            matches!(session.pages.management.management_state.form.as_ref().map(|form| &form.purpose), Some(FormPurpose::Action(action, Some(row))) if action.id == "install" && row.id == "mock-selected-item")
        );
    }
}

#[test]
fn common_account_actions_are_visible_and_dangerous_changes_stay_in_more() {
    let mut session = shortcut_action_state(ManagementKind::Users, "user_info");
    session.pages.management.management_state.snapshot.rows[0]
        .actions
        .extend(
            [
                "user_password",
                "user_groups",
                "user_delete",
                "user_lock",
                "user_expiry",
            ]
            .map(|id| ManagementAction {
                id: id.into(),
                ..Default::default()
            }),
        );
    session
        .pages
        .management
        .management_state
        .snapshot
        .actions
        .extend(["user_create", "set_view"].map(|id| ManagementAction {
            id: id.into(),
            ..Default::default()
        }));
    let actions = session.management_actions();
    assert_eq!(
        actions
            .iter()
            .map(|(action, _)| action.id.as_str())
            .collect::<Vec<_>>(),
        [
            "user_create",
            "set_view",
            "more_actions",
            "user_info",
            "user_password",
            "user_groups"
        ]
    );
    assert_eq!(
        session.to_management_view_model().detail_action_start,
        Some(0)
    );
    session.activate_management_action(2);
    assert!(
        matches!(&session.pages.management.management_state.form.as_ref().unwrap().purpose, FormPurpose::Menu(items) if items.iter().any(|(action, _)| action.id == "user_delete"))
    );
}

#[test]
fn services_expose_restart_and_logs_but_keep_startup_changes_in_more() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.pages.management.management_state.snapshot.rows[0]
        .actions
        .extend(
            ["stop", "restart", "view_logs", "enable", "disable"].map(|id| ManagementAction {
                id: id.into(),
                ..Default::default()
            }),
        );
    let model = session.to_management_view_model();
    assert_eq!(model.actions.len(), 5);
    let layout = ui::management_layout(Rect::new(0, 0, 100, 24), &model);
    assert_eq!(layout.actions.len(), 5);
    assert!(layout.action_next.is_empty());
    assert!(model.actions[2].0.starts_with("[T]"));
    assert!(model.actions[3].0.starts_with("[L]"));
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
        session.pages.management.management_state.filter_input = "bash".into();
        session.apply_management_filter();
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .query
                .as_ref()
                .unwrap()
                .filter,
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
        session.register_management_job(&search_job, TaskDomain::ManagementQuery, TaskClass::Query);
        session.pages.management.management_state.query_job = Some(search_job);
        session.poll_management();
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .snapshot
                .rows
                .len(),
            1
        );

        session
            .pages
            .management
            .management_state
            .query
            .as_mut()
            .unwrap()
            .target = Some("bash".into());
        session
            .pages
            .management
            .management_state
            .list_scroll_explicit = true;
        session.management_touch_control(ui::ManagementControl::Search);
        for _ in 0..4 {
            session.handle_management_key(&KeyInput::new(InputKey::Backspace));
        }
        assert!(
            session
                .pages
                .management
                .management_state
                .filter_input
                .is_empty()
        );
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
        let query = session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap();
        assert!(query.filter.is_empty());
        assert!(query.target.is_none());
        assert!(
            !session
                .pages
                .management
                .management_state
                .list_scroll_explicit
        );

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
        session.register_management_job(&full_job, TaskDomain::ManagementQuery, TaskClass::Query);
        session.pages.management.management_state.query_job = Some(full_job);
        session.poll_management();
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .snapshot
                .rows
                .len(),
            3
        );
    }
}

#[test]
fn refreshing_management_discards_pending_results_even_without_a_worker() {
    let mut session = shortcut_action_state(ManagementKind::Packages, "install");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.pages.management.management_state.filter_input = "bash".into();
    session.apply_management_filter();
    let (old_job, _) = session.management_job();
    session.register_management_job(&old_job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(old_job.clone());
    session
        .pages
        .management
        .management_state
        .filter_input
        .clear();
    session.management_touch_control(ui::ManagementControl::Refresh);
    assert!(
        session
            .pages
            .management
            .management_state
            .query_job
            .is_none()
    );

    *old_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "old-search-result".into(),
        ..Default::default()
    }));
    session.poll_management();
    assert_ne!(
        session.pages.management.management_state.snapshot.backend,
        "old-search-result"
    );

    let (current_job, _) = session.management_job();
    *current_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "full-list".into(),
        ..Default::default()
    }));
    session.register_management_job(&current_job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(current_job);
    session.poll_management();
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "full-list"
    );
    session.poll_management();
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "full-list"
    );
}

#[test]
fn management_refresh_updates_drafts_but_retains_an_unchanged_detail_target() {
    let mut session = shortcut_action_state(ManagementKind::Packages, "install");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.pages.management.management_state.filter_input = "bash".into();
    session.apply_management_filter();
    session
        .pages
        .management
        .management_state
        .query
        .as_mut()
        .unwrap()
        .target = Some("bash".into());
    session.management_touch_control(ui::ManagementControl::Refresh);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .target
            .as_deref(),
        Some("bash")
    );
    session.pages.management.management_state.filter_input = "curl".into();
    session.management_touch_control(ui::ManagementControl::Refresh);
    let query = session
        .pages
        .management
        .management_state
        .query
        .as_ref()
        .unwrap();
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
                "pacman_install",
                "pacman_upgrade",
                "pacman_upgrade_all",
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
            let (shortcut, key) = management_action_shortcut(Some(kind), id)
                .expect("each current action has a shortcut");
            keyed.handle_management_key(&KeyInput::new(key));
            let visible = clicked
                .management_actions()
                .iter()
                .position(|(action, _)| action.id == id && action.disabled_reason.is_none());
            let area = if let Some(index) = visible {
                let model = clicked.to_management_view_model();
                assert_eq!(
                    model.actions[index].0,
                    format!(
                        "[{shortcut}] {}",
                        action_label(
                            &clicked.pages.management.management_state.snapshot.rows[0].actions[0]
                        )
                    )
                );
                ui::management_layout(clicked.management_main(), &model).actions[index]
            } else {
                let more = clicked
                    .management_actions()
                    .iter()
                    .position(|(action, _)| action.id == "more_actions")
                    .unwrap();
                clicked.activate_management_action(more);
                let FormPurpose::Menu(items) = &clicked
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .unwrap()
                    .purpose
                else {
                    panic!("more menu");
                };
                let index = items
                    .iter()
                    .position(|(action, _)| action.id == id)
                    .unwrap();
                clicked.pages.management.management_state.choice_scroll = index;
                ui::management_layout(
                    clicked.management_main(),
                    &clicked.to_management_view_model(),
                )
                .choice_rows
                .into_iter()
                .find(|(item, _)| *item == index)
                .unwrap()
                .1
            };
            click_management_point(&mut clicked, (area.x, area.y));
            assert_eq!(
                keyed.pages.management.management_state.form,
                clicked.pages.management.management_state.form,
                "{kind:?}/{id}"
            );
            assert!(
                keyed.pages.management.management_state.form.is_some(),
                "{id} still collects operation fields before AutoAdmin approval"
            );
            assert!(
                keyed
                    .pages
                    .management
                    .management_state
                    .operation_job
                    .is_none(),
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
        keyed.pages.management.management_state.filter_input = "needle".into();
        clicked.pages.management.management_state.filter_input = "needle".into();
        keyed.handle_management_key(&key);
        let layout = ui::management_layout(
            clicked.management_main(),
            &clicked.to_management_view_model(),
        );
        match control {
            ui::ManagementControl::Search => {
                click_management_point(&mut clicked, (layout.filter.x, layout.filter.y))
            }
            ui::ManagementControl::Refresh | ui::ManagementControl::ClearSearch => {
                let area = layout
                    .controls
                    .iter()
                    .find(|(id, _)| *id == control)
                    .unwrap()
                    .1;
                click_management_point(&mut clicked, (area.x, area.y));
            }
            ui::ManagementControl::ApplySearch => {
                click_management_point(&mut clicked, (layout.filter.x, layout.filter.y));
                clicked.handle_management_key(&KeyInput::new(InputKey::Enter));
            }
            // Retain the existing keyboard commands even though these controls
            // now live in menus or the current-task entry.
            _ => clicked.management_touch_control(control),
        }
        assert_eq!(
            keyed.pages.management.management_state.query,
            clicked.pages.management.management_state.query,
            "{control:?}"
        );
        assert_eq!(
            keyed.pages.management.management_state.filter_input,
            clicked.pages.management.management_state.filter_input
        );
        assert_eq!(
            keyed.pages.management.management_state.filtering,
            clicked.pages.management.management_state.filtering
        );
        assert_eq!(
            keyed.pages.management.management_state.details_only,
            clicked.pages.management.management_state.details_only
        );
        assert_eq!(
            keyed.pages.management.management_state.terminal_mode,
            clicked.pages.management.management_state.terminal_mode
        );
    }
}

#[test]
fn management_shortcuts_cannot_bypass_disabled_actions_or_unrelated_modifiers() {
    let mut session = shortcut_action_state(ManagementKind::Processes, "kill");
    session.pages.management.management_state.snapshot.rows[0].actions[0].disabled_reason =
        Some("Protected process".into());
    for key in [InputKey::Char('k'), InputKey::Char('1')] {
        session.handle_management_key(&KeyInput::new(key));
        assert!(session.pages.management.management_state.form.is_none());
        assert_eq!(
            session.pages.management.management_state.status,
            i18n::tr!("management-problem-unavailable")
        );
        assert_eq!(
            session.pages.management.management_state.output,
            "Protected process"
        );
    }
    session.pages.management.management_state.snapshot.rows[0].actions[0].disabled_reason = None;
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
            assert!(session.pages.management.management_state.form.is_none());
            assert!(!session.pages.management.management_state.filtering);
            assert!(
                session
                    .pages
                    .management
                    .management_state
                    .query_job
                    .is_none()
            );
        }
    }
    let (job, _) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session.pages.management.management_state.operation_job = Some(job);
    assert!(!session.to_management_view_model().actions[0].1);
    session.handle_management_key(&KeyInput::new(InputKey::Char('k')));
    assert!(session.pages.management.management_state.form.is_none());
    session.handle_management_key(&KeyInput::new(InputKey::F(10)));
    assert!(
        matches!(session.pages.management.management_state.form.as_ref().unwrap().purpose, FormPurpose::Action(ref action, _) if action.id == "cancel_operation")
    );
}

#[test]
fn management_text_input_and_shortcut_repeats_do_not_trigger_actions() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.handle_management_key(&KeyInput::new(InputKey::Char('s')));
    session.handle_management_key(&KeyInput::new(InputKey::Char('s')).repeated());
    assert!(
        session
            .pages
            .management
            .management_state
            .filter_input
            .is_empty(),
        "held Search does not type into its new field"
    );
    for c in "srak1".chars() {
        session.handle_management_key(&KeyInput::new(InputKey::Char(c)));
    }
    assert_eq!(
        session.pages.management.management_state.filter_input,
        "srak1"
    );
    assert!(session.pages.management.management_state.form.is_none());
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    session.pages.management.management_state.snapshot.rows[0].actions[0]
        .fields
        .push(ManagementField {
            id: "text".into(),
            ..Default::default()
        });
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')).repeated());
    assert!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value
            .is_empty()
    );
    for c in "srak1".chars() {
        session.handle_management_key(&KeyInput::new(InputKey::Char(c)));
    }
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "srak1"
    );
    assert!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Char('t'),
        ui::KeyModifiers::CTRL,
    ));
    assert!(session.pages.management.management_state.terminal_mode);
    session.handle_management_key(
        &KeyInput::with_modifiers(InputKey::Char('t'), ui::KeyModifiers::CTRL).repeated(),
    );
    assert!(
        session.pages.management.management_state.terminal_mode,
        "held Output does not toggle again"
    );
}

#[test]
fn management_choice_enter_and_form_control_enter_preserve_input_and_required_checks() {
    let mut session = state();
    let (job, responses) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session.pages.management.management_state.operation_job = Some(job);
    session.pages.management.management_state.form = Some(ManagementEditor {
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
    assert_eq!(
        session.pages.management.management_state.choice_field,
        Some(0)
    );
    session.handle_management_key(&KeyInput::new(InputKey::End));
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    session.handle_management_key(&KeyInput::new(InputKey::Enter).repeated());
    assert!(session.pages.management.management_state.form.is_some());
    assert!(responses.try_recv().is_err());
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "Yes"
    );
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Enter,
        ui::KeyModifiers::CTRL,
    ));
    assert!(
        matches!(responses.try_recv().unwrap(), OperationInput::Answer { id, value } if id == "mock-question" && value == "Yes")
    );
    assert!(session.pages.management.management_state.form.is_none());

    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.pages.management.management_state.snapshot.rows[0].actions[0]
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
    assert!(session.pages.management.management_state.form.is_some());
    assert!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
    assert_eq!(
        session.pages.management.management_state.status,
        i18n::tr!("management-required")
    );
}

#[test]
fn duplicate_action_mnemonics_use_first_target_and_menu_keeps_each_target() {
    let mut session = shortcut_action_state(ManagementKind::Disks, "scan");
    session
        .pages
        .management
        .management_state
        .snapshot
        .actions
        .push(ManagementAction {
            id: "scan".into(),
            confirm: true,
            ..Default::default()
        });
    let model = session.to_management_view_model();
    assert_eq!(
        model.actions[0].0,
        format!(
            "[A] {}",
            action_label(&session.pages.management.management_state.snapshot.rows[0].actions[0])
        )
    );
    assert!(model.actions[0].0.starts_with("[A]"));
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    assert!(matches!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .purpose,
        FormPurpose::Action(_, Some(_))
    ));
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    let more = session
        .management_actions()
        .iter()
        .position(|(action, _)| action.id == "more_actions")
        .unwrap();
    session.activate_management_action(more);
    session.handle_management_choice_key(&KeyInput::new(InputKey::End));
    session.handle_management_choice_key(&KeyInput::new(InputKey::Enter));
    assert!(matches!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .purpose,
        FormPurpose::Action(_, None)
    ));
}

#[test]
fn management_menu_paging_keys_keep_secondary_actions_reachable() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.terminal_size = (60, 18);
    session.pages.management.management_state.snapshot.rows[0].actions = (0..8)
        .map(|index| ManagementAction {
            id: format!("mock-{index}"),
            label: format!("Mock {index}"),
            confirm: true,
            ..Default::default()
        })
        .collect();
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    assert_eq!(
        layout.actions.len(),
        1,
        "only the more-actions entry stays on the page"
    );
    click_management_point(&mut session, (layout.actions[0].x, layout.actions[0].y));
    let visible_rows = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    )
    .choice_rows
    .len()
    .max(1);
    session.handle_management_key(&KeyInput::new(InputKey::PageDown));
    let selected = visible_rows.min(7);
    assert_eq!(
        session.pages.management.management_state.choice_selected,
        selected
    );
    assert!(matches!(
        &session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .purpose,
        FormPurpose::Menu(_)
    ));
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    assert!(
        layout
            .choice_rows
            .iter()
            .any(|(index, _)| *index == selected)
    );
    session.handle_management_key(&KeyInput::new(InputKey::Home));
    assert_eq!(session.pages.management.management_state.choice_selected, 0);
    session.handle_management_key(&KeyInput::new(InputKey::End));
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    assert!(
        matches!(&session.pages.management.management_state.form.as_ref().unwrap().purpose, FormPurpose::Action(action,_) if action.id=="mock-7")
    );
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
        .pages
        .management
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
    assert!(state.pages.management.management_state.form.is_some());
    assert!(
        state
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn switching_applications_keeps_background_questions_and_operation() {
    let mut state = state();
    let original = state.active_page_id();
    let (job, _) = state.management_job();
    state.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    state.pages.management.management_state.operation_job = Some(job.clone());
    state.open_management(ManagementKind::Packages);
    assert_eq!(
        state.pages.management.management_state.kind,
        Some(ManagementKind::Packages)
    );
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
    assert!(state.pages.management.management_state.form.is_none());
    assert!(
        state
            .pages
            .management
            .get(original)
            .unwrap()
            .management_state
            .form
            .is_some()
    );
    state.close_page_instance(state.active_page_id());
    assert_eq!(state.active_page_id(), original);
    assert!(
        state
            .pages
            .management
            .management_state
            .operation_job
            .is_some()
    );
    assert_eq!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "Cancel"
    );
}

#[test]
fn terminal_ctrl_c_is_routed_to_task_and_configuration_cannot_be_dismissed() {
    let mut state = state();
    state.pages.management.management_state.terminal_mode = true;
    let key = KeyInput::from_label("Ctrl+C");
    assert!(matches!(
        state.route_key_input(&key).1,
        ShellCommand::ManagementKey(_)
    ));
    state.pages.management.management_state.form = Some(ManagementEditor {
        message_scroll: 0,
        title: "Configuration".into(),
        message: String::new(),
        fields: Vec::new(),
        selected: 0,
        purpose: FormPurpose::Answer("package-config-1".into()),
    });
    state.pages.management.management_state.terminal_mode = false;
    state.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert!(state.pages.management.management_state.form.is_some());
}

#[test]
fn changed_process_identity_invalidates_pressed_action() {
    let mut state = state();
    state
        .pages
        .management
        .management_state
        .snapshot
        .rows
        .push(ManagementRow {
            id: "42".into(),
            identity: BTreeMap::from([("start_time_ticks".into(), "100".into())]),
            actions: vec![ManagementAction {
                id: "kill".into(),
                label: "Kill".into(),
                primary: true,
                confirm: true,
                ..Default::default()
            }],
            ..Default::default()
        });
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let point = (layout.actions[0].x, layout.actions[0].y);
    let before = state.management_button_at(point);
    state.pages.management.management_state.snapshot.rows[0]
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
    state.enter_screen(ShellScreen::Management);
    state.pages.management.management_state.kind = Some(ManagementKind::Services);
    state.pages.management.management_state.query =
        Some(ManagementQuery::new(ManagementKind::Services));
    state
}

#[test]
fn terminal_button_reopens_finished_auto_admin_instead_of_a_second_input_surface() {
    let mut state = state();
    let (responses, _inputs) = mpsc::channel();
    let job = state
        .begin_auto_admin("Operation".into(), false, responses)
        .unwrap();
    state.pages.management.management_state.auto_admin_job = Some(job.clone());
    job.finish(Ok("Done".into()));
    state.close_auto_admin();
    assert!(!state.auto_admin_visible());
    state.management_touch_control(ui::ManagementControl::Terminal);
    assert!(state.auto_admin_visible());
    assert!(!state.pages.management.management_state.terminal_mode);
    assert!(!job.running());
}

#[test]
fn text_fields_take_priority_over_actions_and_keep_secrets_out_of_view_model() {
    let mut state = state();
    state.pages.management.management_state.form = Some(ManagementEditor {
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
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "r 1"
    );
    assert_eq!(
        state.to_management_view_model().form.unwrap().fields[0].value,
        "•••"
    );
    assert!(state.pages.management.management_state.query_job.is_none());
}

#[test]
fn choice_fields_reject_free_text_and_paste() {
    let mut state = state();
    state.pages.management.management_state.form = Some(ManagementEditor {
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
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "No"
    );
    state.handle_management_key(&KeyInput::new(InputKey::Right));
    assert_eq!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
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
    state.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    state.pages.management.management_state.operation_job = Some(job);
    state.poll_management();
    assert_eq!(
        state.pages.management.management_state.snapshot.backend,
        "authorized-inspection"
    );
    assert_eq!(
        state.pages.management.management_state.status,
        "Inspection complete"
    );
    assert!(state.pages.management.management_state.query_job.is_none());
}

#[test]
fn action_capture_requires_matching_release() {
    let mut state = state();
    state
        .pages
        .management
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
    assert!(state.pages.management.management_state.form.is_none());
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
    assert!(state.pages.management.management_state.form.is_some());
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
    state.set_navigation_path(vec![ShellScreen::Home]);
    state.open_management(ManagementKind::Packages);
    assert_eq!(state.active_screen(), ShellScreen::Home);
}

#[test]
fn management_scrollbar_drag_preserves_viewport_and_cancel_stops_capture() {
    let mut state = state();
    state.pages.management.management_state.snapshot.columns = vec!["Name".into()];
    state.pages.management.management_state.snapshot.rows = (0..100)
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
    assert_eq!(
        state.pages.management.management_state.scroll,
        100 - layout.list_capacity
    );
    state.poll_management();
    assert_eq!(
        state.pages.management.management_state.scroll,
        100 - layout.list_capacity
    );
    state.cancel_management_pointer_gesture();
    assert!(!state.management_pointer_drag_active());
}

#[test]
fn touch_choice_list_selects_without_cycling_or_submitting() {
    let mut state = state();
    state.pages.management.management_state.form = Some(ManagementEditor {
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
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "No"
    );
    assert_eq!(
        state.pages.management.management_state.choice_field,
        Some(0)
    );
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let choice = layout
        .choice_rows
        .iter()
        .find(|(index, _)| *index == 1)
        .unwrap()
        .1;
    state.handle_management_pointer(pointer(choice));
    assert_eq!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "Yes"
    );
    assert!(
        state
            .pages
            .management
            .management_state
            .choice_field
            .is_none()
    );
    assert!(state.pages.management.management_state.form.is_some());
}

#[test]
fn escape_asks_before_transferring_management_operation_to_the_session() {
    let mut state = state();
    state.set_navigation_path(vec![
        ShellScreen::Home,
        ShellScreen::Launcher,
        ShellScreen::Management,
    ]);
    let (job, _) = state.management_job();
    state.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    state.pages.management.management_state.operation_job = Some(job);
    let page = state.active_page_id();
    state.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert_eq!(state.active_screen(), ShellScreen::Management);
    assert!(state.to_notification_view_model().is_some());
    state.continue_page_tasks_in_background(page);
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::Launcher
    );
    assert!(state.pages.management.get(page).is_none());
    assert_eq!(state.detached_management_operations.len(), 1);
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
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Management),
        anchor: point,
    });
    assert!(state.management_button_at(point).is_none());
}

#[test]
fn management_shortcuts_route_to_the_foreground_popup_and_escape_closes_it() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.pages.common.active_popup = Some(ShellPopup {
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
        assert!(session.pages.management.management_state.form.is_none());
        assert!(!session.pages.management.management_state.filtering);
        assert!(
            session
                .pages
                .management
                .management_state
                .query_job
                .is_none()
        );
    }
    session.apply_input(InputEvent::from_key_label("Esc"));
    assert!(session.pages.common.active_popup.is_none());
    assert_eq!(session.active_screen(), ShellScreen::Management);
}

#[test]
fn global_notification_releases_management_scroll_capture() {
    let mut state = state();
    state.pages.management.management_state.scrollbar_grab =
        Some((ui::ManagementScrollTarget::Rows, 0));
    state.notify_modal(
        "Notice",
        "Stop dragging",
        ui::NotificationTone::Info,
        vec![],
    );
    assert!(
        state
            .pages
            .management
            .management_state
            .scrollbar_grab
            .is_none()
    );
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
        state.pages.management.management_state.form = Some(ManagementEditor {
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
        state.pages.management.management_state.choice_field = Some(0);
        state.pages.management.management_state.choice_scroll = 3;
        state.pages.management.management_state.choice_columns = 8;
        state.pages.management.management_state.form_field_scroll = Some(4);
        state.pages.management.management_state.scrollbar_grab =
            Some((ui::ManagementScrollTarget::Choices, 0));
        state.pages.management.management_state.received_snapshot = true;
        let (job, _) = state.management_job();
        job.0.events.lock().unwrap().push_back(event);
        state.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
        state.pages.management.management_state.operation_job = Some(job);
        state.poll_management();
        assert!(state.pages.management.management_state.form.is_none());
        assert!(
            state
                .pages
                .management
                .management_state
                .choice_field
                .is_none()
        );
        assert!(
            state
                .pages
                .management
                .management_state
                .form_field_scroll
                .is_none()
        );
        assert_eq!(state.pages.management.management_state.choice_scroll, 0);
        assert_eq!(state.pages.management.management_state.choice_columns, 0);
        assert!(!state.management_pointer_drag_active());
        assert!(!state.handle_management_choice_key(&KeyInput::new(InputKey::Right)));
    }
}

#[test]
fn compact_more_actions_makes_every_secondary_action_touch_reachable() {
    let mut state = state();
    state.terminal_size = (60, 18);
    state.pages.management.management_state.snapshot.actions = (0..6)
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
        assert!(layout.action_next.is_empty());
        state.handle_management_pointer(tap(layout.actions[0]));
        let mut layout =
            ui::management_layout(state.management_main(), &state.to_management_view_model());
        if !layout.choice_rows.iter().any(|(i, _)| *i == index) {
            let bar = *layout
                .scrollbars
                .iter()
                .find(|bar| bar.target == ui::ManagementScrollTarget::Choices)
                .unwrap();
            state.handle_management_pointer(MouseInput {
                position: ui::Point::new(bar.thumb.x, bar.thumb.y),
                kind: ui::MouseEventKind::Down(PointerButton::Left),
                modifiers: ui::KeyModifiers::NONE,
            });
            let end = ui::Point::new(bar.track.x, bar.track.bottom());
            state.handle_management_pointer(MouseInput {
                position: end,
                kind: ui::MouseEventKind::Drag(PointerButton::Left),
                modifiers: ui::KeyModifiers::NONE,
            });
            state.handle_management_pointer(MouseInput {
                position: end,
                kind: ui::MouseEventKind::Up(PointerButton::Left),
                modifiers: ui::KeyModifiers::NONE,
            });
            layout =
                ui::management_layout(state.management_main(), &state.to_management_view_model());
        }
        let row = layout
            .choice_rows
            .iter()
            .find(|(i, _)| *i == index)
            .unwrap()
            .1;
        state.handle_management_pointer(tap(row));
        assert!(
            matches!(&state.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Action(action,_) if action.id==format!("action-{index}"))
        );
        state.cancel_management_form();
    }
}

#[test]
fn more_menu_keyboard_and_touch_cannot_execute_a_disabled_item() {
    for by_touch in [false, true] {
        let mut state = state();
        state.pages.management.management_state.snapshot.actions = vec![ManagementAction {
            id: "remove".into(),
            label: "Remove".into(),
            confirm: true,
            disabled_reason: Some("Protected object".into()),
            ..Default::default()
        }];
        state.activate_management_action(0);
        assert!(matches!(
            &state
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap()
                .purpose,
            FormPurpose::Menu(_)
        ));
        if by_touch {
            let layout =
                ui::management_layout(state.management_main(), &state.to_management_view_model());
            let region = layout.choice_rows[0].1;
            state.handle_management_pointer(MouseInput {
                position: ui::Point::new(region.x, region.y),
                kind: ui::MouseEventKind::Click(PointerButton::Left),
                modifiers: ui::KeyModifiers::NONE,
            });
        } else {
            state.handle_management_key(&KeyInput::new(InputKey::Space));
        }
        assert!(matches!(
            &state
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap()
                .purpose,
            FormPurpose::Menu(_)
        ));
        assert!(
            state
                .pages
                .management
                .management_state
                .operation_job
                .is_none()
        );
        assert_eq!(
            state.pages.management.management_state.choice_field,
            Some(0)
        );
        state.handle_management_key(&KeyInput::new(InputKey::Escape));
        assert!(state.pages.management.management_state.form.is_none());
        assert_eq!(state.active_screen(), ShellScreen::Management);
    }
}

#[test]
fn main_actions_are_limited_to_two_and_tab_enter_reaches_the_more_menu() {
    let mut state = state();
    state.pages.management.management_state.snapshot.actions = (0..5)
        .map(|index| ManagementAction {
            id: format!("action-{index}"),
            label: format!("Action {index}"),
            primary: true,
            confirm: true,
            ..Default::default()
        })
        .collect();
    let actions = state.management_actions();
    assert_eq!(actions.len(), 3);
    assert_eq!(actions[2].0.id, "more_actions");
    state.handle_management_key(&KeyInput::new(InputKey::Tab));
    state.handle_management_key(&KeyInput::new(InputKey::End));
    assert_eq!(state.pages.management.management_state.selected_action, 2);
    state.handle_management_key(&KeyInput::new(InputKey::Enter));
    assert!(
        matches!(&state.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Menu(items) if items.len()==5)
    );
    state.handle_management_key(&KeyInput::new(InputKey::End));
    state.handle_management_key(&KeyInput::new(InputKey::Space));
    assert!(
        matches!(&state.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Action(action,_) if action.id=="action-4")
    );
}

#[test]
fn more_menu_shortcuts_open_the_named_action_and_held_keys_do_not_type_into_its_form() {
    let mut state = shortcut_action_state(ManagementKind::Disks, "scan");
    state.pages.management.management_state.snapshot.rows[0].actions[0].primary = false;
    let more = state
        .management_actions()
        .iter()
        .position(|(action, _)| action.id == "more_actions")
        .unwrap();
    state.activate_management_action(more);
    assert!(
        state
            .to_management_view_model()
            .form
            .unwrap()
            .choice
            .unwrap()
            .values[0]
            .ends_with("    A")
    );
    state.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    assert!(
        matches!(&state.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Action(action,_) if action.id=="scan")
    );
    state.handle_management_key(&KeyInput::new(InputKey::Char('a')).repeated());
    assert!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value
            .is_empty()
    );
    assert!(
        state
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
}

fn package_list_state() -> ShellSession {
    let mut session = state();
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.pages.management.management_state.kind = Some(ManagementKind::Packages);
    session.pages.management.management_state.query =
        Some(ManagementQuery::new(ManagementKind::Packages));
    session.pages.management.management_state.snapshot = ManagementSnapshot {
        backend: "apt/dpkg".into(),
        columns: vec![
            "Package".into(),
            "Installed".into(),
            "Available".into(),
            "Description".into(),
        ],
        rows: ["alpha:amd64", "bash:amd64", "omega:amd64"]
            .into_iter()
            .map(|id| ManagementRow {
                id: id.into(),
                cells: vec![id.into(), "1".into(), String::new(), "Summary".into()],
                actions: vec![ManagementAction {
                    id: "remove".into(),
                    label: "Remove".into(),
                    privileged: true,
                    confirm: true,
                    primary: true,
                    ..Default::default()
                }],
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    session.pages.management.management_state.selected = 1;
    session
}

#[test]
fn all_package_subviews_keep_navigation_after_empty_or_failed_queries() {
    for scope in [
        "installed",
        "search",
        "updates",
        "sources",
        "conflicts",
        "status",
    ] {
        let mut session = package_list_state();
        session.pages.management.management_state.snapshot = ManagementSnapshot::default();
        session
            .pages
            .management
            .management_state
            .query
            .as_mut()
            .unwrap()
            .scope = scope.into();
        let actions = session.all_management_actions();
        for destination in [
            "search",
            "installed",
            "updates",
            "sources",
            "conflicts",
            "status",
        ] {
            assert_eq!(
                actions
                    .iter()
                    .filter(|(action, _)| action.id == format!("scope_{destination}"))
                    .count(),
                1
            );
        }
        assert!(
            session
                .to_management_view_model()
                .title
                .contains(&management_label(&format!("scope-{scope}"), scope))
        );
        session.handle_management_key(&KeyInput::new(InputKey::F(7)));
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .query
                .as_ref()
                .unwrap()
                .scope,
            "installed"
        );
        assert!(
            session
                .pages
                .management
                .management_state
                .operation_job
                .is_none()
        );
    }
}

#[test]
fn switching_package_subviews_clears_old_file_target_filter_and_focus() {
    let mut session = package_list_state();
    let query = session
        .pages
        .management
        .management_state
        .query
        .as_mut()
        .unwrap();
    query.scope = "conflicts".into();
    query.target = Some("/etc/demo.conf.dpkg-dist".into());
    query.filter = "demo.conf".into();
    query.options.insert("scope-only".into(), "old".into());
    session.pages.management.management_state.filter_input = "demo.conf".into();
    session.pages.management.management_state.filtering = true;
    session.pages.management.management_state.details_only = true;
    session.pages.management.management_state.terminal_mode = true;
    session.pages.management.management_state.selected_action = 3;
    session.pages.management.management_state.actions_focused = true;
    session.perform_management_action(
        ManagementAction {
            id: "scope_installed".into(),
            ..Default::default()
        },
        None,
    );
    let query = session
        .pages
        .management
        .management_state
        .query
        .as_ref()
        .unwrap();
    assert_eq!(query.scope, "installed");
    assert!(query.target.is_none() && query.filter.is_empty() && query.options.is_empty());
    assert!(
        session
            .pages
            .management
            .management_state
            .filter_input
            .is_empty()
    );
    assert!(
        !session.pages.management.management_state.filtering
            && !session.pages.management.management_state.details_only
            && !session.pages.management.management_state.terminal_mode
    );
    assert_eq!(session.pages.management.management_state.selected_action, 0);
    assert!(!session.pages.management.management_state.actions_focused);
    assert!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
}

#[test]
fn package_details_keep_all_rows_and_escape_returns_to_the_list() {
    for open_key in [InputKey::Enter, InputKey::F(4)] {
        let mut session = package_list_state();
        let before = session.pages.management.management_state.snapshot.clone();
        session.handle_management_key(&KeyInput::new(open_key));
        assert!(session.pages.management.management_state.details_only);
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .query
                .as_ref()
                .unwrap()
                .target
                .as_deref(),
            Some("bash:amd64")
        );
        let mut snapshot = before.clone();
        let mut detailed = snapshot.rows[1].clone();
        detailed.detail = vec![("Description".into(), "A useful shell".into())];
        merge_package_details(
            &mut snapshot,
            "bash:amd64",
            Ok(ManagementSnapshot {
                rows: vec![detailed.clone()],
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(snapshot.rows.len(), before.rows.len());
        assert_eq!(snapshot.rows[0], before.rows[0]);
        assert_eq!(snapshot.rows[1], detailed);
        assert_eq!(snapshot.rows[2], before.rows[2]);
        let (job, _) = session.management_job();
        *job.0.snapshot.lock().unwrap() = Some(Ok(snapshot));
        session.register_management_job(&job, TaskDomain::ManagementQuery, TaskClass::Query);
        session.pages.management.management_state.query_job = Some(job);
        session.poll_management();
        assert_eq!(session.pages.management.management_state.selected, 1);
        assert!(
            session
                .to_management_view_model()
                .details
                .contains("A useful shell")
        );
        session.handle_management_key(&KeyInput::new(InputKey::Escape));
        assert!(!session.pages.management.management_state.details_only);
        assert!(
            session
                .pages
                .management
                .management_state
                .query
                .as_ref()
                .unwrap()
                .target
                .is_none()
        );
        assert_eq!(session.active_screen(), ShellScreen::Management);
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .snapshot
                .rows
                .len(),
            3
        );
        assert!(!session.pages.management.management_state.actions_focused);
    }
}

#[test]
fn package_detail_navigation_queries_the_new_selection_and_list_views_keep_search_text() {
    let mut session = package_list_state();
    session.pages.management.management_state.filter_input = "shell".into();
    session
        .pages
        .management
        .management_state
        .query
        .as_mut()
        .unwrap()
        .filter = "shell".into();
    session.perform_management_action(
        ManagementAction {
            id: "scope_search".into(),
            ..Default::default()
        },
        None,
    );
    assert_eq!(
        session.pages.management.management_state.filter_input,
        "shell"
    );
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .filter,
        "shell"
    );
    session.pages.management.management_state.selected = 1;
    session.handle_management_key(&KeyInput::new(InputKey::F(4)));
    session.handle_management_key(&KeyInput::new(InputKey::Down));
    assert_eq!(session.pages.management.management_state.selected, 2);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .target
            .as_deref(),
        Some("omega:amd64")
    );
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .target
            .is_none()
    );
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .filter,
        "shell"
    );
}

#[test]
fn unavailable_package_detail_keeps_local_rows_and_exposes_the_error() {
    let mut session = package_list_state();
    merge_package_details(
        &mut session.pages.management.management_state.snapshot,
        "bash:amd64",
        Err(ManagementError::Unavailable(
            "Repository cache is missing".into(),
        )),
    )
    .unwrap();
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .snapshot
            .rows
            .len(),
        3
    );
    assert_eq!(
        session.pages.management.management_state.snapshot.rows[1].actions[0].id,
        "remove"
    );
    assert!(
        session
            .to_management_view_model()
            .details
            .contains("Repository cache is missing")
    );
    assert_eq!(
        merge_package_details(
            &mut session.pages.management.management_state.snapshot,
            "bash:amd64",
            Err(ManagementError::Cancelled)
        ),
        Err(ManagementError::Cancelled)
    );
}

#[test]
fn package_navigation_remains_enabled_during_a_background_operation() {
    let mut session = package_list_state();
    let (job, _) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session.pages.management.management_state.operation_job = Some(job.clone());
    let more = session
        .management_actions()
        .iter()
        .position(|(action, _)| action.id == "more_actions")
        .unwrap();
    session.activate_management_action(more);
    let model = session.to_management_view_model();
    let FormPurpose::Menu(items) = &session
        .pages
        .management
        .management_state
        .form
        .as_ref()
        .unwrap()
        .purpose
    else {
        panic!("expected menu");
    };
    let choice = model.form.unwrap().choice.unwrap();
    let installed = items
        .iter()
        .position(|(action, _)| action.id == "scope_installed")
        .unwrap();
    assert!(!choice.disabled[installed]);
    let remove = items
        .iter()
        .position(|(action, _)| action.id == "remove")
        .unwrap();
    assert!(choice.disabled[remove]);
    session.handle_management_key(&KeyInput::new(InputKey::F(7)));
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .scope,
        "installed"
    );
    assert_eq!(
        session.pages.management.management_state.operation_job,
        Some(job)
    );
}

#[test]
fn configuration_snapshots_and_completion_preserve_the_package_page() {
    for action in ["history", "read"] {
        let mut session = package_list_state();
        let before = session.pages.management.management_state.snapshot.clone();
        session
            .pages
            .management
            .management_state
            .configuration_operation = true;
        session.pages.management.management_state.received_snapshot = true;
        session.pages.editor.editor_config.pending_action = Some(action.into());
        let (job, _) = session.management_job();
        job.0.events.lock().unwrap().extend([
            OperationEvent::Snapshot {
                snapshot: ManagementSnapshot {
                    backend: "config-history".into(),
                    rows: vec![ManagementRow {
                        id: "backup-1".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            },
            OperationEvent::Completed {
                message: "Configuration read".into(),
            },
        ]);
        session.register_management_job(
            &job,
            TaskDomain::ManagementOperation,
            TaskClass::Operation,
        );
        session.pages.management.management_state.operation_job = Some(job);
        session.poll_management();
        assert_eq!(session.pages.management.management_state.snapshot, before);
        assert!(session.pages.management.management_state.received_snapshot);
        assert!(session.pages.management.management_state.outcome.is_none());
        assert!(
            !session
                .pages
                .management
                .management_state
                .status
                .contains("Configuration read")
        );
        if action == "history" {
            assert_eq!(
                session.pages.editor.editor_config.history.rows[0].id,
                "backup-1"
            );
            session.cancel_management_form();
            assert!(session.pages.management.management_state.form.is_none());
        }
        assert_eq!(session.active_screen(), ShellScreen::Management);
    }
}

#[test]
fn all_management_modules_sort_headers_and_keep_action_target_after_refresh() {
    for kind in [
        ManagementKind::Services,
        ManagementKind::Processes,
        ManagementKind::Packages,
        ManagementKind::Network,
        ManagementKind::Disks,
        ManagementKind::Users,
        ManagementKind::SystemConfig,
    ] {
        let mut session = shortcut_action_state(kind, "inspect");
        session.terminal_size = (120, 32);
        let mut row = session.pages.management.management_state.snapshot.rows[0].clone();
        row.cells = vec!["Zulu".into(), "2".into()];
        let mut other = row.clone();
        other.id = "other".into();
        other.cells = vec!["alpha".into(), "10".into()];
        other.identity.insert("identity".into(), "other".into());
        session.pages.management.management_state.snapshot.columns =
            vec!["Name".into(), "Count".into()];
        session.pages.management.management_state.snapshot.rows = vec![row, other];
        let original = session.pages.management.management_state.snapshot.clone();
        let layout = ui::management_layout(
            session.management_main(),
            &session.to_management_view_model(),
        );
        assert!(layout.actions_panel.is_empty());
        for area in layout.actions.iter().filter(|area| !area.is_empty()) {
            assert_eq!(area.intersection(layout.detail_actions_panel), *area);
        }
        let header = layout.headers[0].1;
        click_management_point(&mut session, (header.x, header.y));
        assert_eq!(
            session.pages.management.management_state.snapshot.rows[0].id,
            "other"
        );
        assert_eq!(
            session.pages.management.management_state.snapshot.rows
                [session.pages.management.management_state.selected]
                .id,
            "mock-selected-item"
        );
        click_management_point(&mut session, (header.x, header.y));
        assert_eq!(
            session.pages.management.management_state.snapshot.rows[0].id,
            "mock-selected-item"
        );
        session.sort_management(1);
        assert_eq!(
            session.pages.management.management_state.snapshot.rows[0].cells[1],
            "2"
        );
        session.sort_management(1);
        let (job, _) = session.management_job();
        job.0
            .events
            .lock()
            .unwrap()
            .push_back(OperationEvent::Snapshot { snapshot: original });
        session.register_management_job(
            &job,
            TaskDomain::ManagementOperation,
            TaskClass::Operation,
        );
        session.pages.management.management_state.operation_job = Some(job);
        session.poll_management();
        assert_eq!(
            session.pages.management.management_state.snapshot.rows[0].cells[1],
            "10"
        );
        assert_eq!(
            session.pages.management.management_state.snapshot.rows
                [session.pages.management.management_state.selected]
                .identity["identity"],
            "mock"
        );
    }
}
