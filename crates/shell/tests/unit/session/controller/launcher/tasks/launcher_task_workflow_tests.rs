use super::*;

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
                    id,
                    result,
                } => {
                    assert_eq!(event_request_id, request_id);
                    assert_eq!(id, "program");
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
