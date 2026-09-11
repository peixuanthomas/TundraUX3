//! Language state belongs to the session, independently of the visual theme.
use super::*;

#[derive(Debug, Clone)]
pub(super) struct PreparedLanguage {
    pub snapshot: Arc<i18n::LanguageSnapshot>,
    pub catalog: i18n::LanguageCatalog,
    pub diagnostics: Vec<i18n::RepairDiagnostic>,
}
impl PreparedLanguage {
    pub fn load(root: &std::path::Path, code: &str) -> Self {
        let mut loaded = i18n::LanguageSnapshot::load_startup(root, code, 1);
        let catalog = i18n::LanguageCatalog::discover(root)
            .unwrap_or_else(|_| i18n::LanguageCatalog::built_in());
        loaded.diagnostics.extend(catalog.diagnostics.clone());
        Self {
            snapshot: Arc::new(loaded.snapshot),
            catalog,
            diagnostics: loaded.diagnostics,
        }
    }
    pub fn update_from(&mut self, state: &ShellSession) {
        self.snapshot = state.language.clone();
        self.catalog = state.language_catalog.clone();
        self.diagnostics.clear();
    }
}

impl ShellSession {
    pub fn language_code(&self) -> &str {
        self.language.code()
    }

    pub fn language_snapshot(&self) -> Arc<i18n::LanguageSnapshot> {
        self.language.clone()
    }

    pub fn language_options(&self) -> Vec<app::SetupLanguageOption> {
        self.language_catalog
            .options()
            .iter()
            .map(|option| app::SetupLanguageOption {
                code: option.code.clone(),
                label: option.native_name.clone(),
            })
            .collect()
    }

    pub(super) fn prepare_language(
        &self,
        code: &str,
    ) -> Result<(i18n::LanguageCatalog, i18n::LanguageLoad), i18n::LanguageError> {
        let root = self.ascii_assets.store().root();
        // Loading repairs the default before discovery, so even a missing root
        // can be restored. The candidate is not published until config is saved.
        let mut loaded =
            i18n::LanguageSnapshot::load(root, code, self.language.generation().saturating_add(1))?;
        let catalog = i18n::LanguageCatalog::discover(root)
            .unwrap_or_else(|_| i18n::LanguageCatalog::built_in());
        loaded.diagnostics.extend(catalog.diagnostics.clone());
        Ok((catalog, loaded))
    }

    pub(super) fn publish_language(
        &mut self,
        catalog: i18n::LanguageCatalog,
        loaded: i18n::LanguageLoad,
    ) {
        self.language_catalog = catalog;
        self.language = Arc::new(loaded.snapshot);
        self.setup_selected_language_index = self
            .language_catalog
            .options()
            .iter()
            .position(|option| option.code == self.language.code())
            .unwrap_or(0);
        let _language = i18n::enter_snapshot(self.language.clone());
        self.notification_pointer_capture = None;
        self.scrollbar_drag = None;
        self.scroll_notification_message(0, false);
        self.clamp_settings_scroll();
        self.clamp_system_status_dashboard_scroll();
        self.sync_setup_timezone_window();
        self.rebuild_editor_rich_render_cache();
        self.refresh_hit_map();
        self.repaired_resource_paths.clear();
        self.fallback_resource_paths.clear();
        self.report_language_diagnostics(&loaded.diagnostics);
    }

    pub(super) fn rollback_failed_language_config(
        &self,
        storage: &StorageManager,
        previous: &storage::StorageConfig,
        candidate: &storage::StorageConfig,
    ) {
        // Atomic writers may report a directory-fsync failure after rename.
        // Reconcile only our candidate; never overwrite an unrelated writer.
        if storage.load_config().ok().as_ref() != Some(candidate) {
            return;
        }
        let result = storage.save_config(previous);
        if storage.load_config().ok().as_ref() == Some(previous) {
            return;
        }
        let mut event = runtime_log::RuntimeLogEvent::new(
            runtime_log::LogContext {
                module: "ux.i18n".into(),
                operation: "rollback_language_configuration".into(),
                ..Default::default()
            },
            runtime_log::LogLevel::Error,
            runtime_log::LogPhase::Failed,
            "Could not restore previous language configuration after a persistence failure",
        );
        event.error_code = Some("UX_LANGUAGE_CONFIG_ROLLBACK_FAILED".into());
        event.source_path = Some(storage.layout().config_path.clone());
        if let Err(error) = result {
            event.error_chain.push(error.to_string());
        }
        record_shell_runtime_event(event);
    }

    pub(super) fn report_language_failure(&mut self, error: &i18n::LanguageError) {
        if !error.diagnostics.is_empty() {
            self.repaired_resource_paths.clear();
            self.fallback_resource_paths.clear();
            self.report_language_diagnostics(&error.diagnostics);
        }
        let mut event = runtime_log::RuntimeLogEvent::new(
            runtime_log::LogContext {
                module: "ux.i18n".into(),
                operation: "reload_language".into(),
                ..Default::default()
            },
            runtime_log::LogLevel::Error,
            runtime_log::LogPhase::Failed,
            "Language reload failed; previous language remains active",
        );
        event.error_code = Some("UX_LANGUAGE_RELOAD_FAILED".into());
        event.message_id = Some("language-reload-failed".into());
        event
            .message_args
            .insert("reason".into(), error.to_string().into());
        event.error_chain.push(error.to_string());
        record_shell_runtime_event(event);
        self.notify_alert_with_key(
            "shell.language-reload",
            i18n::msg!("language-reload-failed", reason = error.to_string()),
            ui::NotificationTone::Error,
        );
    }
}

impl ShellSession {
    pub(super) fn report_language_diagnostics(&mut self, diagnostics: &[i18n::RepairDiagnostic]) {
        for diagnostic in diagnostics {
            let failed = matches!(
                diagnostic.kind,
                i18n::RepairKind::WriteFailed
                    | i18n::RepairKind::StartupFallback
                    | i18n::RepairKind::InvalidResource
            );
            let mut event = runtime_log::RuntimeLogEvent::new(
                runtime_log::LogContext {
                    module: "ux.i18n".into(),
                    operation: "load_language".into(),
                    ..Default::default()
                },
                if failed {
                    runtime_log::LogLevel::Warning
                } else {
                    runtime_log::LogLevel::Info
                },
                if diagnostic.repaired {
                    runtime_log::LogPhase::Recovered
                } else {
                    runtime_log::LogPhase::Failed
                },
                "Language resource validation result",
            );
            event.source_path = Some(diagnostic.path.clone());
            event.error_code = Some(
                if diagnostic.repaired {
                    "UX_LANGUAGE_RESOURCE_REPAIRED"
                } else {
                    "UX_LANGUAGE_RESOURCE_FALLBACK"
                }
                .into(),
            );
            event.error_chain.push(diagnostic.message.clone());
            record_shell_runtime_event(event);
            let path = diagnostic.path.display().to_string();
            if diagnostic.repaired && !self.repaired_resource_paths.contains(&path) {
                self.repaired_resource_paths.push(path);
            } else if failed && !self.fallback_resource_paths.contains(&path) {
                self.fallback_resource_paths.push(path);
            }
        }
        if diagnostics.iter().any(|diagnostic| {
            diagnostic.repaired
                || matches!(
                    diagnostic.kind,
                    i18n::RepairKind::WriteFailed
                        | i18n::RepairKind::StartupFallback
                        | i18n::RepairKind::InvalidResource
                )
        }) {
            self.show_resource_recovery_report();
        }
    }

    pub(super) fn show_resource_recovery_report(&mut self) {
        if self.repaired_resource_paths.is_empty() && self.fallback_resource_paths.is_empty() {
            return;
        }
        let message = if self.fallback_resource_paths.is_empty() {
            i18n::msg!(
                "resources-repaired",
                count = self.repaired_resource_paths.len(),
                files = self.repaired_resource_paths.join("\n")
            )
        } else {
            i18n::msg!(
                "resources-fallback",
                repaired = self.repaired_resource_paths.len(),
                failed = self.fallback_resource_paths.len(),
                repaired_files = self.repaired_resource_paths.join("\n"),
                failed_files = self.fallback_resource_paths.join("\n")
            )
        };
        self.notify_modal_with_options(
            ShellNotification::modal(
                i18n::msg!("resources-recovery-title"),
                message,
                if self.fallback_resource_paths.is_empty() {
                    ui::NotificationTone::Info
                } else {
                    ui::NotificationTone::Warning
                },
                vec![
                    ShellNotificationAction::new("ok", i18n::msg!("resources-recovery-ok"))
                        .cancel(),
                ],
            )
            .with_key("shell.resource-recovery"),
        );
    }
}

impl ShellSession {
    pub(super) fn report_graphical_resource_recovery(
        &mut self,
        report: &ascii_assets::DefaultThemeRecoveryReport,
    ) {
        for (files, repaired) in [(&report.repaired, true), (&report.fallback, false)] {
            for file in files {
                let mut event = runtime_log::RuntimeLogEvent::new(
                    runtime_log::LogContext {
                        module: "ux.assets".into(),
                        operation: "restore_default_theme".into(),
                        ..Default::default()
                    },
                    if repaired {
                        runtime_log::LogLevel::Info
                    } else {
                        runtime_log::LogLevel::Warning
                    },
                    if repaired {
                        runtime_log::LogPhase::Recovered
                    } else {
                        runtime_log::LogPhase::Failed
                    },
                    if repaired {
                        "Default graphical resource automatically repaired"
                    } else {
                        "Default graphical resource repair failed; embedded content active"
                    },
                );
                event.error_code = Some(
                    if repaired {
                        "UX_ASSET_REPAIRED"
                    } else {
                        "UX_ASSET_RESTORE_FAILED"
                    }
                    .into(),
                );
                event.source_path = Some(file.path.clone());
                event.error_chain.push(file.issue.clone());
                event.error_chain.extend(file.repair_error.clone());
                record_shell_runtime_event(event);
                let paths = if repaired {
                    &mut self.ui.repaired_resource_paths
                } else {
                    &mut self.ui.fallback_resource_paths
                };
                let path = file.path.display().to_string();
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        self.show_resource_recovery_report();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct Fixture {
        root: PathBuf,
        state: ShellSession,
        storage: StorageManager,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(parent) = self.storage.layout().config_path.parent() {
                    let _ =
                        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
                }
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn copy_tree(source: &Path, destination: &Path) {
        std::fs::create_dir_all(destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    fn fixture() -> Fixture {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "tux3-language-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let assets = root.join("assets");
        let (store, _) = ui::AsciiAssetStore::load_default_with_root_and_recovery(&assets).unwrap();
        copy_tree(
            &Path::new(ascii_assets::CANONICAL_ASSETS_DIR).join("locales"),
            &assets.join("locales"),
        );
        let paths = platform::build_linux_app_paths(
            root.join("config"),
            root.join("data"),
            root.join("cache"),
            root.join("state"),
            root.join("temp"),
        )
        .unwrap();
        let storage = StorageManager::open(paths).unwrap().manager;
        let mut startup = ShellStartupState::clean(
            PlatformKind::Linux,
            PlatformCapabilities::native_supported(),
        );
        startup.storage_manager = Some(storage.clone());
        let mut state = ShellSession::new_with_startup_and_assets(
            ShellLaunchConfig::default(),
            (120, 40),
            startup,
            ui::RuntimeAsciiAssets::from_store(store),
        );
        state.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(AuthSession {
                session_id: "language-test".into(),
                user_id: "language-admin".into(),
                username: "admin".into(),
                role: UserRole::Admin,
                started_at_epoch_ms: 1,
            })),
            Instant::now(),
        );
        Fixture {
            root,
            state,
            storage,
        }
    }

    #[test]
    fn same_language_selection_reloads_disk_and_keeps_retained_messages() {
        let mut f = fixture();
        f.state
            .save_region_picker_value(Some("zh-Hans".into()), None);
        assert_eq!(f.state.language_code(), "zh-CN");
        assert_eq!(f.storage.load_config().unwrap().language, "zh-CN");
        let generation = f.state.language.generation();
        let message = i18n::msg!("resources-recovery-ok");
        assert_eq!(f.state.language.render(&message), "确定");
        let path = f.root.join("assets/locales/zh-CN/recovery/startup.ftl");
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            original.replace(
                "resources-recovery-ok = 确定",
                "resources-recovery-ok = 重载成功",
            ),
        )
        .unwrap();
        f.state.save_region_picker_value(Some("zh-CN".into()), None);
        assert!(f.state.language.generation() > generation);
        assert_eq!(f.state.language.render(&message), "重载成功");
    }

    #[test]
    fn failed_reload_preserves_snapshot_configuration_and_user_input() {
        let mut f = fixture();
        f.state.save_region_picker_value(Some("zh-CN".into()), None);
        f.state.login_username = "unchanged input".into();
        let snapshot = f.state.language.clone();
        let config = std::fs::read(&f.storage.layout().config_path).unwrap();
        std::fs::write(
            f.root.join("assets/locales/zh-CN/recovery/startup.ftl"),
            "broken = {\n",
        )
        .unwrap();
        f.state.save_region_picker_value(Some("zh-CN".into()), None);
        assert!(Arc::ptr_eq(&snapshot, &f.state.language));
        assert_eq!(
            std::fs::read(&f.storage.layout().config_path).unwrap(),
            config
        );
        assert_eq!(f.state.login_username, "unchanged input");
        assert!(
            f.state
                .app
                .notification_center()
                .alert_message_for_key("shell.language-reload")
                .is_some()
        );
    }

    #[test]
    fn reloading_discovers_new_language_metadata_without_loading_it_during_render() {
        let mut f = fixture();
        let directory = f.root.join("assets/locales/fr-FR");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("manifest.toml"),
            "format_version = 1\ncode = \"fr-FR\"\nnative_name = \"Français\"\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("messages.ftl"),
            "resources-recovery-ok = D’accord\n",
        )
        .unwrap();
        assert!(
            !f.state
                .language_options()
                .iter()
                .any(|option| option.code == "fr-FR")
        );
        f.state.save_region_picker_value(Some("en-US".into()), None);
        assert!(
            f.state
                .language_options()
                .iter()
                .any(|option| option.code == "fr-FR")
        );
        f.state.save_region_picker_value(Some("fr-FR".into()), None);
        std::fs::remove_dir_all(f.root.join("assets/locales")).unwrap();
        assert_eq!(
            f.state
                .language
                .render(&i18n::msg!("resources-recovery-ok")),
            "D’accord"
        );
        assert_eq!(
            f.state
                .language
                .render(&i18n::msg!("resources-recovery-title")),
            "Resource recovery"
        );
    }

    #[test]
    fn recovery_summary_is_updated_in_place_and_distinguishes_fallback() {
        let mut f = fixture();
        f.state.repaired_resource_paths.push("assets/a".into());
        f.state.show_resource_recovery_report();
        let id = f.state.app.notification_center().active_modal_id();
        f.state.fallback_resource_paths.push("assets/b".into());
        f.state.show_resource_recovery_report();
        assert_eq!(f.state.app.notification_center().active_modal_id(), id);
        let modal = f.state.app.notification_center().active_modal().unwrap();
        assert!(
            f.state
                .language
                .render_text(&modal.message)
                .contains("Built-in resources are active")
        );
        assert_eq!(f.state.app.notification_center().queued_modal_count(), 0);
    }

    #[test]
    fn configuration_write_failure_does_not_publish_valid_candidate() {
        let mut f = fixture();
        let before = f.state.language.clone();
        let disk = std::fs::read(&f.storage.layout().config_path).unwrap();
        let mut attempted = false;
        f.state.save_region_picker_value_with(
            Some("zh-CN".into()),
            None,
            |_, storage, candidate| {
                attempted = true;
                assert_eq!(candidate.language, "zh-CN");
                Err(storage::StorageError::Io {
                    operation: "write test configuration",
                    path: storage.layout().config_path.clone(),
                    message: "injected persistence failure".into(),
                })
            },
        );
        assert!(attempted);
        assert!(Arc::ptr_eq(&before, &f.state.language));
        assert_eq!(
            std::fs::read(&f.storage.layout().config_path).unwrap(),
            disk
        );
        assert_eq!(f.storage.load_config().unwrap().language, "en-US");
    }

    #[test]
    fn healthy_reload_does_not_reopen_acknowledged_startup_recovery() {
        let mut f = fixture();
        f.state.repaired_resource_paths.push("old-repair".into());
        f.state.show_resource_recovery_report();
        f.state.app.dispatch_at(
            app::AppCommand::Notification(app::NotificationCommand::DismissModalByKey(
                "shell.resource-recovery".into(),
            )),
            Instant::now(),
        );
        f.state.save_region_picker_value(Some("en-US".into()), None);
        assert!(f.state.app.notification_center().active_modal().is_none());
        assert!(f.state.repaired_resource_paths.is_empty());
    }
    #[test]
    fn post_rename_persistence_failure_restores_previous_configuration() {
        let mut f = fixture();
        let before = f.state.language.clone();
        let config = f.storage.load_config().unwrap();
        f.state.save_region_picker_value_with(
            Some("zh-CN".into()),
            None,
            |_, storage, candidate| {
                storage.save_config(candidate).unwrap();
                Err(storage::StorageError::Io {
                    operation: "sync parent after rename",
                    path: storage.layout().config_path.clone(),
                    message: "injected directory sync failure".into(),
                })
            },
        );
        assert!(Arc::ptr_eq(&before, &f.state.language));
        assert_eq!(f.storage.load_config().unwrap(), config);
    }
    #[test]
    fn discovered_language_rows_share_setup_render_and_mouse_geometry() {
        let mut f = fixture();
        let directory = f.root.join("assets/locales/fr-FR");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("manifest.toml"),
            "format_version = 1\ncode = \"fr-FR\"\nnative_name = \"Français\"\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("messages.ftl"),
            "resources-recovery-ok = Oui\n",
        )
        .unwrap();
        f.state.save_region_picker_value(Some("en-US".into()), None);
        f.state.screen_stack = vec![ShellScreen::FirstRunSetup];
        f.state.setup_step = ui::SetupStep::Language;
        f.state.focused_component = ShellComponent::SetupLanguage;
        f.state.refresh_hit_map();
        let count = f.state.language_options().len();
        assert_eq!(count, 3);
        let main = setup_main_rect(f.state.terminal_size).unwrap();
        let rendered = ui::setup_language_list_area(main, count);
        let region = f
            .state
            .hit_map
            .regions()
            .iter()
            .find(|region| region.component == ShellComponent::SetupLanguage)
            .unwrap();
        assert_eq!(region.area, rendered);
        let last_row = (rendered.x, rendered.y + 2);
        assert_eq!(f.state.setup_language_index_at(last_row), Some(2));
    }
}
