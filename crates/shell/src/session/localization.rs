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
#[path = "../../tests/unit/session/localization/tests.rs"]
mod tests;
