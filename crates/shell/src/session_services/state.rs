use super::{SessionProfile, SystemStatusRequests, clock::ServerClockState};
use app::explorer::{ExplorerFileService, ExplorerSharedRequest};
use identity::{
    AuthSession, CoreError, IdentityBackend, IdentitySource, PermissionAction, PermissionService,
    UserRole, UserService,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use storage::{StorageConfig, StorageManager};
use workspace_protocol::{ServiceError, ServiceOperation, ServiceValue, WorkerId, WorkerRole};

pub(super) struct ServiceState {
    storage: StorageManager,
    config: (u64, StorageConfig),
    authorization: (u64, Option<AuthSession>),
    profile: (u64, SessionProfile),
    clock: ServerClockState,
    system: (u64, Value),
    files: ExplorerFileService,
    files_revision: u64,
    busy: HashSet<WorkerId>,
    system_active: HashSet<WorkerId>,
    system_refresh: bool,
    identities: HashMap<WorkerId, Option<String>>,
}

fn value(revision: u64, value: impl serde::Serialize) -> Result<ServiceValue, ServiceError> {
    serde_json::to_value(value)
        .map(|value| ServiceValue::Versioned { revision, value })
        .map_err(|_| ServiceError::InvalidRequest)
}

impl ServiceState {
    pub(super) fn new(storage: StorageManager) -> Result<Self, storage::StorageError> {
        Ok(Self {
            config: (0, storage.load_config()?),
            storage,
            authorization: (0, None),
            profile: (0, SessionProfile::default()),
            clock: ServerClockState::default(),
            system: (0, Value::Null),
            files: ExplorerFileService::default(),
            files_revision: 0,
            busy: HashSet::new(),
            system_active: HashSet::new(),
            system_refresh: false,
            identities: HashMap::new(),
        })
    }

    pub(super) fn unregister(&mut self, worker: WorkerId) {
        self.identities.remove(&worker);
        self.busy.remove(&worker);
        self.system_active.remove(&worker);
        let _ = self.files.set_operation_busy(!self.busy.is_empty());
    }

    pub(super) fn register(&mut self, worker: WorkerId) {
        self.identities.entry(worker).or_insert_with(|| {
            self.authorization
                .1
                .as_ref()
                .map(|session| session.session_id.clone())
        });
    }

    pub(super) fn execute(
        &mut self,
        worker: WorkerId,
        operation: ServiceOperation,
        platform: &dyn platform::Platform,
    ) -> Result<ServiceValue, ServiceError> {
        let Some(registered_session) = self.identities.get(&worker) else {
            return Err(ServiceError::Forbidden);
        };
        let shell = worker.role == WorkerRole::Shell;
        let authorization = self.authorization.1.as_ref();
        if !shell {
            if registered_session.as_deref() != authorization.map(|auth| auth.session_id.as_str()) {
                return if matches!(operation, ServiceOperation::ReadAuthorization) {
                    value(self.authorization.0, Option::<AuthSession>::None)
                } else {
                    Err(ServiceError::Forbidden)
                };
            }
            if authorization.is_none_or(|auth| auth.role == UserRole::Guest)
                && !matches!(
                    operation,
                    ServiceOperation::ReadAuthorization
                        | ServiceOperation::ReadConfig
                        | ServiceOperation::ReadClockTime
                )
            {
                return Err(ServiceError::Forbidden);
            }
        }
        match operation {
            ServiceOperation::ReadConfig => value(self.config.0, &self.config.1),
            ServiceOperation::WriteConfig { revision, config } => {
                if !shell
                    && !PermissionService::default()
                        .authorize(
                            self.authorization.1.as_ref(),
                            PermissionAction::ChangeSettings,
                            None,
                        )
                        .allowed
                {
                    return Err(ServiceError::Forbidden);
                }
                if revision != self.config.0 {
                    return Err(ServiceError::Conflict);
                }
                let mut config: StorageConfig =
                    serde_json::from_value(config).map_err(|_| ServiceError::InvalidRequest)?;
                config.normalize();
                self.storage
                    .save_config(&config)
                    .map_err(|_| ServiceError::Unavailable)?;
                self.config = (self.config.0 + 1, config);
                value(self.config.0, &self.config.1)
            }
            ServiceOperation::ReadAuthorization => {
                value(self.authorization.0, &self.authorization.1)
            }
            ServiceOperation::WriteAuthorization { revision, policy } => {
                if !shell {
                    return Err(ServiceError::Forbidden);
                }
                if revision != self.authorization.0 {
                    return Err(ServiceError::Conflict);
                }
                let next: Option<AuthSession> =
                    serde_json::from_value(policy).map_err(|_| ServiceError::InvalidRequest)?;
                if next.as_ref().map(|s| &s.session_id) != authorization.map(|s| &s.session_id) {
                    self.files = ExplorerFileService::default();
                    self.files_revision += 1;
                    self.busy.clear();
                    self.profile = (self.profile.0 + 1, SessionProfile::default());
                    self.system_active.clear();
                    self.system_refresh = false;
                }
                self.authorization = (self.authorization.0 + 1, next);
                value(self.authorization.0, &self.authorization.1)
            }
            ServiceOperation::ReadProfile => value(self.profile.0, &self.profile.1),
            ServiceOperation::WriteProfile { revision, profile } => {
                let actor = authorization.ok_or(ServiceError::Forbidden)?;
                if revision != self.profile.0 {
                    return Err(ServiceError::Conflict);
                }
                let profile: SessionProfile =
                    serde_json::from_value(profile).map_err(|_| ServiceError::InvalidRequest)?;
                let profile = if shell
                    && self.profile.1 == SessionProfile::default()
                    && profile.appearance.is_some()
                    && profile.dashboard.is_some()
                {
                    // Shell has already loaded the account profile at login.
                    profile
                } else {
                    let backend = match actor.source {
                        IdentitySource::LocalAccount => IdentityBackend::Local,
                        IdentitySource::LinuxCurrentProcess => IdentityBackend::Linux,
                    };
                    let users = UserService::new(self.storage.clone()).with_backend(backend);
                    let account = match (profile.appearance, profile.dashboard) {
                        (Some(appearance), None) => {
                            users.update_user_appearance(actor, &actor.username, appearance)
                        }
                        (None, Some(dashboard)) => users.update_user_system_status_dashboard(
                            actor,
                            &actor.username,
                            dashboard,
                        ),
                        _ => return Err(ServiceError::InvalidRequest),
                    }
                    .map_err(|error| match error {
                        CoreError::PermissionDenied { .. }
                        | CoreError::UserNotFound
                        | CoreError::AccountDisabled => ServiceError::Forbidden,
                        _ => ServiceError::Unavailable,
                    })?;
                    SessionProfile {
                        appearance: Some(account.appearance),
                        dashboard: Some(account.system_status_dashboard),
                    }
                };
                self.profile = (self.profile.0 + 1, profile);
                value(self.profile.0, &self.profile.1)
            }
            operation @ (ServiceOperation::ReadClockProfile
            | ServiceOperation::WriteClockProfile { .. }
            | ServiceOperation::DeleteClockProfile { .. }
            | ServiceOperation::ReadClockTime
            | ServiceOperation::WriteClockTime { .. }) => {
                self.clock
                    .execute(operation, shell, authorization, &self.storage)
            }
            ServiceOperation::ReadFileOperations => {
                value(self.files_revision, self.files.shared_snapshot())
            }
            ServiceOperation::UpdateFileOperations { operation } => {
                let mut operation: ExplorerSharedRequest =
                    serde_json::from_value(operation).map_err(|_| ServiceError::InvalidRequest)?;
                if !matches!(operation, ExplorerSharedRequest::Snapshot)
                    && !PermissionService::default()
                        .authorize(
                            self.authorization.1.as_ref(),
                            PermissionAction::ReadFile,
                            None,
                        )
                        .allowed
                {
                    return Err(ServiceError::Forbidden);
                }
                if let ExplorerSharedRequest::SetBusy(busy) = operation {
                    if busy {
                        self.busy.insert(worker);
                    } else {
                        self.busy.remove(&worker);
                    }
                    operation = ExplorerSharedRequest::SetBusy(!self.busy.is_empty());
                }
                let snapshot = self
                    .files
                    .apply_shared_request(operation, self.authorization.1.as_ref(), platform)
                    .map_err(|error| ServiceError::OperationFailed {
                        message: serde_json::to_value(error.localized().message)
                            .expect("localized error serializes"),
                    })?;
                self.files_revision += 1;
                value(self.files_revision, snapshot)
            }
            ServiceOperation::ReadSystemSnapshot => value(self.system.0, &self.system.1),
            ServiceOperation::WriteSystemSnapshot { snapshot } => {
                if !shell {
                    return Err(ServiceError::Forbidden);
                }
                self.system = (self.system.0 + 1, snapshot);
                Ok(ServiceValue::Empty)
            }
            ServiceOperation::RequestSystemStatus { active, refresh } => {
                if let Some(active) = active {
                    if active {
                        self.system_active.insert(worker);
                    } else {
                        self.system_active.remove(&worker);
                    }
                }
                self.system_refresh |= refresh;
                Ok(ServiceValue::Empty)
            }
            ServiceOperation::ReadSystemStatusRequests => {
                if !shell {
                    return Err(ServiceError::Forbidden);
                }
                let requests = SystemStatusRequests {
                    active: !self.system_active.is_empty(),
                    refresh: std::mem::take(&mut self.system_refresh),
                };
                value(0, requests)
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session_services.rs"]
mod tests;
