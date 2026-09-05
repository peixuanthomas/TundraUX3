use chrono::{TimeZone, Timelike, Utc};

use super::*;

fn session(id: &str, username: &str) -> AuthSession {
    AuthSession {
        session_id: format!("session-{id}"),
        user_id: format!("user-{id}"),
        username: username.to_string(),
        role: identity::UserRole::User,
        started_at_epoch_ms: 1,
    }
}

#[test]
fn dispatch_without_open_apps_is_a_safe_noop() {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    let root = std::env::temp_dir().join(format!(
        "tundra-app-state-empty-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos()
    ));
    fs::create_dir_all(root.join("Documents")).expect("test documents directory");
    let app_paths = platform::build_windows_app_paths(
        root.join("Roaming"),
        root.join("Local"),
        root.join("Temp"),
    )
    .expect("test paths");
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("Roaming"),
    )
    .expect("test user directories");
    let platform = platform::mock::MockPlatform::new(user_dirs, app_paths.clone());
    let storage = storage::StorageManager::open(app_paths)
        .expect("test storage")
        .manager;
    let mut state = AppState::default();

    assert_eq!(
        state.dispatch_explorer_at(
            ExplorerCommand::Refresh,
            &platform,
            &storage,
            Instant::now()
        ),
        (AppAction::Redraw, ExplorerEffect::None)
    );
    assert_eq!(
        state.dispatch_launcher_at(
            LauncherCommand::Refresh,
            &platform,
            &storage,
            Instant::now()
        ),
        (AppAction::Redraw, LauncherEffect::None)
    );
    let _ = platform::cleanup_temp_path(&root);
}

#[test]
fn editor_viewport_is_applied_when_an_editor_is_open_and_is_safe_otherwise() {
    let mut state = AppState::default();
    let viewport = EditorViewport {
        top_line: 12,
        left_column: 4,
    };
    let now = Instant::now();

    assert_eq!(
        state.dispatch_at(AppCommand::SetEditorViewport(viewport), now),
        AppAction::Redraw
    );
    assert!(state.editor_state().is_none());

    state.dispatch_at(AppCommand::SetEditorState(Some(EditorState::new())), now);
    assert_eq!(
        state.dispatch_at(AppCommand::SetEditorViewport(viewport), now),
        AppAction::Redraw
    );
    assert_eq!(
        state.editor_state().map(|editor| editor.viewport),
        Some(viewport)
    );
}

#[test]
fn editor_commands_preserve_graphemes_and_queue_effects_in_order() {
    let mut state = AppState::default();
    let now = Instant::now();

    assert_eq!(
        state.dispatch_at(AppCommand::SetEditorState(Some(EditorState::new())), now),
        AppAction::Redraw
    );
    assert_eq!(
        state.dispatch_at(
            AppCommand::Editor(EditorCommand::InsertText("A好e\u{301}🙂".to_string())),
            now,
        ),
        AppAction::Redraw
    );
    assert_eq!(
        state
            .snapshot()
            .editor
            .map(EditorState::export_text)
            .as_deref(),
        Some("A好e\u{301}🙂")
    );

    state.dispatch_at(AppCommand::Editor(EditorCommand::RequestOpen), now);
    state.dispatch_at(AppCommand::Editor(EditorCommand::RequestPaste), now);
    assert_eq!(
        state.take_editor_effects(),
        vec![EditorEffect::OpenFilePicker, EditorEffect::ReadClipboard]
    );
}

#[test]
fn replacing_or_clearing_editor_state_discards_pending_effects() {
    let mut state = AppState::default();
    let now = Instant::now();
    state.dispatch_at(AppCommand::SetEditorState(Some(EditorState::new())), now);
    state.dispatch_at(AppCommand::Editor(EditorCommand::RequestOpen), now);

    state.dispatch_at(AppCommand::SetEditorState(Some(EditorState::new())), now);
    assert!(state.take_editor_effects().is_empty());
    assert!(state.snapshot().editor.is_some());

    state.dispatch_at(AppCommand::Editor(EditorCommand::RequestPaste), now);
    state.dispatch_at(AppCommand::SetEditorState(None), now);
    assert!(state.take_editor_effects().is_empty());
    assert!(state.editor_state().is_none());
    assert!(state.snapshot().editor.is_none());
}

#[test]
fn auth_session_can_be_set_replaced_cleared_and_borrowed_from_a_snapshot() {
    let mut state = AppState::default();
    let first = session("first", "ada");
    let replacement = session("replacement", "lin");

    assert_eq!(
        state.dispatch_at(
            AppCommand::SetAuthSession(Some(first.clone())),
            Instant::now()
        ),
        AppAction::Redraw
    );
    assert_eq!(state.auth_session(), Some(&first));
    let snapshot = state.snapshot();
    assert_eq!(snapshot.auth_session, Some(&first));
    drop(snapshot);
    assert_eq!(state, state.clone());

    assert_eq!(
        state.dispatch_at(
            AppCommand::SetAuthSession(Some(replacement.clone())),
            Instant::now(),
        ),
        AppAction::Redraw
    );
    assert_eq!(state.auth_session(), Some(&replacement));
    assert_ne!(state, AppState::default());

    assert_eq!(
        state.dispatch_at(AppCommand::SetAuthSession(None), Instant::now()),
        AppAction::Redraw
    );
    assert!(state.auth_session().is_none());
    assert!(state.snapshot().auth_session.is_none());
}

#[test]
fn commands_request_the_expected_runtime_actions() {
    let mut state = AppState::default();
    let now = Instant::now();
    let synced_utc = Utc
        .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
        .single()
        .unwrap();

    assert_eq!(state.dispatch_at(AppCommand::Tick, now), AppAction::Redraw);
    assert_eq!(
        state.dispatch_at(AppCommand::ApplyTimeSync(Ok(synced_utc)), now),
        AppAction::Redraw
    );
    assert_eq!(
        state.dispatch_at(AppCommand::RequestExit, now),
        AppAction::Redraw
    );
    assert_eq!(
        state.dispatch_at(AppCommand::ConfirmExit, now),
        AppAction::Exit
    );
    assert_eq!(
        state.dispatch_at(AppCommand::RequestPowerOff, now),
        AppAction::PowerOff
    );
}

#[test]
fn cancelling_exit_hides_the_confirmation() {
    let mut state = AppState::default();
    let now = Instant::now();

    state.dispatch_at(AppCommand::RequestExit, now);
    assert!(state.snapshot().exit_confirmation_requested);

    assert_eq!(
        state.dispatch_at(AppCommand::CancelExit, now),
        AppAction::Redraw
    );
    assert!(!state.snapshot().exit_confirmation_requested);
}

#[test]
fn snapshot_keeps_timezone_identity_and_dst_projection_together() {
    let mut state = AppState::new(Some("America/New_York".to_string()));
    let now = Instant::now();
    let dst_boundary_utc = Utc.with_ymd_and_hms(2026, 3, 8, 6, 30, 0).single().unwrap();

    state.dispatch_at(AppCommand::ApplyTimeSync(Ok(dst_boundary_utc)), now);
    let snapshot = state.snapshot();
    let timezone = snapshot.clock.timezone.expect("known timezone is resolved");
    let projected = snapshot.clock.utc.with_timezone(&timezone);

    assert_eq!(snapshot.clock_timezone_id, Some("America/New_York"));
    assert_eq!(snapshot.clock.date, projected.date_naive());
    assert_eq!(snapshot.clock.time, projected.time());
    assert_eq!(snapshot.clock.time.hour(), 1);
    assert!(snapshot.clock.warning.is_none());
}

#[test]
fn clone_equality_ignores_the_network_clock_anchor() {
    let mut synchronized = AppState::new(Some("UTC".to_string()));
    let utc = Utc
        .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
        .single()
        .unwrap();
    synchronized.dispatch_at(AppCommand::ApplyTimeSync(Ok(utc)), Instant::now());

    let unsynchronized = AppState::new(Some("UTC".to_string()));
    assert_eq!(synchronized, unsynchronized);
    assert_eq!(synchronized.clone(), synchronized);
}

#[test]
fn resetting_timezone_handles_invalid_ids_and_installs_a_sync_anchor() {
    let mut state = AppState::new(Some("Asia/Shanghai".to_string()));
    let now = Instant::now();
    let synchronized_utc = Utc
        .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
        .single()
        .unwrap();

    assert_eq!(
        state.dispatch_at(
            AppCommand::SetClockTimezone {
                timezone_id: Some("Not/AZone".to_string()),
                synchronized_utc: None,
            },
            now,
        ),
        AppAction::Redraw
    );
    let invalid_snapshot = state.snapshot();
    assert_eq!(invalid_snapshot.clock_timezone_id, Some("Not/AZone"));
    assert!(invalid_snapshot.clock.timezone.is_none());
    assert!(
        invalid_snapshot
            .clock
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("Invalid timezone Not/AZone"))
    );

    assert_eq!(
        state.dispatch_at(
            AppCommand::SetClockTimezone {
                timezone_id: Some("UTC".to_string()),
                synchronized_utc: Some(synchronized_utc),
            },
            now,
        ),
        AppAction::Redraw
    );
    let utc_snapshot = state.snapshot();
    assert_eq!(utc_snapshot.clock_timezone_id, Some("UTC"));
    assert_eq!(
        utc_snapshot
            .clock
            .timezone
            .map(|timezone| timezone.to_string()),
        Some("UTC".to_string())
    );
    assert_eq!(
        utc_snapshot.clock.utc.date_naive(),
        synchronized_utc.date_naive()
    );
    assert_eq!(utc_snapshot.clock.utc.hour(), synchronized_utc.hour());
    assert_eq!(utc_snapshot.clock.utc.minute(), synchronized_utc.minute());
    assert!(utc_snapshot.clock.warning.is_none());
}

#[test]
fn storage_config_and_active_appearance_are_canonical_app_state() {
    let config = storage::StorageConfig {
        timezone: "Asia/Shanghai".to_string(),
        language: "en-US".to_string(),
        ..storage::StorageConfig::default()
    };
    let mut state = AppState::with_storage_config(config.clone());
    let appearance = storage::AppearanceConfig {
        border_shape: storage::BorderShape::Square,
        ..storage::AppearanceConfig::default()
    };

    assert_eq!(state.storage_config(), &config);
    assert_eq!(state.snapshot().storage_config, &config);
    assert_eq!(state.snapshot().clock_timezone_id, Some("Asia/Shanghai"));
    assert!(state.active_appearance().is_none());

    state.dispatch_at(
        AppCommand::SetActiveAppearance(Some(appearance.clone())),
        Instant::now(),
    );
    assert_eq!(state.active_appearance(), Some(&appearance));
    assert_eq!(state.snapshot().active_appearance, Some(&appearance));

    state.dispatch_at(AppCommand::SetActiveAppearance(None), Instant::now());
    assert!(state.active_appearance().is_none());
}

#[test]
fn replacing_non_timezone_config_preserves_clock_anchor() {
    let mut config = storage::StorageConfig {
        timezone: "UTC".to_string(),
        ..storage::StorageConfig::default()
    };
    let mut state = AppState::with_storage_config(config.clone());
    let synchronized_utc = Utc
        .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
        .single()
        .unwrap();
    state.dispatch_at(
        AppCommand::ApplyTimeSync(Ok(synchronized_utc)),
        Instant::now(),
    );

    config.weather_location = Some("Pudong, Shanghai, China".to_string());
    state.dispatch_at(
        AppCommand::SetStorageConfig {
            config: config.clone(),
            synchronized_utc: None,
        },
        Instant::now(),
    );

    let snapshot = state.snapshot();
    assert_eq!(snapshot.storage_config, &config);
    assert_eq!(snapshot.clock_timezone_id, Some("UTC"));
    assert_eq!(
        snapshot.clock.utc.date_naive(),
        synchronized_utc.date_naive()
    );
    assert_eq!(snapshot.clock.utc.hour(), synchronized_utc.hour());
    assert_eq!(snapshot.clock.utc.minute(), synchronized_utc.minute());
}

#[test]
fn replacing_timezone_updates_config_and_clock_together() {
    let mut state = AppState::default();
    let mut config = state.storage_config().clone();
    config.timezone = "America/New_York".to_string();
    let synchronized_utc = Utc
        .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
        .single()
        .unwrap();

    state.dispatch_at(
        AppCommand::SetStorageConfig {
            config: config.clone(),
            synchronized_utc: Some(synchronized_utc),
        },
        Instant::now(),
    );

    let snapshot = state.snapshot();
    assert_eq!(snapshot.storage_config, &config);
    assert_eq!(snapshot.clock_timezone_id, Some("America/New_York"));
    assert_eq!(
        snapshot.clock.utc.date_naive(),
        synchronized_utc.date_naive()
    );
    assert_eq!(snapshot.clock.utc.hour(), synchronized_utc.hour());
}
#[test]
fn notifications_are_owned_dispatched_and_snapshotted_by_app_state() {
    let started_at = Instant::now();
    let mut state = AppState::default();
    assert_eq!(state.snapshot().notifications.status(), "Ready");

    state.dispatch_at(
        AppCommand::Notification(NotificationCommand::ShowToast("Saved".to_string())),
        started_at,
    );
    assert_eq!(state.notification_center().toast(), Some("Saved"));

    state.dispatch_at(AppCommand::Tick, started_at + crate::DEFAULT_TOAST_DURATION);
    assert_eq!(state.notification_center().toast(), None);
}

#[test]
fn notification_modal_response_stays_in_the_app_domain_queue() {
    let mut state = AppState::default();
    let id = state.push_notification_modal(Notification::modal(
        "Confirm",
        "Continue?",
        crate::NotificationTone::Warning,
        vec![crate::NotificationAction::new("continue", "Continue")],
    ));

    let activated = state.activate_selected_notification_action().unwrap();
    assert_eq!(activated.notification_id, id);
    assert_eq!(activated.action_id, "continue");
    assert_eq!(state.take_notification_response(), Some(activated));
}
#[test]
fn system_status_snapshot_is_copied_and_stored_as_read_only_app_state() {
    let observed_at = Utc::now();
    let source = system_services_model::SystemSnapshot {
        revision: 7,
        observed_at,
        weather: system_services_model::WeatherState::Loading,
        time: system_services_model::TimeState::Local {
            local_time: observed_at.fixed_offset(),
        },
        storage: system_services_model::StorageState::Unavailable {
            reason: "storage offline".to_string(),
        },
        network: system_services_model::NetworkState::Unavailable {
            reason: "network offline".to_string(),
        },
        metrics: system_services_model::SystemMetricsSnapshot::loading(),
    };
    let status = AppSystemStatusSnapshot::from(&source);
    let mut state = AppState::default();
    assert_eq!(
        state.dispatch_at(
            AppCommand::SetSystemStatusSnapshot(Some(status.clone())),
            Instant::now(),
        ),
        AppAction::Redraw
    );
    assert_eq!(state.system_status_snapshot(), Some(&status));
    assert_eq!(state.snapshot().system_status, Some(&status));
    assert_ne!(state, AppState::default());
}
