use super::*;

#[cfg(target_os = "linux")]
#[test]
fn opening_packages_defaults_to_installed_with_navigation_in_display_order() {
    let mut session = state();
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.open_management(ManagementKind::Packages);
    let state = &session.pages.management.management_state;
    assert_eq!(state.query.as_ref().unwrap().scope, "installed");
    assert!(!state.filtering);
    assert_eq!(state.workspace_focus, ui::components::WorkspaceFocus::List);
    assert_eq!(
        session
            .to_management_view_model()
            .workspace
            .unwrap()
            .active_navigation,
        0
    );
    for (key, scope, active) in [(6, "search", 2), (8, "updates", 1), (7, "installed", 0)] {
        session.handle_management_key(&KeyInput::new(InputKey::F(key)));
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .query
                .as_ref()
                .unwrap()
                .scope,
            scope
        );
        assert_eq!(
            session
                .to_management_view_model()
                .workspace
                .unwrap()
                .active_navigation,
            active
        );
    }
}

#[test]
fn package_shortcuts_follow_the_actions_on_the_current_page() {
    for scope in ["updates", "installed"] {
        let mut session = shortcut_action_state(ManagementKind::Packages, "remove");
        session
            .pages
            .management
            .management_state
            .query
            .as_mut()
            .unwrap()
            .scope = scope.into();
        session.handle_management_key(&KeyInput::new(InputKey::Char('x')));
        assert_eq!(
            session.pages.management.management_state.form.is_some(),
            scope == "installed"
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
fn package_pointer_and_touch_can_switch_pages_and_open_their_advanced_views() {
    for hover in [false, true] {
        for (navigation, scope, advanced_scope) in [
            (0, "installed", "conflicts"),
            (1, "updates", "status"),
            (2, "search", "sources"),
        ] {
            let mut session = package_list_state();
            session.terminal_size = (140, 40);
            for action in [navigation, 3, 4] {
                let model = session.to_management_view_model();
                let rect = ui::management_layout(session.management_main(), &model).actions[action];
                assert!(!rect.is_empty());
                if hover {
                    let _ = session.prepare_button_input(
                        InputEvent::mouse_moved((rect.x, rect.y)),
                        Instant::now(),
                    );
                }
                click_management_point(&mut session, (rect.x, rect.y));
                if action == navigation {
                    assert_eq!(
                        session
                            .pages
                            .management
                            .management_state
                            .query
                            .as_ref()
                            .unwrap()
                            .scope,
                        scope
                    );
                }
            }
            assert_eq!(
                session
                    .pages
                    .management
                    .management_state
                    .query
                    .as_ref()
                    .unwrap()
                    .scope,
                advanced_scope
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

#[test]
fn package_keyboard_reaches_primary_actions_and_advanced_pages_without_pointer_input() {
    use ui::components::WorkspaceFocus;
    let mut session = shortcut_action_state(ManagementKind::Packages, "install");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.terminal_size = (140, 40);
    for expected in [WorkspaceFocus::Details, WorkspaceFocus::Actions] {
        session.handle_management_key(&KeyInput::new(InputKey::Tab));
        assert_eq!(
            session.pages.management.management_state.workspace_focus,
            expected
        );
    }
    session.handle_management_key(&KeyInput::new(InputKey::Space));
    assert!(
        matches!(session.pages.management.management_state.form.as_ref().map(|form| &form.purpose), Some(FormPurpose::Action(action, Some(row))) if action.id == "install" && row.id == "mock-selected-item")
    );
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert!(session.pages.management.management_state.form.is_none());
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::Actions
    );
    session.handle_management_key(&KeyInput::new(InputKey::Tab));
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::Advanced
    );
    session.handle_management_key(&KeyInput::new(InputKey::End));
    assert_eq!(session.pages.management.management_state.selected_action, 3);
    session.handle_management_key(&KeyInput::new(InputKey::Space));
    assert!(
        session
            .pages
            .management
            .management_state
            .package_advanced_open
    );
    session.handle_management_key(&KeyInput::new(InputKey::Down));
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .scope,
        "conflicts"
    );
    assert!(
        session
            .pages
            .management
            .management_state
            .snapshot
            .rows
            .is_empty()
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

#[test]
fn package_navigation_and_shortcuts_work_while_the_search_field_is_focused() {
    use ui::components::WorkspaceFocus;
    let mut session = package_list_state();
    session.handle_management_key(&KeyInput::new(InputKey::BackTab));
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::Search
    );
    assert!(session.pages.management.management_state.filtering);
    session.handle_management_key(&KeyInput::new(InputKey::BackTab));
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::Navigation
    );
    session.handle_management_key(&KeyInput::new(InputKey::Down));
    session.handle_management_key(&KeyInput::new(InputKey::Space));
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .scope,
        "updates"
    );
    session.handle_management_key(&KeyInput::new(InputKey::Tab));
    assert!(session.pages.management.management_state.filtering);
    session.handle_management_key(&KeyInput::new(InputKey::F(6)));
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .scope,
        "search"
    );
    session.handle_management_key(&KeyInput::new(InputKey::F(3)));
    assert!(
        session
            .pages
            .management
            .management_state
            .package_advanced_open
    );
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::Advanced
    );
    let actions = session.management_actions();
    let workspace = session.package_workspace_model(&actions);
    assert!(
        workspace
            .advanced_actions
            .iter()
            .any(|index| actions[*index].0.id == "scope_sources")
    );
    assert!(!actions.iter().any(|(action, _)| matches!(
        action.id.as_str(),
        "scope_status" | "scope_conflicts" | "upgrade_all"
    )));
    session.handle_management_key(&KeyInput::new(InputKey::F(6)));
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert!(!session.pages.management.management_state.filtering);
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::List
    );
}

#[test]
fn package_details_have_independent_keyboard_scrolling_and_return_to_the_list() {
    use ui::components::WorkspaceFocus;
    let mut session = package_list_state();
    session.terminal_size = (140, 40);
    session.pages.management.management_state.snapshot.rows[1].detail =
        vec![("Description".into(), "Long description\n".repeat(80))];
    session.handle_management_key(&KeyInput::new(InputKey::Tab));
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::Details
    );
    session.handle_management_key(&KeyInput::new(InputKey::End));
    assert!(session.pages.management.management_state.details_scroll > 0);
    assert_eq!(session.pages.management.management_state.selected, 1);
    session.handle_management_key(&KeyInput::new(InputKey::Home));
    assert_eq!(session.pages.management.management_state.details_scroll, 0);
    session.handle_management_key(&KeyInput::new(InputKey::BackTab));
    session.handle_management_key(&KeyInput::new(InputKey::Down));
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::List
    );
    assert_eq!(session.pages.management.management_state.selected, 2);
}
