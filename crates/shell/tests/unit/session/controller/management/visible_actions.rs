use super::*;

#[test]
fn service_logs_and_persistent_expert_controls_remain_directly_reachable() {
    for size in [(108, 20), (140, 40)] {
        let mut session = shortcut_action_state(ManagementKind::Services, "stop");
        session.terminal_size = size;
        let row = &mut session.pages.management.management_state.snapshot.rows[0];
        row.actions = [
            "start",
            "stop",
            "restart",
            "view_logs",
            "reload",
            "enable",
            "disable",
            "edit_config",
            "show_dependencies",
        ]
        .into_iter()
        .map(|id| ManagementAction {
            id: id.into(),
            label: id.into(),
            primary: id == "stop",
            ..Default::default()
        })
        .collect();
        let second = row.clone();
        session
            .pages
            .management
            .management_state
            .snapshot
            .rows
            .push(ManagementRow {
                id: "another.service".into(),
                ..second
            });
        let actions = session.management_actions();
        let workspace = session.management_workspace_model(&actions).unwrap();
        assert!(
            workspace
                .primary_actions
                .iter()
                .any(|index| actions[*index].0.id == "view_logs")
        );
        assert!(
            !workspace
                .primary_actions
                .iter()
                .any(|index| actions[*index].0.id == "start")
        );
        session.handle_management_key(&KeyInput::new(InputKey::F(3)));
        assert!(session.pages.management.management_state.expert_open);
        let model = session.to_management_view_model();
        let layout = ui::management_layout(session.management_main(), &model);
        let workspace = model.workspace.unwrap();
        assert!(layout.list.height > 0);
        for index in workspace
            .primary_actions
            .iter()
            .chain(&workspace.advanced_actions)
        {
            assert!(
                !layout.actions[*index].is_empty(),
                "{size:?}: {}",
                actions[*index].0.id
            );
        }
        session.pages.management.management_state.selected = 1;
        assert!(
            session
                .to_management_view_model()
                .workspace
                .unwrap()
                .advanced_open
        );
    }
}

#[test]
fn scheduled_run_and_available_pause_or_resume_are_primary() {
    for enabled in ["pause", "resume"] {
        let mut session = shortcut_action_state(ManagementKind::ScheduledTasks, "run_now");
        session.pages.management.management_state.snapshot.rows[0]
            .actions
            .extend(["pause", "resume"].map(|id| ManagementAction {
                id: id.into(),
                disabled_reason: (id != enabled).then(|| "Not available in this state".into()),
                ..Default::default()
            }));
        session
            .pages
            .management
            .management_state
            .snapshot
            .actions
            .push(ManagementAction {
                id: "create_task".into(),
                ..Default::default()
            });
        let actions = session.management_actions();
        let workspace = session.management_workspace_model(&actions).unwrap();
        let primary = workspace
            .primary_actions
            .iter()
            .map(|index| actions[*index].0.id.as_str())
            .collect::<Vec<_>>();
        assert!(primary.contains(&"run_now"));
        assert!(primary.contains(&enabled));
        assert_eq!(
            primary
                .iter()
                .filter(|id| matches!(**id, "pause" | "resume"))
                .count(),
            1
        );
    }
}

#[test]
fn danger_color_uses_operation_risk_independently_of_confirmation() {
    for (confirm, dangerous, expected) in [
        (false, true, ui::components::ComponentTone::Danger),
        (true, false, ui::components::ComponentTone::Default),
    ] {
        let mut session = shortcut_action_state(ManagementKind::Processes, "cont");
        let action = &mut session.pages.management.management_state.snapshot.rows[0].actions[0];
        action.confirm = confirm;
        action.dangerous = dangerous;
        let actions = session.management_actions();
        let index = actions
            .iter()
            .position(|(action, _)| action.id == "cont")
            .unwrap();
        assert_eq!(
            session.to_management_view_model().action_tones[index],
            expected
        );
    }
}
