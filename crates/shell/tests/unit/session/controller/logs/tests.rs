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

#[test]
fn paused_live_capture_keeps_the_view_and_counts_new_records() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.logs_state.job = None;
    let old = runtime_log::RuntimeLogEvent::new(
        runtime_log::LogContext::default(),
        LogLevel::Info,
        runtime_log::LogPhase::Observed,
        "old",
    );
    state.logs_state.snapshot.result.events = vec![old.clone()];
    state.logs_state.paused = true;
    state.logs_state.explicit_scroll = true;
    let new = runtime_log::RuntimeLogEvent::new(
        runtime_log::LogContext::default(),
        LogLevel::Info,
        runtime_log::LogPhase::Observed,
        "new",
    );
    let mut snapshot = LogsSnapshot::default();
    snapshot.result.events = vec![new, old];
    state.logs_state.job = Some(LogsJob(Arc::new(LogsJobShared {
        cancelled: Arc::new(AtomicBool::new(false)),
        result: Mutex::new(Some(LogsJobResult::Snapshot(snapshot))),
        worker: Mutex::new(None),
        background: true,
    })));
    state.poll_logs_tasks();
    assert_eq!(state.logs_state.new_events, 1);
    assert_eq!(state.logs_state.selected, 1);
    assert_eq!(state.logs_state.scroll, 1);
    assert_eq!(state.logs_state.snapshot.result.events[1].message, "old");
    state.logs_follow_control();
    assert!(!state.logs_state.paused);
    assert_eq!(
        (
            state.logs_state.selected,
            state.logs_state.scroll,
            state.logs_state.new_events
        ),
        (0, 0, 0)
    );
}

#[test]
fn more_menu_opens_filters_and_applies_service_boot_and_file_choices() {
    let mut state = state(UserRole::Admin);
    state.open_logs();
    state.logs_state.job = None;
    key(&mut state, "F10");
    assert_eq!(state.logs_state.more_selected, Some(0));
    key(&mut state, "Enter");
    assert!(state.logs_state.filter_form.is_some());
    let form = state.logs_state.filter_form.as_mut().unwrap();
    form.fields
        .iter_mut()
        .find(|field| field.id == "unit")
        .unwrap()
        .value = "sshd.service".into();
    form.fields
        .iter_mut()
        .find(|field| field.id == "boot")
        .unwrap()
        .value = "-1".into();
    state.logs_apply_filter_form();
    assert_eq!(
        state.logs_state.query.systemd_unit.as_deref(),
        Some("sshd.service")
    );
    assert_eq!(state.logs_state.query.systemd_boot.as_deref(), Some("-1"));
    state.logs_state.job = None;
    state.logs_open_filter_form();
    let path = std::env::temp_dir().join("selected-system.log");
    state
        .logs_state
        .filter_form
        .as_mut()
        .unwrap()
        .fields
        .iter_mut()
        .find(|field| field.id == "file")
        .unwrap()
        .value = path.display().to_string();
    state.logs_apply_filter_form();
    assert_eq!(state.logs_state.query.file_path, Some(path));
    assert!(state.logs_state.query.systemd_unit.is_none());
    assert!(state.logs_state.query.systemd_boot.is_none());
}

#[test]
fn live_buffer_is_bounded_without_duplicate_events() {
    let mut old = LogsSnapshot::default();
    old.result.events = (0..1200)
        .map(|index| {
            let mut event = runtime_log::RuntimeLogEvent::new(
                runtime_log::LogContext::default(),
                LogLevel::Info,
                runtime_log::LogPhase::Observed,
                "entry",
            );
            event.event_id = format!("entry-{index}");
            event
        })
        .collect();
    let mut next = LogsSnapshot::default();
    next.result.events = old.result.events[..200].to_vec();
    assert_eq!(merge_live_events(&mut next, &old), 0);
    assert_eq!(next.result.events.len(), 1000);
    assert!(next.result.truncated);
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

fn press_log_control(state: &mut ShellSession, target: ui::LogsHitTarget) {
    if !matches!(
        target,
        ui::LogsHitTarget::Refresh
            | ui::LogsHitTarget::Open
            | ui::LogsHitTarget::Follow
            | ui::LogsHitTarget::More
    ) {
        state.logs_state.more_selected = Some(0);
    }
    let layout = ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
    let area = layout
        .controls
        .iter()
        .chain(layout.menu_controls.iter())
        .find(|control| control.target == target)
        .unwrap()
        .area;
    state.handle_logs_pointer(MouseInput::down(area.x, area.y, PointerButton::Left));
}

#[test]
fn log_action_shortcuts_ignore_modifiers_release_and_repeated_presses() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.logs_state.query.min_level = Some(LogLevel::Warning);
    let before = state.logs_state.clone();
    for phase in [InputPhase::Release, InputPhase::Repeat] {
        for key in [
            InputKey::Char('c'),
            InputKey::Char('l'),
            InputKey::Enter,
            InputKey::F(5),
        ] {
            state.handle_logs_key(&KeyInput::with_phase(key, InputModifiers::none(), phase));
            assert_eq!(state.logs_state, before);
        }
    }
    state.handle_logs_key(&KeyInput::from_label("Ctrl+C"));
    assert_eq!(state.logs_state, before);
    state.handle_logs_key(&KeyInput::from_label("C"));
    assert_eq!(state.logs_state.query.min_level, None);
}

#[test]
fn disabled_log_controls_cannot_be_activated_by_shortcuts() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.handle_logs_key(&KeyInput::from_label("Enter"));
    assert!(
        state.logs_state.last_document.is_none(),
        "an empty list cannot be opened"
    );
    state.handle_logs_key(&KeyInput::from_label("I"));
    assert_eq!(state.logs_state.section, ui::LogsSection::Events);

    state.logs_state.query.min_level = Some(LogLevel::Warning);
    state.logs_state.job = Some(LogsJob(Arc::new(LogsJobShared {
        cancelled: Arc::new(AtomicBool::new(false)),
        result: Mutex::new(None),
        worker: Mutex::new(None),
        background: false,
    })));
    let before = state.logs_state.clone();
    for label in ["R", "F5", "L", "M", "T", "C", "I", "E", "Enter"] {
        state.handle_logs_key(&KeyInput::from_label(label));
        assert_eq!(
            state.logs_state, before,
            "{label} must respect the loading state"
        );
    }
    state.logs_state.job = None;
    state.logs_state.category = ui::LogsCategory::Linux;
    let before = state.logs_state.clone();
    for label in ["C", "L", "F5", "Enter"] {
        state.handle_logs_key(&KeyInput::from_label(label));
        assert_eq!(
            state.logs_state, before,
            "{label} must respect Linux log permissions"
        );
    }
}

#[test]
fn f5_refresh_and_clear_filters_match_their_log_buttons() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.logs_state.feedback = None;
    state.handle_logs_key(&KeyInput::from_label("F5"));
    let feedback = state.logs_state.feedback.clone();
    assert!(
        feedback.is_some(),
        "the fixture has no storage, so a real refresh reports that prerequisite"
    );
    state.logs_state.feedback = None;
    press_log_control(&mut state, ui::LogsHitTarget::Refresh);
    assert_eq!(state.logs_state.feedback, feedback);
    state.logs_state.query.min_level = Some(LogLevel::Warning);
    press_log_control(&mut state, ui::LogsHitTarget::ClearFilters);
    assert_eq!(state.logs_state.query.min_level, None);
    state.logs_state.query.min_level = Some(LogLevel::Warning);
    state.handle_logs_key(&KeyInput::from_label("C"));
    assert_eq!(state.logs_state.query.min_level, None);
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
        background: false,
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
