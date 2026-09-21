use super::*;

struct TestEditorTaskDriver;

impl TestEditorTaskDriver {
    fn install(state: &mut ShellSession) -> Self {
        state.editor_task_runtime = ShellEditorTaskRuntime::new();
        Self
    }

    fn complete_next_load(&self, state: &mut ShellSession) {
        for _ in 0..400 {
            state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);
            if state.editor_load_state.is_none() {
                return;
            }
            std::thread::yield_now();
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("Editor load task did not finish in time");
    }
}

fn temporary_document(name: &str, contents: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "tundra-shell-diagnostics-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let directory = std::fs::canonicalize(directory).unwrap();
    let path = directory.join(name);
    std::fs::write(&path, contents).unwrap();
    (directory, path)
}

fn log_file(path: std::path::PathBuf, relative_path: &str) -> app::diagnostics::DiagnosticLogFile {
    let size_bytes = std::fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    app::diagnostics::DiagnosticLogFile {
        path,
        relative_path: std::path::PathBuf::from(relative_path),
        modified_at: Utc::now(),
        size_bytes,
    }
}

fn session(role: UserRole) -> AuthSession {
    AuthSession {
        source: identity::IdentitySource::LocalAccount,
        session_id: format!("{}-session", role.as_str()),
        user_id: format!("{}-id", role.as_str()),
        username: role.as_str().to_ascii_lowercase(),
        role,
        started_at_epoch_ms: 1,
    }
}

fn snapshot() -> app::diagnostics::DiagnosticsSnapshot {
    app::diagnostics::DiagnosticsSnapshot {
        scanned_at: Utc::now(),
        checks: vec![app::diagnostics::DiagnosticCheck {
            id: "path.data".to_string(),
            category: app::diagnostics::DiagnosticCategory::Paths,
            label: "Data path".to_string(),
            status: app::diagnostics::DiagnosticStatus::Warning,
            summary: "Directory is missing".to_string(),
            detail: "/private/example/data is missing".to_string(),
            remediation: Some("Create the missing directory".to_string()),
            repair: Some(app::diagnostics::DiagnosticsRepairAction::CreateDirectory {
                label: "Data path".to_string(),
                path: std::path::PathBuf::from("/private/example/data"),
            }),
        }],
        incidents: Vec::new(),
        logs: Vec::new(),
        warnings: Vec::new(),
    }
}

fn terminal_snapshot(
    status: app::diagnostics::DiagnosticStatus,
) -> app::diagnostics::DiagnosticsSnapshot {
    let mut snapshot = snapshot();
    snapshot.checks = vec![app::diagnostics::DiagnosticCheck {
        id: "environment.terminal".to_string(),
        category: app::diagnostics::DiagnosticCategory::Environment,
        label: "Terminal".to_string(),
        status,
        summary: "legacy terminal result".to_string(),
        detail: "legacy terminal result".to_string(),
        remediation: None,
        repair: None,
    }];
    snapshot
}

fn state(role: UserRole) -> ShellSession {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 30));
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(session(role))),
        Instant::now(),
    );
    state.app.dispatch_at(
        app::AppCommand::SetDiagnosticsSnapshot(Some(snapshot())),
        Instant::now(),
    );
    state.screen_stack = vec![ShellScreen::Home, ShellScreen::SystemStatus];
    state.focused_component = ShellComponent::SystemStatus;
    state
}

fn update_diagnostics_snapshot(
    state: &mut ShellSession,
    update: impl FnOnce(&mut app::diagnostics::DiagnosticsSnapshot),
) {
    let mut snapshot = state.app.diagnostics_snapshot().cloned().unwrap();
    update(&mut snapshot);
    state.app.dispatch_at(
        app::AppCommand::SetDiagnosticsSnapshot(Some(snapshot)),
        Instant::now(),
    );
}

#[test]
fn probed_graphics_protocol_controls_terminal_diagnostic_status() {
    let mut snapshot = terminal_snapshot(app::diagnostics::DiagnosticStatus::Pass);
    apply_terminal_graphics_check(
        &mut snapshot,
        Some(&ui::TerminalGraphicsProbeStatus::Unsupported),
    );
    let check = &snapshot.checks[0];
    assert_eq!(
        check.status,
        app::diagnostics::DiagnosticStatus::Unsupported
    );
    assert!(check.summary.contains("Unsupported"));
    assert!(check.remediation.is_none());

    apply_terminal_graphics_check(
        &mut snapshot,
        Some(&ui::TerminalGraphicsProbeStatus::NoResponse {
            reason: "query timeout".to_string(),
        }),
    );
    let check = &snapshot.checks[0];
    assert_eq!(check.status, app::diagnostics::DiagnosticStatus::Warning);
    assert!(check.summary.contains("no response"));
    assert!(check.remediation.is_some());

    apply_terminal_graphics_check(
        &mut snapshot,
        Some(&ui::TerminalGraphicsProbeStatus::Verified(
            ui::EditorGraphicsProtocol::Sixel,
        )),
    );
    let check = &snapshot.checks[0];
    assert_eq!(check.status, app::diagnostics::DiagnosticStatus::Pass);
    assert!(check.summary.contains("Sixel graphics protocol verified"));
    assert!(check.remediation.is_none());
}

#[test]
fn startup_graphics_policy_only_schedules_theme_change_for_unsupported() {
    let mut unsupported = ShellSession::new(ShellLaunchConfig::default(), (120, 30));
    unsupported
        .apply_terminal_graphics_startup_policy(&ui::TerminalGraphicsProbeStatus::Unsupported);
    assert!(unsupported.pending_default_ascii_icon_fallback);
    assert_eq!(
        unsupported
            .to_notification_view_model()
            .expect("unsupported notification")
            .title,
        "Terminal graphics unsupported"
    );

    let mut no_response = ShellSession::new(ShellLaunchConfig::default(), (120, 30));
    no_response.apply_terminal_graphics_startup_policy(
        &ui::TerminalGraphicsProbeStatus::NoResponse {
            reason: "query timeout".to_string(),
        },
    );
    assert!(!no_response.pending_default_ascii_icon_fallback);
    let notice = no_response
        .to_notification_view_model()
        .expect("no-response notification");
    assert_eq!(notice.title, "No terminal graphics response");
    assert!(notice.message.contains("not changed"));

    let mut verified = ShellSession::new(ShellLaunchConfig::default(), (120, 30));
    verified.apply_terminal_graphics_startup_policy(&ui::TerminalGraphicsProbeStatus::Verified(
        ui::EditorGraphicsProtocol::Kitty,
    ));
    assert!(!verified.pending_default_ascii_icon_fallback);
    assert!(verified.to_notification_view_model().is_none());
}

#[test]
fn unsupported_custom_theme_warns_without_scheduling_a_change() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tundra-terminal-custom-theme-{}-{nonce}",
        std::process::id()
    ));
    ui::restore_default_theme(&root).expect("restore theme fixture");
    std::fs::rename(
        root.join("themes").join(ui::DEFAULT_THEME_ID),
        root.join("themes").join("custom"),
    )
    .expect("rename theme fixture");
    let custom_assets =
        ui::RuntimeAsciiAssets::load_with_root(&root, "custom").expect("load custom theme");
    let startup = ShellStartupState::clean(
        platform::PlatformKind::Windows,
        platform::PlatformCapabilities::native_supported(),
    );
    let mut state = ShellSession::new_with_startup_and_assets(
        ShellLaunchConfig::default(),
        (120, 30),
        startup,
        custom_assets,
    );

    // This theme-only fixture intentionally omits locale files. Acknowledge their
    // startup recovery before checking the separate terminal graphics warning.
    state.notification_dismiss_modal_by_key("shell.resource-recovery");
    state.apply_terminal_graphics_startup_policy(&ui::TerminalGraphicsProbeStatus::Unsupported);

    assert!(!state.pending_default_ascii_icon_fallback);
    let notice = state
        .to_notification_view_model()
        .expect("custom theme warning");
    assert!(notice.message.contains("custom theme was left unchanged"));
    platform::cleanup_temp_path(&root).expect("clean fixture");
}

#[test]
fn unsupported_default_image_mode_is_persisted_as_ascii_after_login() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tundra-terminal-icon-fallback-{}-{nonce}",
        std::process::id()
    ));
    let paths = platform::build_windows_app_paths(
        root.join("roaming"),
        root.join("local"),
        root.join("temp"),
    )
    .expect("test app paths");
    let storage = StorageManager::open(paths).expect("test storage").manager;
    UserService::new(storage.clone())
        .bootstrap_admin_with_hint_and_appearance(
            "AdminUser",
            "StrongPass123",
            None,
            storage::AppearanceConfig::default(),
        )
        .expect("bootstrap user");
    let session = SessionService::new(storage.clone())
        .login("AdminUser", "StrongPass123")
        .expect("login");
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 30));
    state.storage_manager = Some(storage.clone());
    state.pending_default_ascii_icon_fallback = true;

    state.complete_login(session);

    let user = storage
        .load_users()
        .expect("load users")
        .users
        .into_iter()
        .find(|user| user.username == "AdminUser")
        .expect("admin user");
    assert_eq!(
        user.appearance.icon_display_mode,
        storage::IconDisplayMode::Ascii
    );
    assert_eq!(
        state
            .app
            .active_appearance()
            .expect("active appearance")
            .icon_display_mode,
        storage::IconDisplayMode::Ascii
    );

    platform::cleanup_temp_path(&root).expect("clean fixture");
}

fn install_temporary_storage(state: &mut ShellSession) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tundra-shell-diagnostics-storage-{}-{nonce}",
        std::process::id()
    ));
    let app_paths = platform::AppPaths::from_parts(
        root.join("config.toml"),
        root.join("state"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap();
    state.storage_manager = Some(StorageManager::open(app_paths).unwrap().manager);
    root
}

#[test]
fn diagnostics_shares_details_with_users_but_keeps_repairs_admin_only() {
    let mut user_state = state(UserRole::User);
    let (private_log_directory, private_log_path) = temporary_document("private.log", b"private");
    update_diagnostics_snapshot(&mut user_state, |snapshot| {
        snapshot
            .logs
            .push(log_file(private_log_path, "private.log"));
        snapshot.checks[0].summary = "/private/example/data cannot be opened".to_string();
    });
    let user = user_state.to_diagnostics_view_model();
    assert!(user.can_view_details);
    assert!(!user.can_repair);
    assert!(user.checks[0].summary.contains("/private/example/data"));
    assert!(user.checks[0].detail.contains("/private/example/data"));
    assert_eq!(user.logs[0].relative_path, "private.log");

    let mut admin_state = state(UserRole::Admin);
    let user_logs = user_state.app.diagnostics_snapshot().unwrap().logs.clone();
    update_diagnostics_snapshot(&mut admin_state, |snapshot| {
        snapshot.logs = user_logs;
    });
    let admin = admin_state.to_diagnostics_view_model();
    assert!(admin.can_view_details);
    assert!(admin.can_repair);
    assert!(admin.checks[0].detail.contains("/private/example/data"));
    assert_eq!(admin.logs[0].relative_path, "private.log");
    std::fs::remove_dir_all(private_log_directory).unwrap();
}

#[test]
fn diagnostics_log_opens_read_only_in_editor_and_returns_to_logs() {
    let (directory, path) = temporary_document("application.log", b"first\nsecond\n");
    let mut state = state(UserRole::Admin);
    update_diagnostics_snapshot(&mut state, |snapshot| {
        snapshot
            .logs
            .push(log_file(path.clone(), "application.log"));
    });
    state.open_diagnostics();
    state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
    let editor_tasks = TestEditorTaskDriver::install(&mut state);

    let platform = platform::mock::UnsupportedPlatform;
    state.open_selected_diagnostics_report(&platform);
    editor_tasks.complete_next_load(&mut state);

    assert_eq!(state.active_screen(), ShellScreen::Editor);
    let editor = state.app.editor_state().unwrap();
    assert!(editor.is_read_only());
    assert_eq!(editor.source_buffer().as_deref(), Some("first\nsecond\n"));
    assert!(state.editor_read_session.is_some());

    state.handle_editor_key(
        KeyInput::with_phase(
            InputKey::Char('n'),
            InputModifiers {
                control: true,
                ..InputModifiers::none()
            },
            InputPhase::Press,
        ),
        &platform,
    );
    state.handle_editor_paste("mutating paste".to_string());
    state.activate_editor_toolbar(ui::EditorToolbarAction::Open, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(
        state.app.editor_state().unwrap().source_buffer().as_deref(),
        Some("first\nsecond\n")
    );
    assert!(!state.app.editor_state().unwrap().is_dirty());

    state.request_editor_close(&platform);
    assert_eq!(state.active_screen(), ShellScreen::Logs);
    assert_eq!(state.to_logs_view_model().category, ui::LogsCategory::Ux);
    assert_eq!(state.diagnostics_tab, ui::DiagnosticsTab::Logs);
    assert!(state.app.editor_state().is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diagnostics_log_reload_replaces_only_after_a_successful_read() {
    let (directory, path) = temporary_document("reload.log", b"before\n");
    let mut state = state(UserRole::Admin);
    update_diagnostics_snapshot(&mut state, |snapshot| {
        snapshot.logs.push(log_file(path.clone(), "reload.log"));
    });
    state.open_diagnostics();
    state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
    let editor_tasks = TestEditorTaskDriver::install(&mut state);
    state.open_selected_diagnostics_report(&platform::mock::UnsupportedPlatform);
    editor_tasks.complete_next_load(&mut state);

    std::fs::write(&path, b"before\nafter\n").unwrap();
    state.handle_editor_key(
        KeyInput::with_phase(
            InputKey::Char('r'),
            InputModifiers::none(),
            InputPhase::Press,
        ),
        &platform::mock::UnsupportedPlatform,
    );
    editor_tasks.complete_next_load(&mut state);
    assert_eq!(
        state.app.editor_state().unwrap().source_buffer().as_deref(),
        Some("before\nafter\n")
    );

    std::fs::remove_file(&path).unwrap();
    state.handle_editor_key(
        KeyInput::with_phase(
            InputKey::Char('r'),
            InputModifiers::none(),
            InputPhase::Press,
        ),
        &platform::mock::UnsupportedPlatform,
    );
    editor_tasks.complete_next_load(&mut state);
    assert_eq!(
        state.app.editor_state().unwrap().source_buffer().as_deref(),
        Some("before\nafter\n")
    );
    assert!(
        state
            .editor_message
            .as_ref()
            .unwrap()
            .render_current()
            .contains("Could not reload")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diagnostics_log_reload_preserves_bottom_or_clamps_the_previous_scroll() {
    let before = (0..100)
        .map(|index| format!("before-{index}\n"))
        .collect::<String>();
    let (directory, path) = temporary_document("scroll.log", before.as_bytes());
    let mut state = state(UserRole::Admin);
    update_diagnostics_snapshot(&mut state, |snapshot| {
        snapshot.logs.push(log_file(path.clone(), "scroll.log"));
    });
    state.open_diagnostics();
    state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
    let platform = platform::mock::UnsupportedPlatform;
    let editor_tasks = TestEditorTaskDriver::install(&mut state);
    state.open_selected_diagnostics_report(&platform);
    editor_tasks.complete_next_load(&mut state);

    let editor_layout = |state: &ShellSession| {
        let area = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
        let editor_area = match ui::compute_shell_layout(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        ui::editor_layout(editor_area, &state.to_editor_view_model())
    };
    let layout = editor_layout(&state);
    assert_eq!(
        layout.visible_start,
        layout
            .document_line_count
            .saturating_sub(layout.visible_capacity)
    );

    state.app.dispatch_at(
        app::AppCommand::Editor(app::editor::EditorCommand::SelectAll),
        Instant::now(),
    );
    let _ = state.app.take_editor_effects();
    let appended = format!("{before}after-100\nafter-101\n");
    std::fs::write(&path, appended).unwrap();
    state.handle_editor_key(
        KeyInput::with_phase(
            InputKey::Char('r'),
            InputModifiers::none(),
            InputPhase::Press,
        ),
        &platform,
    );
    editor_tasks.complete_next_load(&mut state);
    let layout = editor_layout(&state);
    assert_eq!(
        layout.visible_start,
        layout
            .document_line_count
            .saturating_sub(layout.visible_capacity)
    );
    assert!(state.app.editor_state().unwrap().selection.is_none());

    let mut viewport = state.app.editor_state().unwrap().viewport;
    viewport.top_line = 2;
    state
        .app
        .dispatch_at(app::AppCommand::SetEditorViewport(viewport), Instant::now());
    std::fs::write(&path, before).unwrap();
    state.handle_editor_key(
        KeyInput::with_phase(
            InputKey::Char('r'),
            InputModifiers::none(),
            InputPhase::Press,
        ),
        &platform,
    );
    editor_tasks.complete_next_load(&mut state);
    assert_eq!(state.app.editor_state().unwrap().viewport.top_line, 2);
    state.request_editor_close(&platform);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diagnostics_log_editor_loads_a_single_long_line_in_full() {
    let contents = vec![b'x'; 5 * 1024 * 1024 + 137];
    let (directory, path) = temporary_document("large.log", &contents);
    let mut state = state(UserRole::Admin);
    update_diagnostics_snapshot(&mut state, |snapshot| {
        snapshot.logs.push(log_file(path, "large.log"));
    });
    state.open_diagnostics();
    state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
    let editor_tasks = TestEditorTaskDriver::install(&mut state);

    state.open_selected_diagnostics_report(&platform::mock::UnsupportedPlatform);
    editor_tasks.complete_next_load(&mut state);

    let source = state.app.editor_state().unwrap().source_buffer().unwrap();
    assert_eq!(source.len(), contents.len());
    assert_eq!(source.as_bytes(), contents);
    let session = state.editor_read_session.as_ref().unwrap();
    assert_eq!(session.total_bytes, contents.len() as u64);
    state.request_editor_close(&platform::mock::UnsupportedPlatform);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diagnostics_viewer_never_replaces_an_existing_unsaved_editor_document() {
    let (directory, path) = temporary_document("blocked.log", b"diagnostic\n");
    let mut state = state(UserRole::Admin);
    update_diagnostics_snapshot(&mut state, |snapshot| {
        snapshot.logs.push(log_file(path, "blocked.log"));
    });
    let mut editor = EditorState::new();
    editor.apply(app::editor::EditorCommand::InsertText(
        "unsaved".to_string(),
    ));
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(editor)),
        Instant::now(),
    );
    state.open_diagnostics();
    state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);

    state.open_selected_diagnostics_report(&platform::mock::UnsupportedPlatform);

    assert_eq!(state.active_screen(), ShellScreen::Logs);
    let editor = state.app.editor_state().unwrap();
    assert!(editor.is_dirty());
    assert_eq!(editor.export_text(), "unsaved");
    assert!(state.editor_read_session.is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn incident_prefers_text_report_and_falls_back_to_json_in_read_only_editor() {
    let (directory, text_path) = temporary_document("incident.txt", b"text report\n");
    let json_path = directory.join("incident.json");
    std::fs::write(&json_path, b"{\"source\":\"json\"}\n").unwrap();
    let mut state = state(UserRole::Admin);
    update_diagnostics_snapshot(&mut state, |snapshot| {
        snapshot.incidents.push(watchdog::IncidentReportSummary {
            incident_id: "incident-1".to_string(),
            occurred_at: Utc::now(),
            kind: watchdog::IncidentKind::Error,
            severity: watchdog::IncidentSeverity::Error,
            app: None,
            component: Some("test".to_string()),
            boundary: "test".to_string(),
            summary: "test incident".to_string(),
            recovery: watchdog::RecoveryOutcome::Pending,
            json_report_path: json_path.clone(),
            text_report_path: Some(text_path.clone()),
        })
    });
    state.open_diagnostics();
    state.set_diagnostics_tab(ui::DiagnosticsTab::Incidents);
    let platform = platform::mock::UnsupportedPlatform;
    let editor_tasks = TestEditorTaskDriver::install(&mut state);

    state.open_selected_diagnostics_report(&platform);
    editor_tasks.complete_next_load(&mut state);
    assert_eq!(
        state.app.editor_state().unwrap().source_buffer().as_deref(),
        Some("text report\n")
    );
    assert!(state.app.editor_state().unwrap().is_read_only());
    assert_eq!(state.app.editor_state().unwrap().viewport.top_line, 0);
    std::fs::write(&text_path, b"updated text report\n").unwrap();
    state.handle_editor_key(
        KeyInput::with_phase(
            InputKey::Char('r'),
            InputModifiers::none(),
            InputPhase::Press,
        ),
        &platform,
    );
    assert_eq!(
        state.app.editor_state().unwrap().source_buffer().as_deref(),
        Some("text report\n")
    );
    state.request_editor_close(&platform);

    update_diagnostics_snapshot(&mut state, |snapshot| {
        snapshot.incidents[0].text_report_path = None;
    });
    state.open_selected_diagnostics_report(&platform);
    editor_tasks.complete_next_load(&mut state);
    assert!(
        state.app.editor_state().is_some(),
        "JSON incident should load: {:?}",
        state.editor_message
    );
    assert_eq!(
        state.app.editor_state().unwrap().source_buffer().as_deref(),
        Some("{\"source\":\"json\"}\n")
    );
    state.request_editor_close(&platform);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diagnostics_rescan_preserves_log_selection_by_relative_path() {
    let (directory, first_path) = temporary_document("first.log", b"first\n");
    let second_path = directory.join("second.log");
    std::fs::write(&second_path, b"second\n").unwrap();
    let first = log_file(first_path, "first.log");
    let second = log_file(second_path, "nested/second.log");
    let mut state = state(UserRole::Admin);
    update_diagnostics_snapshot(&mut state, |snapshot| {
        snapshot.logs = vec![first.clone(), second.clone()];
    });
    state.diagnostics_selected_log = 1;

    let mut replacement = snapshot();
    replacement.logs = vec![second, first];
    state.install_diagnostics_snapshot(replacement);

    assert_eq!(state.diagnostics_selected_log, 0);
    assert_eq!(
        state.app.diagnostics_snapshot().unwrap().logs[0].relative_path,
        std::path::PathBuf::from("nested/second.log")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diagnostics_navigation_only_exposes_health() {
    let mut state = state(UserRole::User);
    state.open_diagnostics();
    for key in ["Tab", "Right", "Left"] {
        assert_eq!(
            state.route_diagnostics_key(&KeyInput::from_label(key)).1,
            ShellCommand::Noop
        );
    }
    assert_eq!(
        state.route_diagnostics_key(&KeyInput::from_label("e")).1,
        ShellCommand::RecordInput
    );
    state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
    assert_eq!(state.active_screen(), ShellScreen::Logs);
    assert_eq!(state.to_logs_view_model().category, ui::LogsCategory::Ux);
}

#[test]
fn diagnostics_opens_log_directory_in_explorer_and_returns_on_close() {
    let mut state = state(UserRole::Admin);
    let root = install_temporary_storage(&mut state);
    let logs_path = state
        .storage_manager
        .as_ref()
        .unwrap()
        .layout()
        .logs_path
        .clone();
    std::fs::write(logs_path.join("application.log"), b"application\n").unwrap();
    state.open_diagnostics();

    state.open_diagnostics_logs_in_explorer(&platform::mock::UnsupportedPlatform);

    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(state.explorer_purpose, ExplorerPurpose::DiagnosticsLogs);
    assert_eq!(state.app.explorer_state().unwrap().current_path, logs_path);
    assert!(
        state
            .app
            .explorer_state()
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.name == "application.log")
    );

    state.close_explorer();

    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(state.focused_component, ShellComponent::SystemStatus);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn diagnostics_log_directory_explorer_requires_detail_permission() {
    let mut state = state(UserRole::User);
    state.open_diagnostics();

    state.open_diagnostics_logs_in_explorer(&platform::mock::UnsupportedPlatform);

    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert!(
        state
            .app
            .notification_center()
            .alert()
            .is_some_and(|message| message.render_current().contains("Only administrators"))
    );
}

#[test]
fn diagnostics_navigation_and_repair_preview_are_modal() {
    let mut state = state(UserRole::Admin);
    state.open_diagnostics();
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(state.system_status_tab, ui::SystemStatusTab::Health);

    state.preview_selected_diagnostics_repair();
    assert_eq!(state.diagnostics_repair_preview.len(), 1);
    assert_eq!(
        state.focus_order(),
        vec![ShellComponent::DiagnosticsRepairDialog]
    );

    state.cancel_diagnostics_repair_preview();
    state.set_diagnostics_tab(ui::DiagnosticsTab::Incidents);
    assert_eq!(state.diagnostics_tab, ui::DiagnosticsTab::Incidents);
    assert_eq!(state.active_screen(), ShellScreen::Logs);
    assert_eq!(
        state.to_logs_view_model().section,
        ui::LogsSection::Incidents
    );
    state.handle_logs_key(&KeyInput::from_label("Esc"));
    state.close_diagnostics();
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(state.system_status_tab, ui::SystemStatusTab::Overview);
    assert_eq!(state.focused_component, ShellComponent::SystemStatus);
}

#[test]
fn diagnostics_close_without_system_status_parent_falls_back_to_home() {
    let mut state = state(UserRole::Admin);
    state.screen_stack = vec![ShellScreen::Settings, ShellScreen::Diagnostics];
    state.focused_component = ShellComponent::Diagnostics;

    state.close_diagnostics();

    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.focused_component, ShellComponent::Home);
}

#[test]
fn diagnostics_repair_preview_and_restart_required_view_offer_restart() {
    let mut state = state(UserRole::Admin);
    state.open_diagnostics();
    state.preview_selected_diagnostics_repair();

    let (_, preview_command) = state.route_key_input(&KeyInput::from_label("r"));
    assert_eq!(preview_command, ShellCommand::Restart);

    let area = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
        panic!("Diagnostics restart test requires a full layout");
    };
    let model = state.to_diagnostics_view_model();
    let layout = ui::diagnostics_layout(main, &model);
    let restart = layout.repair_dialog.expect("repair preview").restart;
    let (_, mouse_command) = state.route_diagnostics_mouse(
        MouseInput::down(restart.x, restart.y, PointerButton::Left),
        Some(ShellComponent::DiagnosticsRepairDialog),
    );
    assert_eq!(mouse_command, ShellCommand::Restart);

    state.cancel_diagnostics_repair_preview();
    state.diagnostics_restart_required = true;
    let (_, required_command) = state.route_key_input(&KeyInput::from_label("Enter"));
    assert_eq!(required_command, ShellCommand::Restart);
}

#[test]
fn diagnostics_scrollbar_thumb_drags_to_the_end_without_moving_selection() {
    let mut state = state(UserRole::Admin);
    let template = state.app.diagnostics_snapshot().unwrap().checks[0].clone();
    update_diagnostics_snapshot(&mut state, |snapshot| {
        snapshot.checks = (0..40)
            .map(|index| {
                let mut check = template.clone();
                check.id = format!("check-{index}");
                check.label = format!("Check {index}");
                check
            })
            .collect();
    });
    state.open_diagnostics();

    let area = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
        panic!("Diagnostics scrollbar test requires a full layout");
    };
    let layout = ui::system_status_layout(
        main,
        &state
            .to_system_status_view_model()
            .expect("system status model"),
    );
    let scrollbar = layout
        .diagnostics_content
        .expect("integrated diagnostics content")
        .list_scrollbar
        .expect("overflowing Diagnostics scrollbar");
    let grab = (
        scrollbar.thumb.x,
        scrollbar.thumb.y.saturating_add(scrollbar.thumb.height / 2),
    );
    let bottom = (
        scrollbar.track.x,
        scrollbar.track.bottom().saturating_sub(1),
    );
    let platform = platform::mock::UnsupportedPlatform;

    state.apply_input_with_platform(InputEvent::mouse_down(PointerButton::Left, grab), &platform);
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, bottom),
        &platform,
    );
    state.apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, bottom), &platform);

    let model = state.to_diagnostics_view_model();
    let final_layout = ui::system_status_layout(
        main,
        &state
            .to_system_status_view_model()
            .expect("system status model"),
    )
    .diagnostics_content
    .expect("integrated diagnostics content");
    assert_eq!(model.selected_check, 0);
    assert!(model.list_window_is_explicit);
    assert_eq!(
        final_layout.visible_start,
        model
            .item_count()
            .saturating_sub(final_layout.visible_capacity)
    );

    let scrollbar = final_layout
        .list_scrollbar
        .expect("Diagnostics scrollbar after dragging down");
    let grab = (
        scrollbar.thumb.x,
        scrollbar.thumb.y.saturating_add(scrollbar.thumb.height / 2),
    );
    let top = (scrollbar.track.x, scrollbar.track.y);
    state.apply_input_with_platform(InputEvent::mouse_down(PointerButton::Left, grab), &platform);
    state.apply_input_with_platform(InputEvent::mouse_drag(PointerButton::Left, top), &platform);
    state.apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, top), &platform);

    let model = state.to_diagnostics_view_model();
    assert_eq!(model.selected_check, 0);
    assert!(model.list_window_is_explicit);
    assert_eq!(ui::diagnostics_layout(main, &model).visible_start, 0);
}

#[test]
fn restart_requirement_disables_follow_up_repairs() {
    let mut state = state(UserRole::Admin);
    state.diagnostics_restart_required = true;

    let model = state.to_diagnostics_view_model();
    assert!(model.restart_required);
    assert!(!model.can_repair);

    assert!(!state.logout_to_lockscreen_at(Instant::now()));
    assert!(state.diagnostics_restart_required);
    assert!(state.auth_session().is_some());
    assert!(!state.return_to_lockscreen_requested);
}
