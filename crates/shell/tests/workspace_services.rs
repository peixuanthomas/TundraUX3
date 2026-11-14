#![cfg(unix)]

use std::io;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use app::explorer::{
    ExplorerClipboard, ExplorerClipboardMode, ExplorerSharedRequest, ExplorerSharedSnapshot,
};
use identity::{AuthSession, IdentitySource, UserRole};
use platform::shell_control::{LocalConnection, LocalListener, create_private_directory};
use serde_json::json;
use storage::StorageConfig;
use workspace_protocol::transport::Framed;
use workspace_protocol::{
    Hello, MAX_CONTROL_FRAME_BYTES, MAX_SERVICE_FRAME_BYTES, PROTOCOL_VERSION, ProcessGeneration,
    RunId, ServiceClientMessage, ServiceError, ServiceHostMessage, ServiceOperation, ServiceReply,
    ServiceRequest, ServiceValue, SessionId, SupervisorToWorker, WorkerId, WorkerRole,
    WorkerToSupervisor,
};

const DEADLINE: Duration = Duration::from_secs(5);

#[test]
fn managed_services_share_window_state_and_reject_stale_writes_over_real_connections() {
    let mut fixture = Fixture::new();
    let worker = |role, generation| WorkerId {
        run: fixture.run,
        role,
        generation: ProcessGeneration(generation),
    };
    let services = worker(WorkerRole::Services, 1);
    let shell = worker(WorkerRole::Shell, 2);
    let first = worker(WorkerRole::App(SessionId(1)), 3);
    let second = worker(WorkerRole::App(SessionId(2)), 4);
    let mut control = fixture.start(services);
    register(&mut control, shell);
    let mut shell_client = Client::connect(&fixture, services, shell);
    let auth = AuthSession {
        source: IdentitySource::LocalAccount,
        session_id: "first-login".into(),
        user_id: "workspace-test".into(),
        username: "workspace-test".into(),
        role: UserRole::Admin,
        started_at_epoch_ms: 1,
    };
    assert!(
        shell_client
            .request(ServiceOperation::WriteAuthorization {
                revision: 0,
                policy: json!(auth)
            })
            .is_ok()
    );

    register(&mut control, first);
    register(&mut control, second);
    let mut first_client = Client::connect(&fixture, services, first);
    let mut second_client = Client::connect(&fixture, services, second);
    let (revision, draft) = versioned(first_client.request(ServiceOperation::ReadConfig));
    let (same_revision, new_draft) = versioned(first_client.request(ServiceOperation::ReadConfig));
    assert_eq!(revision, same_revision);
    let mut newer: StorageConfig = serde_json::from_value(new_draft).unwrap();
    newer.language = "zh-CN".into();
    let (updated_revision, _) = versioned(first_client.request(ServiceOperation::WriteConfig {
        revision: same_revision,
        config: json!(newer),
    }));
    assert_eq!(updated_revision, revision + 1);
    assert_eq!(
        first_client.request(ServiceOperation::WriteConfig {
            revision,
            config: draft
        }),
        Err(ServiceError::Conflict)
    );
    let (shared_revision, config) = versioned(second_client.request(ServiceOperation::ReadConfig));
    assert_eq!(shared_revision, updated_revision);
    assert_eq!(config["language"], "zh-CN");

    let clipboard = ExplorerClipboard {
        paths: vec![fixture.root.join("测试.txt")],
        mode: ExplorerClipboardMode::Copy,
    };
    for request in [
        ExplorerSharedRequest::SetClipboard(Some(clipboard.clone())),
        ExplorerSharedRequest::SetBusy(true),
    ] {
        assert!(
            first_client
                .request(ServiceOperation::UpdateFileOperations {
                    operation: json!(request)
                })
                .is_ok()
        );
    }
    let (_, shared) = versioned(second_client.request(ServiceOperation::ReadFileOperations));
    let shared: ExplorerSharedSnapshot = serde_json::from_value(shared).unwrap();
    assert_eq!(shared.clipboard, Some(clipboard));
    assert!(shared.busy);

    control
        .queue(&SupervisorToWorker::UnregisterWorker { worker: first })
        .unwrap();
    control.poll::<WorkerToSupervisor>().unwrap();
    assert_eq!(
        receive::<ServiceHostMessage, _>(&mut first_client.wire, Some)
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    let (_, shared) = versioned(second_client.request(ServiceOperation::ReadFileOperations));
    let shared: ExplorerSharedSnapshot = serde_json::from_value(shared).unwrap();
    assert!(!shared.busy);
    let error = second_client
        .request(ServiceOperation::UpdateFileOperations {
            operation: json!(ExplorerSharedRequest::Undo),
        })
        .unwrap_err();
    let ServiceError::OperationFailed { message } = error else {
        panic!("expected a localized error, received {error:?}");
    };
    let message: i18n::LocalizedMessage = serde_json::from_value(message).unwrap();
    assert_eq!(message.id, "app-explorer-undo-empty");

    control.queue(&SupervisorToWorker::Exit).unwrap();
    assert!(fixture.wait(&mut control).success());
}

fn register(control: &mut Framed<LocalConnection>, worker: WorkerId) {
    control
        .queue(&SupervisorToWorker::RegisterWorker {
            worker,
            pid: std::process::id(),
        })
        .unwrap();
    receive(control, |message| {
        matches!(message, WorkerToSupervisor::Registered { worker: registered } if registered == worker).then_some(())
    }).unwrap();
}

fn versioned(value: Result<ServiceValue, ServiceError>) -> (u64, serde_json::Value) {
    match value.unwrap() {
        ServiceValue::Versioned { revision, value } => (revision, value),
        ServiceValue::Empty => panic!("expected a versioned result"),
    }
}

fn receive<T: serde::de::DeserializeOwned, R>(
    wire: &mut Framed<LocalConnection>,
    mut accept: impl FnMut(T) -> Option<R>,
) -> io::Result<R> {
    let limit = Instant::now() + DEADLINE;
    loop {
        for message in wire.poll::<T>()? {
            if let Some(result) = accept(message) {
                return Ok(result);
            }
        }
        if Instant::now() >= limit {
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

struct Client {
    wire: Framed<LocalConnection>,
    request_id: u64,
}

impl Client {
    fn connect(fixture: &Fixture, services: WorkerId, worker: WorkerId) -> Self {
        let stream = LocalConnection::connect(
            &fixture.service_endpoint,
            fixture.child.as_ref().unwrap().id(),
        )
        .unwrap();
        let mut wire = Framed::new(stream, MAX_SERVICE_FRAME_BYTES);
        wire.queue(&ServiceClientMessage::Hello(Hello::new(worker)))
            .unwrap();
        let message: ServiceHostMessage = receive(&mut wire, Some).unwrap();
        assert_eq!(message, ServiceHostMessage::Welcome { worker: services });
        Self {
            wire,
            request_id: 0,
        }
    }

    fn request(&mut self, operation: ServiceOperation) -> Result<ServiceValue, ServiceError> {
        self.request_id += 1;
        let id = self.request_id;
        self.wire
            .queue(&ServiceClientMessage::Request(ServiceRequest {
                id,
                operation,
            }))
            .unwrap();
        let reply = receive(&mut self.wire, Some).unwrap();
        let ServiceHostMessage::Reply(ServiceReply {
            id: reply_id,
            result,
        }) = reply
        else {
            panic!("expected an RPC reply, received {reply:?}");
        };
        assert_eq!(reply_id, id);
        result
    }
}

struct Fixture {
    root: PathBuf,
    run: RunId,
    service_endpoint: String,
    child: Option<Child>,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("tux-rpc-{nonce:x}"));
        create_private_directory(&root).unwrap();
        let service_endpoint = root.join("services").to_str().unwrap().to_owned();
        Self {
            root,
            run: RunId(nonce),
            service_endpoint,
            child: None,
        }
    }

    fn start(&mut self, services: WorkerId) -> Framed<LocalConnection> {
        let guardian_endpoint = self.root.join("control");
        let guardian_endpoint = guardian_endpoint.to_str().unwrap();
        let listener = LocalListener::bind(guardian_endpoint).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_tundra-shell"))
            .args([
                "__workspace-worker",
                &self.run.to_string(),
                "services",
                &services.generation.0.to_string(),
                guardian_endpoint,
                &std::process::id().to_string(),
                &self.service_endpoint,
                "0",
                "0",
            ])
            .env("HOME", &self.root)
            .env("TMPDIR", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_RUNTIME_DIR", &self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        self.child = Some(child);
        let limit = Instant::now() + DEADLINE;
        let stream = loop {
            match listener.accept() {
                Ok(stream) => break stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(
                        self.child.as_mut().unwrap().try_wait().unwrap().is_none(),
                        "services worker exited before handshake"
                    );
                    assert!(Instant::now() < limit, "services worker never connected");
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("guardian accept failed: {error}"),
            }
        };
        assert_eq!(stream.peer_pid().unwrap(), pid);
        let mut wire = Framed::new(stream, MAX_CONTROL_FRAME_BYTES);
        assert_eq!(
            receive::<WorkerToSupervisor, _>(&mut wire, Some).unwrap(),
            WorkerToSupervisor::Hello(Hello {
                worker: services,
                pid,
                protocol: PROTOCOL_VERSION
            })
        );
        wire.queue(&SupervisorToWorker::Welcome { worker: services })
            .unwrap();
        receive(&mut wire, |message: WorkerToSupervisor| {
            (message == WorkerToSupervisor::Ready).then_some(())
        })
        .unwrap();
        wire
    }

    fn wait(&mut self, control: &mut Framed<LocalConnection>) -> std::process::ExitStatus {
        let limit = Instant::now() + DEADLINE;
        loop {
            let _ = control.poll::<WorkerToSupervisor>();
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < limit, "services worker did not exit");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
