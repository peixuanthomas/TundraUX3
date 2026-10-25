use super::*;

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
    let mut session = package_list_state();
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
    session.select_package_row(2, true);
    session.handle_management_key(&KeyInput::new(InputKey::Home));
    let (action, row) = session
        .management_actions()
        .into_iter()
        .find(|(action, _)| action.id == "remove")
        .unwrap();
    assert!(row.is_none());
    let targets: Vec<platform::management::package_selection::PackageTarget> =
        serde_json::from_str(&action.values[platform::management::package_selection::KEY]).unwrap();
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
fn package_selection_survives_sorting_and_refresh_but_clears_on_a_different_list() {
    let mut session = package_list_state();
    session.select_package_row(2, true);
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
