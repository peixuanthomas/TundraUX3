//! Presentation text derived from producer IDs and typed outcomes, never diagnostic prose.
use super::*;
use i18n::{LocalizedError, LocalizedText, msg};

impl DiagnosticCategory {
    pub fn localized_label(self) -> LocalizedText {
        match self {
            Self::Environment => msg!("app-diagnostics-category-environment"),
            Self::Paths => msg!("app-diagnostics-category-paths"),
            Self::Storage => msg!("app-diagnostics-category-storage"),
            Self::Assets => msg!("app-diagnostics-category-assets"),
        }
        .into()
    }
}

impl DiagnosticStatus {
    pub fn localized_label(self) -> LocalizedText {
        match self {
            Self::Pass => msg!("app-diagnostics-status-pass"),
            Self::Unsupported => msg!("app-diagnostics-status-unsupported"),
            Self::Warning => msg!("app-diagnostics-status-warning"),
            Self::Fail => msg!("app-diagnostics-status-fail"),
        }
        .into()
    }
}

impl DiagnosticCheck {
    pub fn localized_label(&self) -> LocalizedText {
        match self.id.as_str() {
            "environment.platform" => msg!("app-diagnostics-label-platform").into(),
            "environment.terminal" => msg!("app-diagnostics-label-terminal").into(),
            "environment.startup-permissions" => {
                msg!("app-diagnostics-label-startup-permissions").into()
            }
            "path.config-parent" => msg!("app-diagnostics-label-config-parent").into(),
            "path.data-path" => msg!("app-diagnostics-label-data-path").into(),
            "path.cache-path" => msg!("app-diagnostics-label-cache-path").into(),
            "path.logs-path" => msg!("app-diagnostics-label-logs-path").into(),
            "path.temp-path" => msg!("app-diagnostics-label-temp-path").into(),
            "assets.root" => msg!("app-diagnostics-label-asset-root").into(),
            id if id.starts_with("incident-history.warning-") => {
                msg!("app-diagnostics-label-incident-history").into()
            }
            id if id.starts_with("asset.") => {
                msg!("app-diagnostics-label-asset", key = &id[6..]).into()
            }
            id if id.starts_with("storage.") => storage_kind(id)
                .map(storage_label)
                .unwrap_or_else(|| LocalizedText::Raw(self.label.clone())),
            id if id.starts_with("environment.capability.") => capability_label(&id[23..])
                .unwrap_or_else(|| LocalizedText::Raw(self.label.clone())),
            _ => LocalizedText::Raw(self.label.clone()),
        }
    }

    /// The raw `summary` and `detail` remain available for diagnostic exports.
    /// These concise UI summaries do not inspect or reinterpret their contents.
    pub fn localized_summary(&self) -> LocalizedText {
        let message = match self.id.as_str() {
            "assets.root" => msg!("app-diagnostics-summary-asset-root"),
            id if id.starts_with("incident-history.warning-") => {
                msg!("app-diagnostics-summary-incident-history")
            }
            "environment.terminal" => match self.status {
                DiagnosticStatus::Pass => msg!("app-diagnostics-summary-terminal-pass"),
                DiagnosticStatus::Unsupported => {
                    msg!("app-diagnostics-summary-terminal-unsupported")
                }
                DiagnosticStatus::Warning => msg!("app-diagnostics-summary-terminal-warning"),
                DiagnosticStatus::Fail => msg!("app-diagnostics-summary-terminal-fail"),
            },
            "environment.platform" | "environment.startup-permissions" => match self.status {
                DiagnosticStatus::Pass => msg!("app-diagnostics-summary-environment-pass"),
                DiagnosticStatus::Unsupported => {
                    msg!("app-diagnostics-summary-environment-unsupported")
                }
                DiagnosticStatus::Warning => msg!("app-diagnostics-summary-environment-warning"),
                DiagnosticStatus::Fail => msg!("app-diagnostics-summary-environment-fail"),
            },
            id if id.starts_with("environment.capability.")
                && capability_label(&id[23..]).is_some() =>
            {
                match self.status {
                    DiagnosticStatus::Pass => msg!("app-diagnostics-summary-capability-pass"),
                    DiagnosticStatus::Unsupported => {
                        msg!("app-diagnostics-summary-capability-unsupported")
                    }
                    DiagnosticStatus::Warning => msg!("app-diagnostics-summary-capability-warning"),
                    DiagnosticStatus::Fail => msg!("app-diagnostics-summary-capability-fail"),
                }
            }
            "path.config-parent" | "path.data-path" | "path.cache-path" | "path.logs-path"
            | "path.temp-path" => match self.status {
                DiagnosticStatus::Pass => msg!("app-diagnostics-summary-path-pass"),
                DiagnosticStatus::Warning
                    if matches!(
                        self.repair,
                        Some(DiagnosticsRepairAction::CreateDirectory { .. })
                    ) =>
                {
                    msg!("app-diagnostics-summary-path-missing")
                }
                DiagnosticStatus::Unsupported => msg!("app-diagnostics-summary-path-unsupported"),
                DiagnosticStatus::Warning => msg!("app-diagnostics-summary-path-warning"),
                DiagnosticStatus::Fail => msg!("app-diagnostics-summary-path-fail"),
            },
            id if storage_kind(id).is_some() => match self.status {
                DiagnosticStatus::Pass => msg!("app-diagnostics-summary-storage-pass"),
                DiagnosticStatus::Warning => msg!("app-diagnostics-summary-storage-missing"),
                DiagnosticStatus::Fail
                    if matches!(
                        self.repair,
                        Some(DiagnosticsRepairAction::RepairStorageDocument(_))
                    ) =>
                {
                    msg!("app-diagnostics-summary-storage-corrupt")
                }
                DiagnosticStatus::Fail | DiagnosticStatus::Unsupported => {
                    msg!("app-diagnostics-summary-storage-schema")
                }
            },
            id if id.starts_with("asset.") => match self.status {
                DiagnosticStatus::Pass => msg!("app-diagnostics-summary-asset-pass"),
                _ => msg!("app-diagnostics-summary-asset-warning"),
            },
            _ => return LocalizedText::Raw(self.summary.clone()),
        };
        message.into()
    }

    pub fn localized_remediation(&self) -> Option<LocalizedText> {
        self.remediation.as_ref()?;
        let message = match self.id.as_str() {
            "assets.root" => msg!("app-diagnostics-remedy-asset-root"),
            id if id.starts_with("incident-history.warning-") => {
                msg!("app-diagnostics-remedy-incident-history")
            }
            id if id.starts_with("asset.") => msg!("app-diagnostics-remedy-asset"),
            "environment.terminal" => msg!("app-diagnostics-remedy-terminal"),
            "environment.platform" | "environment.startup-permissions" => match self.status {
                DiagnosticStatus::Fail => msg!("app-diagnostics-remedy-environment-fail"),
                _ => msg!("app-diagnostics-remedy-environment-warning"),
            },
            id if id.starts_with("environment.capability.")
                && capability_label(&id[23..]).is_some() =>
            {
                msg!("app-diagnostics-remedy-environment-warning")
            }
            "path.config-parent" | "path.data-path" | "path.cache-path" | "path.logs-path"
            | "path.temp-path" => {
                if matches!(
                    self.repair,
                    Some(DiagnosticsRepairAction::CreateDirectory { .. })
                ) {
                    msg!("app-diagnostics-remedy-path-create")
                } else {
                    msg!("app-diagnostics-remedy-path-permissions")
                }
            }
            id if storage_kind(id).is_some() => match self.status {
                DiagnosticStatus::Warning => msg!("app-diagnostics-remedy-storage-create"),
                DiagnosticStatus::Fail
                    if matches!(
                        self.repair,
                        Some(DiagnosticsRepairAction::RepairStorageDocument(_))
                    ) =>
                {
                    msg!("app-diagnostics-remedy-storage-rebuild")
                }
                _ => msg!("app-diagnostics-remedy-storage-schema"),
            },
            _ => return self.remediation.clone().map(LocalizedText::Raw),
        };
        Some(message.into())
    }
}

fn storage_kind(id: &str) -> Option<StorageDocumentKind> {
    Some(match id {
        "storage.config" => StorageDocumentKind::Config,
        "storage.users" => StorageDocumentKind::Users,
        "storage.state" => StorageDocumentKind::State,
        "storage.recent-files" => StorageDocumentKind::RecentFiles,
        "storage.sessions" => StorageDocumentKind::Sessions,
        "storage.clock" => StorageDocumentKind::Clock,
        "storage.trash-manifest" => StorageDocumentKind::TrashManifest,
        _ => return None,
    })
}

fn storage_label(kind: StorageDocumentKind) -> LocalizedText {
    match kind {
        StorageDocumentKind::Config => msg!("app-diagnostics-label-storage-config"),
        StorageDocumentKind::Users => msg!("app-diagnostics-label-storage-users"),
        StorageDocumentKind::State => msg!("app-diagnostics-label-storage-state"),
        StorageDocumentKind::RecentFiles => msg!("app-diagnostics-label-storage-recent-files"),
        StorageDocumentKind::Sessions => msg!("app-diagnostics-label-storage-sessions"),
        StorageDocumentKind::Clock => msg!("app-diagnostics-label-storage-clock"),
        StorageDocumentKind::TrashManifest => msg!("app-diagnostics-label-storage-trash-manifest"),
    }
    .into()
}

fn capability_label(key: &str) -> Option<LocalizedText> {
    Some(
        match key {
            "open_path" => msg!("app-diagnostics-capability-open-path"),
            "open_with" => msg!("app-diagnostics-capability-open-with"),
            "open_uri" => msg!("app-diagnostics-capability-open-uri"),
            "spawn_detached" => msg!("app-diagnostics-capability-spawn-detached"),
            "spawn_wait" => msg!("app-diagnostics-capability-spawn-wait"),
            "clipboard_text" => msg!("app-diagnostics-capability-clipboard-text"),
            "user_dirs" => msg!("app-diagnostics-capability-user-dirs"),
            "app_paths" => msg!("app-diagnostics-capability-app-paths"),
            "temp" => msg!("app-diagnostics-capability-temp"),
            "file_attributes" => msg!("app-diagnostics-capability-file-attributes"),
            "directory_listing" => msg!("app-diagnostics-capability-directory-listing"),
            "local_volumes" => msg!("app-diagnostics-capability-local-volumes"),
            "network_status" => msg!("app-diagnostics-capability-network-status"),
            "trash" => msg!("app-diagnostics-capability-trash"),
            "critical_dialog" => msg!("app-diagnostics-capability-critical-dialog"),
            "power" => msg!("app-diagnostics-capability-power"),
            _ => return None,
        }
        .into(),
    )
}

impl DiagnosticsRepairAction {
    pub fn localized_label(&self) -> LocalizedText {
        match self {
            Self::CreateDirectory { path, .. } => msg!(
                "app-diagnostics-action-create-directory",
                path = path.display().to_string()
            ),
            Self::RestoreDefaultThemeFile { file_key, .. } => msg!(
                "app-diagnostics-action-restore-asset",
                key = file_key.clone()
            ),
            Self::RepairStorageDocument(kind) => msg!(
                "app-diagnostics-action-repair-storage",
                document = storage_label(*kind)
            ),
        }
        .into()
    }
}

impl DiagnosticsRepairResult {
    pub fn localized_summary(&self) -> LocalizedText {
        if !self.success {
            return msg!("app-diagnostics-repair-failed").into();
        }
        match &self.action {
            DiagnosticsRepairAction::CreateDirectory { .. } => {
                if self.changed {
                    msg!("app-diagnostics-repair-directory-created")
                } else {
                    msg!("app-diagnostics-repair-directory-ready")
                }
            }
            DiagnosticsRepairAction::RestoreDefaultThemeFile { .. } => {
                if self.changed {
                    msg!("app-diagnostics-repair-asset-restored")
                } else {
                    msg!("app-diagnostics-repair-asset-unchanged")
                }
            }
            DiagnosticsRepairAction::RepairStorageDocument(_) => {
                if !self.changed {
                    msg!("app-diagnostics-repair-storage-healthy")
                } else if self.backup_path.is_some() {
                    msg!("app-diagnostics-repair-storage-rebuilt")
                } else {
                    msg!("app-diagnostics-repair-storage-created")
                }
            }
        }
        .into()
    }
}

impl DiagnosticsTaskError {
    pub fn localized(&self) -> LocalizedError {
        let (code, message) = match self {
            Self::Busy => ("DIAGNOSTICS_BUSY", msg!("app-diagnostics-error-busy")),
            Self::EmptyRepairPlan => (
                "DIAGNOSTICS_EMPTY_REPAIR_PLAN",
                msg!("app-diagnostics-error-empty-plan"),
            ),
            Self::RestartRequired => (
                "DIAGNOSTICS_RESTART_REQUIRED",
                msg!("app-diagnostics-error-restart-required"),
            ),
            Self::WorkerStopped => (
                "DIAGNOSTICS_WORKER_STOPPED",
                msg!("app-diagnostics-error-worker-stopped"),
            ),
        };
        LocalizedError::new(code, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use i18n::LanguageSnapshot;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "tundra-diagnostics-localization-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed),
            ));
            let locale = root.join("locales/zh-CN");
            fs::create_dir_all(locale.join("modules")).unwrap();
            fs::write(
                locale.join("manifest.toml"),
                "format_version = 1\ncode = \"zh-CN\"\nnative_name = \"简体中文\"\n",
            )
            .unwrap();
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../ascii-assets/assets/locales/zh-CN/modules/app-diagnostics.ftl"),
                locale.join("modules/app-diagnostics.ftl"),
            )
            .unwrap();
            Self(root)
        }
        fn chinese(&self) -> LanguageSnapshot {
            LanguageSnapshot::load(&self.0, "zh-CN", 2)
                .unwrap()
                .snapshot
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn check(id: &str, category: DiagnosticCategory, status: DiagnosticStatus) -> DiagnosticCheck {
        DiagnosticCheck {
            id: id.into(),
            category,
            status,
            label: "Unrelated custom diagnostic label".into(),
            summary: "RAW summary {literal}".into(),
            detail: "RAW detail /private/path".into(),
            remediation: Some("RAW guidance {literal}".into()),
            repair: None,
        }
    }

    #[test]
    fn project_summaries_use_ids_and_typed_outcomes_without_changing_raw_diagnostics() {
        let fixture = Fixture::new();
        let english = LanguageSnapshot::embedded(1);
        let chinese = fixture.chinese();
        let mut checks = vec![
            check(
                "environment.terminal",
                DiagnosticCategory::Environment,
                DiagnosticStatus::Warning,
            ),
            check(
                "environment.capability.open_path",
                DiagnosticCategory::Environment,
                DiagnosticStatus::Pass,
            ),
            check(
                "path.data-path",
                DiagnosticCategory::Paths,
                DiagnosticStatus::Warning,
            ),
            check(
                "storage.state",
                DiagnosticCategory::Storage,
                DiagnosticStatus::Fail,
            ),
            check(
                "asset.home_icons/explorer.png",
                DiagnosticCategory::Assets,
                DiagnosticStatus::Warning,
            ),
            check(
                "incident-history.warning-0",
                DiagnosticCategory::Storage,
                DiagnosticStatus::Warning,
            ),
        ];
        checks[2].repair = Some(DiagnosticsRepairAction::CreateDirectory {
            label: "Never match this".into(),
            path: fixture.0.join("missing"),
        });
        checks[3].repair = Some(DiagnosticsRepairAction::RepairStorageDocument(
            StorageDocumentKind::State,
        ));
        for check in &checks {
            let original = check.clone();
            for text in [
                check.localized_label(),
                check.localized_summary(),
                check.localized_remediation().unwrap(),
            ] {
                let en = english.render_text(&text);
                let zh = chinese.render_text(&text);
                assert_ne!(en, zh, "{}", check.id);
                assert!(
                    !en.starts_with('[') && !zh.starts_with('['),
                    "{}: {en} / {zh}",
                    check.id
                );
                assert!(!zh.contains("RAW") && !zh.contains("Unrelated"));
            }
            assert_eq!(*check, original);
        }
        assert_eq!(chinese.render_text(&checks[0].localized_label()), "终端");
        assert_eq!(
            chinese.render_text(&checks[3].localized_summary()),
            "存储文档已损坏"
        );
        checks[3].repair = None;
        assert_eq!(
            chinese.render_text(&checks[3].localized_summary()),
            "不支持此存储文档格式版本"
        );
        checks[0].remediation = None;
        assert!(checks[0].localized_remediation().is_none());
    }

    #[test]
    fn unknown_checks_and_raw_details_are_preserved_verbatim() {
        let fixture = Fixture::new();
        let chinese = fixture.chinese();
        let unknown = check(
            "external.plugin-check",
            DiagnosticCategory::Environment,
            DiagnosticStatus::Fail,
        );
        assert_eq!(
            chinese.render_text(&unknown.localized_label()),
            unknown.label
        );
        assert_eq!(
            chinese.render_text(&unknown.localized_summary()),
            unknown.summary
        );
        assert_eq!(
            chinese.render_text(&unknown.localized_remediation().unwrap()),
            unknown.remediation.as_ref().unwrap().as_str()
        );
    }

    #[test]
    fn repairs_retain_translatable_actions_and_keep_failure_details_raw() {
        let fixture = Fixture::new();
        let chinese = fixture.chinese();
        let action = DiagnosticsRepairAction::RepairStorageDocument(StorageDocumentKind::State);
        let original_label = action.label();
        assert_eq!(
            chinese.render_text(&action.localized_label()),
            "修复状态存储文档"
        );
        assert_eq!(action.label(), original_label);
        let mut result = DiagnosticsRepairResult {
            action,
            success: true,
            changed: true,
            message: "raw repair result".into(),
            backup_path: Some(fixture.0.join("backup")),
        };
        assert_eq!(
            chinese.render_text(&result.localized_summary()),
            "已备份并重建存储文档"
        );
        result.success = false;
        assert_eq!(
            chinese.render_text(&result.localized_summary()),
            "修复失败，请查看诊断详情"
        );
        assert_eq!(result.message, "raw repair result");
        assert_eq!(
            DiagnosticsTaskError::Busy.localized().event_code,
            "DIAGNOSTICS_BUSY"
        );
    }

    #[test]
    fn platform_ids_are_stable_even_when_path_roles_alias_and_labels_change() {
        let fixture = Fixture::new();
        let shared = fixture.0.join("shared");
        fs::create_dir_all(&shared).unwrap();
        let paths = AppPaths::from_parts(
            shared.join("config.toml"),
            shared.clone(),
            shared.clone(),
            shared.clone(),
            shared.clone(),
        )
        .unwrap();
        let dirs = platform::UserDirs::new(
            shared.clone(),
            shared.clone(),
            shared.clone(),
            shared.clone(),
            shared.clone(),
            shared.clone(),
            shared.clone(),
        )
        .unwrap();
        let platform =
            platform::mock::MockPlatform::new(dirs, paths).with_kind(platform::PlatformKind::Macos);
        let report = platform::run_doctor_with(&platform).unwrap();
        let ids = report
            .path_checks
            .iter()
            .map(|check| check.id)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
                "config-parent",
                "data-path",
                "cache-path",
                "logs-path",
                "temp-path"
            ]
        );
        assert!(report.path_checks.iter().all(|check| check.path == shared));
        let env_ids = report
            .environment_checks
            .iter()
            .map(|check| check.id)
            .collect::<HashSet<_>>();
        assert_eq!(env_ids.len(), report.environment_checks.len());
        assert!(
            env_ids.contains("platform")
                && env_ids.contains("terminal")
                && env_ids.contains("startup-permissions")
        );
        for (key, status) in platform.capabilities().checks() {
            let mut check = platform::EnvironmentCheck::capability(key, status);
            assert_eq!(check.id, format!("capability.{key}"));
            assert_eq!(check.label, format!("Capability: {key}"));
            assert_eq!(check.message, status.as_str());
            check.label = "文案发生变化".into();
            assert_eq!(check.id, format!("capability.{key}"));
            let domain = super::super::environment_diagnostic(check);
            assert!(matches!(
                domain.localized_label(),
                LocalizedText::Message(_)
            ));
        }
    }
}
