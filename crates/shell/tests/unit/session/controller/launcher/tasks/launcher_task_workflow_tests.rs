use super::*;

#[test]
fn a_refresh_for_the_old_target_cannot_replace_an_edited_command_status() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::Launcher]);
    let original = storage::LauncherEntryRecord {
        id: "edited".into(),
        path: "/missing/original".into(),
        executable_kind: Some(storage::LauncherExecutableKind::NativeBinary),
        ..Default::default()
    };
    let mut edited = original.clone();
    edited.executable_kind = None;
    edited.custom = Some(storage::LauncherCustomCommand {
        name: "edited".into(),
        command: "printf edited".into(),
        ..Default::default()
    });
    state.app.dispatch_at(
        app::AppCommand::SetLauncherState(Some(LauncherState {
            items: vec![app::launcher::LauncherItem {
                record: edited.clone(),
                status: LauncherItemStatus::Ready,
            }],
            ..Default::default()
        })),
        Instant::now(),
    );
    state.pages.launcher.launcher_refresh_request = Some(1);
    for result in [
        Ok(LauncherItemStatus::Missing),
        Err("old file error".into()),
    ] {
        state.apply_launcher_refresh_event(LauncherRefreshEvent::ItemChecked {
            request_id: 1,
            entry: original.clone(),
            result,
        });
        assert_eq!(
            state.app.launcher_state().unwrap().items[0].status,
            LauncherItemStatus::Ready
        );
        assert!(state.app.launcher_state().unwrap().error.is_none());
    }
    state.apply_launcher_refresh_event(LauncherRefreshEvent::ItemChecked {
        request_id: 1,
        entry: edited,
        result: Ok(LauncherItemStatus::Ready),
    });
    assert_eq!(
        state.app.launcher_state().unwrap().items[0].status,
        LauncherItemStatus::Ready
    );
}

#[test]
fn managed_refresh_publishes_presence_results() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("test clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tundra-launcher-refresh-{}-{unique}",
        std::process::id()
    ));
    let documents = root.join("Documents");
    std::fs::create_dir_all(&documents).expect("test documents");
    let executable = documents.join("program.exe");
    std::fs::write(&executable, b"approved content").expect("test executable");
    let executable = std::fs::canonicalize(executable).expect("canonical executable");
    let metadata = std::fs::metadata(&executable).expect("executable metadata");

    let app_paths = platform::build_windows_app_paths(
        root.join("Roaming"),
        root.join("Local"),
        root.join("Temp"),
    )
    .expect("test app paths");
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        documents,
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("Roaming"),
    )
    .expect("test user dirs");
    let platform = platform::mock::MockPlatform::new(user_dirs, app_paths)
        .with_kind(platform::PlatformKind::Windows);
    platform.set_file_attributes(
        executable.clone(),
        platform::FileAttributes {
            path: executable.clone(),
            is_file: true,
            is_dir: false,
            len: metadata.len(),
            readonly: false,
            modified: metadata.modified().ok(),
            hidden: false,
            system: false,
            archive: false,
            symlink: false,
            junction: false,
            reparse_point: false,
            shortcut: false,
        },
    );
    platform.set_file_open_policy(
        executable.clone(),
        platform::FileOpenPolicy::launcher_required(
            platform::ExecutableKind::NativeBinary,
            "test policy",
        ),
    );
    let entry = storage::LauncherEntryRecord {
        id: "program".to_string(),
        path: executable.to_string_lossy().into_owned(),
        executable_kind: Some(storage::LauncherExecutableKind::NativeBinary),
        fingerprint: None,
        added_by_user_id: "admin".to_string(),
        added_at_epoch_ms: 0,
        terminal: None,
        custom: None,
        name: None,
        icon: None,
    };
    let watchdog = default_editor_watchdog().expect("test watchdog");
    let runtime = ShellLauncherTaskRuntime::new_managed(std::sync::Arc::new(platform), watchdog);

    let request_id = runtime.submit(vec![entry]).expect("submit refresh");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut status = None;
    let mut finished = false;
    while std::time::Instant::now() < deadline && !finished {
        for event in runtime.drain_events() {
            match event {
                LauncherRefreshEvent::ItemChecked {
                    request_id: event_request_id,
                    entry,
                    result,
                } => {
                    assert_eq!(event_request_id, request_id);
                    assert_eq!(entry.id, "program");
                    status = Some(result.expect("presence result"));
                }
                LauncherRefreshEvent::Finished {
                    request_id: event_request_id,
                    error,
                } => {
                    assert_eq!(event_request_id, request_id);
                    assert_eq!(error, None);
                    finished = true;
                }
            }
        }
        if !finished {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    assert!(finished, "Launcher refresh did not finish");
    assert_eq!(status, Some(LauncherItemStatus::Ready));
    drop(runtime);
    platform::cleanup_temp_path(&root).expect("cleanup test fixture");
}

#[test]
fn covered_launcher_receives_its_result_but_closed_and_old_session_results_are_discarded() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (100, 30),
        ShellHomeMode::User,
    );
    state.enter_new_page(ShellScreen::Launcher);
    let first = state.active_page_id();
    state.app.dispatch_at(
        app::AppCommand::SetLauncherState(Some(LauncherState::default())),
        Instant::now(),
    );
    state.pages.launcher.launcher_refresh_request = Some(900);
    state.register_page_task(
        TaskDomain::Launcher,
        900,
        TaskClass::Query,
        TaskControl::new(|| {}, || true),
    );
    state.enter_new_page(ShellScreen::Launcher);
    let second = state.active_page_id();
    state.app.dispatch_at(
        app::AppCommand::SetLauncherState(Some(LauncherState::default())),
        Instant::now(),
    );
    state.deliver_launcher_refresh_event(LauncherRefreshEvent::Finished {
        request_id: 900,
        error: Some("first only".into()),
    });
    assert_eq!(state.active_page_id(), second);
    assert!(state.app.launcher_state().unwrap().error.is_none());
    assert!(
        state
            .with_page_context(first, |s| s.app.launcher_state().unwrap().error.is_some())
            .unwrap()
    );
    state.pages.launcher.launcher_refresh_request = Some(901);
    state.register_page_task(
        TaskDomain::Launcher,
        901,
        TaskClass::Query,
        TaskControl::new(|| {}, || true),
    );
    state.close_page_instance(second);
    state.enter_new_page(ShellScreen::Launcher);
    let third = state.active_page_id();
    state.app.dispatch_at(
        app::AppCommand::SetLauncherState(Some(LauncherState::default())),
        Instant::now(),
    );
    state.deliver_launcher_refresh_event(LauncherRefreshEvent::Finished {
        request_id: 901,
        error: Some("late closed page".into()),
    });
    assert_eq!(state.active_page_id(), third);
    assert!(state.app.launcher_state().unwrap().error.is_none());
    state.pages.launcher.launcher_refresh_request = Some(902);
    state.register_page_task(
        TaskDomain::Launcher,
        902,
        TaskClass::Query,
        TaskControl::new(|| {}, || true),
    );
    state.tasks.reset();
    state.deliver_launcher_refresh_event(LauncherRefreshEvent::Finished {
        request_id: 902,
        error: Some("old login".into()),
    });
    assert!(state.app.launcher_state().unwrap().error.is_none());
}
