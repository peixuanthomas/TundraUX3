use super::*;
fn state(role: UserRole) -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LocalAccount,
            session_id: "logs-session".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            role,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    state
}
fn key(state: &mut ShellSession, value: &str) {
    state.apply_input(InputEvent::from_key_label(value));
}
#[test]
fn logs_home_entry_and_direct_route_enforce_guest_gate() {
    for role in [UserRole::Admin, UserRole::User, UserRole::Guest] {
        let mut state = state(role);
        assert_eq!(
            state.user_home_entries().iter().any(|e| e.label == "Logs"),
            role != UserRole::Guest
        );
        state.open_logs();
        assert_eq!(
            state.active_screen() == ShellScreen::Logs,
            role != UserRole::Guest
        );
        assert_eq!(
            state.to_logs_view_model().can_view_system,
            role == UserRole::Admin
        );
    }
}
#[test]
fn logs_navigation_retains_filters_and_selection_across_close() {
    let mut state = state(UserRole::User);
    state.open_logs();
    key(&mut state, "L");
    key(&mut state, "Tab");
    assert_eq!(state.logs_state.section, ui::LogsSection::Files);
    assert_eq!(state.logs_state.query.min_level, Some(LogLevel::Warning));
    key(&mut state, "Esc");
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state.open_logs();
    assert_eq!(state.logs_state.section, ui::LogsSection::Files);
    assert_eq!(state.logs_state.query.min_level, Some(LogLevel::Warning));
    key(&mut state, "Right");
    assert_eq!(state.logs_state.category, ui::LogsCategory::Linux);
    assert_eq!(state.logs_state.query.min_level, None);
    assert_eq!(
        state.to_logs_view_model().linux_available,
        cfg!(target_os = "linux")
    );
}
#[test]
fn legacy_status_navigation_redirects_to_logs_app() {
    let mut state = state(UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.set_system_status_tab(ui::SystemStatusTab::Incidents);
    assert_eq!(state.active_screen(), ShellScreen::Logs);
    assert_eq!(state.logs_state.section, ui::LogsSection::Incidents);
    key(&mut state, "Esc");
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert!(!ui::SystemStatusWidgetKind::ALL.contains(&ui::SystemStatusWidgetKind::Logs));
    assert!(!ui::SystemStatusWidgetKind::ALL.contains(&ui::SystemStatusWidgetKind::Incidents));
}
#[test]
fn refresh_preserves_selected_event_and_scroll() {
    let mut state = state(UserRole::User);
    state.open_logs();
    let event = runtime_log::RuntimeLogEvent::new(
        runtime_log::LogContext::default(),
        LogLevel::Info,
        runtime_log::LogPhase::Succeeded,
        "finished",
    );
    state.logs_state.snapshot.result.events.push(event.clone());
    state.logs_state.scroll = 4;
    state.logs_state.explicit_scroll = true;
    let mut snapshot = LogsSnapshot::default();
    let mut newer = event.clone();
    newer.event_id = "newer".into();
    snapshot.result.events = vec![newer, event];
    state.logs_state.job = Some(LogsJob(Arc::new(LogsJobShared {
        cancelled: Arc::new(AtomicBool::new(false)),
        result: Mutex::new(Some(LogsJobResult::Snapshot(snapshot))),
        worker: Mutex::new(None),
    })));
    state.poll_logs_tasks();
    assert_eq!(state.logs_state.selected, 1);
    assert_eq!(state.logs_state.scroll, 4);
    assert!(state.logs_state.explicit_scroll);
}
