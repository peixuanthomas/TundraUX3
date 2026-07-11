use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tundra_storage::StorageManager;

use crate::authorization::{PermissionAction, PermissionService};
use crate::error::CoreError;
use crate::identity::AuthSession;
use crate::time::unix_millis;

pub const AUDIT_SCHEMA_VERSION: u32 = 1;
pub const AUDIT_GENESIS_HASH: &str = "GENESIS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
}

impl AuditOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::Failure => "Failure",
            Self::Denied => "Denied",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditRecord {
    pub schema_version: u32,
    pub sequence: u64,
    pub timestamp_epoch_ms: u64,
    pub actor_user_id: Option<String>,
    pub actor_username: Option<String>,
    pub session_id: Option<String>,
    pub action: String,
    pub resource: Option<String>,
    pub outcome: String,
    pub reason: Option<String>,
    pub previous_hash: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize)]
struct AuditHashPayload {
    schema_version: u32,
    sequence: u64,
    timestamp_epoch_ms: u64,
    actor_user_id: Option<String>,
    actor_username: Option<String>,
    session_id: Option<String>,
    action: String,
    resource: Option<String>,
    outcome: String,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AuditService {
    storage: StorageManager,
    permission_service: PermissionService,
}

impl AuditService {
    pub fn new(storage: StorageManager) -> Self {
        Self {
            storage,
            permission_service: PermissionService::default(),
        }
    }

    pub fn with_permission_service(
        storage: StorageManager,
        permission_service: PermissionService,
    ) -> Self {
        Self {
            storage,
            permission_service,
        }
    }

    pub fn record(
        &self,
        actor: Option<&AuthSession>,
        action: PermissionAction,
        resource: Option<&str>,
        outcome: AuditOutcome,
        reason: Option<&str>,
    ) -> Result<AuditRecord, CoreError> {
        let existing = self.parse_raw_records()?;
        let previous_hash = existing
            .last()
            .map(|record| record.hash.clone())
            .unwrap_or_else(|| AUDIT_GENESIS_HASH.to_string());
        let sequence = existing.len() as u64 + 1;
        let timestamp_epoch_ms = unix_millis();
        let payload = AuditHashPayload {
            schema_version: AUDIT_SCHEMA_VERSION,
            sequence,
            timestamp_epoch_ms,
            actor_user_id: actor.map(|session| session.user_id.clone()),
            actor_username: actor.map(|session| session.username.clone()),
            session_id: actor.map(|session| session.session_id.clone()),
            action: action.as_str().to_string(),
            resource: resource.map(ToOwned::to_owned),
            outcome: outcome.as_str().to_string(),
            reason: reason.map(ToOwned::to_owned),
        };
        let hash = hash_audit_payload(&payload, &previous_hash)?;
        let record = AuditRecord {
            schema_version: payload.schema_version,
            sequence: payload.sequence,
            timestamp_epoch_ms: payload.timestamp_epoch_ms,
            actor_user_id: payload.actor_user_id,
            actor_username: payload.actor_username,
            session_id: payload.session_id,
            action: payload.action,
            resource: payload.resource,
            outcome: payload.outcome,
            reason: payload.reason,
            previous_hash,
            hash,
        };
        let line = serde_json::to_string(&record)?;
        self.storage.append_audit_line(&line)?;
        Ok(record)
    }

    pub fn read_records(&self, actor: &AuthSession) -> Result<Vec<AuditRecord>, CoreError> {
        let permission =
            self.permission_service
                .authorize(Some(actor), PermissionAction::ViewAuditLog, None);
        if !permission.allowed {
            let reason = permission
                .reason
                .unwrap_or_else(|| "permission_denied".to_string());
            self.record(
                Some(actor),
                PermissionAction::ViewAuditLog,
                None,
                AuditOutcome::Denied,
                Some(&reason),
            )?;
            return Err(CoreError::PermissionDenied {
                action: PermissionAction::ViewAuditLog,
                reason,
            });
        }

        self.parse_raw_records()
    }

    pub fn verify_chain(&self) -> Result<(), CoreError> {
        let records = self.parse_raw_records()?;
        let mut previous_hash = AUDIT_GENESIS_HASH.to_string();
        for (index, record) in records.iter().enumerate() {
            let expected_sequence = index as u64 + 1;
            if record.sequence != expected_sequence {
                return Err(CoreError::AuditIntegrity(format!(
                    "expected sequence {expected_sequence}, found {}",
                    record.sequence
                )));
            }
            if record.previous_hash != previous_hash {
                return Err(CoreError::AuditIntegrity(format!(
                    "sequence {} has mismatched previous_hash",
                    record.sequence
                )));
            }
            let payload = AuditHashPayload {
                schema_version: record.schema_version,
                sequence: record.sequence,
                timestamp_epoch_ms: record.timestamp_epoch_ms,
                actor_user_id: record.actor_user_id.clone(),
                actor_username: record.actor_username.clone(),
                session_id: record.session_id.clone(),
                action: record.action.clone(),
                resource: record.resource.clone(),
                outcome: record.outcome.clone(),
                reason: record.reason.clone(),
            };
            let expected_hash = hash_audit_payload(&payload, &record.previous_hash)?;
            if record.hash != expected_hash {
                return Err(CoreError::AuditIntegrity(format!(
                    "sequence {} has invalid hash",
                    record.sequence
                )));
            }
            previous_hash = record.hash.clone();
        }
        Ok(())
    }

    fn parse_raw_records(&self) -> Result<Vec<AuditRecord>, CoreError> {
        self.storage
            .read_audit_lines()?
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                serde_json::from_str::<AuditRecord>(&line).map_err(|error| {
                    CoreError::AuditIntegrity(format!("line {} is invalid: {error}", index + 1))
                })
            })
            .collect()
    }
}

fn hash_audit_payload(
    payload: &AuditHashPayload,
    previous_hash: &str,
) -> Result<String, CoreError> {
    let canonical = serde_json::to_string(payload)?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hasher.update(previous_hash.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}
