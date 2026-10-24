use super::*;

#[test]
fn homebrew_advanced_actions_keep_refresh_without_linux_only_views() {
    let mut session = package_list_state();
    session.pages.management.management_state.snapshot.backend = "brew".into();
    session
        .pages
        .management
        .management_state
        .snapshot
        .actions
        .push(ManagementAction {
            id: "refresh".into(),
            confirm: true,
            ..Default::default()
        });
    let actions = session.package_actions();
    assert!(actions.iter().any(|(action, _)| action.id == "refresh"));
    let workspace = session.package_workspace_model(&actions);
    assert!(workspace.advanced_toggle.is_some());
    assert!(
        workspace
            .advanced_actions
            .iter()
            .any(|index| actions[*index].0.id == "refresh")
    );
    assert!(!actions.iter().any(|(action, _)| matches!(
        action.id.as_str(),
        "scope_sources" | "scope_conflicts" | "scope_status"
    )));
}

#[test]
fn package_actions_require_aa_approval_without_system_privileges() {
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "tundra-package-approval-{}-{}",
        std::process::id(),
        PageInstanceId::new().get()
    ));
    let (runtime, process) =
        watchdog::WatchdogRuntime::start_isolated(watchdog::WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("state"),
            "package-approval-test",
            "test",
        ))
        .unwrap();
    let watchdog = process.register_app(shell_watchdog_descriptor()).unwrap();
    let mut session = state();
    session.settings_task_runtime = ShellSettingsTaskRuntime::new_managed(watchdog);
    session.pages.management.management_state.kind = Some(ManagementKind::Packages);
    session.perform_management_action(
        ManagementAction {
            id: "invalid-test-action".into(),
            requires_approval: true,
            privileged: false,
            ..Default::default()
        },
        None,
    );
    let job = session
        .pages
        .management
        .management_state
        .auto_admin_job
        .clone()
        .unwrap();
    assert_eq!(job.phase(), ::auto_admin::JobPhase::Waiting);
    job.decide(false);
    runtime.shutdown().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
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

#[cfg(windows)]
#[test]
fn windows_package_actions_have_no_empty_advanced_menu_or_f3_install() {
    let mut session = shortcut_action_state(ManagementKind::Packages, "install");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    let actions = session.management_actions();
    let workspace = session.package_workspace_model(&actions);
    assert_eq!(workspace.advanced_toggle, None);
    assert!(workspace.advanced_actions.is_empty());
    assert_eq!(actions[3].0.id, "install");
    assert!(workspace.primary_actions.contains(&3));
    session.handle_management_key(&KeyInput::new(InputKey::F(3)));
    assert!(
        !session
            .pages
            .management
            .management_state
            .package_advanced_open
    );
    assert!(session.pages.management.management_state.form.is_none());
    session.handle_management_key(&KeyInput::new(InputKey::Char('i')));
    assert!(session.pages.management.management_state.form.is_some());
}

#[cfg(windows)]
#[test]
fn windows_package_completion_refreshes_the_current_view_without_a_helper_snapshot() {
    for success in [false, true] {
        let mut session = package_list_state();
        let state = &mut session.pages.management.management_state;
        state.received_snapshot = true;
        let query = state.query.as_mut().unwrap();
        query.scope = "updates".into();
        query.filter = "demo".into();
        let expected_query = state.query.clone();
        let (job, _) = session.management_job();
        job.0.events.lock().unwrap().push_back(if success {
            OperationEvent::Completed {
                message: "Updated".into(),
            }
        } else {
            OperationEvent::Failed {
                message: "Update failed".into(),
            }
        });
        session.register_management_job(
            &job,
            TaskDomain::ManagementOperation,
            TaskClass::Operation,
        );
        session.pages.management.management_state.operation_job = Some(job);
        session.poll_management();
        let state = &session.pages.management.management_state;
        assert!(state.operation_job.is_none());
        assert_eq!(state.query, expected_query);
        assert_eq!(state.refresh_requested, success);
        assert_eq!(
            state.status,
            if success {
                i18n::tr!("management-worker-unavailable")
            } else {
                "Update failed".into()
            }
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
fn package_shortcuts_reach_primary_actions_and_advanced_pages_without_pointer_input() {
    let mut session = shortcut_action_state(ManagementKind::Packages, "install");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.terminal_size = (140, 40);
    session.handle_management_key(&KeyInput::new(InputKey::Char('i')));
    assert!(
        matches!(session.pages.management.management_state.form.as_ref().map(|form| &form.purpose), Some(FormPurpose::Action(action, Some(row))) if action.id == "install" && row.id == "mock-selected-item")
    );
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert!(session.pages.management.management_state.form.is_none());
    session.handle_management_key(&KeyInput::new(InputKey::F(3)));
    assert!(
        session
            .pages
            .management
            .management_state
            .package_advanced_open
    );
    session.handle_management_key(&KeyInput::new(InputKey::Char('5')));
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
#[cfg(target_os = "linux")]
fn package_navigation_and_shortcuts_work_while_the_search_field_is_focused() {
    use ui::components::WorkspaceFocus;
    let mut session = package_list_state();
    session.handle_management_key(&KeyInput::new(InputKey::Char('/')));
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::Search
    );
    assert!(session.pages.management.management_state.filtering);
    session.handle_management_key(&KeyInput::new(InputKey::Tab));
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
    session.pages.management.management_state.snapshot.rows[1].detail = vec![(
        "Description".into(),
        format!("{}\n", "长描述".repeat(100)).repeat(80),
    )];
    session.handle_management_key(&KeyInput::new(InputKey::F(4)));
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::Details
    );
    session.handle_management_key(&KeyInput::new(InputKey::Down));
    session.handle_management_key(&KeyInput::new(InputKey::Right));
    assert_eq!(session.pages.management.management_state.details_scroll, 1);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .details_horizontal_scroll,
        1
    );
    session.handle_management_key(&KeyInput::new(InputKey::Up));
    session.handle_management_key(&KeyInput::new(InputKey::Left));
    assert_eq!(session.pages.management.management_state.details_scroll, 0);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .details_horizontal_scroll,
        0
    );
    session.handle_management_key(&KeyInput::new(InputKey::End));
    assert!(session.pages.management.management_state.details_scroll > 0);
    assert_eq!(session.pages.management.management_state.selected, 1);
    session.handle_management_key(&KeyInput::new(InputKey::Home));
    assert_eq!(session.pages.management.management_state.details_scroll, 0);
    session
        .pages
        .management
        .management_state
        .details_horizontal_scroll = u16::MAX;
    session.handle_management_key(&KeyInput::new(InputKey::Right));
    let horizontal_end = session
        .pages
        .management
        .management_state
        .details_horizontal_scroll;
    assert!(horizontal_end > 0 && horizontal_end < u16::MAX);
    session.handle_management_key(&KeyInput::new(InputKey::Right));
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .details_horizontal_scroll,
        horizontal_end
    );
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .details_horizontal_scroll,
        0
    );
    session.handle_management_key(&KeyInput::new(InputKey::Down));
    assert_eq!(
        session.pages.management.management_state.workspace_focus,
        WorkspaceFocus::List
    );
    assert_eq!(session.pages.management.management_state.selected, 2);
}

#[test]
fn package_details_without_overflow_keep_scroll_and_selection_unchanged() {
    let mut session = package_list_state();
    session.terminal_size = (140, 40);
    session.handle_management_key(&KeyInput::new(InputKey::F(4)));
    for key in [
        InputKey::Up,
        InputKey::Down,
        InputKey::Left,
        InputKey::Right,
    ] {
        session.handle_management_key(&KeyInput::new(key));
        let state = &session.pages.management.management_state;
        assert_eq!(state.selected, 1);
        assert_eq!(
            state.query.as_ref().unwrap().target.as_deref(),
            Some("bash:amd64")
        );
        assert_eq!(state.details_scroll, 0);
        assert_eq!(state.details_horizontal_scroll, 0);
    }
}

#[test]
fn package_tab_cycles_main_pages_in_both_directions_from_every_region() {
    use ui::components::WorkspaceFocus;
    for focus in [
        WorkspaceFocus::Navigation,
        WorkspaceFocus::Search,
        WorkspaceFocus::List,
        WorkspaceFocus::Details,
        WorkspaceFocus::Actions,
        WorkspaceFocus::Advanced,
    ] {
        for key in [
            KeyInput::new(InputKey::Tab),
            KeyInput::new(InputKey::BackTab),
            KeyInput::with_modifiers(InputKey::Tab, ui::KeyModifiers::SHIFT),
        ] {
            let mut session = package_list_state();
            session.focus_package_workspace(focus);
            let scopes = if key.key == InputKey::Tab && !key.modifiers.shift {
                ["updates", "search", "installed"]
            } else {
                ["search", "updates", "installed"]
            };
            for scope in scopes {
                session.handle_management_key(&key);
                let state = &session.pages.management.management_state;
                assert_eq!(state.query.as_ref().unwrap().scope, scope);
                assert_eq!(state.filtering, scope == "search");
                assert!(!state.actions_focused);
                assert!(!state.details_only);
            }
        }
    }
}

#[test]
fn package_arrows_select_rows_regardless_of_previous_focus() {
    use ui::components::WorkspaceFocus;
    for focus in [
        WorkspaceFocus::Navigation,
        WorkspaceFocus::Search,
        WorkspaceFocus::List,
        WorkspaceFocus::Details,
        WorkspaceFocus::Actions,
        WorkspaceFocus::Advanced,
    ] {
        let mut session = package_list_state();
        session.focus_package_workspace(focus);
        session.handle_management_key(&KeyInput::new(InputKey::Down));
        assert_eq!(session.pages.management.management_state.selected, 2);
        assert_eq!(
            session.pages.management.management_state.workspace_focus,
            WorkspaceFocus::List
        );
        assert!(!session.pages.management.management_state.filtering);
        assert!(!session.pages.management.management_state.actions_focused);
        session.handle_management_key(&KeyInput::new(InputKey::Up));
        assert_eq!(session.pages.management.management_state.selected, 1);
    }
}
