use super::*;
use platform::ProcessStream;
use std::cell::Cell;
use std::io::Write;

#[test]
fn update_compare_relations_are_mapped_from_local_to_remote() {
    assert_eq!(
        relation_from_compare("identical", 0, 0),
        UpdateRelation::Identical
    );
    assert_eq!(
        relation_from_compare("ahead", 3, 0),
        UpdateRelation::Behind { remote_ahead: 3 }
    );
    assert_eq!(
        relation_from_compare("behind", 0, 2),
        UpdateRelation::Ahead { local_ahead: 2 }
    );
    assert_eq!(
        relation_from_compare("diverged", 4, 2),
        UpdateRelation::Diverged {
            remote_ahead: 4,
            local_ahead: 2
        }
    );
    assert_eq!(
        relation_from_compare("mystery", 0, 0),
        UpdateRelation::Unknown
    );
}

#[test]
fn update_rust_version_is_read_and_compared() {
    let root = std::env::temp_dir().join(format!("tundra-update-manifest-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("Cargo.toml");
    fs::write(&path, "[workspace.package]\nrust-version = \"1.82\"\n").unwrap();
    assert_eq!(
        required_rust_version(&path).unwrap(),
        Some(Version::new(1, 82, 0))
    );
    assert_eq!(
        parse_rustc_version("rustc 1.85.1\nrelease: 1.85.1\n").unwrap(),
        Version::new(1, 85, 1)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_zip_rejects_parent_traversal() {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        writer
            .start_file("../escape", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"bad").unwrap();
        writer.finish().unwrap();
    }
    let root = std::env::temp_dir().join(format!("tundra-update-zip-{}", std::process::id()));
    let error = extract_archive(cursor.get_ref(), &root).unwrap_err();
    assert!(error.to_string().contains("unsafe path"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn update_product_validation_requires_all_outputs() {
    let root = std::env::temp_dir().join(format!("tundra-update-products-{}", std::process::id()));
    fs::create_dir_all(root.join("source/assets/themes/default")).unwrap();
    fs::create_dir_all(root.join("target/release")).unwrap();
    fs::write(root.join("target/release").join(SHELL_FILE), b"shell").unwrap();
    assert!(
        validate_product_paths(&root, &root.join("source"), &root.join("target"), "abc").is_err()
    );
    fs::write(root.join("target/release").join(CLI_FILE), b"cli").unwrap();
    assert!(
        validate_product_paths(&root, &root.join("source"), &root.join("target"), "abc").is_ok()
    );
    fs::remove_dir_all(root).unwrap();
}

#[derive(Clone, Copy)]
enum PreparationFailure {
    MissingRustc,
    MissingCargo,
    RustcTooOld,
    Locked,
    Compile,
    MissingProduct,
    CliProbe,
    ShellProbe,
}

struct FakePreparationOperations {
    failure: PreparationFailure,
}

impl PreparationOperations for FakePreparationOperations {
    fn run(&self, spec: &ProcessSpec, name: &str) -> Result<ProcessExit, UpdateError> {
        if name == "rustc" && matches!(self.failure, PreparationFailure::MissingRustc) {
            return Err(UpdateError::new("could not run rustc: missing"));
        }
        if name == "cargo" && matches!(self.failure, PreparationFailure::MissingCargo) {
            return Err(UpdateError::new("could not run cargo: missing"));
        }
        let failed_build = name == "cargo build"
            && matches!(
                self.failure,
                PreparationFailure::Locked | PreparationFailure::Compile
            );
        let stdout = if name == "rustc" {
            if matches!(self.failure, PreparationFailure::RustcTooOld) {
                b"rustc 1.70.0\nrelease: 1.70.0\n".to_vec()
            } else {
                b"rustc 1.90.0\nrelease: 1.90.0\n".to_vec()
            }
        } else {
            Vec::new()
        };
        assert!(name != "cargo build" || spec.args_slice().iter().any(|arg| arg == "--locked"));
        if failed_build {
            return Err(UpdateError::new(
                if matches!(self.failure, PreparationFailure::Locked) {
                    "cargo build failed: lock file needs to be updated"
                } else {
                    "cargo build failed: compiler error"
                },
            ));
        }
        Ok(ProcessExit {
            code: Some(0),
            stdout: ProcessStream::from_bytes(stdout),
            stderr: ProcessStream::from_bytes(Vec::new()),
        })
    }

    fn probe(&self, executable: &Path, _expected_sha: &str) -> Result<(), UpdateError> {
        let cli = executable.file_name().is_some_and(|name| name == CLI_FILE);
        if (cli && matches!(self.failure, PreparationFailure::CliProbe))
            || (!cli && matches!(self.failure, PreparationFailure::ShellProbe))
        {
            Err(UpdateError::new(
                "compiled program reported the wrong update protocol or commit",
            ))
        } else {
            Ok(())
        }
    }
}

#[test]
fn update_preparation_failures_never_touch_installation() {
    for (index, failure) in [
        PreparationFailure::MissingRustc,
        PreparationFailure::MissingCargo,
        PreparationFailure::RustcTooOld,
        PreparationFailure::Locked,
        PreparationFailure::Compile,
        PreparationFailure::MissingProduct,
        PreparationFailure::CliProbe,
        PreparationFailure::ShellProbe,
    ]
    .into_iter()
    .enumerate()
    {
        let root = update_test_root(&format!("prepare-failure-{index}"));
        let source = root.join("source");
        let target = root.join("target/release");
        let install = root.join("unrelated-install");
        fs::create_dir_all(source.join("assets/themes/default")).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&install).unwrap();
        fs::write(
            source.join("Cargo.toml"),
            "[workspace.package]\nrust-version = \"1.80\"\n",
        )
        .unwrap();
        fs::write(install.join("sentinel"), b"unchanged").unwrap();
        if !matches!(failure, PreparationFailure::MissingProduct) {
            fs::write(target.join(SHELL_FILE), b"shell").unwrap();
            fs::write(target.join(CLI_FILE), b"cli").unwrap();
        }
        let check = UpdateCheckResult {
            default_branch: "master".to_owned(),
            head_sha: "target-sha".to_owned(),
            relation: UpdateRelation::Behind { remote_ahead: 1 },
            commits: Vec::new(),
        };
        let error = prepare_extracted_with_operations(
            &check,
            &mut |_| {},
            &root,
            &source,
            &FakePreparationOperations { failure },
        )
        .unwrap_err();
        assert!(!error.to_string().is_empty());
        assert_eq!(fs::read(install.join("sentinel")).unwrap(), b"unchanged");
        fs::remove_dir_all(root).unwrap();
    }
}

pub(super) fn update_test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tundra-update-{name}-{}-{}",
        std::process::id(),
        unix_millis()
    ))
}

#[test]
fn update_manifest_must_stay_below_the_install_update_directory() {
    let root = update_test_root("manifest-location");
    let install = root.join("install");
    let transaction_dir = install.join(".tundra-update/tx");
    fs::create_dir_all(&transaction_dir).unwrap();
    let path = transaction_dir.join("transaction.json");
    let manifest = TransactionManifest {
        protocol: UPDATE_PROTOCOL_VERSION,
        target_sha: "abc".to_string(),
        install_dir: install.clone(),
        transaction_dir: transaction_dir.clone(),
        state: TransactionState::Prepared,
        assets_replaced: false,
        cli_replaced: false,
        shell_replaced: false,
    };
    write_manifest(&path, &manifest).unwrap();
    assert_eq!(load_manifest(&path).unwrap(), manifest);

    let outside = install.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let outside_path = outside.join("transaction.json");
    let mut invalid = manifest;
    invalid.transaction_dir = outside;
    write_manifest(&outside_path, &invalid).unwrap();
    assert!(
        load_manifest(&outside_path)
            .unwrap_err()
            .to_string()
            .contains("outside the installation update directory")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_compare_fetches_every_commit_page() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for page in 1..=2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let length = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..length]);
            assert!(request.contains(&format!("page={page}")));
            let count = if page == 1 { 100 } else { 2 };
            let commits = (0..count)
                .map(|index| {
                    serde_json::json!({
                        "sha": format!("{page:02}{index:038}"),
                        "commit": { "message": format!("commit {page}-{index}") }
                    })
                })
                .collect::<Vec<_>>();
            let body = serde_json::json!({
                "status": "ahead",
                "ahead_by": 102,
                "behind_by": 0,
                "commits": commits
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let client = Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let (relation, commits) =
        fetch_comparison_from(&client, &format!("http://{address}"), "local", "remote").unwrap();
    server.join().unwrap();
    assert_eq!(relation, UpdateRelation::Behind { remote_ahead: 102 });
    assert_eq!(commits.len(), 102);
    assert_eq!(commits.last().unwrap().message, "commit 2-1");
}

#[test]
fn update_network_failure_is_reported_clearly() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let client = Client::builder()
        .timeout(Duration::from_millis(250))
        .build()
        .unwrap();
    let error =
        get_json::<Repository>(&client, &format!("http://{address}/repository")).unwrap_err();
    assert!(error.to_string().contains("GitHub request failed"));
}

#[test]
fn update_api_rate_limit_at_any_step_uses_git_fallback() {
    use std::io::Read;
    for failed_step in 0..3 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for step in 0..=failed_step {
                let (mut stream, _) = listener.accept().unwrap();
                stream.read(&mut [0; 2048]).unwrap();
                let (status, headers, body) = if step == failed_step {
                    (
                        "403 Forbidden",
                        "x-ratelimit-remaining: 0\r\n",
                        "{}".to_owned(),
                    )
                } else if step == 0 {
                    ("200 OK", "", r#"{"default_branch":"master"}"#.to_owned())
                } else {
                    (
                        "200 OK",
                        "",
                        format!(r#"{{"commit":{{"sha":"{}"}}}}"#, "a".repeat(40)),
                    )
                };
                write!(stream, "HTTP/1.1 {status}\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let identity = BuildIdentity {
            package_version: "0.1.1".into(),
            commit_sha: Some("b".repeat(40)),
            dirty: false,
        };
        let expected = UpdateCheckResult {
            default_branch: "master".into(),
            head_sha: "a".repeat(40),
            relation: UpdateRelation::Behind { remote_ahead: 1 },
            commits: Vec::new(),
        };
        let called = Cell::new(false);
        let actual = check_with_fallback(&identity, &format!("http://{address}"), || {
            called.set(true);
            Ok(expected.clone())
        })
        .unwrap();
        server.join().unwrap();
        assert!(called.get());
        assert_eq!(actual, expected);
    }
}

#[test]
fn update_failed_fallback_preserves_both_errors() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let error = check_with_fallback(
        &current_build_identity(),
        &format!("http://{address}"),
        || Err(UpdateError::new("git missing")),
    )
    .unwrap_err();
    assert!(error.to_string().contains("GitHub request failed"));
    assert!(
        error
            .to_string()
            .contains("Git fallback failed: git missing")
    );
}

#[test]
fn update_source_download_uses_codeload_pinned_to_a_full_sha() {
    let sha = "a".repeat(40);
    assert_eq!(
        source_archive_url(&sha).unwrap(),
        format!("https://codeload.github.com/peixuanthomas/TundraUX3/zip/{sha}")
    );
    for invalid in ["master", "../master", "short", ""] {
        assert!(source_archive_url(invalid).is_err());
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn update_rollback_restores_programs_and_default_assets_but_keeps_custom_themes() {
    let root = update_test_root("rollback");
    let install = root.join("install");
    let transaction_dir = install.join(".tundra-update/tx");
    let new = transaction_dir.join("new");
    fs::create_dir_all(new.join("default-assets")).unwrap();
    fs::create_dir_all(transaction_dir.join("backup")).unwrap();
    fs::create_dir_all(install.join("assets/themes/default")).unwrap();
    fs::create_dir_all(install.join("assets/themes/custom")).unwrap();
    fs::write(install.join(SHELL_FILE), b"old shell").unwrap();
    fs::write(install.join(CLI_FILE), b"old cli").unwrap();
    fs::write(
        install.join("assets/themes/default/theme.txt"),
        b"old theme",
    )
    .unwrap();
    fs::write(
        install.join("assets/themes/custom/theme.txt"),
        b"custom theme",
    )
    .unwrap();
    fs::write(new.join(SHELL_FILE), b"new shell").unwrap();
    fs::write(new.join(CLI_FILE), b"new cli").unwrap();
    fs::write(new.join("default-assets/theme.txt"), b"new theme").unwrap();
    let path = transaction_dir.join("transaction.json");
    let mut manifest = TransactionManifest {
        protocol: UPDATE_PROTOCOL_VERSION,
        target_sha: "abc".to_string(),
        install_dir: install.clone(),
        transaction_dir,
        state: TransactionState::Prepared,
        assets_replaced: false,
        cli_replaced: false,
        shell_replaced: false,
    };
    write_manifest(&path, &manifest).unwrap();
    apply_prepared_files(&path, &mut manifest).unwrap();
    assert_eq!(fs::read(install.join(SHELL_FILE)).unwrap(), b"new shell");
    assert_eq!(fs::read(install.join(CLI_FILE)).unwrap(), b"new cli");
    assert_eq!(
        fs::read(install.join("assets/themes/default/theme.txt")).unwrap(),
        b"new theme"
    );

    rollback_files(&path, &mut manifest).unwrap();
    assert_eq!(fs::read(install.join(SHELL_FILE)).unwrap(), b"old shell");
    assert_eq!(fs::read(install.join(CLI_FILE)).unwrap(), b"old cli");
    assert_eq!(
        fs::read(install.join("assets/themes/default/theme.txt")).unwrap(),
        b"old theme"
    );
    assert_eq!(
        fs::read(install.join("assets/themes/custom/theme.txt")).unwrap(),
        b"custom theme"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(any(windows, target_os = "linux"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum TransactionFailure {
    Assets,
    Cli,
    Shell,
    NewLaunch,
    ReadyTimeout,
    RestoredLaunch,
    None,
}

#[cfg(any(windows, target_os = "linux"))]
struct FakeTransactionOperations {
    failure: TransactionFailure,
    injected: Cell<bool>,
}

#[cfg(any(windows, target_os = "linux"))]
impl TransactionOperations for FakeTransactionOperations {
    fn rename(&self, source: &Path, target: &Path) -> Result<(), UpdateError> {
        if self.failure == TransactionFailure::Assets
            && !self.injected.get()
            && source.ends_with("new/default-assets")
        {
            self.injected.set(true);
            return Err(UpdateError::new("injected asset replacement failure"));
        }
        fs::rename(source, target).map_err(UpdateError::from)
    }

    fn replace(&self, target: &Path, replacement: &Path, backup: &Path) -> Result<(), UpdateError> {
        let failure = if target.file_name().is_some_and(|name| name == CLI_FILE) {
            TransactionFailure::Cli
        } else {
            TransactionFailure::Shell
        };
        if self.failure == failure
            && !self.injected.get()
            && replacement
                .parent()
                .is_some_and(|path| path.ends_with("new"))
        {
            self.injected.set(true);
            return Err(UpdateError::new("injected executable replacement failure"));
        }
        if target.exists() {
            fs::rename(target, backup)?;
        }
        fs::rename(replacement, target)?;
        Ok(())
    }

    fn launch_new_and_wait(
        &self,
        _paths: &TransactionPaths,
        _target_sha: &str,
    ) -> Result<(), UpdateError> {
        match self.failure {
            TransactionFailure::NewLaunch | TransactionFailure::RestoredLaunch => {
                Err(UpdateError::new("injected new Shell launch failure"))
            }
            TransactionFailure::ReadyTimeout => Err(UpdateError::new(
                "updated Shell did not become ready within 60 seconds",
            )),
            _ => Ok(()),
        }
    }

    fn launch_restored(&self, _shell: &Path, _reason: &str) -> Result<(), UpdateError> {
        if self.failure == TransactionFailure::RestoredLaunch {
            Err(UpdateError::new("injected restored Shell launch failure"))
        } else {
            Ok(())
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn transaction_fixture(name: &str) -> (PathBuf, PathBuf, TransactionManifest) {
    let root = update_test_root(name);
    let install = root.join("install");
    let transaction_dir = install.join(".tundra-update/tx");
    let new = transaction_dir.join("new");
    fs::create_dir_all(new.join("default-assets")).unwrap();
    fs::create_dir_all(transaction_dir.join("backup")).unwrap();
    fs::create_dir_all(install.join("assets/themes/default")).unwrap();
    fs::create_dir_all(install.join("assets/themes/custom")).unwrap();
    fs::write(install.join(SHELL_FILE), b"old shell").unwrap();
    fs::write(install.join(CLI_FILE), b"old cli").unwrap();
    fs::write(
        install.join("assets/themes/default/theme.txt"),
        b"old theme",
    )
    .unwrap();
    fs::write(
        install.join("assets/themes/custom/theme.txt"),
        b"custom theme",
    )
    .unwrap();
    fs::write(new.join(SHELL_FILE), b"new shell").unwrap();
    fs::write(new.join(CLI_FILE), b"new cli").unwrap();
    fs::write(new.join("default-assets/theme.txt"), b"new theme").unwrap();
    let manifest_path = transaction_dir.join("transaction.json");
    let manifest = TransactionManifest {
        protocol: UPDATE_PROTOCOL_VERSION,
        target_sha: "abc".to_owned(),
        install_dir: install,
        transaction_dir,
        state: TransactionState::Prepared,
        assets_replaced: false,
        cli_replaced: false,
        shell_replaced: false,
    };
    write_manifest(&manifest_path, &manifest).unwrap();
    (root, manifest_path, manifest)
}

#[cfg(any(windows, target_os = "linux"))]
fn assert_old_install_preserved(manifest: &TransactionManifest) {
    assert_eq!(
        fs::read(manifest.install_dir.join(SHELL_FILE)).unwrap(),
        b"old shell"
    );
    assert_eq!(
        fs::read(manifest.install_dir.join(CLI_FILE)).unwrap(),
        b"old cli"
    );
    assert_eq!(
        fs::read(manifest.install_dir.join("assets/themes/default/theme.txt")).unwrap(),
        b"old theme"
    );
    assert_eq!(
        fs::read(manifest.install_dir.join("assets/themes/custom/theme.txt")).unwrap(),
        b"custom theme"
    );
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn update_injected_transaction_failures_restore_every_installed_file() {
    for failure in [
        TransactionFailure::Assets,
        TransactionFailure::Cli,
        TransactionFailure::Shell,
        TransactionFailure::NewLaunch,
        TransactionFailure::ReadyTimeout,
        TransactionFailure::RestoredLaunch,
    ] {
        let (root, path, mut manifest) = transaction_fixture("injected-transaction");
        let operations = FakeTransactionOperations {
            failure,
            injected: Cell::new(false),
        };
        let result = run_update_transaction(&path, &mut manifest, false, &operations);
        if failure == TransactionFailure::RestoredLaunch {
            assert!(result.is_err());
            assert_eq!(
                load_manifest(&path).unwrap().state,
                TransactionState::Failed
            );
        } else {
            assert!(result.is_ok());
        }
        assert_old_install_preserved(&manifest);
        assert!(path.is_file(), "transaction journal must be retained");
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn update_interrupted_journal_recovers_old_installation() {
    let (root, path, mut manifest) = transaction_fixture("interrupted");
    let operations = FakeTransactionOperations {
        failure: TransactionFailure::None,
        injected: Cell::new(false),
    };
    manifest.state = TransactionState::Applying;
    write_manifest(&path, &manifest).unwrap();
    apply_prepared_files_with_operations(&path, &mut manifest, &operations).unwrap();
    run_update_transaction(&path, &mut manifest, true, &operations).unwrap();
    assert_old_install_preserved(&manifest);
    assert_eq!(
        load_manifest(&path).unwrap().state,
        TransactionState::RolledBack
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(any(windows, target_os = "linux"))]
fn recovery_scan_fixture(name: &str, state: TransactionState) -> (PathBuf, PathBuf, PathBuf) {
    let root = update_test_root(name);
    fs::create_dir_all(root.join("install")).unwrap();
    let install = root.join("install/../install");
    let canonical_install = fs::canonicalize(&install).unwrap();
    assert_ne!(install, canonical_install);
    let transaction_dir = canonical_install.join(".tundra-update/tx");
    fs::create_dir_all(&transaction_dir).unwrap();
    let manifest_path = transaction_dir.join("transaction.json");
    let manifest = TransactionManifest {
        protocol: UPDATE_PROTOCOL_VERSION,
        target_sha: "abc".to_owned(),
        install_dir: canonical_install,
        transaction_dir,
        state,
        assets_replaced: false,
        cli_replaced: false,
        shell_replaced: false,
    };
    write_manifest(&manifest_path, &manifest).unwrap();
    (root, install, manifest_path)
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn update_recovery_scan_canonicalizes_install_path_for_cleanup_and_recovery() {
    let (root, install, manifest_path) =
        recovery_scan_fixture("recovery-scan-committed", TransactionState::Committed);
    let launches = std::sync::Mutex::new(Vec::new());
    assert!(
        !scan_update_recovery(&install, 41, &|path, pid, recover_only| {
            launches
                .lock()
                .unwrap()
                .push((path.to_owned(), pid, recover_only));
            Ok(())
        })
        .unwrap()
    );
    assert!(!manifest_path.parent().unwrap().exists());
    assert!(launches.lock().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();

    for state in [TransactionState::Applying, TransactionState::AwaitingReady] {
        let (root, install, manifest_path) = recovery_scan_fixture("recovery-scan", state);
        let launches = std::sync::Mutex::new(Vec::new());
        assert!(
            scan_update_recovery(&install, 42, &|path, pid, recover_only| {
                launches
                    .lock()
                    .unwrap()
                    .push((path.to_owned(), pid, recover_only));
                Ok(())
            })
            .unwrap()
        );
        assert_eq!(*launches.lock().unwrap(), vec![(manifest_path, 42, true)]);
        fs::remove_dir_all(root).unwrap();
    }
}
