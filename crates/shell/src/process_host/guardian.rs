use super::client::WorkerArgs;
use super::terminal::TerminalState;
use super::{HANDSHAKE_TIMEOUT, POLL_INTERVAL};
use platform::shell_control::{LocalConnection, LocalListener, Server, watchdog_directory};
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use watchdog::{SupervisionEvent, Supervisor};
use workspace_protocol::transport::Framed;
use workspace_protocol::{
    Hello, MAX_CONTROL_FRAME_BYTES, PROTOCOL_VERSION, ProcessGeneration, ProcessState, RunId,
    ShellCompletion, SupervisorToWorker, WatchdogAction, WatchdogError, WatchdogReply,
    WatchdogRequest, WatchdogResponse, WorkerId, WorkerRole, WorkerToSupervisor,
};

const SERVICES_STARTUP_TIMEOUT: Duration = Duration::from_secs(4);
const APPLICATION_NAME_LIMIT: usize = 256;
const RESPONSE_DRAIN_TIMEOUT: Duration = Duration::from_millis(200);

pub struct GuardianExit {
    pub code: i32,
    pub completion: Option<ShellCompletion>,
}

struct Guardian<'a> {
    supervisor: Supervisor,
    terminal: Option<TerminalState>,
    workers: BTreeMap<WorkerId, Worker>,
    worker_control: LocalListener,
    #[cfg(target_os = "linux")]
    delegated_resources: super::delegated::DelegatedResources,
    watchdog_control: Server,
    addresses: Addresses,
    executable: &'a Path,
    next_generation: u64,
    services: WorkerId,
    shell: Option<WorkerId>,
    exit: GuardianExit,
    shutting_down: bool,
    final_error: Option<io::Error>,
    service_unregistrations: VecDeque<WorkerId>,
    shell_notifications: VecDeque<SupervisorToWorker>,
    #[cfg(windows)]
    desktop: super::desktop::DesktopRelay,
}

struct Worker {
    pid: u32,
    application: String,
    endpoint: String,
    channel: Option<Framed<LocalConnection>>,
    authenticated: bool,
    ready: bool,
    ready_since: Instant,
    register_sent: bool,
    registered: bool,
    service_available_sent: bool,
    app_started_sent: bool,
    published_state: Option<ProcessState>,
    cleanup_error: Option<String>,
    close_delivered: bool,
    close_requested: bool,
    forced: bool,
}

impl Worker {
    fn send(&mut self, message: &SupervisorToWorker) -> bool {
        self.authenticated
            && self
                .channel
                .as_mut()
                .is_some_and(|channel| channel.queue(message).is_ok())
    }
}

pub(super) fn run(executable: &Path) -> io::Result<GuardianExit> {
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let run = RunId(u128::from_be_bytes(random));
    let paths = platform::native_platform()
        .app_paths()
        .map_err(io::Error::other)?;
    let addresses = Addresses::new(run)?;
    let worker_control = LocalListener::bind(&addresses.endpoint("control"))?;
    let watchdog_control = Server::start_watchdog(
        &watchdog_directory(&paths),
        run.to_string(),
        addresses.endpoint("watchdog"),
    )?;
    let terminal = TerminalState::capture()?;
    let signals = platform::TerminalControlHandler::install();
    let services = WorkerId {
        run,
        role: WorkerRole::Services,
        generation: ProcessGeneration(1),
    };
    let mut guardian = Guardian {
        supervisor: Supervisor::new(run),
        terminal,
        workers: BTreeMap::new(),
        worker_control,
        #[cfg(target_os = "linux")]
        delegated_resources: super::delegated::DelegatedResources::bind(
            addresses.private_directory(),
        )?,
        watchdog_control,
        addresses,
        executable,
        next_generation: 2,
        services,
        shell: None,
        exit: GuardianExit {
            code: 1,
            completion: None,
        },
        shutting_down: false,
        final_error: None,
        service_unregistrations: Default::default(),
        shell_notifications: Default::default(),
        #[cfg(windows)]
        desktop: super::desktop::DesktopRelay::start(&paths)?,
    };
    guardian.start_worker(services, "session-services".into())?;
    let startup = Instant::now();
    while guardian.supervisor.has_workers() {
        guardian.poll_watchdog();
        guardian.poll_workers();
        #[cfg(target_os = "linux")]
        guardian.delegated_resources.poll(&mut guardian.supervisor);
        #[cfg(windows)]
        while let Ok(reply) = guardian.desktop.replies.try_recv() {
            guardian.send(
                reply.worker,
                &SupervisorToWorker::DesktopLaunched {
                    request: reply.request,
                    error: reply.error,
                },
            );
        }
        if !guardian.shutting_down {
            if signals.shutdown_requested() {
                guardian.shutting_down = true;
                if let Some(shell) = guardian.shell {
                    guardian.supervisor.request_close(shell);
                } else {
                    let _ = guardian.supervisor.request_force_kill(services);
                }
            } else if guardian.shell.is_none() {
                if guardian
                    .workers
                    .get(&services)
                    .is_some_and(|worker| worker.ready)
                {
                    let shell = guardian.new_worker(WorkerRole::Shell);
                    match guardian.start_worker(shell, "shell".into()) {
                        Ok(()) => guardian.shell = Some(shell),
                        Err(error) => {
                            guardian.final_error = Some(error);
                            guardian.shutting_down = true;
                            let _ = guardian.supervisor.request_force_kill(services);
                        }
                    }
                } else if startup.elapsed() >= SERVICES_STARTUP_TIMEOUT {
                    guardian.final_error = Some(io::ErrorKind::TimedOut.into());
                    guardian.shutting_down = true;
                    let _ = guardian.supervisor.request_force_kill(services);
                }
            }
        }
        guardian.supervise();
        guardian.publish_changes();
        if let Some(terminal) = &mut guardian.terminal {
            terminal.poll_cleanup();
        }
        if guardian.supervisor.has_workers() {
            std::thread::sleep(POLL_INTERVAL);
        }
    }
    guardian.watchdog_control.stop_accepting();
    let drain = Instant::now();
    while drain.elapsed() < RESPONSE_DRAIN_TIMEOUT {
        let terminal_pending = guardian
            .terminal
            .as_mut()
            .is_some_and(TerminalState::poll_cleanup);
        if !terminal_pending && !guardian.watchdog_control.has_pending_responses() {
            break;
        }
        guardian.poll_watchdog();
        std::thread::sleep(POLL_INTERVAL);
    }
    guardian.final_error.map_or(Ok(guardian.exit), Err)
}

impl Guardian<'_> {
    fn new_worker(&mut self, role: WorkerRole) -> WorkerId {
        let generation = ProcessGeneration(self.next_generation);
        self.next_generation += 1;
        WorkerId {
            role,
            generation,
            run: self.services.run,
        }
    }

    fn start_worker(&mut self, worker: WorkerId, application: String) -> io::Result<()> {
        let endpoint = match worker.role {
            WorkerRole::Shell => String::new(),
            WorkerRole::Services => self.addresses.endpoint("services"),
            WorkerRole::App(_) => self
                .addresses
                .endpoint(&format!("app-{:x}", worker.generation.0)),
        };
        let shell_peer = self.shell.and_then(|shell| {
            self.workers
                .get(&shell)
                .map(|metadata| (shell, metadata.pid))
        });
        let mut command = Command::new(self.executable);
        WorkerArgs {
            worker,
            guardian_endpoint: &self.addresses.endpoint("control"),
            guard_pid: std::process::id(),
            endpoint: &endpoint,
            shell_peer,
        }
        .apply_to(&mut command);
        if worker.role != WorkerRole::Shell {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            command.env_remove(app::update::UPDATE_READY_FILE_ENV);
        }
        let pid = self.supervisor.spawn_worker(worker, &mut command)?;
        self.workers.insert(
            worker,
            Worker {
                pid,
                application,
                endpoint,
                channel: None,
                authenticated: false,
                ready: false,
                ready_since: Instant::now(),
                register_sent: false,
                registered: false,
                service_available_sent: false,
                app_started_sent: false,
                published_state: None,
                cleanup_error: None,
                close_delivered: false,
                close_requested: false,
                forced: false,
            },
        );
        if worker.role == WorkerRole::Shell
            && let Some(terminal) = &mut self.terminal
            && let Err(error) = terminal.hand_off(pid)
        {
            let _ = self.supervisor.request_force_kill(worker);
            return Err(error);
        }
        Ok(())
    }

    fn poll_workers(&mut self) {
        for _ in 0..16 {
            let stream = match self.worker_control.accept() {
                Ok(stream) => stream,
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue,
                Err(_) => break,
            };
            let Ok(pid) = stream.peer_pid() else { continue };
            if let Some(metadata) = self.workers.values_mut().find(|worker| worker.pid == pid)
                && !metadata.authenticated
                && metadata.channel.is_none()
            {
                metadata.channel = Some(Framed::new(stream, MAX_CONTROL_FRAME_BYTES));
            }
        }
        let mut received = Vec::new();
        for (&worker, metadata) in &mut self.workers {
            if let Some(channel) = &mut metadata.channel {
                match channel.poll::<WorkerToSupervisor>() {
                    Ok(messages) => {
                        received.extend(messages.into_iter().map(|value| (worker, value)))
                    }
                    Err(_) => metadata.channel = None,
                }
            }
        }
        for (worker, message) in received {
            self.handle_worker(worker, message);
        }
    }

    fn handle_worker(&mut self, worker: WorkerId, message: WorkerToSupervisor) {
        let Some(metadata) = self.workers.get_mut(&worker) else {
            return;
        };
        if !metadata.authenticated {
            if let WorkerToSupervisor::Hello(Hello {
                worker: claimed,
                pid,
                protocol,
            }) = message
                && claimed == worker
                && pid == metadata.pid
                && protocol == PROTOCOL_VERSION
                && let Some(channel) = metadata.channel.as_mut()
                && channel
                    .queue(&SupervisorToWorker::Welcome { worker })
                    .is_ok()
            {
                metadata.authenticated = true;
            } else {
                metadata.channel = None;
            }
            return;
        }

        let owns_shell = Some(worker) == self.shell;
        match message {
            WorkerToSupervisor::Ready => metadata.ready = true,
            WorkerToSupervisor::Progress { counter } => {
                self.supervisor.progress(worker, counter, None)
            }
            WorkerToSupervisor::Pong { nonce, counter } => {
                self.supervisor.progress(worker, counter, Some(nonce))
            }
            WorkerToSupervisor::Finished(completion) if owns_shell => {
                if !metadata.forced {
                    self.exit.completion = Some(completion);
                    self.shutting_down = true;
                }
            }
            WorkerToSupervisor::SpawnApp {
                session,
                application,
            } if owns_shell && !self.shutting_down => {
                let role = WorkerRole::App(session);
                if self.workers.keys().any(|worker| worker.role == role) {
                    return;
                }
                let app = self.new_worker(role);
                if application.is_empty()
                    || application.len() > APPLICATION_NAME_LIMIT
                    || !self.services_available()
                    || self.start_worker(app, application).is_err()
                {
                    self.shell_notifications
                        .push_back(SupervisorToWorker::AppState {
                            worker: app,
                            state: ProcessState::Exited {
                                code: None,
                                forced: false,
                            },
                        });
                }
            }
            WorkerToSupervisor::AppClosing if matches!(worker.role, WorkerRole::App(_)) => {
                metadata.close_delivered = true;
                self.supervisor.request_close(worker);
            }
            WorkerToSupervisor::CloseApp { worker: app }
                if owns_shell && matches!(app.role, WorkerRole::App(_)) =>
            {
                self.supervisor.request_close(app);
            }
            WorkerToSupervisor::ForceKill { worker: app }
                if owns_shell && matches!(app.role, WorkerRole::App(_)) =>
            {
                let _ = self.supervisor.request_force_kill(app);
            }
            WorkerToSupervisor::Registered { worker: registered } if worker == self.services => {
                if let Some(peer) = self.workers.get_mut(&registered)
                    && peer.register_sent
                {
                    peer.registered = true;
                }
            }
            WorkerToSupervisor::RegisterResource { request, resource } => {
                let accepted = self.supervisor.register_resource(worker, resource).is_ok();
                self.send(
                    worker,
                    &SupervisorToWorker::ResourceRegistered { request, accepted },
                );
            }
            WorkerToSupervisor::UnregisterResource { resource } => {
                let _ = self.supervisor.unregister_resource(worker, resource);
            }
            #[cfg(windows)]
            WorkerToSupervisor::LaunchDesktop { request, operation }
                if !self.shutting_down
                    && !metadata.forced
                    && worker.role != WorkerRole::Services =>
            {
                if !self.desktop.enqueue(worker, request, operation) {
                    self.send(
                        worker,
                        &SupervisorToWorker::DesktopLaunched {
                            request,
                            error: Some(io::Error::from(io::ErrorKind::WouldBlock).to_string()),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    fn publish_changes(&mut self) {
        while let Some(&worker) = self.service_unregistrations.front() {
            if !self.send(
                self.services,
                &SupervisorToWorker::UnregisterWorker { worker },
            ) {
                break;
            }
            self.service_unregistrations.pop_front();
        }
        let services =
            self.workers
                .get(&self.services)
                .map(|metadata| SupervisorToWorker::ServicesStarted {
                    worker: self.services,
                    pid: metadata.pid,
                    endpoint: metadata.endpoint.clone(),
                });
        let mut apps = Vec::new();
        let mut registrations = Vec::new();
        for (&worker, metadata) in &mut self.workers {
            if metadata.close_requested && !metadata.close_delivered {
                metadata.close_delivered = metadata.send(&SupervisorToWorker::Close);
            }
            if worker.role != WorkerRole::Services && !metadata.register_sent {
                registrations.push((worker, metadata.pid));
            }
            if metadata.registered
                && !metadata.service_available_sent
                && let Some(services) = &services
            {
                metadata.service_available_sent = metadata.send(services);
            }
            if worker.role != WorkerRole::Shell {
                if let Some(state) = self.supervisor.state(worker)
                    && metadata.published_state != Some(state)
                {
                    apps.push((worker, SupervisorToWorker::AppState { worker, state }));
                }
                if matches!(worker.role, WorkerRole::App(_))
                    && metadata.ready
                    && metadata.service_available_sent
                    && !metadata.app_started_sent
                {
                    apps.push((
                        worker,
                        SupervisorToWorker::AppStarted {
                            worker,
                            pid: metadata.pid,
                            endpoint: metadata.endpoint.clone(),
                        },
                    ));
                }
            }
        }
        for (worker, pid) in registrations {
            if self.send(
                self.services,
                &SupervisorToWorker::RegisterWorker { worker, pid },
            ) {
                self.workers.get_mut(&worker).unwrap().register_sent = true;
            }
        }
        if let Some(shell) = self.shell {
            while let Some(message) = self.shell_notifications.pop_front() {
                if !self.send(shell, &message) {
                    self.shell_notifications.push_front(message);
                    break;
                }
            }
            for (worker, message) in apps {
                if self.send(shell, &message)
                    && let Some(metadata) = self.workers.get_mut(&worker)
                {
                    match message {
                        SupervisorToWorker::AppState { state, .. } => {
                            metadata.published_state = Some(state)
                        }
                        SupervisorToWorker::AppStarted { .. } => metadata.app_started_sent = true,
                        _ => {}
                    }
                }
            }
        }
    }

    fn supervise(&mut self) {
        for event in self.supervisor.tick() {
            match event {
                SupervisionEvent::Probe { worker, nonce } => {
                    self.send(worker, &SupervisorToWorker::Ping { nonce });
                }
                SupervisionEvent::RequestClose { worker } => {
                    if let Some(metadata) = self.workers.get_mut(&worker) {
                        metadata.close_requested = true;
                    }
                }
                SupervisionEvent::ForceStopping { worker } => {
                    if let Some(metadata) = self.workers.get_mut(&worker) {
                        metadata.forced = true;
                    }
                    if worker.role == WorkerRole::Shell {
                        self.shutting_down = true;
                        self.exit.completion = None;
                    }
                }
                SupervisionEvent::ShellStopped { worker } => {
                    self.shutting_down = true;
                    self.shell_notifications.clear();
                    let forced = self.workers.get(&worker).is_some_and(|meta| meta.forced);
                    if let Some(terminal) = &mut self.terminal
                        && let Err(error) =
                            terminal.restore(self.exit.completion.is_none() || forced)
                    {
                        self.final_error = Some(error);
                    }
                }
                SupervisionEvent::Exited { worker, status } => {
                    let metadata = self.workers.remove(&worker);
                    if worker.role == WorkerRole::Shell {
                        self.exit.code = status.code().unwrap_or(1);
                    } else if worker.role == WorkerRole::Services
                        && self.shell.is_none()
                        && !self.shutting_down
                    {
                        self.final_error = Some(io::ErrorKind::BrokenPipe.into());
                        self.shutting_down = true;
                    }
                    if worker.role == WorkerRole::Services {
                        self.service_unregistrations.clear();
                    } else if self.workers.contains_key(&self.services) {
                        self.service_unregistrations.push_back(worker);
                    }
                    if self
                        .workers
                        .keys()
                        .any(|worker| worker.role == WorkerRole::Shell)
                        && worker.role != WorkerRole::Shell
                    {
                        self.shell_notifications
                            .push_back(SupervisorToWorker::AppState {
                                worker,
                                state: ProcessState::Exited {
                                    code: status.code(),
                                    forced: metadata.is_some_and(|metadata| metadata.forced),
                                },
                            });
                    }
                }
                SupervisionEvent::ProcessError { worker, error } => {
                    let error_message = error.to_string();
                    if let Some(metadata) = self.workers.get_mut(&worker) {
                        metadata.cleanup_error = Some(error_message.clone());
                    }
                    if self
                        .workers
                        .keys()
                        .any(|worker| worker.role == WorkerRole::Shell)
                    {
                        self.shell_notifications
                            .push_back(SupervisorToWorker::CleanupError {
                                worker,
                                error: error_message,
                            });
                    }
                    if worker.role == WorkerRole::Shell {
                        self.final_error = Some(error);
                    }
                }
                SupervisionEvent::NoResponse { .. }
                | SupervisionEvent::Recovered { .. }
                | SupervisionEvent::CloseTimedOut { .. } => {}
            }
        }
        for (&worker, metadata) in &self.workers {
            if !metadata.authenticated
                && !metadata.forced
                && metadata.ready_since.elapsed() >= HANDSHAKE_TIMEOUT
            {
                let _ = self.supervisor.request_force_kill(worker);
            }
        }
    }

    fn send(&mut self, worker: WorkerId, message: &SupervisorToWorker) -> bool {
        self.workers
            .get_mut(&worker)
            .is_some_and(|metadata| metadata.send(message))
    }

    fn services_available(&self) -> bool {
        self.workers
            .get(&self.services)
            .is_some_and(|worker| worker.ready && worker.channel.is_some())
            && matches!(
                self.supervisor.state(self.services),
                Some(ProcessState::Starting | ProcessState::Running)
            )
    }

    fn poll_watchdog(&mut self) {
        for (client, payload) in self.watchdog_control.poll() {
            let response = serde_json::from_slice::<WatchdogRequest>(&payload)
                .map_err(|_| WatchdogError::Unavailable)
                .and_then(|request| self.watchdog_request(request));
            self.watchdog_control.respond_json(client, &response);
        }
    }

    fn watchdog_request(&mut self, request: WatchdogRequest) -> WatchdogResponse {
        if request.protocol != PROTOCOL_VERSION {
            return Err(WatchdogError::Version);
        }
        if request.run != self.services.run || request.pid != std::process::id() {
            return Err(WatchdogError::WrongRun);
        }
        match request.action {
            WatchdogAction::List => Ok(WatchdogReply::List(
                self.workers
                    .iter()
                    .filter_map(|(&worker, metadata)| {
                        Some(workspace_protocol::SessionInfo {
                            worker,
                            pid: metadata.pid,
                            application: metadata.application.clone(),
                            state: self.supervisor.state(worker)?,
                            cleanup_error: metadata.cleanup_error.clone(),
                        })
                    })
                    .collect(),
            )),
            WatchdogAction::Kill { worker } => {
                if worker.role == WorkerRole::Services || !self.workers.contains_key(&worker) {
                    return Err(WatchdogError::NotFound);
                }
                self.supervisor
                    .request_force_kill(worker)
                    .map_err(|_| WatchdogError::Unavailable)?;
                Ok(WatchdogReply::KillRequested { worker })
            }
        }
    }
}

struct Addresses {
    #[cfg(unix)]
    path: std::path::PathBuf,
    #[cfg(windows)]
    run: RunId,
}

impl Addresses {
    fn new(run: RunId) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let temporary = std::env::temp_dir().canonicalize()?;
            let temporary =
                if temporary.as_os_str().as_bytes().len() <= 35 && temporary.to_str().is_some() {
                    temporary
                } else {
                    Path::new("/tmp").canonicalize()?
                };
            let path = temporary.join(format!(".tux-{run}"));
            platform::shell_control::create_private_directory(&path)?;
            Ok(Self { path })
        }
        #[cfg(windows)]
        {
            Ok(Self { run })
        }
    }

    fn endpoint(&self, purpose: &str) -> String {
        #[cfg(unix)]
        {
            self.path.join(purpose).to_string_lossy().into_owned()
        }
        #[cfg(windows)]
        {
            format!("tundra-{purpose}-{}", self.run)
        }
    }

    #[cfg(target_os = "linux")]
    fn private_directory(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for Addresses {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
