use crate::{Operation, Principal};
use session_protocol::linux::{account, account_in_group, session_for_pid};
use session_protocol::{OperationStatus, SessionSnapshot, SessionState, SystemAction};
use std::collections::HashMap;
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use watchdog::{
    AppCriticality, AppDescriptor, AppId, ManagedTaskGroup, TaskId, TaskSpec, WatchdogConfig,
    WatchdogRuntime,
};
use zbus::blocking::{Connection, Proxy, connection::Builder};
use zbus::{fdo, message::Header};

type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    admin_groups: Vec<String>,
}

fn policy() -> Result<Policy, Error> {
    let path = "/etc/tundra/privileged.toml";
    // Every ancestor is protected, so an unprivileged caller cannot replace
    // the checked policy or redirect its final open through a symlink.
    for ancestor in ["/", "/etc", "/etc/tundra"] {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err("unsafe privileged policy directory".into());
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || metadata.len() > 16384
    {
        return Err("unsafe privileged policy permissions".into());
    }
    let mut contents = String::new();
    (&mut file).take(16385).read_to_string(&mut contents)?;
    if contents.len() > 16384 {
        return Err("privileged policy too large".into());
    }
    let policy: Policy = toml::from_str(&contents)?;
    if policy.admin_groups.is_empty() || policy.admin_groups.len() > 32 {
        return Err("admin_groups must contain between 1 and 32 group names".into());
    }
    Ok(policy)
}

fn bus(timeout: Duration) -> Result<Connection, Error> {
    Ok(Builder::system()?.method_timeout(timeout).build()?)
}

fn process_birth(pid: u32) -> Result<u64, Error> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let (_, fields) = stat.rsplit_once(')').ok_or("invalid process stat")?;
    Ok(fields
        .split_whitespace()
        .nth(19)
        .ok_or("missing process birth")?
        .parse()?)
}

fn principal(sender: &str) -> Result<Principal, Error> {
    if !sender.starts_with(':') {
        return Err("unique D-Bus sender required".into());
    }
    let connection = bus(Duration::from_secs(5))?;
    let dbus = Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let uid: u32 = dbus.call("GetConnectionUnixUser", &(sender,))?;
    let pid: u32 = dbus.call("GetConnectionUnixProcessID", &(sender,))?;
    if uid == 0 {
        return Err("root is not a desktop authorization subject".into());
    }
    let birth = process_birth(pid)?;
    let session = session_for_pid(&connection, pid)?;
    if session.identity.uid != uid
        || session.remote
        || !session.active
        || session.locked
        || session.seat != "seat0"
    {
        return Err("an active unlocked local seat0 session is required".into());
    }
    let user = account(uid)?;
    let mut allowed = false;
    for group in policy()?.admin_groups {
        allowed |= account_in_group(&user, &group)?;
    }
    if !allowed {
        return Err("user is not in an authorized administrator group".into());
    }
    let sessiond = Proxy::new(
        &connection,
        session_protocol::SESSION_BUS,
        session_protocol::SESSION_PATH,
        session_protocol::SESSION_BUS,
    )?;
    let snapshot: String = sessiond.call("GetSnapshot", &())?;
    let snapshot: Option<SessionSnapshot> = serde_json::from_str(&snapshot)?;
    if !snapshot.is_some_and(|s| s.identity == session.identity && s.state == SessionState::Active)
    {
        return Err("a managed active Tundra session is required".into());
    }
    // Recheck both bus ownership and process birth after external identity lookups.
    let current_pid: u32 = dbus.call("GetConnectionUnixProcessID", &(sender,))?;
    if current_pid != pid || process_birth(pid)? != birth {
        return Err("D-Bus caller changed".into());
    }
    Ok(Principal {
        sender: sender.into(),
        pid,
        birth,
        session: session.identity,
    })
}

fn sender(header: &Header<'_>) -> fdo::Result<String> {
    header
        .sender()
        .map(|s| s.to_string())
        .ok_or_else(|| fdo::Error::AccessDenied("sender required".into()))
}
fn denied(error: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::AccessDenied(error.to_string())
}

#[derive(Clone)]
struct Service {
    operations: Arc<Mutex<HashMap<String, Operation>>>,
    in_flight: Arc<AtomicBool>,
    connection: Arc<OnceLock<Connection>>,
    workers: ManagedTaskGroup,
}

// A one-shot operation is never replayed after panic. Leave a terminal result
// for the caller even when unwinding interrupts the normal completion path.
struct Reservation {
    service: Service,
    id: String,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut ops) = self.service.operations.lock()
            && let Some(op) = ops.get_mut(&self.id)
            && matches!(
                op.status,
                OperationStatus::AwaitingConfirmation | OperationStatus::Running
            )
        {
            op.status = OperationStatus::Failed("authorized worker stopped".into());
            self.service
                .notify(&self.id, &op.principal.sender, &op.status);
        }
        self.service.in_flight.store(false, Ordering::Release);
    }
}

#[zbus::interface(name = "org.tundra.Privileged1")]
impl Service {
    #[zbus(signal)]
    async fn operation_changed(
        emitter: zbus::object_server::SignalEmitter<'_>,
        id: &str,
        status_json: &str,
    ) -> zbus::Result<()>;

    fn protocol_version(&self) -> u32 {
        session_protocol::VERSION
    }

    fn can_request(&self, #[zbus(header)] header: Header<'_>) -> fdo::Result<bool> {
        Ok(principal(&sender(&header)?).is_ok())
    }

    fn request(
        &self,
        action_json: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        if action_json.len() > 4096 {
            return Err(fdo::Error::InvalidArgs("request too large".into()));
        }
        let action: SystemAction = serde_json::from_str(action_json)
            .map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        action
            .validate()
            .map_err(|e| fdo::Error::InvalidArgs(e.into()))?;
        let who = principal(&sender(&header)?).map_err(denied)?;
        let mut ops = self.operations.lock().map_err(denied)?;
        ops.retain(|_, o| {
            o.created.elapsed() < Duration::from_secs(600)
                || matches!(o.status, OperationStatus::Running)
        });
        if self.in_flight.load(Ordering::Acquire) {
            return Err(fdo::Error::LimitsExceeded(
                "another system operation is in progress".into(),
            ));
        }
        if ops.len() >= 64 {
            return Err(fdo::Error::LimitsExceeded(
                "operation history limit reached".into(),
            ));
        }
        let mut nonce = [0u8; 24];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut nonce))
            .map_err(denied)?;
        let id: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        ops.insert(
            id.clone(),
            Operation {
                principal: who.clone(),
                action: action.clone(),
                status: OperationStatus::AwaitingConfirmation,
                created: Instant::now(),
                result: String::new(),
            },
        );
        self.notify(&id, &who.sender, &OperationStatus::AwaitingConfirmation);
        self.in_flight.store(true, Ordering::Release);
        drop(ops);
        let service = self.clone();
        let work_id = id.clone();
        self.workers
            .spawn_thread(
                TaskSpec::one_shot(TaskId::from_static("authorized-operation")),
                move || {
                    // The reservation outlives cancellation and is released even on unwind.
                    let _release = Reservation {
                        service: service.clone(),
                        id: work_id.clone(),
                    };
                    let outcome = service.perform(&work_id, &who, &action);
                    if let Ok(mut ops) = service.operations.lock()
                        && let Some(op) = ops.get_mut(&work_id)
                    {
                        match outcome {
                            Ok(result) => {
                                op.result = result;
                                op.status = OperationStatus::Completed;
                            }
                            Err(error) if op.status != OperationStatus::Cancelled => {
                                op.status = OperationStatus::Failed(error.to_string());
                            }
                            Err(_) => {}
                        }
                        service.notify(&work_id, &who.sender, &op.status);
                        // Auditing contains identity/action/outcome only, never PAM material or log contents.
                        eprintln!(
                            "request={work_id} uid={} session={} action={} status={:?}",
                            who.session.uid,
                            who.session.logind_session_id,
                            action.policy_id(),
                            op.status
                        );
                    }
                },
            )
            .map_err(|e| {
                self.in_flight.store(false, Ordering::Release);
                if let Ok(mut ops) = self.operations.lock() {
                    ops.remove(&id);
                }
                fdo::Error::Failed(e.to_string())
            })?;
        Ok(id)
    }

    fn get_result(&self, id: &str, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        let source = sender(&header)?;
        let ops = self.operations.lock().map_err(denied)?;
        let op = ops
            .get(id)
            .ok_or_else(|| fdo::Error::UnknownObject("unknown operation".into()))?;
        if op.principal.sender != source {
            return Err(denied("operation belongs to another sender"));
        }
        serde_json::to_string(&(&op.status, &op.result))
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    fn cancel(&self, id: &str, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        let source = sender(&header)?;
        let mut ops = self.operations.lock().map_err(denied)?;
        let op = ops
            .get_mut(id)
            .ok_or_else(|| fdo::Error::UnknownObject("unknown operation".into()))?;
        if op.principal.sender != source {
            return Err(denied("operation belongs to another sender"));
        }
        if op.status == OperationStatus::AwaitingConfirmation {
            op.status = OperationStatus::Cancelled;
            self.notify(id, &source, &op.status);
        } else {
            return Err(fdo::Error::Failed(
                "operation already started or finished".into(),
            ));
        }
        Ok(())
    }
}

impl Service {
    fn notify(&self, id: &str, sender: &str, status: &OperationStatus) {
        if let Some(connection) = self.connection.get()
            && let Ok(status) = serde_json::to_string(status)
        {
            // Unicast metadata: unrelated users cannot observe operation history.
            let _ = connection.emit_signal(
                Some(sender),
                session_protocol::PRIVILEGED_PATH,
                session_protocol::PRIVILEGED_BUS,
                "OperationChanged",
                &(id, status),
            );
        }
    }

    fn perform(&self, id: &str, who: &Principal, action: &SystemAction) -> Result<String, Error> {
        if let SystemAction::InstallUpdate { release_id } = action {
            system_maintenance::linux::prepare_official_release(release_id)?;
            // Download/verification precedes the consent window; no unverified details
            // are presented as a trusted install candidate.
            let current = principal(&who.sender)?;
            if current != *who {
                return Err("update requester changed".into());
            }
            let mut ops = self
                .operations
                .lock()
                .map_err(|_| "operations lock poisoned")?;
            let op = ops.get_mut(id).ok_or("request disappeared")?;
            if op.status != OperationStatus::AwaitingConfirmation {
                return Err("update cancelled".into());
            }
            op.created = Instant::now();
        }
        if self
            .operations
            .lock()
            .map_err(|_| "operations lock poisoned")?
            .get(id)
            .is_none_or(|op| op.status != OperationStatus::AwaitingConfirmation)
        {
            return Err("operation cancelled before confirmation".into());
        }
        let connection = bus(Duration::from_secs(150))?;
        let sessiond = Proxy::new(
            &connection,
            session_protocol::SESSION_BUS,
            session_protocol::SESSION_PATH,
            session_protocol::SESSION_BUS,
        )?;
        let confirmed: bool = sessiond.call(
            "RequestConsent",
            &(
                &who.sender,
                serde_json::to_string(&who.session)?,
                serde_json::to_string(action)?,
            ),
        )?;
        if !confirmed {
            if let Some(op) = self
                .operations
                .lock()
                .map_err(|_| "operations lock poisoned")?
                .get_mut(id)
            {
                op.status = OperationStatus::Cancelled;
            }
            return Err("authorization cancelled".into());
        }
        let current = principal(&who.sender)?;
        self.operations
            .lock()
            .map_err(|_| "operations lock poisoned")?
            .get_mut(id)
            .ok_or("request disappeared")?
            .authorize_execution(&current, true)?;
        self.notify(id, &who.sender, &OperationStatus::Running);
        match action {
            SystemAction::PowerOff | SystemAction::Reboot => {
                let logind = Proxy::new(
                    &connection,
                    "org.freedesktop.login1",
                    "/org/freedesktop/login1",
                    "org.freedesktop.login1.Manager",
                )?;
                let method = if matches!(action, SystemAction::PowerOff) {
                    "PowerOffWithFlags"
                } else {
                    "RebootWithFlags"
                };
                // SD_LOGIND_ROOT_CHECK_INHIBITORS: root must honor block inhibitors.
                // No SKIP_INHIBITORS or interactive authentication flag is permitted.
                logind.call::<_, _, ()>(method, &(1u64,))?;
                Ok(String::new())
            }
            SystemAction::ReadSystemLogs {
                max_records,
                since_epoch_seconds,
            } => system_logs(*max_records, *since_epoch_seconds),
            SystemAction::InstallUpdate { release_id } => {
                system_maintenance::release_version(release_id)?;
                sessiond
                    .call::<_, _, ()>("CloseSession", &(serde_json::to_string(&who.session)?,))?;
                // A separate package-owned systemd worker survives service replacement.
                let status = Command::new("/usr/bin/systemctl")
                    .args([
                        "start",
                        "--no-block",
                        &format!("tundra-update@{release_id}.service"),
                    ])
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()?;
                if !status.success() {
                    return Err("could not launch the verified update transaction".into());
                }
                Ok("Update activated; the session has been closed".into())
            }
        }
    }
}

fn system_logs(max: u32, since: u64) -> Result<String, Error> {
    use std::os::fd::AsRawFd;
    let mut child = Command::new("/usr/bin/journalctl")
        .args([
            "--no-pager",
            "--output=json",
            "--lines",
            &max.to_string(),
            "--since",
            &format!("@{since}"),
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("SYSTEMD_PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut output = child.stdout.take().ok_or("journal pipe unavailable")?;
    let flags = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(output.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::last_os_error().into());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = (|| {
        let mut bytes = Vec::new();
        let mut block = [0u8; 16384];
        loop {
            if Instant::now() > deadline {
                return Err("journal query timed out".into());
            }
            match output.read(&mut block) {
                Ok(0) => break,
                Ok(count) => {
                    if bytes.len() + count > 4 * 1024 * 1024 {
                        return Err("journal query exceeded size limit".into());
                    }
                    bytes.extend_from_slice(&block[..count]);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => return Err(e.into()),
            }
        }
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    return Err("journal query failed".into());
                }
                break;
            }
            if Instant::now() > deadline {
                return Err("journal process timed out".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(String::from_utf8(bytes)?)
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

pub fn run() -> Result<(), Error> {
    if unsafe { libc::getuid() } != 0 || unsafe { libc::geteuid() } != 0 {
        return Err("system service requires root".into());
    }
    policy()?;
    // Never use HOME, TMPDIR, or a desktop-supplied path for root diagnostics.
    for ancestor in ["/", "/var", "/var/lib", "/var/lib/tundra"] {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err("unsafe privileged diagnostic directory".into());
        }
    }
    let root = std::path::Path::new("/var/lib/tundra/privileged-watchdog");
    match std::fs::DirBuilder::new().mode(0o700).create(root) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = std::fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o077 != 0 {
        return Err("unsafe privileged diagnostic permissions".into());
    }
    let (_runtime, process) = WatchdogRuntime::start(
        WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("state"),
            "tundra-privileged",
            env!("CARGO_PKG_VERSION"),
        )
        .with_unclean_exit_tracking(false),
    )?;
    let app = process.register_app(AppDescriptor::new(
        AppId::from_static("privileged"),
        "Tundra privileged service",
        env!("CARGO_PKG_VERSION"),
        AppCriticality::ProcessCritical,
    ))?;
    let service = Service {
        operations: Arc::default(),
        in_flight: Arc::default(),
        connection: Arc::default(),
        workers: app.task_group("authorization"),
    };
    let connection_slot = service.connection.clone();
    let connection = Builder::system()?
        .name(session_protocol::PRIVILEGED_BUS)?
        .serve_at(session_protocol::PRIVILEGED_PATH, service)?
        .build()?;
    let _ = connection_slot.set(connection);
    loop {
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_worker_releases_slot_and_cannot_leave_pending_consent() {
        let directory = std::env::temp_dir().join(format!(
            "tundra-privileged-watchdog-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let (runtime, process) = WatchdogRuntime::start(
            WatchdogConfig::new(
                directory.join("reports"),
                directory.join("fallback"),
                directory.join("state"),
                "privileged-test",
                "test",
            )
            .with_unclean_exit_tracking(false),
        )
        .unwrap();
        let app = process
            .register_app(AppDescriptor::new(
                AppId::from_static("privileged-test"),
                "test",
                "test",
                AppCriticality::Optional,
            ))
            .unwrap();
        let service = Service {
            operations: Arc::default(),
            in_flight: Arc::default(),
            connection: Arc::default(),
            workers: app.task_group("test"),
        };
        for initial in [
            OperationStatus::AwaitingConfirmation,
            OperationStatus::Running,
            OperationStatus::Cancelled,
        ] {
            service.in_flight.store(true, Ordering::Release);
            service.operations.lock().unwrap().insert(
                "test".into(),
                Operation {
                    principal: Principal {
                        sender: ":1.2".into(),
                        pid: 123,
                        birth: 99,
                        session: session_protocol::SessionIdentity {
                            uid: 1001,
                            logind_session_id: "test".into(),
                        },
                    },
                    action: SystemAction::ReadSystemLogs {
                        max_records: 5,
                        since_epoch_seconds: 0,
                    },
                    status: initial.clone(),
                    created: Instant::now(),
                    result: String::new(),
                },
            );
            let worker = service.clone();
            let handle = service
                .workers
                .spawn_thread(
                    TaskSpec::one_shot(TaskId::from_static("interrupted")),
                    move || {
                        let _reservation = Reservation {
                            service: worker.clone(),
                            id: "test".into(),
                        };
                        panic!("injected worker interruption");
                    },
                )
                .unwrap();
            assert!(handle.join().unwrap().is_none());
            assert!(!service.in_flight.load(Ordering::Acquire));
            let ops = service.operations.lock().unwrap();
            if initial == OperationStatus::Cancelled {
                assert_eq!(ops["test"].status, OperationStatus::Cancelled);
            } else {
                assert!(matches!(ops["test"].status, OperationStatus::Failed(_)));
            }
        }
        drop(service);
        drop(app);
        drop(process);
        runtime.shutdown().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
