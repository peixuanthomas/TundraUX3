use super::*;

#[cfg(unix)]
#[test]
fn update_build_log_retains_both_streams_long_lines_and_failed_vim_output() {
    let root = update_test_root("build-log-streams");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("build.log");
    let log = File::create(&path).unwrap();
    let platform = platform::native_platform();
    let cancel = UpdateCancellation::default();
    let operations = PlatformPreparationOperations {
        platform: platform.as_ref(),
        toolchain: toolchain::Toolchain::discover().unwrap(),
        cancel: &cancel,
        build_log: &log,
    };
    let long_line = format!(
        "{}{} end-of-long-line",
        "x".repeat(5000),
        "编译".repeat(200)
    );
    let spec = ProcessSpec::new("/bin/sh")
        .args([
            "-c",
            "printf '%s\\n' \"$1\"; i=0; while [ $i -lt 1200 ]; do printf 'stdout-%s: compiled dependency with full diagnostic context\\n' \"$i\"; printf 'stderr-%s\\n' \"$i\" >&2; i=$((i+1)); done; printf 'Building [==>] 2/3: shell, cli\\r'; sleep 0.1",
            "build-log-test",
        ])
        .arg(&long_line);
    let mut saw_live_log = false;
    operations
        .run_live(&spec, "cargo build", &mut |progress| {
            if progress.message.contains("end-of-long-line") {
                assert!(fs::read_to_string(&path).unwrap().contains(&long_line));
                saw_live_log = true;
            }
        })
        .unwrap();
    assert!(saw_live_log);
    let error = operations
        .run_live(
            &ProcessSpec::new("/bin/sh").args(["-c", "printf 'Vim compiler error\\n' >&2; exit 7"]),
            "Vim build",
            &mut |_| {},
        )
        .unwrap_err();
    assert!(error.to_string().contains("Vim compiler error"));
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains(&long_line));
    for index in 0..1200 {
        assert!(contents.contains(&format!("stdout-{index}: compiled dependency")));
        assert!(contents.contains(&format!("stderr-{index}\n")));
    }
    assert!(contents.contains("Building [==>] 2/3: shell, cli"));
    assert!(contents.contains("=== cargo build exited: Some(0) ==="));
    assert!(contents.contains("=== Vim build failed:"));
    assert!(contents.contains("Vim compiler error"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn update_build_log_keeps_output_after_cancellation() {
    let root = update_test_root("build-log-cancel");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("build.log");
    let log = File::create(&path).unwrap();
    let platform = platform::native_platform();
    let cancel = UpdateCancellation::default();
    let operations = PlatformPreparationOperations {
        platform: platform.as_ref(),
        toolchain: toolchain::Toolchain::discover().unwrap(),
        cancel: &cancel,
        build_log: &log,
    };
    let error = operations
        .run_live(
            &ProcessSpec::new("/bin/sh").args(["-c", "printf 'before cancel\\n'; sleep 5"]),
            "cargo build",
            &mut |_| cancel.cancel(),
        )
        .unwrap_err();
    assert!(error.is_cancelled());
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.contains("before cancel"));
    assert!(contents.contains("Update cancelled"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_build_log_survives_failed_preparation_cleanup_and_reports_its_path() {
    let root = update_test_root("build-log-cleanup");
    let paths = platform::AppPaths::from_binary_dir(&root).unwrap();
    let user_dirs =
        platform::UserDirs::new(&root, &root, &root, &root, &root, &root, &root).unwrap();
    let platform = platform::mock::MockPlatform::new(user_dirs, paths.clone());
    let work_dir =
        platform::create_temp_dir(&paths.cache_path().join("updates"), "update").unwrap();
    let source = work_dir.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    let check = UpdateCheckResult {
        release: None,
        default_branch: "master".into(),
        head_sha: "a".repeat(40),
        relation: UpdateRelation::Unknown,
        commits: Vec::new(),
    };
    let mut events = Vec::new();
    let result = prepare_extracted(
        &platform,
        &check,
        &mut |event| events.push(event),
        &work_dir,
        &source,
        &UpdateCancellation::default(),
    );
    let error = WorkDirectory::new(work_dir.clone())
        .finish(result)
        .unwrap_err();
    assert!(!work_dir.exists());
    let logs = fs::read_dir(paths.logs_path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(logs.len(), 1);
    let path = &logs[0];
    assert_eq!(path.extension().unwrap(), "log");
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.contains(&check.head_sha));
    assert!(contents.contains("Update build failed:"));
    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(
        events
            .iter()
            .any(|event| event.message.contains(&path.display().to_string()))
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    fs::remove_dir_all(root).unwrap();
}
