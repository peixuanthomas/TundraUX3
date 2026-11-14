use super::{HANDSHAKE_TIMEOUT, POLL_INTERVAL};
use platform::shell_control::{LocalConnection, LocalListener};
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use workspace_protocol::transport::Framed;
use workspace_protocol::{
    Hello, MAX_CONTROL_FRAME_BYTES, MAX_SERVICE_FRAME_BYTES, MAX_WINDOW_FRAME_BYTES,
    ProcessGeneration, ProcessResource, ServiceClientMessage, ServiceHostMessage, ShellToApp,
    SupervisorToWorker, WorkerId, WorkerRole, WorkerToSupervisor,
};

static CONNECTION: OnceLock<WorkerConnection> = OnceLock::new();

pub type WindowConnection = Framed<LocalConnection>;
type ServiceConnection = Framed<LocalConnection>;

#[derive(Clone)]
pub(super) struct WorkerEndpoint {
    pub(super) worker: WorkerId,
    pub(super) pid: u32,
    pub(super) endpoint: String,
}

pub(super) struct WorkerArgs<'a> {
    pub(super) worker: WorkerId,
    pub(super) guardian_endpoint: &'a str,
    pub(super) guard_pid: u32,
    pub(super) endpoint: &'a str,
    pub(super) shell_peer: Option<(WorkerId, u32)>,
}

impl WorkerArgs<'_> {
    pub(super) fn apply_to(&self, command: &mut Command) {
        let role = match self.worker.role {
            WorkerRole::Shell => "shell".into(),
            WorkerRole::Services => "services".into(),
            WorkerRole::App(session) => session.to_string(),
        };
        let (shell_generation, shell_pid) = self
            .shell_peer
            .map(|(shell, pid)| (shell.generation.0, pid))
            .unwrap_or_default();
        command
            .arg(super::WORKER_ARGUMENT)
            .arg(self.worker.run.to_string())
            .arg(role)
            .arg(self.worker.generation.0.to_string())
            .arg(self.guardian_endpoint)
            .arg(self.guard_pid.to_string())
            .arg(self.endpoint)
            .arg(shell_generation.to_string())
            .arg(shell_pid.to_string());
    }
}

pub struct WorkerConnection {
    worker: WorkerId,
    endpoint: String,
    shell_peer: Option<(WorkerId, u32)>,
    #[cfg(unix)]
    guardian_endpoint: String,
    #[cfg(target_os = "linux")]
    guardian_pid: u32,
    control: Mutex<Control>,
}

struct Control {
    framed: Framed<LocalConnection>,
    messages: VecDeque<SupervisorToWorker>,
    services: Option<WorkerEndpoint>,
    replies: BTreeMap<u64, Option<io::Result<()>>>,
    next_request: u64,
    progress: u64,
    last_progress: Option<Instant>,
    probe: Option<u64>,
    shutdown: bool,
}

impl WorkerConnection {
    pub fn current() -> Option<&'static Self> {
        CONNECTION.get()
    }

    pub fn worker(&self) -> WorkerId {
        self.worker
    }

    pub fn endpoint(&self) -> Option<&str> {
        (!self.endpoint.is_empty()).then_some(self.endpoint.as_str())
    }

    pub fn shell_peer(&self) -> Option<(WorkerId, u32)> {
        self.shell_peer
    }

    #[cfg(unix)]
    pub fn private_directory(&self) -> Option<&std::path::Path> {
        std::path::Path::new(&self.guardian_endpoint).parent()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn embedded_cli_resource_environment(
        &self,
    ) -> io::Result<(std::ffi::OsString, std::ffi::OsString)> {
        let directory = self
            .private_directory()
            .ok_or(io::ErrorKind::InvalidInput)?;
        super::delegated::environment(self.worker, self.guardian_pid, directory)
    }

    pub fn try_recv(&self) -> io::Result<Option<SupervisorToWorker>> {
        let mut control = self.lock();
        if let Some(message) = control.messages.pop_front() {
            return Ok(Some(message));
        }
        control.poll(self.worker)?;
        Ok(control.messages.pop_front())
    }

    pub fn send(&self, message: WorkerToSupervisor) -> io::Result<()> {
        let mut control = self.lock();
        control.framed.queue(&message)?;
        control.poll(self.worker)
    }

    /// The caller must be the worker's actual business or UI event loop.
    pub fn progress(&self) -> io::Result<()> {
        let mut control = self.lock();
        control.poll(self.worker)?;
        let due = control
            .last_progress
            .is_none_or(|last| last.elapsed() >= watchdog::WORKER_PROGRESS_INTERVAL);
        if control.probe.is_some() || due {
            let counter = control.progress + 1;
            let message = match control.probe {
                Some(nonce) => WorkerToSupervisor::Pong { nonce, counter },
                None => WorkerToSupervisor::Progress { counter },
            };
            control.framed.queue(&message)?;
            control.progress = counter;
            control.last_progress = Some(Instant::now());
            control.probe = None;
            control.poll(self.worker)?;
        }
        Ok(())
    }

    pub fn shutdown_requested(&self) -> bool {
        self.lock().shutdown
    }

    pub fn accept_shell(&self, listener: &LocalListener) -> io::Result<WindowConnection> {
        let (_, expected_pid) = self.shell_peer.ok_or(io::ErrorKind::PermissionDenied)?;
        let stream = listener.accept()?;
        if stream.peer_pid()? != expected_pid {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        #[cfg(unix)]
        stream.increase_buffer_capacity(1024 * 1024);
        Ok(Framed::new(stream, MAX_WINDOW_FRAME_BYTES))
    }

    pub fn connect_app(
        &self,
        worker: WorkerId,
        pid: u32,
        endpoint: &str,
    ) -> io::Result<WindowConnection> {
        if self.worker.role != WorkerRole::Shell
            || worker.run != self.worker.run
            || !matches!(worker.role, WorkerRole::App(_))
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let stream = LocalConnection::connect(endpoint, pid)?;
        #[cfg(unix)]
        stream.increase_buffer_capacity(1024 * 1024);
        let mut framed = Framed::new(stream, MAX_WINDOW_FRAME_BYTES);
        framed.queue(&ShellToApp::Hello(Hello::new(self.worker)))?;
        Ok(framed)
    }

    pub fn connect_services(&self) -> io::Result<ServiceConnection> {
        let start = Instant::now();
        let endpoint = loop {
            let service = {
                let mut control = self.lock();
                control.poll(self.worker)?;
                control.services.clone()
            };
            if let Some(endpoint) = service {
                break endpoint;
            }
            wait(start)?;
        };
        let stream = connect(&endpoint.endpoint, endpoint.pid, start)?;
        let mut framed = Framed::new(stream, MAX_SERVICE_FRAME_BYTES);
        framed.queue(&ServiceClientMessage::Hello(Hello::new(self.worker)))?;
        loop {
            let messages = framed.poll()?;
            match messages.as_slice() {
                [ServiceHostMessage::Welcome { worker }] if *worker == endpoint.worker => {
                    return Ok(framed);
                }
                [] => wait(start)?,
                _ => return Err(io::ErrorKind::PermissionDenied.into()),
            }
        }
    }

    pub fn finish(&self, completion: workspace_protocol::ShellCompletion) -> io::Result<()> {
        self.send(WorkerToSupervisor::Finished(completion))?;
        let start = Instant::now();
        loop {
            let mut control = self.lock();
            if !control.framed.pending_write() {
                return Ok(());
            }
            control.poll(self.worker)?;
            drop(control);
            wait(start)?;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Control> {
        self.control
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn register_resource(&self, resource: ProcessResource) -> io::Result<()> {
        self.request(|request| WorkerToSupervisor::RegisterResource { request, resource })
    }

    #[cfg(windows)]
    pub(super) fn launch_desktop(
        &self,
        operation: workspace_protocol::DesktopOperation,
    ) -> io::Result<()> {
        self.request(|request| WorkerToSupervisor::LaunchDesktop { request, operation })
    }

    fn request(&self, message: impl FnOnce(u64) -> WorkerToSupervisor) -> io::Result<()> {
        let request = {
            let mut control = self.lock();
            let request = control.next_request;
            control.next_request += 1;
            control.framed.queue(&message(request))?;
            control.replies.insert(request, None);
            request
        };
        let start = Instant::now();
        let result = loop {
            {
                let mut control = self.lock();
                if let Err(error) = control.poll(self.worker) {
                    break Err(error);
                }
                if let Some(reply) = control.replies.get_mut(&request).and_then(Option::take) {
                    break reply;
                }
            }
            if let Err(error) = wait(start) {
                break Err(error);
            }
        };
        self.lock().replies.remove(&request);
        result
    }
}

impl Control {
    fn poll(&mut self, identity: WorkerId) -> io::Result<()> {
        if self.messages.len() >= 224 {
            return Ok(());
        }
        for message in self.framed.poll()? {
            match message {
                SupervisorToWorker::Ping { nonce } => self.probe = Some(nonce),
                SupervisorToWorker::ServicesStarted {
                    worker,
                    pid,
                    endpoint,
                } if worker.run == identity.run && worker.role == WorkerRole::Services => {
                    self.services = Some(WorkerEndpoint {
                        worker,
                        pid,
                        endpoint,
                    });
                }
                SupervisorToWorker::ResourceRegistered { request, accepted } => {
                    if let Some(reply) = self.replies.get_mut(&request) {
                        *reply = Some(if accepted {
                            Ok(())
                        } else {
                            Err(io::ErrorKind::PermissionDenied.into())
                        });
                    }
                }
                #[cfg(windows)]
                SupervisorToWorker::DesktopLaunched { request, error } => {
                    if let Some(reply) = self.replies.get_mut(&request) {
                        *reply = Some(error.map_or(Ok(()), |error| Err(io::Error::other(error))));
                    }
                }
                message => {
                    self.shutdown |= matches!(
                        message,
                        SupervisorToWorker::Close | SupervisorToWorker::Exit
                    );
                    self.messages.push_back(message);
                }
            }
        }
        Ok(())
    }
}

pub(super) fn connect_worker(args: &[String]) -> io::Result<&'static WorkerConnection> {
    #[cfg(target_os = "linux")]
    platform::linux::identity::ProcessIdentity::current().validate()?;
    let [
        run,
        role,
        generation,
        guardian_endpoint,
        guard_pid,
        endpoint,
        shell_generation,
        shell_pid,
    ] = args
    else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    fn parse<T: std::str::FromStr>(value: &str) -> io::Result<T> {
        value
            .parse()
            .map_err(|_| io::ErrorKind::InvalidInput.into())
    }
    let worker = WorkerId {
        run: parse(run)?,
        role: match role.as_str() {
            "shell" => WorkerRole::Shell,
            "services" => WorkerRole::Services,
            _ => WorkerRole::App(parse(role)?),
        },
        generation: ProcessGeneration(parse(generation)?),
    };
    let guard_pid = parse(guard_pid)?;
    super::terminal::verify_parent(guard_pid)?;
    let shell_pid = parse(shell_pid)?;
    let shell_peer = match worker.role {
        WorkerRole::App(_) => Some((
            WorkerId {
                role: WorkerRole::Shell,
                generation: ProcessGeneration(parse(shell_generation)?),
                ..worker
            },
            shell_pid,
        )),
        _ => None,
    };

    let start = Instant::now();
    let stream = connect(guardian_endpoint, guard_pid, start)?;
    let mut framed = Framed::new(stream, MAX_CONTROL_FRAME_BYTES);
    framed.queue(&WorkerToSupervisor::Hello(Hello::new(worker)))?;
    let mut control = Control {
        framed,
        messages: VecDeque::new(),
        services: None,
        replies: BTreeMap::new(),
        next_request: 0,
        progress: 0,
        last_progress: None,
        probe: None,
        shutdown: false,
    };
    loop {
        control.poll(worker)?;
        match control.messages.pop_front() {
            Some(SupervisorToWorker::Welcome { worker: accepted }) if accepted == worker => break,
            Some(_) => return Err(io::ErrorKind::PermissionDenied.into()),
            None => wait(start)?,
        }
    }
    let connection = CONNECTION.get_or_init(|| WorkerConnection {
        worker,
        endpoint: endpoint.clone(),
        shell_peer,
        #[cfg(unix)]
        guardian_endpoint: guardian_endpoint.clone(),
        #[cfg(target_os = "linux")]
        guardian_pid: guard_pid,
        control: Mutex::new(control),
    });
    watchdog::set_process_resource_registrar(
        move |resource| connection.register_resource(resource),
        move |resource| {
            let _ = connection.send(WorkerToSupervisor::UnregisterResource { resource });
        },
    )?;
    Ok(connection)
}

fn connect(endpoint: &str, pid: u32, start: Instant) -> io::Result<LocalConnection> {
    loop {
        match LocalConnection::connect(endpoint, pid) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::NotFound | io::ErrorKind::TimedOut
                ) =>
            {
                wait(start)?
            }
            Err(error) => return Err(error),
        }
    }
}

fn wait(start: Instant) -> io::Result<()> {
    if start.elapsed() >= HANDSHAKE_TIMEOUT {
        return Err(io::ErrorKind::TimedOut.into());
    }
    std::thread::sleep(POLL_INTERVAL);
    Ok(())
}
