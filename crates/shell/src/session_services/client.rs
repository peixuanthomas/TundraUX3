use super::SessionProfile;
use crate::process_host::WorkerConnection;
use app::explorer::{
    ExplorerError, ExplorerSharedOperations, ExplorerSharedRequest, ExplorerSharedSnapshot,
};
use platform::shell_control::LocalConnection;
use serde::{Serialize, de::DeserializeOwned};
use std::io;
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::time::{Duration, Instant};
use workspace_protocol::transport::Framed;
use workspace_protocol::{
    ServiceClientMessage, ServiceError, ServiceHostMessage, ServiceOperation, ServiceRequest,
    ServiceValue, WorkerId,
};

static CLIENT: OnceLock<Arc<SessionServicesClient>> = OnceLock::new();
const REQUEST_TIMEOUT: Duration = Duration::from_millis(200);

struct ClientWire {
    framed: Framed<LocalConnection>,
    next_id: u64,
}

#[derive(Default)]
pub(crate) struct SyncState {
    pub auth: Option<(u64, Option<identity::AuthSession>)>,
    pub profile: Option<(u64, SessionProfile)>,
    pub system_revision: u64,
    pub published_system_revision: Option<u64>,
    pub failed: bool,
    pub system_active: bool,
}

pub(crate) struct SessionServicesClient {
    worker: WorkerId,
    wire: Mutex<ClientWire>,
    pub sync: Mutex<SyncState>,
}

impl std::fmt::Debug for SessionServicesClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionServicesClient")
            .field("worker", &self.worker)
            .finish_non_exhaustive()
    }
}

impl SessionServicesClient {
    pub(crate) fn connect(connection: &'static WorkerConnection) -> io::Result<Arc<Self>> {
        let client = Arc::new(Self {
            worker: connection.worker(),
            wire: Mutex::new(ClientWire {
                framed: connection.connect_services()?,
                next_id: 0,
            }),
            sync: Mutex::new(SyncState::default()),
        });
        client.read_config()?;
        Ok(client)
    }

    pub(crate) fn install(self: &Arc<Self>) {
        if let Ok(paths) = platform::native_platform().app_paths() {
            storage::install_session_config_store(
                storage::StorageLayout::from_app_paths(&paths).config_path,
                self.clone(),
            );
        }
        app::explorer::install_shared_explorer_operations(self.clone());
        let _ = CLIENT.set(self.clone());
    }

    pub(crate) fn current() -> Option<&'static Arc<Self>> {
        CLIENT.get()
    }

    pub(crate) fn worker(&self) -> WorkerId {
        self.worker
    }

    pub(crate) fn request(&self, operation: ServiceOperation) -> io::Result<ServiceValue> {
        let started = Instant::now();
        let mut wire = loop {
            match self.wire.try_lock() {
                Ok(wire) => break wire,
                Err(TryLockError::Poisoned(_)) => {
                    return Err(service_error(ServiceError::Unavailable));
                }
                Err(TryLockError::WouldBlock) if started.elapsed() < REQUEST_TIMEOUT => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(service_error(ServiceError::Unavailable));
                }
            }
        };
        wire.next_id += 1;
        let id = wire.next_id;
        wire.framed
            .queue(&ServiceClientMessage::Request(ServiceRequest {
                id,
                operation,
            }))?;
        loop {
            for message in wire.framed.poll::<ServiceHostMessage>()? {
                if let ServiceHostMessage::Reply(reply) = message
                    && reply.id == id
                {
                    return reply.result.map_err(service_error);
                }
            }
            if started.elapsed() >= REQUEST_TIMEOUT {
                return Err(service_error(ServiceError::Unavailable));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub(crate) fn read<T: DeserializeOwned>(
        &self,
        operation: ServiceOperation,
    ) -> io::Result<(u64, T)> {
        match self.request(operation)? {
            ServiceValue::Versioned { revision, value } => serde_json::from_value(value)
                .map(|value| (revision, value))
                .map_err(io::Error::other),
            ServiceValue::Empty => Err(io::Error::other("missing workspace service value")),
        }
    }

    pub(crate) fn encode(value: &impl Serialize) -> io::Result<serde_json::Value> {
        serde_json::to_value(value).map_err(io::Error::other)
    }

    pub(crate) fn read_config(&self) -> io::Result<(u64, storage::StorageConfig)> {
        let (revision, mut config): (_, storage::StorageConfig) =
            self.read(ServiceOperation::ReadConfig)?;
        config.session_revision = Some(revision);
        Ok((revision, config))
    }

    pub(crate) fn request_system_status(
        &self,
        active: Option<bool>,
        refresh: bool,
    ) -> io::Result<()> {
        self.request(ServiceOperation::RequestSystemStatus { active, refresh })
            .map(|_| ())
    }
}

fn service_error(error: ServiceError) -> io::Error {
    let message = match error {
        ServiceError::Conflict => i18n::render_current(&i18n::msg!("workspace-services-conflict")),
        ServiceError::Forbidden => {
            i18n::render_current(&i18n::msg!("workspace-services-forbidden"))
        }
        ServiceError::OperationFailed { message } => serde_json::from_value(message)
            .map(|message| i18n::render_current(&message))
            .unwrap_or_else(|_| {
                i18n::render_current(&i18n::msg!("workspace-services-unavailable"))
            }),
        ServiceError::InvalidRequest | ServiceError::Unavailable => {
            i18n::render_current(&i18n::msg!("workspace-services-unavailable"))
        }
    };
    io::Error::other(message)
}

impl storage::SessionConfigStore for SessionServicesClient {
    fn load_config(&self) -> Result<storage::StorageConfig, storage::StorageError> {
        self.read_config()
            .map(|(_, config)| config)
            .map_err(|error| storage::StorageError::Platform {
                message: error.to_string(),
            })
    }

    fn save_config(&self, config: &storage::StorageConfig) -> Result<(), storage::StorageError> {
        let save = || -> io::Result<()> {
            let revision = config
                .session_revision
                .ok_or_else(|| io::Error::other("configuration was not read"))?;
            self.request(ServiceOperation::WriteConfig {
                revision,
                config: Self::encode(config)?,
            })?;
            Ok(())
        };
        save().map_err(|error| storage::StorageError::Platform {
            message: error.to_string(),
        })
    }
}

impl ExplorerSharedOperations for SessionServicesClient {
    fn execute(
        &self,
        request: ExplorerSharedRequest,
    ) -> Result<ExplorerSharedSnapshot, ExplorerError> {
        let read = || -> io::Result<ExplorerSharedSnapshot> {
            let operation = if matches!(request, ExplorerSharedRequest::Snapshot) {
                ServiceOperation::ReadFileOperations
            } else {
                ServiceOperation::UpdateFileOperations {
                    operation: Self::encode(&request)?,
                }
            };
            self.read(operation).map(|(_, snapshot)| snapshot)
        };
        read().map_err(|error| ExplorerError::InvalidOperation(error.to_string()))
    }
}
