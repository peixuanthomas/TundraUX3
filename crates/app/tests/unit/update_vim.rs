use super::*;

pub(super) fn write_bundle(directory: &Path, program: &[u8]) {
    fs::create_dir_all(directory.join("runtime/doc")).unwrap();
    fs::write(
        directory.join(if cfg!(windows) { "vim.exe" } else { "vim" }),
        program,
    )
    .unwrap();
    fs::write(directory.join("runtime/defaults.vim"), b"defaults").unwrap();
    fs::write(directory.join("runtime/doc/help.txt"), b"help").unwrap();
    fs::write(directory.join("LICENSE"), b"Vim License").unwrap();
    fs::write(directory.join("UPSTREAM.json"), vim::UPSTREAM_MANIFEST).unwrap();
    #[cfg(windows)]
    for name in vim::WINDOWS_ICONV_FILES {
        let path = directory.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"iconv fixture").unwrap();
    }
}

#[test]
fn vim_bundle_requires_runtime_and_accepts_a_new_valid_pin() {
    let root = update_test_root("vim-validation");
    write_bundle(&root, b"Vim");
    vim::validate_bundle(&root).unwrap();
    let path = root.join("UPSTREAM.json");
    let mut metadata: serde_json::Value = serde_json::from_str(vim::UPSTREAM_MANIFEST).unwrap();
    metadata["version"] = "9.3.1".into();
    metadata["commit"] = "a".repeat(40).into();
    metadata["source_url"] = format!(
        "https://codeload.github.com/vim/vim/tar.gz/{}",
        "a".repeat(40)
    )
    .into();
    fs::write(&path, serde_json::to_vec(&metadata).unwrap()).unwrap();
    vim::validate_bundle(&root).unwrap();
    metadata["required_features"] = serde_json::json!(["eval", "autocmd"]);
    fs::write(&path, serde_json::to_vec(&metadata).unwrap()).unwrap();
    assert!(vim::validate_bundle(&root).is_err());
    fs::write(&path, vim::UPSTREAM_MANIFEST).unwrap();
    fs::remove_file(root.join("runtime/doc/help.txt")).unwrap();
    assert!(vim::validate_bundle(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn vim_bundle_copy_preserves_runtime_and_rejects_links_or_cancellation() {
    let root = update_test_root("vim-copy");
    let source = root.join("source");
    write_bundle(&source, b"Vim");
    fs::create_dir_all(source.join("runtime/syntax")).unwrap();
    fs::write(source.join("runtime/syntax/rust.vim"), b"syntax fixture").unwrap();
    let copied = root.join("complete");
    vim::copy_directory(&source, &copied, &UpdateCancellation::default()).unwrap();
    vim::validate_bundle(&copied).unwrap();
    assert_eq!(
        fs::read(copied.join("runtime/syntax/rust.vim")).unwrap(),
        b"syntax fixture"
    );
    std::os::unix::fs::symlink("defaults.vim", source.join("runtime/link.vim")).unwrap();
    assert!(
        vim::copy_directory(
            &source,
            &root.join("linked"),
            &UpdateCancellation::default()
        )
        .is_err()
    );
    let cancel = UpdateCancellation::default();
    cancel.cancel();
    assert!(
        vim::copy_directory(&source, &root.join("cancelled"), &cancel)
            .unwrap_err()
            .is_cancelled()
    );
    assert!(!root.join("cancelled").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn beta_builds_vim_explicitly_beside_release_products() {
    struct Recording(std::cell::RefCell<Vec<(String, ProcessSpec)>>);
    impl PreparationOperations for Recording {
        fn run(&self, spec: &ProcessSpec, name: &str) -> Result<ProcessExit, UpdateError> {
            self.0.borrow_mut().push((name.to_owned(), spec.clone()));
            Ok(ProcessExit {
                code: Some(0),
                stdout: ProcessStream::from_bytes(b"rustc 1.99.0\nrelease: 1.99.0\n".to_vec()),
                stderr: ProcessStream::from_bytes(Vec::new()),
            })
        }
        fn probe(&self, _executable: &Path, _sha: &str) -> Result<(), UpdateError> {
            Ok(())
        }
    }
    let root = update_test_root("vim-beta");
    let source = root.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("Cargo.toml"),
        "[workspace.package]\nrust-version = \"1.95\"\n",
    )
    .unwrap();
    let release = root.join("target/release");
    write_bundle(&release.join("vim"), b"Vim");
    for name in [SHELL_FILE, CLI_FILE] {
        fs::write(release.join(name), b"program").unwrap();
    }
    let check = UpdateCheckResult {
        release: None,
        default_branch: "master".into(),
        head_sha: "a".repeat(40),
        relation: UpdateRelation::Behind { remote_ahead: 1 },
        commits: Vec::new(),
    };
    let operations = Recording(Default::default());
    prepare_extracted_with_operations(&check, &mut |_| {}, &root, &source, &operations).unwrap();
    let calls = operations.0.borrow();
    let build = calls
        .iter()
        .position(|(name, _)| name == "cargo build")
        .unwrap();
    let vim = calls
        .iter()
        .position(|(name, _)| name == "Vim build")
        .unwrap();
    assert!(vim > build);
    let args = calls[vim].1.args_slice();
    assert_eq!(Path::new(&args[0]), source.join("scripts/build-vim.py"));
    assert_eq!(args[1], "--output");
    assert_eq!(Path::new(&args[2]), release.join("vim"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(any(windows, target_os = "linux"))]
fn vim_transaction(name: &str, had_previous: bool) -> (PathBuf, PathBuf, TransactionManifest) {
    let (root, path, mut manifest) = transaction_fixture(name);
    write_bundle(&manifest.transaction_dir.join("new/vim"), b"new Vim");
    if had_previous {
        write_bundle(&manifest.install_dir.join("vim"), b"old Vim");
    } else {
        fs::remove_dir_all(manifest.install_dir.join("vim")).unwrap();
    }
    manifest.vim = Some(VimTransaction {
        had_previous,
        replaced: false,
    });
    write_manifest(&path, &manifest).unwrap();
    (root, path, manifest)
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn vim_runtime_commits_with_both_programs_and_rolls_back_on_startup_failure() {
    for had_previous in [false, true] {
        for failure in [TransactionFailure::None, TransactionFailure::NewLaunch] {
            let (root, path, mut manifest) = vim_transaction("vim-transaction", had_previous);
            let operations = FakeTransactionOperations {
                failure,
                injected: Cell::new(false),
            };
            run_update_transaction(&path, &mut manifest, false, &operations).unwrap();
            let installed = manifest.install_dir.join("vim");
            if failure == TransactionFailure::None {
                assert_eq!(manifest.state, TransactionState::Committed);
                assert_eq!(
                    fs::read(installed.join(if cfg!(windows) { "vim.exe" } else { "vim" }))
                        .unwrap(),
                    b"new Vim"
                );
                assert!(!manifest.transaction_dir.join("backup/vim").exists());
            } else {
                assert_eq!(manifest.state, TransactionState::RolledBack);
                assert_old_install_preserved(&manifest);
                if had_previous {
                    assert_eq!(
                        fs::read(installed.join(if cfg!(windows) { "vim.exe" } else { "vim" }))
                            .unwrap(),
                        b"old Vim"
                    );
                } else {
                    assert!(!installed.exists());
                }
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn interrupted_vim_directory_publish_restores_the_original_installation() {
    struct FailAfterBackup;
    impl TransactionOperations for FailAfterBackup {
        fn replace(
            &self,
            target: &Path,
            replacement: &Path,
            backup: &Path,
        ) -> Result<(), UpdateError> {
            if target.exists() {
                fs::rename(target, backup)?;
            }
            fs::rename(replacement, target)?;
            Ok(())
        }
        fn replace_directory(
            &self,
            target: &Path,
            replacement: &Path,
            backup: &Path,
        ) -> Result<(), UpdateError> {
            if replacement
                .parent()
                .is_some_and(|parent| parent.ends_with("new"))
            {
                if target.exists() {
                    fs::rename(target, backup)?;
                }
                return Err(UpdateError::new("injected Vim publish failure"));
            }
            vim::replace_directory(target, replacement, backup)
        }
        fn launch_new_and_wait(&self, _: &TransactionPaths, _: &str) -> Result<(), UpdateError> {
            panic!("failed publish must not launch new Shell")
        }
        fn launch_restored(&self, _: &Path, _: &str) -> Result<(), UpdateError> {
            Ok(())
        }
    }
    for had_previous in [false, true] {
        let (root, path, mut manifest) = vim_transaction("vim-publish-failure", had_previous);
        run_update_transaction(&path, &mut manifest, false, &FailAfterBackup).unwrap();
        assert_old_install_preserved(&manifest);
        assert_eq!(manifest.state, TransactionState::RolledBack);
        let installed = manifest.install_dir.join("vim");
        assert_eq!(installed.exists(), had_previous);
        if had_previous {
            assert_eq!(
                fs::read(installed.join(if cfg!(windows) { "vim.exe" } else { "vim" })).unwrap(),
                b"old Vim"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn vim_recovery_handles_a_publish_before_its_journal_write() {
    for had_previous in [false, true] {
        let (root, path, mut manifest) = vim_transaction("vim-journal-crash", had_previous);
        let paths = transaction_paths(&manifest);
        vim::replace_directory(&paths.installed_vim, &paths.new_vim, &paths.backup_vim).unwrap();
        manifest.state = TransactionState::Applying;
        write_manifest(&path, &manifest).unwrap();
        let operations = FakeTransactionOperations {
            failure: TransactionFailure::None,
            injected: Cell::new(false),
        };
        run_update_transaction(&path, &mut manifest, true, &operations).unwrap();
        assert_old_install_preserved(&manifest);
        assert_eq!(paths.installed_vim.exists(), had_previous);
        if had_previous {
            assert_eq!(
                fs::read(
                    paths
                        .installed_vim
                        .join(if cfg!(windows) { "vim.exe" } else { "vim" })
                )
                .unwrap(),
                b"old Vim"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn legacy_update_journals_leave_an_existing_vim_runtime_untouched() {
    for applying in [false, true] {
        let (root, path, mut manifest) = transaction_fixture("vim-legacy-journal");
        write_bundle(&manifest.install_dir.join("vim"), b"existing Vim");
        if applying {
            let paths = transaction_paths(&manifest);
            fs::rename(&paths.installed_cli, &paths.backup_cli).unwrap();
            fs::rename(&paths.new_cli, &paths.installed_cli).unwrap();
            manifest.cli_replaced = true;
            manifest.state = TransactionState::Applying;
        }
        let mut legacy = serde_json::to_value(&manifest).unwrap();
        legacy["protocol"] = 2.into();
        legacy.as_object_mut().unwrap().remove("vim");
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut manifest = load_manifest(&path).unwrap();
        assert!(manifest.vim.is_none());
        let operations = FakeTransactionOperations {
            failure: TransactionFailure::Cli,
            injected: Cell::new(false),
        };
        run_update_transaction(&path, &mut manifest, applying, &operations).unwrap();
        assert!(
            !operations.injected.get(),
            "legacy updates must not apply new files"
        );
        assert_eq!(manifest.state, TransactionState::RolledBack);
        assert_old_install_preserved(&manifest);
        assert_eq!(
            fs::read(manifest.install_dir.join("vim").join(if cfg!(windows) {
                "vim.exe"
            } else {
                "vim"
            }))
            .unwrap(),
            b"existing Vim"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn current_update_protocol_rejects_a_missing_vim_transaction() {
    let root = update_test_root("vim-required-journal");
    let install = root.join("install");
    let transaction_dir = install.join(".tundra-update/tx");
    fs::create_dir_all(&transaction_dir).unwrap();
    mark_portable_fixture(&install);
    let path = transaction_dir.join("transaction.json");
    let manifest = TransactionManifest {
        protocol: UPDATE_PROTOCOL_VERSION,
        target_sha: "a".repeat(40),
        install_dir: install,
        transaction_dir,
        state: TransactionState::Prepared,
        cli_replaced: false,
        shell_replaced: false,
        vim: None,
    };
    write_manifest(&path, &manifest).unwrap();
    assert!(
        load_manifest(&path)
            .unwrap_err()
            .to_string()
            .contains("missing bundled Vim")
    );
    fs::remove_dir_all(root).unwrap();
}
