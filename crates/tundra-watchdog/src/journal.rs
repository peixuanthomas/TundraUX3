use crate::durable;
use crate::runtime::{self, AppWatchdog, OperationContext, Shared};
use crate::sanitize;
use crate::{
    AppId, OperationCheckpoint, OperationDescriptor, OperationKind, OperationRecord,
    OperationStatus, RecoveryHandler, RecoveryOutcome, ReplaySafety, WatchdogError,
};
use chrono::Utc;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct OperationGuard {
    record: OperationRecord,
    path: PathBuf,
    committed: bool,
    shared: Arc<Shared>,
    active_key: (AppId, String),
    incident_id: Option<String>,
}

pub(crate) struct RecoverySweep {
    outcomes: Vec<RecoveryOutcome>,
    examined: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RecoveryScope {
    ConfirmedStaleRuns,
    CurrentRunInterrupted,
}

impl RecoverySweep {
    pub(crate) fn restart_outcome(self) -> RecoveryOutcome {
        if self.outcomes.is_empty() {
            return RecoveryOutcome::Recovered(
                "no pending operation journal required reconciliation".to_string(),
            );
        }
        if let Some(outcome) = self.outcomes.iter().find(|outcome| {
            matches!(
                outcome,
                RecoveryOutcome::Pending
                    | RecoveryOutcome::ManualActionRequired(_)
                    | RecoveryOutcome::Unrecoverable(_)
            )
        }) {
            return match outcome {
                RecoveryOutcome::Pending => RecoveryOutcome::ManualActionRequired(
                    "operation recovery is still pending".to_string(),
                ),
                other => other.clone(),
            };
        }
        if self
            .outcomes
            .iter()
            .any(|outcome| matches!(outcome, RecoveryOutcome::RecoveredWithWarnings(_)))
        {
            RecoveryOutcome::RecoveredWithWarnings(
                "pending operations were reconciled with warnings".to_string(),
            )
        } else {
            RecoveryOutcome::Recovered("pending operations were reconciled".to_string())
        }
    }

    pub(crate) fn examined(&self) -> usize {
        self.examined
    }
}

pub(crate) fn begin_operation(
    app: &AppWatchdog,
    mut descriptor: OperationDescriptor,
) -> Result<OperationGuard, WatchdogError> {
    if descriptor.id.is_empty() {
        descriptor.id = app.process.next_operation_id();
    } else if !is_safe_operation_id(&descriptor.id) {
        return Err(WatchdogError::InvalidIdentifier(descriptor.id));
    }
    if let Some(outcome) = app
        .process
        .shared
        .recovery_status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&(app.descriptor.id.clone(), descriptor.kind.clone()))
        .cloned()
        && !outcome.is_recovered()
    {
        return Err(WatchdogError::RecoveryBlocked(format!(
            "operation {} is blocked until recovery is resolved: {outcome:?}",
            descriptor.kind
        )));
    }
    let recovery_handler_version = app
        .process
        .shared
        .recovery_handlers
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&(app.descriptor.id.clone(), descriptor.kind.clone()))
        .map(|handler| handler.version().to_string());
    let now = Utc::now();
    let original_payload = sanitize::json(&descriptor.payload);
    let record = OperationRecord {
        schema_version: 1,
        run_id: app.process.shared.run_id.clone(),
        app_id: app.descriptor.id.clone(),
        component: app.component.clone(),
        operation_id: descriptor.id,
        replay_safety: ReplaySafety::Checkpointed(descriptor.kind.clone()),
        recovery_handler_version,
        kind: descriptor.kind,
        summary: sanitize::text(descriptor.summary),
        original_payload: original_payload.clone(),
        checkpoint_sequence: 0,
        checkpoint: OperationCheckpoint::new("planned", original_payload),
        status: OperationStatus::Active,
        started_at: now,
        updated_at: now,
    };
    let path = operation_path(app, &record.operation_id);
    create_record(&path, &record)?;
    let active_key = (record.app_id.clone(), record.operation_id.clone());
    app.process
        .shared
        .active_operations
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(active_key.clone());
    let incident_id = runtime::current_execution_context().map(|context| context.incident_id);
    if let Some(incident_id) = &incident_id {
        app.process
            .shared
            .operation_contexts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(incident_id.clone())
            .or_default()
            .push(OperationContext {
                operation_id: record.operation_id.clone(),
                recovery_handler_version: record.recovery_handler_version.clone(),
            });
    }
    Ok(OperationGuard {
        record,
        path,
        committed: false,
        shared: app.process.shared.clone(),
        active_key,
        incident_id,
    })
}

impl OperationGuard {
    pub fn operation_id(&self) -> &str {
        &self.record.operation_id
    }

    pub fn record(&self) -> &OperationRecord {
        &self.record
    }

    pub fn checkpoint(&mut self, checkpoint: OperationCheckpoint) -> Result<(), WatchdogError> {
        self.record.checkpoint_sequence = self.record.checkpoint_sequence.saturating_add(1);
        self.record.checkpoint = OperationCheckpoint::new(
            sanitize::text(checkpoint.phase),
            sanitize::json(&checkpoint.payload),
        );
        self.record.updated_at = Utc::now();
        write_record(&self.path, &self.record)
    }

    pub fn commit(mut self, detail: impl Into<String>) -> Result<(), WatchdogError> {
        self.record.status = OperationStatus::Committed;
        self.record.checkpoint_sequence = self.record.checkpoint_sequence.saturating_add(1);
        self.record.checkpoint = OperationCheckpoint::new(
            "committed",
            serde_json::json!({ "detail": sanitize::text(detail.into()) }),
        );
        self.record.updated_at = Utc::now();
        write_record(&self.path, &self.record)?;
        self.committed = true;
        self.unregister(false);
        durable::remove_file(&self.path).map_err(|source| WatchdogError::Io {
            operation: "remove committed operation journal",
            path: self.path.clone(),
            source,
        })?;
        Ok(())
    }

    fn unregister(&self, keep_incident_context: bool) {
        self.shared
            .active_operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.active_key);
        if keep_incident_context {
            return;
        }
        let Some(incident_id) = &self.incident_id else {
            return;
        };
        let mut contexts = self
            .shared
            .operation_contexts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(operations) = contexts.get_mut(incident_id) {
            operations.retain(|operation| operation.operation_id != self.record.operation_id);
            if operations.is_empty() {
                contexts.remove(incident_id);
            }
        }
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.record.status = OperationStatus::Interrupted;
        self.record.updated_at = Utc::now();
        let _ = write_record(&self.path, &self.record);
        self.unregister(std::thread::panicking());
    }
}

pub(crate) fn recover_pending(
    app: &AppWatchdog,
    operation_kind: &OperationKind,
    handler: Arc<dyn RecoveryHandler>,
    scope: RecoveryScope,
) -> Result<RecoverySweep, WatchdogError> {
    let _gate = app
        .process
        .shared
        .recovery_gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let directory = operations_directory(app);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RecoverySweep {
                outcomes: Vec::new(),
                examined: 0,
            });
        }
        Err(source) => {
            return Err(WatchdogError::Io {
                operation: "read operation journal directory",
                path: directory,
                source,
            });
        }
    };
    let mut outcomes = Vec::new();
    let mut examined = 0_usize;
    for entry in entries {
        let entry = entry.map_err(|source| WatchdogError::Io {
            operation: "read operation journal entry",
            path: directory.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path).map_err(|source| WatchdogError::Io {
            operation: "read operation journal",
            path: path.clone(),
            source,
        })?;
        let record = serde_json::from_slice::<OperationRecord>(&bytes)?;
        if record.app_id != app.descriptor.id {
            return Err(WatchdogError::InvalidTaskPolicy(format!(
                "operation journal {} belongs to app {}, not {}",
                path.display(),
                record.app_id,
                app.descriptor.id
            )));
        }
        if &record.kind != operation_kind {
            continue;
        }
        let eligible = match scope {
            RecoveryScope::ConfirmedStaleRuns => {
                record.run_id != app.process.shared.run_id
                    && app
                        .process
                        .shared
                        .confirmed_stale_runs
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .contains(&record.run_id)
            }
            RecoveryScope::CurrentRunInterrupted => {
                record.run_id == app.process.shared.run_id
                    && record.status == OperationStatus::Interrupted
                    && !app
                        .process
                        .shared
                        .active_operations
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .contains(&(record.app_id.clone(), record.operation_id.clone()))
            }
        };
        if !eligible {
            continue;
        }
        examined = examined.saturating_add(1);
        if record.status == OperationStatus::Committed {
            remove_recovered_journal(&path)?;
            continue;
        }
        if record
            .recovery_handler_version
            .as_deref()
            .is_some_and(|version| version != handler.version())
        {
            outcomes.push(RecoveryOutcome::ManualActionRequired(format!(
                "operation {} requires recovery handler version {}, but version {} is registered",
                record.operation_id,
                record
                    .recovery_handler_version
                    .as_deref()
                    .unwrap_or("unknown"),
                handler.version()
            )));
            continue;
        }
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| handler.recover(&record)))
            .unwrap_or_else(|_| {
                RecoveryOutcome::Unrecoverable(format!(
                    "recovery handler panicked for operation {}",
                    record.operation_id
                ))
            });
        let outcome = sanitize::recovery(outcome);
        match &outcome {
            RecoveryOutcome::Recovered(_) | RecoveryOutcome::RecoveredWithWarnings(_) => {
                remove_recovered_journal(&path)?;
            }
            RecoveryOutcome::Pending
            | RecoveryOutcome::ManualActionRequired(_)
            | RecoveryOutcome::Unrecoverable(_) => {}
        }
        outcomes.push(outcome);
    }
    Ok(RecoverySweep { outcomes, examined })
}

fn remove_recovered_journal(path: &Path) -> Result<(), WatchdogError> {
    durable::remove_file(path).map_err(|source| WatchdogError::Io {
        operation: "remove recovered operation journal",
        path: path.to_path_buf(),
        source,
    })
}

fn is_safe_operation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn operation_path(app: &AppWatchdog, operation_id: &str) -> PathBuf {
    operations_directory(app).join(format!("{operation_id}.json"))
}

fn operations_directory(app: &AppWatchdog) -> PathBuf {
    app.process
        .shared
        .config
        .data_dir
        .join("watchdog")
        .join("operations")
        .join(app.descriptor.id.as_str())
}

fn write_record(path: &Path, record: &OperationRecord) -> Result<(), WatchdogError> {
    let bytes = serde_json::to_vec_pretty(record)?;
    durable::atomic_write(path, &bytes).map_err(|source| WatchdogError::Io {
        operation: "atomically replace operation journal",
        path: path.to_path_buf(),
        source,
    })
}

fn create_record(path: &Path, record: &OperationRecord) -> Result<(), WatchdogError> {
    let bytes = serde_json::to_vec_pretty(record)?;
    durable::atomic_create_new(path, &bytes).map_err(|source| {
        if source.kind() == std::io::ErrorKind::AlreadyExists {
            WatchdogError::OperationAlreadyExists(record.operation_id.clone())
        } else {
            WatchdogError::Io {
                operation: "create operation journal without replacement",
                path: path.to_path_buf(),
                source,
            }
        }
    })
}
