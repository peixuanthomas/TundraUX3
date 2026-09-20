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
#[path = "../../tests/unit/diagnostics/localization/tests.rs"]
mod tests;
