use super::*;

#[test]
fn compact_log_times_use_the_configured_timezone_and_keep_full_source_timestamps() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.app.dispatch_at(
        app::AppCommand::SetClockTimezone {
            timezone_id: Some("Asia/Shanghai".into()),
            synchronized_utc: None,
        },
        Instant::now(),
    );
    let mut event = runtime_log::RuntimeLogEvent::new(
        runtime_log::LogContext::default(),
        LogLevel::Info,
        runtime_log::LogPhase::Observed,
        "read complete",
    );
    event.timestamp = chrono::DateTime::parse_from_rfc3339("2026-10-19T07:34:56Z")
        .unwrap()
        .with_timezone(&Utc);
    state.pages.logs.logs_state.snapshot.result.events = vec![event];
    let model = state.to_logs_view_model();
    assert_eq!(model.events[0].time_label, "15:34:56");
    assert!(model.events[0].timestamp.starts_with("2026-10-19T07:34:56"));
}

fn pending_snapshot(state: &mut ShellSession, text: &str) -> LogsJob {
    let mut snapshot = LogsSnapshot::default();
    snapshot
        .result
        .events
        .push(runtime_log::RuntimeLogEvent::new(
            runtime_log::LogContext::default(),
            LogLevel::Info,
            runtime_log::LogPhase::Observed,
            text,
        ));
    let job = LogsJob(Arc::new(LogsJobShared {
        id: PageInstanceId::new().get(),
        cancelled: Arc::new(AtomicBool::new(false)),
        result: Mutex::new(Some(LogsJobResult::Snapshot(snapshot))),
        worker: Mutex::new(None),
        background: state.pages.logs.logs_state.background_refresh,
    }));
    state.register_logs_job(&job);
    state.pages.logs.logs_state.job = Some(job.clone());
    job
}

#[test]
fn successful_log_refreshes_keep_the_content_geometry_stable_without_idle_feedback() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.pages.logs.logs_state.feedback = None;
    let area = Rect::new(0, 0, 120, 40);
    for background in [false, true, true] {
        state.pages.logs.logs_state.background_refresh = background;
        pending_snapshot(&mut state, "read complete");
        let before = ui::logs_layout(area, &state.to_logs_view_model());
        state.poll_logs_tasks();
        let model = state.to_logs_view_model();
        let after = ui::logs_layout(area, &model);
        assert_eq!(before.content.list_panel, after.content.list_panel);
        assert_eq!(before.content.detail_panel, after.content.detail_panel);
        assert!(model.feedback.is_none());
    }
}

#[test]
fn log_refresh_feedback_keeps_notices_and_non_ready_source_states() {
    use runtime_log::LogSourceState;

    let mut state = state(UserRole::User);
    state.open_logs();
    let notice = "Showing a bounded tail of the file. Earlier records remain in the log file.";
    for source_state in [
        LogSourceState::Ready,
        LogSourceState::Partial,
        LogSourceState::PermissionDenied,
        LogSourceState::Unavailable,
        LogSourceState::Unsupported,
        LogSourceState::Cancelled,
    ] {
        let job = pending_snapshot(&mut state, "read complete");
        if let Some(LogsJobResult::Snapshot(snapshot)) = job.0.result.lock().unwrap().as_mut() {
            snapshot.result.state = source_state;
            snapshot.result.notices = vec![notice.into()];
        }
        state.poll_logs_tasks();
        let feedback = state.to_logs_view_model().feedback.unwrap();
        let notice_label = log_notice_label(notice);
        if source_state == LogSourceState::Ready {
            assert_eq!(feedback, notice_label);
        } else {
            assert_eq!(
                feedback,
                format!("{} — {notice_label}", log_state_label(source_state))
            );
        }
    }
}

#[test]
fn covered_log_queries_preserve_each_pages_filter_and_result() {
    let mut state = state(UserRole::User);
    state.open_logs();
    let first = state.active_page_id();
    state.pages.logs.logs_state.query.module = Some("first module".into());
    pending_snapshot(&mut state, "first result");
    state.open_logs();
    let second = state.active_page_id();
    assert_ne!(first, second);
    state.pages.logs.logs_state.query.module = Some("second module".into());
    pending_snapshot(&mut state, "second result");
    state.poll_logs_tasks();
    assert_eq!(state.active_page_id(), second);
    assert_eq!(
        state.pages.logs.logs_state.snapshot.result.events[0].message,
        "second result"
    );
    let hidden = &state.pages.logs.get(first).unwrap().logs_state;
    assert_eq!(hidden.snapshot.result.events[0].message, "first result");
    assert_eq!(hidden.query.module.as_deref(), Some("first module"));
    state.close_page_instance(second);
    assert_eq!(
        state.pages.logs.logs_state.query.module.as_deref(),
        Some("first module")
    );
    assert_eq!(
        state.pages.logs.logs_state.snapshot.result.events[0].message,
        "first result"
    );
}

#[test]
fn closing_a_log_page_cancels_its_query_and_rejects_late_result() {
    let mut state = state(UserRole::User);
    state.open_logs();
    let first = state.active_page_id();
    let job = pending_snapshot(&mut state, "late result");
    state.close_page_instance(first);
    assert!(job.0.cancelled.load(AtomicOrdering::Relaxed));
    state.open_logs();
    state.poll_logs_tasks();
    assert!(
        state
            .pages
            .logs
            .logs_state
            .snapshot
            .result
            .events
            .is_empty()
    );
    assert_eq!(state.tasks.owner(TaskDomain::Logs, job.0.id), None);
}

#[test]
fn global_notification_releases_log_detail_scroll_capture() {
    let mut state = state(UserRole::User);
    state.pages.logs.logs_state.detail_scrollbar_grab = Some(0);
    state.notify_modal(
        "Notice",
        "Stop dragging",
        ui::NotificationTone::Info,
        vec![],
    );
    assert!(state.pages.logs.logs_state.detail_scrollbar_grab.is_none());
}

#[test]
fn paused_live_capture_keeps_the_view_and_counts_new_records() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.pages.logs.logs_state.job = None;
    let old = runtime_log::RuntimeLogEvent::new(
        runtime_log::LogContext::default(),
        LogLevel::Info,
        runtime_log::LogPhase::Observed,
        "old",
    );
    state.pages.logs.logs_state.snapshot.result.events = vec![old.clone()];
    state.pages.logs.logs_state.paused = true;
    state.pages.logs.logs_state.explicit_scroll = true;
    let new = runtime_log::RuntimeLogEvent::new(
        runtime_log::LogContext::default(),
        LogLevel::Info,
        runtime_log::LogPhase::Observed,
        "new",
    );
    let mut snapshot = LogsSnapshot::default();
    snapshot.result.events = vec![new, old];
    state.pages.logs.logs_state.job = Some(LogsJob(Arc::new(LogsJobShared {
        id: PageInstanceId::new().get(),
        cancelled: Arc::new(AtomicBool::new(false)),
        result: Mutex::new(Some(LogsJobResult::Snapshot(snapshot))),
        worker: Mutex::new(None),
        background: true,
    })));
    state.register_logs_job(&state.pages.logs.logs_state.job.clone().unwrap());
    state.poll_logs_tasks();
    assert_eq!(state.pages.logs.logs_state.new_events, 1);
    assert_eq!(state.pages.logs.logs_state.selected, 1);
    assert_eq!(state.pages.logs.logs_state.scroll, 1);
    assert_eq!(
        state.pages.logs.logs_state.snapshot.result.events[1].message,
        "old"
    );
    state.logs_follow_control();
    assert!(!state.pages.logs.logs_state.paused);
    assert_eq!(
        (
            state.pages.logs.logs_state.selected,
            state.pages.logs.logs_state.scroll,
            state.pages.logs.logs_state.new_events
        ),
        (0, 0, 0)
    );
}

#[test]
fn visible_filters_button_applies_service_boot_and_file_choices() {
    let mut state = state(UserRole::Admin);
    state.open_logs();
    state.pages.logs.logs_state.job = None;
    press_log_control(&mut state, ui::LogsHitTarget::Filters);
    assert!(state.pages.logs.logs_state.filter_form.is_some());
    let form = state.pages.logs.logs_state.filter_form.as_mut().unwrap();
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
        state.pages.logs.logs_state.query.systemd_unit.as_deref(),
        Some("sshd.service")
    );
    assert_eq!(
        state.pages.logs.logs_state.query.systemd_boot.as_deref(),
        Some("-1")
    );
    state.pages.logs.logs_state.job = None;
    state.logs_open_filter_form();
    let path = std::env::temp_dir().join("selected-system.log");
    state
        .pages
        .logs
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
    assert_eq!(state.pages.logs.logs_state.query.file_path, Some(path));
    assert!(state.pages.logs.logs_state.query.systemd_unit.is_none());
    assert!(state.pages.logs.logs_state.query.systemd_boot.is_none());
}

#[test]
fn more_dialog_navigation_skips_disabled_actions_and_space_closes() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.pages.logs.logs_state.job = None;
    key(&mut state, "F10");
    state.pages.logs.logs_state.more_selected = Some(3);
    key(&mut state, "Down");
    assert_eq!(state.pages.logs.logs_state.more_selected, Some(6));
    key(&mut state, "Home");
    assert_eq!(state.pages.logs.logs_state.more_selected, Some(0));
    key(&mut state, "End");
    assert_eq!(state.pages.logs.logs_state.more_selected, Some(6));
    key(&mut state, "Tab");
    assert_eq!(state.pages.logs.logs_state.more_selected, Some(0));
    state.pages.logs.logs_state.more_selected = Some(4);
    key(&mut state, "Enter");
    assert_eq!(state.pages.logs.logs_state.more_selected, Some(4));
    assert_eq!(state.pages.logs.logs_state.category, ui::LogsCategory::Ux);
    key(&mut state, "End");
    state.handle_logs_key(&KeyInput::new(InputKey::Space));
    assert!(state.pages.logs.logs_state.more_selected.is_none());
    assert_eq!(state.active_screen(), ShellScreen::Logs);
}

#[test]
fn more_dialog_blocks_stale_background_buttons_before_its_first_redraw() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.pages.logs.logs_state.job = None;
    let layout = ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
    let control = layout
        .controls
        .iter()
        .find(|control| control.target == ui::LogsHitTarget::Refresh)
        .unwrap();
    let region = state
        .logs_button_at((control.area.x, control.area.y))
        .unwrap();
    state.button_regions.push(region.clone());
    key(&mut state, "F10");
    assert!(state.button_at((region.area.x, region.area.y)).is_none());
    state.handle_logs_pointer(MouseInput::down(
        region.area.x,
        region.area.y,
        PointerButton::Left,
    ));
    assert!(state.pages.logs.logs_state.more_selected.is_none());
    assert!(
        state.pages.logs.logs_state.job.is_none(),
        "outside dismissal must not refresh the covered page"
    );
    key(&mut state, "F");
    assert!(state.pages.logs.logs_state.filter_form.is_some());
    assert!(state.button_at((region.area.x, region.area.y)).is_none());
}

#[test]
fn more_dialog_waits_for_release_and_cancels_press_on_drag_resize_or_focus_loss() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.pages.logs.logs_state.job = None;
    key(&mut state, "F10");
    let main = state.logs_main_area().unwrap();
    let layout = ui::logs_layout(main, &state.to_logs_view_model());
    let control = layout
        .menu_controls
        .iter()
        .find(|control| control.target == ui::LogsHitTarget::FilterLevel)
        .unwrap();
    let point = (control.area.x, control.area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
    assert!(state.pages.logs.logs_state.more_selected.is_some());
    state.apply_input(InputEvent::mouse_drag(
        PointerButton::Left,
        (layout.menu.x, layout.menu.y),
    ));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
    assert!(state.pages.logs.logs_state.more_selected.is_some());
    for cancelled in [
        InputEvent::FocusLost,
        InputEvent::Resize {
            width: 120,
            height: 40,
        },
    ] {
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        state.apply_input(cancelled);
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        assert_eq!(state.pages.logs.logs_state.query.min_level, None);
        assert!(state.pages.logs.logs_state.more_selected.is_some());
    }
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(
        state.pages.logs.logs_state.query.min_level,
        Some(LogLevel::Trace)
    );
    assert!(state.pages.logs.logs_state.more_selected.is_none());
}

#[test]
fn more_dialog_padding_and_outside_wheel_do_not_change_or_dismiss_the_selection() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.pages.logs.logs_state.job = None;
    key(&mut state, "F10");
    let layout = ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
    let padding = (layout.menu.x + 1, layout.menu.y + 1);
    state.handle_logs_pointer(MouseInput::down(padding.0, padding.1, PointerButton::Left));
    assert_eq!(state.pages.logs.logs_state.more_selected, Some(0));
    state.apply_input(InputEvent::mouse_scroll(ScrollDirection::Down, (0, 1)));
    assert_eq!(state.pages.logs.logs_state.more_selected, Some(0));
    assert_eq!(state.pages.logs.logs_state.scroll, 0);
    state.apply_input(InputEvent::mouse_scroll(ScrollDirection::Down, padding));
    assert_eq!(state.pages.logs.logs_state.more_selected, Some(1));
    state.pages.logs.logs_state.more_selected = Some(0);
    state.apply_input(InputEvent::mouse_scroll(ScrollDirection::Up, padding));
    assert_eq!(
        state.pages.logs.logs_state.more_selected,
        Some(0),
        "scrolling stops at the first option"
    );
    let close = layout
        .menu_controls
        .iter()
        .find(|control| control.target == ui::LogsHitTarget::CloseMore)
        .unwrap();
    let point = (close.area.x, close.area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(state.pages.logs.logs_state.more_selected.is_some());
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert!(state.pages.logs.logs_state.more_selected.is_none());
}

#[test]
fn compact_more_dialog_scrollbar_drags_within_the_dialog_and_cancels_on_focus_loss() {
    let mut state = state(UserRole::User);
    state.terminal_size = (49, 11);
    state.open_logs();
    state.pages.logs.logs_state.job = None;
    key(&mut state, "F10");
    let layout = ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
    let bar = layout.menu_scrollbar.unwrap();
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        (bar.thumb.x - 1, bar.thumb.y),
    ));
    assert!(state.logs_pointer_drag_active());
    state.apply_input(InputEvent::mouse_drag(
        PointerButton::Left,
        (bar.track.x, bar.track.bottom()),
    ));
    let layout = ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
    assert_eq!(
        layout.menu_scrollbar.unwrap().offset,
        bar.content_len - bar.viewport_len
    );
    assert_eq!(state.pages.logs.logs_state.scroll, 0);
    state.apply_input(InputEvent::FocusLost);
    assert!(!state.logs_pointer_drag_active());
    assert!(state.pages.logs.logs_state.more_selected.is_some());
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
            | ui::LogsHitTarget::Filters
            | ui::LogsHitTarget::More
            | ui::LogsHitTarget::FilterLevel
            | ui::LogsHitTarget::OpenFolder
    ) {
        state.pages.logs.logs_state.more_selected = Some(0);
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
    state.pages.logs.logs_state.query.min_level = Some(LogLevel::Warning);
    let before = state.pages.logs.logs_state.clone();
    for phase in [InputPhase::Release, InputPhase::Repeat] {
        for key in [
            InputKey::Char('c'),
            InputKey::Char('l'),
            InputKey::Enter,
            InputKey::F(5),
        ] {
            state.handle_logs_key(&KeyInput::with_phase(key, InputModifiers::none(), phase));
            assert_eq!(state.pages.logs.logs_state, before);
        }
    }
    state.handle_logs_key(&KeyInput::from_label("Ctrl+C"));
    assert_eq!(state.pages.logs.logs_state, before);
    state.handle_logs_key(&KeyInput::from_label("C"));
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
}

#[test]
fn disabled_log_controls_cannot_be_activated_by_shortcuts() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.handle_logs_key(&KeyInput::from_label("Enter"));
    assert!(
        state.pages.logs.logs_state.last_document.is_none(),
        "an empty list cannot be opened"
    );
    state.handle_logs_key(&KeyInput::from_label("I"));
    assert_eq!(state.pages.logs.logs_state.category, ui::LogsCategory::Ux);

    state.pages.logs.logs_state.query.min_level = Some(LogLevel::Warning);
    state.pages.logs.logs_state.job = Some(LogsJob(Arc::new(LogsJobShared {
        id: PageInstanceId::new().get(),
        cancelled: Arc::new(AtomicBool::new(false)),
        result: Mutex::new(None),
        worker: Mutex::new(None),
        background: false,
    })));
    state.register_logs_job(&state.pages.logs.logs_state.job.clone().unwrap());
    let before = state.pages.logs.logs_state.clone();
    for label in ["R", "F5", "L", "M", "T", "C", "I", "E", "Enter"] {
        state.handle_logs_key(&KeyInput::from_label(label));
        assert_eq!(
            state.pages.logs.logs_state, before,
            "{label} must respect the loading state"
        );
    }
    state.pages.logs.logs_state.job = None;
    state.pages.logs.logs_state.category = ui::LogsCategory::Linux;
    let before = state.pages.logs.logs_state.clone();
    for label in ["C", "L", "F5", "Enter"] {
        state.handle_logs_key(&KeyInput::from_label(label));
        assert_eq!(
            state.pages.logs.logs_state, before,
            "{label} must respect Linux log permissions"
        );
    }
}

#[test]
fn f5_refresh_and_clear_filters_match_their_log_buttons() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.pages.logs.logs_state.feedback = None;
    state.handle_logs_key(&KeyInput::from_label("F5"));
    let feedback = state.pages.logs.logs_state.feedback.clone();
    assert!(
        feedback.is_some(),
        "the fixture has no storage, so a real refresh reports that prerequisite"
    );
    state.pages.logs.logs_state.feedback = None;
    press_log_control(&mut state, ui::LogsHitTarget::Refresh);
    assert_eq!(state.pages.logs.logs_state.feedback, feedback);
    state.pages.logs.logs_state.query.min_level = Some(LogLevel::Warning);
    press_log_control(&mut state, ui::LogsHitTarget::ClearFilters);
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
    state.pages.logs.logs_state.query.min_level = Some(LogLevel::Warning);
    state.handle_logs_key(&KeyInput::from_label("C"));
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
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
fn logs_navigation_restores_covered_filters_and_new_pages_start_clean() {
    let mut state = state(UserRole::User);
    state.open_logs();
    key(&mut state, "L");
    key(&mut state, "Tab");
    assert_eq!(state.pages.logs.logs_state.category, ui::LogsCategory::Ux);
    assert_eq!(
        state.pages.logs.logs_state.query.min_level,
        Some(LogLevel::Trace)
    );
    let first = state.active_page_id();
    state.open_logs();
    assert_eq!(state.pages.logs.logs_state.category, ui::LogsCategory::Ux);
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
    key(&mut state, "Esc");
    assert_eq!(state.active_page_id(), first);
    assert_eq!(state.pages.logs.logs_state.category, ui::LogsCategory::Ux);
    assert_eq!(
        state.pages.logs.logs_state.query.min_level,
        Some(LogLevel::Trace)
    );
    key(&mut state, "Right");
    assert_eq!(
        state.pages.logs.logs_state.category,
        ui::LogsCategory::Linux
    );
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
    assert_eq!(
        state.to_logs_view_model().linux_available,
        cfg!(target_os = "linux")
    );
    key(&mut state, "Esc");
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state.open_logs();
    assert_eq!(state.pages.logs.logs_state.category, ui::LogsCategory::Ux);
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
}

#[test]
fn level_filter_is_visible_defaults_to_all_and_tab_does_not_switch_views() {
    let mut state = state(UserRole::User);
    state.open_logs();
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
    assert_eq!(
        state.to_logs_view_model().level_filter,
        i18n::tr!("shell-all")
    );
    for level in [
        LogLevel::Trace,
        LogLevel::Debug,
        LogLevel::Info,
        LogLevel::Warning,
        LogLevel::Error,
        LogLevel::Critical,
    ] {
        state.pages.logs.logs_state.job = None;
        let layout = ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
        let control = layout
            .controls
            .iter()
            .find(|control| control.target == ui::LogsHitTarget::FilterLevel)
            .unwrap();
        let point = (control.area.x, control.area.y);
        let before = state.pages.logs.logs_state.query.min_level;
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        assert_eq!(state.pages.logs.logs_state.query.min_level, before);
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        assert_eq!(state.pages.logs.logs_state.query.min_level, Some(level));
        let model = state.to_logs_view_model();
        assert!(model.level_filter.contains(&log_level_label(level)));
        if level != LogLevel::Critical {
            assert_eq!(
                model.level_filter,
                i18n::tr!("ui-logs-level-or-higher", level = log_level_label(level))
            );
        }
        for key in ["Tab", "Shift+Tab"] {
            state.handle_logs_key(&KeyInput::from_label(key));
            assert_eq!(state.to_logs_view_model(), model);
        }
    }
    key(&mut state, "L");
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
}

#[test]
fn open_folder_button_and_shortcut_use_the_ux_root_and_restore_the_log_page() {
    let root = std::env::temp_dir().join(format!(
        "tundra-logs-explorer-{}-{}",
        std::process::id(),
        PageInstanceId::new().get()
    ));
    let paths = platform::AppPaths::from_parts(
        root.join("config.toml"),
        root.join("state"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap();
    let storage = StorageManager::open(paths).unwrap().manager;
    let logs_path = storage.layout().logs_path.clone();
    for name in ["runtime", "crashes"] {
        std::fs::create_dir_all(logs_path.join(name)).unwrap();
    }
    let mut state = state(UserRole::Admin);
    state.open_logs();
    state.storage_manager = Some(storage);
    state.pages.logs.logs_state.query.min_level = Some(LogLevel::Warning);
    state.pages.logs.logs_state.snapshot.result.events = (0..3)
        .map(|_| {
            runtime_log::RuntimeLogEvent::new(
                runtime_log::LogContext::default(),
                LogLevel::Warning,
                runtime_log::LogPhase::Observed,
                "entry",
            )
        })
        .collect();
    state.pages.logs.logs_state.selected = 2;
    let page = state.active_page_id();
    for pointer in [true, false] {
        if pointer {
            let layout =
                ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
            let control = layout
                .controls
                .iter()
                .find(|control| control.target == ui::LogsHitTarget::OpenFolder)
                .unwrap();
            let point = (control.area.x, control.area.y);
            state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
            assert_eq!(state.active_screen(), ShellScreen::Logs);
            state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        } else {
            key(&mut state, "G");
        }
        assert_eq!(state.active_screen(), ShellScreen::Explorer);
        assert_eq!(
            state.pages.explorer.explorer_purpose,
            ExplorerPurpose::DiagnosticsLogs
        );
        let explorer = state.app.explorer_state().unwrap();
        assert_eq!(explorer.current_path, logs_path);
        for name in ["runtime", "crashes"] {
            assert!(explorer.entries.iter().any(|entry| entry.name == name));
        }
        state.close_explorer();
        assert_eq!(state.active_page_id(), page);
        assert_eq!(state.active_screen(), ShellScreen::Logs);
        assert_eq!(
            state.pages.logs.logs_state.query.min_level,
            Some(LogLevel::Warning)
        );
        assert_eq!(state.pages.logs.logs_state.selected, 2);
    }
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn user_log_access_does_not_grant_access_to_the_global_log_folder() {
    let mut state = state(UserRole::User);
    state.open_logs();
    let model = state.to_logs_view_model();
    assert!(!ui::logs_control_enabled(
        &model,
        ui::LogsHitTarget::OpenFolder
    ));
    key(&mut state, "G");
    assert_eq!(state.active_screen(), ShellScreen::Logs);
    assert_eq!(state.to_logs_view_model(), model);
}

#[test]
fn opening_an_incident_log_keeps_the_query_readable_without_its_report() {
    let mut state = state(UserRole::Admin);
    state.open_logs();
    let mut event = runtime_log::RuntimeLogEvent::new(
        runtime_log::LogContext::default(),
        LogLevel::Critical,
        runtime_log::LogPhase::Incident,
        "incident retained in runtime log",
    );
    event.incident_id = Some("removed-report".into());
    state
        .pages
        .logs
        .logs_state
        .snapshot
        .result
        .events
        .push(event);
    key(&mut state, "Enter");
    assert_eq!(
        state.pages.logs.logs_state.last_document,
        Some(LogDocumentSelection::Events)
    );
}

#[test]
fn log_levels_sort_by_severity_and_keep_the_selected_record() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.pages.logs.logs_state.snapshot.result.events = [
        LogLevel::Warning,
        LogLevel::Trace,
        LogLevel::Critical,
        LogLevel::Info,
        LogLevel::Error,
        LogLevel::Debug,
    ]
    .into_iter()
    .map(|level| {
        runtime_log::RuntimeLogEvent::new(
            runtime_log::LogContext::default(),
            level,
            runtime_log::LogPhase::Observed,
            "entry",
        )
    })
    .collect();
    let selected_id = state.logs_id_at(state.pages.logs.logs_state.selected);
    let sort_key = state.logs_sort_key();
    state.pages.common.table_sorts.insert(
        sort_key,
        ui::TableSort {
            column: 1,
            descending: true,
        },
    );
    state.apply_logs_sort();
    assert_eq!(
        state
            .pages
            .logs
            .logs_state
            .snapshot
            .result
            .events
            .iter()
            .map(|event| event.level)
            .collect::<Vec<_>>(),
        [
            LogLevel::Critical,
            LogLevel::Error,
            LogLevel::Warning,
            LogLevel::Info,
            LogLevel::Debug,
            LogLevel::Trace,
        ]
    );
    assert_eq!(
        state.logs_id_at(state.pages.logs.logs_state.selected),
        selected_id
    );
    let model = state.to_logs_view_model();
    assert_eq!(model.events[0].level, log_level_label(LogLevel::Critical));
    assert_eq!(model.events[0].status, ui::DiagnosticsStatus::Fail);
    assert_eq!(model.events[2].status, ui::DiagnosticsStatus::Warning);
}
#[test]
fn legacy_status_navigation_redirects_to_logs_app() {
    let mut state = state(UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.set_system_status_tab(ui::SystemStatusTab::Incidents);
    assert_eq!(state.active_screen(), ShellScreen::Logs);
    assert_eq!(
        state.pages.logs.logs_state.query.min_level,
        Some(LogLevel::Critical)
    );
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
    state
        .pages
        .logs
        .logs_state
        .snapshot
        .result
        .events
        .push(event.clone());
    state.pages.logs.logs_state.scroll = 4;
    state.pages.logs.logs_state.explicit_scroll = true;
    let mut snapshot = LogsSnapshot::default();
    let mut newer = event.clone();
    newer.event_id = "newer".into();
    snapshot.result.events = vec![newer, event];
    state.pages.logs.logs_state.job = Some(LogsJob(Arc::new(LogsJobShared {
        id: PageInstanceId::new().get(),
        cancelled: Arc::new(AtomicBool::new(false)),
        result: Mutex::new(Some(LogsJobResult::Snapshot(snapshot))),
        worker: Mutex::new(None),
        background: false,
    })));
    state.register_logs_job(&state.pages.logs.logs_state.job.clone().unwrap());
    state.poll_logs_tasks();
    assert_eq!(state.pages.logs.logs_state.selected, 1);
    assert_eq!(state.pages.logs.logs_state.scroll, 4);
    assert!(state.pages.logs.logs_state.explicit_scroll);
}

#[test]
fn service_logs_open_exact_linux_filter_and_return_to_the_previous_screen() {
    let mut state = state(UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.pages.logs.logs_state.query.min_level = Some(LogLevel::Error);
    state.open_service_logs("example.service", "user");
    assert_eq!(state.active_screen(), ShellScreen::Logs);
    assert_eq!(
        state.pages.logs.logs_state.category,
        ui::LogsCategory::Linux
    );
    assert_eq!(state.pages.logs.logs_state.query.source, LogSource::Linux);
    assert_eq!(
        state.pages.logs.logs_state.query.systemd_unit.as_deref(),
        Some("example.service")
    );
    assert_eq!(
        state.pages.logs.logs_state.query.systemd_scope.as_deref(),
        Some("user")
    );
    assert_eq!(state.pages.logs.logs_state.query.min_level, None);
    assert!(
        state
            .to_logs_view_model()
            .filter_summary
            .contains("example.service")
    );
    key(&mut state, "Esc");
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::SystemStatus
    );
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
    state.pages.logs.logs_state.job = None;
    state.pages.logs.logs_state.snapshot.result.events = vec![runtime_log::RuntimeLogEvent::new(
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
        state.pages.logs.logs_state.detail_scroll,
        bar.content_len - bar.viewport_len
    );
    assert_eq!(state.pages.logs.logs_state.selected, 0);
    state.cancel_logs_pointer_gesture();
    assert!(!state.logs_pointer_drag_active());
}

#[test]
fn escape_returns_logs_to_launcher_and_global_overlays_hide_background_buttons() {
    let mut state = state(UserRole::User);
    state.enter_screen(ShellScreen::Launcher);
    state.pages.common.focused_component = ShellComponent::Launcher;
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
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Logs),
        anchor: point,
    });
    assert!(state.logs_button_at(point).is_none());
    state.pages.common.active_popup = None;
    state.handle_logs_key(&KeyInput::new(InputKey::Escape));
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::Launcher
    );
}

#[test]
fn compact_logs_uses_the_reserved_shell_content_area() {
    let mut state = state(UserRole::User);
    state.terminal_size = (49, 11);
    state.open_logs();
    assert_eq!(state.logs_main_area(), Some(Rect::new(0, 1, 49, 10)));
    assert!(state.logs_button_at((48, 0)).is_none());
}

#[test]
fn sorting_log_headers_waits_for_release_and_refresh_preserves_selected_event() {
    let mut state = state(UserRole::User);
    state.open_logs();
    state.pages.logs.logs_state.job = None;
    let event = |message: &str| {
        runtime_log::RuntimeLogEvent::new(
            runtime_log::LogContext::default(),
            LogLevel::Info,
            runtime_log::LogPhase::Observed,
            message,
        )
    };
    let zulu = event("Zulu");
    let selected_id = zulu.event_id.clone();
    let alpha = event("alpha");
    state.pages.logs.logs_state.snapshot.result.events = vec![zulu.clone(), alpha.clone()];
    let layout = ui::logs_layout(state.logs_main_area().unwrap(), &state.to_logs_view_model());
    let header = layout
        .content
        .headers
        .iter()
        .find(|(col, _)| *col == 4)
        .unwrap()
        .1;
    let now = Instant::now();
    state.apply_input_at(
        InputEvent::mouse_down(PointerButton::Left, (header.x, header.y)),
        now,
    );
    assert_eq!(
        state.pages.logs.logs_state.snapshot.result.events[0].message,
        "Zulu"
    );
    state.apply_input_at(
        InputEvent::mouse_up(PointerButton::Left, (header.x, header.y)),
        now + Duration::from_millis(20),
    );
    assert_eq!(
        state.pages.logs.logs_state.snapshot.result.events[0].message,
        "alpha"
    );
    assert_eq!(
        state.pages.logs.logs_state.snapshot.result.events[state.pages.logs.logs_state.selected]
            .event_id,
        selected_id
    );
    let mut snapshot = LogsSnapshot::default();
    snapshot.result.events = vec![event("Beta"), zulu, alpha];
    state.pages.logs.logs_state.job = Some(LogsJob(Arc::new(LogsJobShared {
        id: PageInstanceId::new().get(),
        cancelled: Arc::new(AtomicBool::new(false)),
        result: Mutex::new(Some(LogsJobResult::Snapshot(snapshot))),
        worker: Mutex::new(None),
        background: true,
    })));
    state.register_logs_job(&state.pages.logs.logs_state.job.clone().unwrap());
    state.poll_logs_tasks();
    assert_eq!(
        state
            .pages
            .logs
            .logs_state
            .snapshot
            .result
            .events
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "Beta", "Zulu"]
    );
    assert_eq!(
        state.pages.logs.logs_state.snapshot.result.events[state.pages.logs.logs_state.selected]
            .event_id,
        selected_id
    );
    state.logs_follow_control();
    assert!(
        !state
            .pages
            .common
            .table_sorts
            .contains_key(&state.logs_sort_key())
    );
}
