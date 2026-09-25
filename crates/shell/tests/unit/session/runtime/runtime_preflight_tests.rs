use super::*;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[test]
fn panic_screen_waits_for_explicit_restart_or_exit() {
    let mut screen = ui::PanicScreen::new("test error");
    for event in [
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Event::Resize(10, 4),
        Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('r'),
            KeyModifiers::NONE,
            event::KeyEventKind::Release,
        )),
        Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('r'),
            KeyModifiers::NONE,
            event::KeyEventKind::Repeat,
        )),
    ] {
        assert_eq!(apply_panic_screen_event(&mut screen, event), None);
    }
    for (code, modifiers, outcome) in [
        (
            KeyCode::Char('r'),
            KeyModifiers::NONE,
            ShellRunOutcome::RestartRequested,
        ),
        (
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            ShellRunOutcome::Exit,
        ),
        (KeyCode::Esc, KeyModifiers::NONE, ShellRunOutcome::Exit),
        (
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            ShellRunOutcome::Exit,
        ),
    ] {
        assert_eq!(
            apply_panic_screen_event(&mut screen, Event::Key(KeyEvent::new(code, modifiers))),
            Some(outcome)
        );
    }
}

#[test]
fn idle_has_no_background_poll_deadline_but_active_work_does() {
    assert_eq!(
        background_poll_timeout(false, Duration::ZERO),
        Duration::MAX
    );
    assert_eq!(
        background_poll_timeout(true, Duration::ZERO),
        BACKGROUND_POLL_INTERVAL
    );
    assert_eq!(
        background_poll_timeout(true, Duration::from_millis(249)),
        Duration::from_millis(1)
    );
}

#[test]
fn idle_time_sync_wait_has_no_periodic_wakeup() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    let (control_sender, mut control_receiver) = tokio::sync::mpsc::unbounded_channel();
    let snapshot = || {
        let observed_at = Utc::now();
        system_services::SystemSnapshot {
            revision: 0,
            observed_at,
            weather: system_services::WeatherState::Loading,
            time: system_services::TimeState::Local {
                local_time: observed_at.fixed_offset(),
            },
            storage: system_services::StorageState::Loading,
            network: system_services::NetworkState::Loading,
            metrics: system_services::SystemMetricsSnapshot::loading(),
        }
    };
    let (snapshot_sender, mut snapshots) = tokio::sync::watch::channel(snapshot());

    runtime.block_on(async {
        assert!(
            tokio::time::timeout(
                Duration::from_millis(25),
                next_time_sync_wakeup(&mut control_receiver, &mut snapshots),
            )
            .await
            .is_err(),
            "an unchanged idle worker must remain asleep"
        );

        control_sender
            .send(TimeSyncControl::Refresh)
            .expect("refresh control");
        assert_eq!(
            next_time_sync_wakeup(&mut control_receiver, &mut snapshots).await,
            TimeSyncWakeup::Control(TimeSyncControl::Refresh)
        );

        snapshot_sender.send_replace(snapshot());
        assert_eq!(
            next_time_sync_wakeup(&mut control_receiver, &mut snapshots).await,
            TimeSyncWakeup::SnapshotChanged
        );
    });
}

#[test]
fn batched_input_installs_settled_hit_map_before_the_next_event() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let origin = Instant::now();
    let mut redraw = RedrawScheduler::new(origin, RedrawIdentity::from_session(&state), false);
    redraw.did_draw(origin);
    let identity_before = RedrawIdentity::from_session(&state);

    state.apply_input(InputEvent::from_key_label("Esc"));
    assert!(
        state
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::ExitDialog),
        "the controller's immediate map is settled until motion is synchronized"
    );
    synchronize_motion_hit_map_after_input(&mut state, &mut redraw, identity_before, origin);
    assert!(
        state
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::ExitDialog),
        "the next batched event must see the immediately rendered dialog"
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(state.shutdown_requested());
    assert_eq!(state.last_command(), Some(&ShellCommand::ConfirmExit));
}

#[test]
fn fullscreen_runtime_delegates_frame_composition() {
    let source = include_str!("../../../../src/session/runtime.rs");
    let start = source
        .find("pub(super) fn run_fullscreen_shell_session")
        .unwrap();
    let end = source[start..]
        .find("fn read_ready_terminal_event_batch")
        .unwrap()
        + start;
    let runtime = &source[start..end];
    assert!(runtime.contains("compositor.prepare("));
    assert!(runtime.contains("compositor.render(frame,"));
    assert!(
        !runtime.contains("ui::render_"),
        "normal frames must use the compositor"
    );
    assert!(
        !runtime.contains("frame.buffer_mut()"),
        "post effects belong to the compositor"
    );
}

#[test]
fn deferred_alert_toast_reentry_resumes_without_replaying_or_jumping() {
    let frame = |millis| ui::MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
        animation_speed_percent: 100,
    };
    let mut toast = None;
    sync_shell_toast(&mut toast, Some("Saved"), frame(0));
    sync_shell_toast(&mut toast, None, frame(200));
    let exiting = toast
        .as_ref()
        .expect("exiting toast")
        .visible_progress(frame(250));
    sync_shell_toast(&mut toast, Some("Saved"), frame(250));
    let resumed = toast.as_ref().expect("resumed toast");
    assert_eq!(resumed.visible_progress(frame(250)), exiting);

    let shown_at = resumed.shown_at;
    sync_shell_toast(&mut toast, Some("Saved"), frame(500));
    assert_eq!(toast.as_ref().expect("renewed toast").shown_at, shown_at);

    sync_shell_toast(&mut toast, Some("Different"), frame(600));
    let replacement = toast.as_ref().expect("replacement toast");
    assert_eq!(replacement.message, "Different");
    assert_eq!(replacement.visible_progress(frame(600)), 0);
}

fn recovery_asset_root(case: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tundra-startup-asset-recovery-{}-{}-{case}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

#[test]
fn startup_automatically_repairs_without_interactive_input() {
    let root = recovery_asset_root("auto");
    let (store, report) = ui::AsciiAssetStore::load_default_with_root_and_recovery(&root)
        .expect("automatic startup recovery");
    assert!(!report.repaired.is_empty());
    assert!(report.fallback.is_empty());
    assert!(ui::check_default_theme(&root).is_ok());
    assert!(store.home_icon_image_bytes("explorer").is_some());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_uses_memory_when_asset_root_is_not_writable() {
    let root = recovery_asset_root("blocked");
    std::fs::write(&root, b"not a directory").unwrap();
    let (store, report) =
        ui::AsciiAssetStore::load_default_with_root_and_recovery(&root).expect("embedded recovery");
    assert!(!report.fallback.is_empty());
    assert!(store.home_icon_image_bytes("explorer").is_some());
    assert_eq!(std::fs::read(&root).unwrap(), b"not a directory");
    std::fs::remove_file(root).unwrap();
}

fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers,
    })
}

fn collect_one_test_batch(source: &Rc<RefCell<VecDeque<Event>>>) -> Vec<Event> {
    let first = source
        .borrow_mut()
        .pop_front()
        .expect("test event source must not be empty");
    let poll_source = Rc::clone(source);
    let read_source = Rc::clone(source);
    collect_ready_terminal_event_batch(
        first,
        move || Ok(!poll_source.borrow().is_empty()),
        move || {
            read_source
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| io::Error::other("test event source was exhausted"))
        },
    )
    .expect("collect terminal event batch")
}

#[test]
fn command_line_uses_a_low_latency_refresh_without_advancing_state_ticks() {
    assert_eq!(
        command_line_poll_timeout(true, Duration::from_millis(250)),
        (COMMAND_LINE_REFRESH_INTERVAL, false)
    );
    assert_eq!(
        command_line_poll_timeout(true, Duration::from_millis(5)),
        (Duration::from_millis(5), true)
    );
    assert_eq!(
        command_line_poll_timeout(false, Duration::from_millis(250)),
        (Duration::from_millis(250), true)
    );
}

#[test]
fn command_line_runtime_leaves_shell_chrome_mouse_input_for_the_shell() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    state.screen_stack = vec![ShellScreen::Home, ShellScreen::CommandLine];
    state.refresh_hit_map();

    let clock_area = state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == ShellComponent::ClockButton)
        .expect("Command Line must expose the Shell clock button")
        .area;
    let clock_input = InputEvent::mouse_down(ui::MouseButton::Left, (clock_area.x, clock_area.y));
    assert!(!command_line_captures_input(&state, &clock_input));
    assert!(command_line_captures_input(
        &state,
        &InputEvent::mouse_up(ui::MouseButton::Left, (clock_area.x, clock_area.y))
    ));

    let terminal_area = ui::command_line_terminal_area(Rect::new(0, 0, 120, 40)).unwrap();
    let terminal_input = InputEvent::mouse_moved((terminal_area.x, terminal_area.y));
    assert!(command_line_captures_input(&state, &terminal_input));
    assert!(command_line_captures_input(
        &state,
        &InputEvent::key(ui::Key::Char('a'))
    ));
    assert!(command_line_captures_input(
        &state,
        &InputEvent::paste("command")
    ));
}

#[test]
fn mouse_motion_flood_is_consumed_in_a_few_render_batches() {
    let source = Rc::new(RefCell::new(
        (0..10_000)
            .map(|index| {
                mouse_event(
                    MouseEventKind::Moved,
                    (index % 200) as u16,
                    (index % 80) as u16,
                    KeyModifiers::NONE,
                )
            })
            .collect::<VecDeque<_>>(),
    ));
    let mut rendered_events = Vec::new();
    let mut batch_count = 0;

    while !source.borrow().is_empty() {
        rendered_events.extend(collect_one_test_batch(&source));
        batch_count += 1;
    }

    assert_eq!(batch_count, 3);
    assert_eq!(rendered_events.len(), batch_count);
    assert_eq!(
        rendered_events.last(),
        Some(&mouse_event(
            MouseEventKind::Moved,
            (9_999 % 200) as u16,
            (9_999 % 80) as u16,
            KeyModifiers::NONE,
        ))
    );
}

#[test]
fn mouse_coalescing_preserves_semantic_event_boundaries() {
    let source = Rc::new(RefCell::new(VecDeque::from([
        mouse_event(MouseEventKind::Moved, 1, 1, KeyModifiers::NONE),
        mouse_event(MouseEventKind::Moved, 2, 2, KeyModifiers::NONE),
        mouse_event(MouseEventKind::Moved, 3, 3, KeyModifiers::SHIFT),
        Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
        mouse_event(MouseEventKind::Moved, 4, 4, KeyModifiers::NONE),
        mouse_event(MouseEventKind::Moved, 5, 5, KeyModifiers::NONE),
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            5,
            5,
            KeyModifiers::NONE,
        ),
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            6,
            6,
            KeyModifiers::NONE,
        ),
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            7,
            7,
            KeyModifiers::NONE,
        ),
        mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE),
        mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE),
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            7,
            7,
            KeyModifiers::NONE,
        ),
        Event::Paste("preserve me".to_string()),
        Event::FocusGained,
        Event::Resize(100, 40),
        Event::Resize(120, 50),
    ])));

    let first_batch = collect_one_test_batch(&source);
    assert_eq!(
        first_batch,
        vec![
            mouse_event(MouseEventKind::Moved, 2, 2, KeyModifiers::NONE),
            mouse_event(MouseEventKind::Moved, 3, 3, KeyModifiers::SHIFT),
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL,)),
        ]
    );

    let second_batch = collect_one_test_batch(&source);
    assert_eq!(
        second_batch,
        vec![
            mouse_event(MouseEventKind::Moved, 5, 5, KeyModifiers::NONE),
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                5,
                5,
                KeyModifiers::NONE,
            ),
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                7,
                7,
                KeyModifiers::NONE,
            ),
            mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE,),
            mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE,),
            mouse_event(
                MouseEventKind::Up(MouseButton::Left),
                7,
                7,
                KeyModifiers::NONE,
            ),
            Event::Paste("preserve me".to_string()),
        ]
    );

    let third_batch = collect_one_test_batch(&source);
    assert!(source.borrow().is_empty());
    assert_eq!(
        third_batch,
        vec![Event::FocusGained, Event::Resize(120, 50),]
    );
}

#[test]
fn a_ready_key_is_dispatched_without_polling_for_more_events() {
    let polls = Cell::new(0_usize);
    let batch = collect_ready_terminal_event_batch(
        Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        || {
            polls.set(polls.get() + 1);
            Ok(true)
        },
        || panic!("a key batch must not read a later event"),
    )
    .expect("key batch");

    assert_eq!(polls.get(), 0);
    assert_eq!(
        batch,
        vec![Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))]
    );
}

#[test]
fn ready_event_drain_is_bounded_when_the_source_never_goes_idle() {
    let reads = Cell::new(0_usize);
    let batch = collect_ready_terminal_event_batch(
        mouse_event(MouseEventKind::Moved, 0, 0, KeyModifiers::NONE),
        || Ok(true),
        || {
            let next = reads.get() + 1;
            reads.set(next);
            Ok(mouse_event(
                MouseEventKind::Moved,
                (next % 200) as u16,
                (next % 80) as u16,
                KeyModifiers::NONE,
            ))
        },
    )
    .expect("bounded batch");

    assert_eq!(reads.get() + 1, MAX_READY_TERMINAL_EVENTS_PER_FRAME);
    assert_eq!(batch.len(), 1);
}

#[test]
fn configured_operating_system_time_uses_platform_boundary() {
    let root = std::env::temp_dir().join(format!(
        "tundra-runtime-system-time-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let app_paths = platform::build_windows_app_paths(
        root.join("roaming"),
        root.join("local"),
        root.join("temp"),
    )
    .expect("test app paths");
    let user_dirs = platform::UserDirs::new(
        root.join("desktop"),
        root.join("documents"),
        root.join("downloads"),
        root.join("pictures"),
        root.join("videos"),
        root.join("music"),
        root.join("roaming"),
    )
    .expect("test user dirs");
    let platform = platform::mock::MockPlatform::new(user_dirs, app_paths);
    let system_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    platform.set_system_time_result(Ok(system_time));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");

    let result = runtime
        .block_on(synchronize_configured_time(
            &storage::TimeSyncConfig {
                source: storage::TimeSyncSource::OperatingSystem,
                server_url: Some("https://ignored.example.test/".to_string()),
            },
            &platform,
        ))
        .expect("system time sync");

    assert_eq!(result, DateTime::<Utc>::from(system_time));
    assert!(
        platform
            .calls()
            .iter()
            .any(|call| { matches!(call, platform::mock::MockCall::SystemTime) })
    );
}

#[test]
fn failed_terminal_preflight_writes_no_banner_or_fullscreen_sequence() {
    let fail = || Err(io::Error::other("terminal is too small"));

    let mut static_output = Vec::new();
    assert!(run_not_fullscreen_without_animation_with_loader(&mut static_output, fail).is_err());
    assert!(static_output.is_empty());

    let mut animated_output = Vec::new();
    assert!(run_not_fullscreen_with_loader(&mut animated_output, fail).is_err());
    assert!(animated_output.is_empty());

    let mut fullscreen_output = Vec::new();
    assert!(
        run_fullscreen_once_without_animation_with_loader(&mut fullscreen_output, fail).is_err()
    );
    assert!(fullscreen_output.is_empty());
}

#[test]
fn theme_reloader_applies_active_user_changes_and_recovers_from_invalid_users() {
    let root = std::env::temp_dir().join(format!(
        "tundra-theme-reload-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let app_paths = platform::build_windows_app_paths(
        root.join("roaming"),
        root.join("local"),
        root.join("temp"),
    )
    .expect("test paths");
    let storage = StorageManager::open(app_paths)
        .expect("test storage")
        .manager;
    let started_at = Instant::now();
    let appearance = storage::AppearanceConfig {
        border_shape: storage::BorderShape::Square,
        border_color: storage::BorderColor::Rgb(0x38, 0xBD, 0xF8),
        accent_color: storage::BorderColor::LightMagenta,
        icon_display_mode: storage::IconDisplayMode::Image,
        ..storage::AppearanceConfig::default()
    };
    UserService::new(storage.clone())
        .bootstrap_admin_with_hint_and_appearance(
            "AdminUser",
            "StrongPass123",
            None,
            appearance.clone(),
        )
        .expect("bootstrap admin with appearance");
    let session = SessionService::new(storage.clone())
        .login("AdminUser", "StrongPass123")
        .expect("login");
    let mut reloader = UserThemeReloader::new(Some(storage.clone()), started_at);
    let mut theme = ui::TundraTheme::default_dark();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.complete_login(session);

    reloader.last_observed = None;
    reloader.next_check = started_at;
    reloader.poll_at(started_at, &mut theme, &mut state);
    assert_eq!(theme.border_shape, ui::BorderShape::Square);
    assert_eq!(
        theme.border_color,
        ratatui::style::Color::Rgb(0x38, 0xBD, 0xF8)
    );
    assert_eq!(theme.accent_color, ratatui::style::Color::LightMagenta);
    assert_eq!(state.app.active_appearance(), Some(&appearance));

    std::fs::write(&storage.layout().users_path, "{ not valid json")
        .expect("corrupt users fixture");
    let failure_at = started_at + THEME_RELOAD_INTERVAL;
    reloader.last_observed = None;
    reloader.next_check = failure_at;
    reloader.poll_at(failure_at, &mut theme, &mut state);
    assert_eq!(
        theme.border_color,
        ratatui::style::Color::Rgb(0x38, 0xBD, 0xF8)
    );
    assert_eq!(theme.accent_color, ratatui::style::Color::LightMagenta);
    assert_eq!(
        state
            .to_notification_view_model()
            .expect("reload failure modal")
            .title,
        "Theme reload failed"
    );

    std::fs::remove_file(&storage.layout().users_path).expect("remove corrupt users");
    let mut users = storage::UsersDocument::default();
    let now = unix_millis();
    users.users.push(storage::UserRecord {
        id: state.auth_session().expect("session").user_id.clone(),
        username: "AdminUser".to_string(),
        display_name: "AdminUser".to_string(),
        role: "Admin".to_string(),
        password_hash: String::new(),
        password_hint: None,
        appearance,
        personalization_pending: false,
        system_status_dashboard: storage::SystemStatusDashboardConfig::for_role("Admin"),
        enabled: true,
        failed_login_attempts: 0,
        locked_until_epoch_ms: None,
        created_at_epoch_ms: now,
        updated_at_epoch_ms: now,
        last_login_at_epoch_ms: Some(now),
    });
    storage.save_users(&users).expect("repaired users");
    let recovery_at = failure_at + THEME_RELOAD_INTERVAL;
    reloader.last_observed = None;
    reloader.next_check = recovery_at;
    reloader.poll_at(recovery_at, &mut theme, &mut state);
    assert!(state.to_notification_view_model().is_none());

    platform::cleanup_temp_path(&root).expect("clean test root");
}

#[test]
fn theme_reloader_switches_from_custom_admin_theme_to_managed_user_defaults() {
    let root = std::env::temp_dir().join(format!(
        "tundra-user-theme-switch-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let app_paths = platform::build_windows_app_paths(
        root.join("roaming"),
        root.join("local"),
        root.join("temp"),
    )
    .expect("test paths");
    let storage = StorageManager::open(app_paths)
        .expect("test storage")
        .manager;
    let custom = storage::AppearanceConfig {
        border_shape: storage::BorderShape::Square,
        border_color: storage::BorderColor::LightGreen,
        accent_color: storage::BorderColor::LightMagenta,
        icon_display_mode: storage::IconDisplayMode::Image,
        ..storage::AppearanceConfig::default()
    };
    let users = UserService::new(storage.clone());
    users
        .bootstrap_admin_with_hint_and_appearance("AdminUser", "StrongPass123", None, custom)
        .expect("bootstrap");
    let admin_session = SessionService::new(storage.clone())
        .login("AdminUser", "StrongPass123")
        .expect("admin login");
    users
        .create_user(
            &admin_session,
            "ManagedUser",
            "Managed User",
            UserRole::User,
            "ManagedPass123",
        )
        .expect("managed user");
    let managed_session = SessionService::new(storage.clone())
        .login("ManagedUser", "ManagedPass123")
        .expect("managed login");

    let started_at = Instant::now();
    let mut reloader = UserThemeReloader::new(Some(storage), started_at);
    let mut theme = ui::TundraTheme::default_dark();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.complete_login(admin_session);
    reloader.poll_at(started_at, &mut theme, &mut state);
    assert_eq!(theme.border_shape, ui::BorderShape::Square);
    assert_eq!(theme.border_color, ratatui::style::Color::LightGreen);
    assert_eq!(theme.accent_color, ratatui::style::Color::LightMagenta);

    state.complete_login(managed_session);
    reloader.poll_at(started_at, &mut theme, &mut state);
    assert_eq!(theme.border_shape, ui::BorderShape::Rounded);
    assert_eq!(
        theme.border_color,
        ratatui::style::Color::Rgb(0x29, 0x43, 0x4E)
    );
    assert_eq!(
        theme.accent_color,
        ratatui::style::Color::Rgb(0x63, 0xD3, 0xE5)
    );

    platform::cleanup_temp_path(&root).expect("clean test root");
}
