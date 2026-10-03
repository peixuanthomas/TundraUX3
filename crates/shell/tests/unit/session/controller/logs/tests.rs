use super::*;

#[test]
fn global_notification_releases_log_detail_scroll_capture() {
    let mut state = state(UserRole::User);
    state.logs_state.detail_scrollbar_grab = Some(0);
    state.notify_modal(
        "Notice",
        "Stop dragging",
        ui::NotificationTone::Info,
        vec![],
    );
    assert!(state.logs_state.detail_scrollbar_grab.is_none());
}
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
fn logs_launcher_entry_and_direct_route_enforce_guest_gate() {
    for role in [UserRole::Admin, UserRole::User, UserRole::Guest] {
        let mut state = state(role);
        assert_eq!(
            state.user_home_entries().iter().any(|e| e.label == "Logs"),
            false
        );
        if role != UserRole::Guest {
            assert!(
                state
                    .built_in_launcher_applications()
                    .iter()
                    .any(|entry| entry.id == "builtin.logs")
            );
        }
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

#[test]
fn service_logs_open_exact_linux_filter_and_return_to_the_previous_screen() {
    let mut state = state(UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.logs_state.query.min_level = Some(LogLevel::Error);
    state.open_service_logs("example.service", "user");
    assert_eq!(state.active_screen(), ShellScreen::Logs);
    assert_eq!(state.logs_state.category, ui::LogsCategory::Linux);
    assert_eq!(state.logs_state.section, ui::LogsSection::Events);
    assert_eq!(state.logs_state.query.source, LogSource::Linux);
    assert_eq!(
        state.logs_state.query.systemd_unit.as_deref(),
        Some("example.service")
    );
    assert_eq!(
        state.logs_state.query.systemd_scope.as_deref(),
        Some("user")
    );
    assert_eq!(state.logs_state.query.min_level, None);
    assert!(
        state
            .to_logs_view_model()
            .filter_summary
            .contains("example.service")
    );
    key(&mut state, "Esc");
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(state.focused_component, ShellComponent::SystemStatus);
}

#[test]
fn guest_and_invalid_service_log_requests_do_not_change_navigation() {
    let mut guest = state(UserRole::Guest);
    guest.open_service_logs("example.service", "system");
    assert_eq!(guest.active_screen(), ShellScreen::Home);
    let mut state = state(UserRole::Admin);
    for (unit, scope) in [
        ("*.service", "system"),
        ("example.service", "other-user"),
        ("../example.service", "user"),
    ] {
        state.open_service_logs(unit, scope);
        assert_eq!(state.active_screen(), ShellScreen::Home);
    }
}

#[test]
fn logs_detail_drag_and_cancel_do_not_move_the_event_selection() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.logs_state.job = None;
    state.logs_state.snapshot.result.events = vec![runtime_log::RuntimeLogEvent::new(
        runtime_log::LogContext::default(),
        LogLevel::Info,
        runtime_log::LogPhase::Succeeded,
        "description\n".repeat(100),
    )];
    let layout = ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
    let bar = layout.detail_scrollbar.unwrap();
    let pointer = |position: (u16, u16), kind| MouseInput {
        position: ui::Point::new(position.0, position.1),
        kind,
        modifiers: ui::KeyModifiers::NONE,
    };
    state.handle_logs_pointer(pointer(
        (bar.thumb.x, bar.thumb.y),
        ui::MouseEventKind::Down(PointerButton::Left),
    ));
    state.handle_logs_pointer(pointer(
        (bar.track.x, bar.track.bottom()),
        ui::MouseEventKind::Drag(PointerButton::Left),
    ));
    assert_eq!(
        state.logs_state.detail_scroll,
        bar.content_len - bar.viewport_len
    );
    assert_eq!(state.logs_state.selected, 0);
    state.cancel_logs_pointer_gesture();
    assert!(!state.logs_pointer_drag_active());
}

#[test]
fn escape_returns_logs_to_launcher_and_global_overlays_hide_background_buttons() {
    let mut state = state(UserRole::User);
    state.screen_stack.push(ShellScreen::Launcher);
    state.focused_component = ShellComponent::Launcher;
    state.open_logs();
    let layout = ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
    let area = layout
        .controls
        .iter()
        .find(|control| control.target == ui::LogsHitTarget::Refresh)
        .unwrap()
        .area;
    let point = (area.x, area.y);
    assert!(state.logs_button_at(point).is_some());
    state.time_sync_dialog_visible = true;
    assert!(state.logs_button_at(point).is_none());
    state.time_sync_dialog_visible = false;
    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Logs),
        anchor: point,
    });
    assert!(state.logs_button_at(point).is_none());
    state.active_popup = None;
    state.handle_logs_key(&KeyInput::new(InputKey::Escape));
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    assert_eq!(state.focused_component, ShellComponent::Launcher);
}

#[test]
fn compact_logs_uses_the_reserved_shell_content_area() {
    let mut state = state(UserRole::User);
    state.terminal_size = (49, 11);
    state.open_logs();
    assert_eq!(state.logs_main_area(), Some(Rect::new(0, 1, 49, 10)));
    assert!(state.logs_button_at((48, 0)).is_none());
}
