use super::*;

pub(super) fn list_state(kind: ManagementKind) -> ShellSession {
    let mut session = package_list_state();
    if kind == ManagementKind::Packages {
        return session;
    }
    let state = &mut session.pages.management.management_state;
    state.kind = Some(kind);
    state.query = Some(ManagementQuery::new(kind));
    for (index, row) in state.snapshot.rows.iter_mut().enumerate() {
        let (id, action, identity) = match kind {
            ManagementKind::Services => {
                let id = format!("worker-{index}.service");
                (
                    id.clone(),
                    "start",
                    BTreeMap::from([("unit".into(), id), ("scope".into(), "system".into())]),
                )
            }
            ManagementKind::Processes => {
                let id = (101 + index).to_string();
                (
                    id.clone(),
                    "term",
                    BTreeMap::from([
                        ("pid".into(), id),
                        ("start_time_ticks".into(), "1000".into()),
                        ("uid".into(), "1000".into()),
                        ("boot_id".into(), "boot".into()),
                    ]),
                )
            }
            ManagementKind::ScheduledTasks => {
                let id = format!("systemd:system:worker-{index}.timer");
                (
                    id,
                    "pause",
                    BTreeMap::from([
                        ("source".into(), "systemd".into()),
                        ("scope".into(), "system".into()),
                        ("timer".into(), format!("worker-{index}.timer")),
                    ]),
                )
            }
            _ => unreachable!(),
        };
        row.id = id.clone();
        row.cells[0] = id;
        row.identity = identity;
        row.actions[0].id = action.into();
    }
    session
}

fn toggle(session: &mut ShellSession) {
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Enter,
        ui::KeyModifiers::SHIFT,
    ));
}

#[test]
fn physical_shift_events_reach_selection_and_repeats_do_not_toggle_it_again() {
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
    for kind in [
        ManagementKind::Packages,
        ManagementKind::Services,
        ManagementKind::Processes,
        ManagementKind::ScheduledTasks,
    ] {
        let mut session = list_state(kind);
        while session.notification_dismiss_active_modal_without_response() {}
        session.refresh_hit_map();
        let layout = ui::management_layout(
            session.management_main(),
            &session.to_management_view_model(),
        );
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            session.apply_input(crossterm_event_to_input(Event::Mouse(MouseEvent {
                kind,
                column: layout.list_rows.x,
                row: layout.list_rows.y + 2,
                modifiers: KeyModifiers::SHIFT,
            })));
        }
        assert_eq!(
            session.to_management_view_model().selected_rows,
            Some(vec![1, 2])
        );
        for (kind, expected) in [
            (KeyEventKind::Press, vec![1]),
            (KeyEventKind::Repeat, vec![1]),
            (KeyEventKind::Release, vec![1]),
        ] {
            session.apply_input(crossterm_event_to_input(Event::Key(KeyEvent {
                kind,
                ..KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)
            })));
            assert_eq!(
                session.to_management_view_model().selected_rows,
                Some(expected)
            );
        }
        session.apply_input(InputEvent::key(InputKey::F(1)));
        assert!(session.help_open);
        assert!(
            session
                .to_page_help_view_model()
                .sections
                .iter()
                .flat_map(|section| &section.rows)
                .any(|row| row.keys == "Shift+Enter")
        );
    }
}

#[test]
fn management_batches_keep_targets_disable_incompatible_actions_and_clear_on_filter_changes() {
    for kind in [
        ManagementKind::Services,
        ManagementKind::Processes,
        ManagementKind::ScheduledTasks,
    ] {
        let mut session = list_state(kind);
        session.select_management_row(2, true);
        session.handle_management_key(&KeyInput::new(InputKey::Home));
        let (action, row) = session
            .all_management_actions()
            .into_iter()
            .find(|(action, _)| platform::management::selection::supports(kind, &action.id))
            .unwrap();
        assert!(row.is_none());
        let targets: Vec<platform::management::selection::Target> =
            serde_json::from_str(&action.values[platform::management::selection::KEY]).unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets[0].id,
            session.pages.management.management_state.snapshot.rows[1].id
        );
        assert_eq!(
            targets[1].identity,
            session.pages.management.management_state.snapshot.rows[2].identity
        );
        assert!(action.privileged);
        session.sort_management(0);
        session.sort_management(0);
        assert_eq!(
            session.to_management_view_model().selected_rows,
            Some(vec![0, 1])
        );
        let selected_id = targets[1].id.clone();
        let row = session
            .pages
            .management
            .management_state
            .snapshot
            .rows
            .iter_mut()
            .find(|row| row.id == selected_id)
            .unwrap();
        row.actions[0].disabled_reason = Some("Unavailable".into());
        let (disabled, _) = session
            .all_management_actions()
            .into_iter()
            .find(|(candidate, _)| candidate.id == action.id)
            .unwrap();
        assert!(!session.management_action_enabled(&disabled));
        session.pages.management.management_state.filter_input = "changed filter".into();
        session.apply_management_filter();
        assert_eq!(session.to_management_view_model().selected_rows, None);
    }
}

#[test]
fn nice_uses_one_value_for_the_whole_selection_and_confirmation_lists_all_pids() {
    let mut session = list_state(ManagementKind::Processes);
    for (index, row) in session
        .pages
        .management
        .management_state
        .snapshot
        .rows
        .iter_mut()
        .enumerate()
    {
        row.actions.push(ManagementAction {
            id: "nice".into(),
            privileged: true,
            confirm: true,
            fields: vec![ManagementField {
                id: "nice".into(),
                value: index.to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    session.select_management_row(2, true);
    let (action, row) = session
        .all_management_actions()
        .into_iter()
        .find(|(action, _)| action.id == "nice")
        .unwrap();
    assert!(session.management_action_enabled(&action));
    session.activate_management_item(action, row);
    let form = session
        .pages
        .management
        .management_state
        .form
        .as_ref()
        .unwrap();
    assert_eq!(form.fields.len(), 1);
    assert!(form.message.contains("102") && form.message.contains("103"));
}

#[test]
fn refreshed_reused_process_ids_and_replaced_cron_lines_leave_the_selection() {
    for kind in [ManagementKind::Processes, ManagementKind::ScheduledTasks] {
        let mut session = list_state(kind);
        if kind == ManagementKind::ScheduledTasks {
            for row in &mut session.pages.management.management_state.snapshot.rows {
                row.identity.insert("result_key".into(), row.id.clone());
            }
        }
        session.select_management_row(2, true);
        let mut snapshot = session.pages.management.management_state.snapshot.clone();
        let key = if kind == ManagementKind::Processes {
            "start_time_ticks"
        } else {
            "result_key"
        };
        snapshot.rows[2]
            .identity
            .insert(key.into(), "replacement".into());
        let (job, _) = session.management_job();
        *job.0.snapshot.lock().unwrap() = Some(Ok(snapshot));
        session.register_management_job(&job, TaskDomain::ManagementQuery, TaskClass::Query);
        session.pages.management.management_state.query_job = Some(job);
        session.poll_management();
        assert_eq!(
            session.to_management_view_model().selected_rows,
            Some(vec![1])
        );
    }
}

#[test]
fn shift_click_and_enter_toggle_individual_packages_and_keep_keyboard_navigation() {
    let mut session = package_list_state();
    session.terminal_size = (140, 40);
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    let point = (layout.list_rows.x, layout.list_rows.y + 2);
    session.handle_management_pointer(
        MouseInput::new(
            point.0,
            point.1,
            ui::MouseEventKind::Down(PointerButton::Left),
        )
        .with_modifiers(ui::KeyModifiers::SHIFT),
    );
    assert_eq!(
        session.to_management_view_model().selected_rows,
        Some(vec![1, 2])
    );
    session.handle_management_key(&KeyInput::new(InputKey::Up));
    assert_eq!(
        session.to_management_view_model().selected_rows,
        Some(vec![1, 2])
    );
    toggle(&mut session);
    assert_eq!(
        session.to_management_view_model().selected_rows,
        Some(vec![2])
    );
    session.handle_management_key(&KeyInput::new(InputKey::Down));
    toggle(&mut session);
    assert_eq!(
        session.to_management_view_model().selected_rows,
        Some(vec![])
    );
    assert!(
        !session
            .management_actions()
            .iter()
            .any(|(action, _)| action.id == "remove")
    );
    toggle(&mut session);
    session.handle_management_key(&KeyInput::new(InputKey::Home));
    toggle(&mut session);
    assert_eq!(
        session.to_management_view_model().selected_rows,
        Some(vec![0, 2])
    );
    session.handle_management_pointer(MouseInput::new(
        point.0,
        point.1,
        ui::MouseEventKind::Down(PointerButton::Left),
    ));
    assert_eq!(session.to_management_view_model().selected_rows, None);
    assert_eq!(session.pages.management.management_state.selected, 2);
}

#[test]
fn batch_actions_capture_all_selected_ids_and_reject_incompatible_rows() {
    let mut session = package_list_state();
    session.select_management_row(2, true);
    session.handle_management_key(&KeyInput::new(InputKey::Home));
    let (action, row) = session
        .management_actions()
        .into_iter()
        .find(|(action, _)| action.id == "remove")
        .unwrap();
    assert!(row.is_none());
    let targets: Vec<platform::management::selection::Target> =
        serde_json::from_str(&action.values[platform::management::selection::KEY]).unwrap();
    assert_eq!(
        targets
            .iter()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>(),
        ["bash:amd64", "omega:amd64"]
    );
    let before = session.to_management_view_model().action_ids;
    session.pages.management.management_state.snapshot.rows[2]
        .identity
        .insert("installed_version".into(), "2".into());
    assert_ne!(session.to_management_view_model().action_ids, before);
    session.pages.management.management_state.snapshot.rows[2]
        .actions
        .clear();
    let actions = session.management_actions();
    let (action, _) = actions
        .iter()
        .find(|(action, _)| action.id == "remove")
        .unwrap();
    assert!(!session.management_action_enabled(action));
    session.activate_management_item(action.clone(), None);
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
fn selection_survives_sorting_and_refresh_but_clears_on_a_different_list() {
    let mut session = package_list_state();
    session.select_management_row(2, true);
    session.sort_management(0);
    session.sort_management(0);
    assert_eq!(
        session.to_management_view_model().selected_rows,
        Some(vec![0, 1])
    );
    let mut snapshot = session.pages.management.management_state.snapshot.clone();
    snapshot.rows.retain(|row| row.id != "omega:amd64");
    let (job, _) = session.management_job();
    *job.0.snapshot.lock().unwrap() = Some(Ok(snapshot));
    session.register_management_job(&job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(job);
    session.poll_management();
    assert_eq!(
        session.to_management_view_model().selected_rows,
        Some(vec![0])
    );
    session.handle_management_key(&KeyInput::new(InputKey::F(8)));
    assert_eq!(session.to_management_view_model().selected_rows, None);
}
