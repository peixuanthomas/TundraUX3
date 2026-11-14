use std::collections::BTreeMap;
use std::io;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};
use workspace_protocol::{
    ProcessGeneration, ProcessResource, ProcessState, RunId, WorkerId, WorkerRole,
};

mod health;
mod process;
mod registrar;
#[cfg(unix)]
#[allow(unsafe_code)]
mod spawn;

use health::{HealthEvent, WorkerHealth};
#[cfg(target_os = "macos")]
pub use process::OwnedProcessSession;
use process::WorkerProcess;
#[cfg(target_os = "linux")]
pub use registrar::register_delegated_session;
#[cfg(windows)]
pub use registrar::register_process_job;
pub use registrar::{
    ProcessResourceGuard, process_session_registration_enabled, set_process_resource_registrar,
};
#[cfg(unix)]
pub use registrar::{register_process_group, register_process_session};
#[cfg(unix)]
pub use spawn::spawn_registered_process_group;

pub const WORKER_PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TICK_GAP: Duration = Duration::from_secs(1);
const STOP_RETRY: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub enum SupervisionEvent {
    Probe {
        worker: WorkerId,
        nonce: u64,
    },
    RequestClose {
        worker: WorkerId,
    },
    NoResponse {
        worker: WorkerId,
    },
    CloseTimedOut {
        worker: WorkerId,
    },
    Recovered {
        worker: WorkerId,
    },
    ForceStopping {
        worker: WorkerId,
    },
    ShellStopped {
        worker: WorkerId,
    },
    Exited {
        worker: WorkerId,
        status: ExitStatus,
    },
    ProcessError {
        worker: WorkerId,
        error: io::Error,
    },
}

/// Owns and reaps this run's children from the independent guardian process.
/// Feed authenticated control events and tick regularly on the guardian loop.
pub struct Supervisor {
    run: RunId,
    workers: BTreeMap<WorkerId, Worker>,
    generations: BTreeMap<WorkerRole, ProcessGeneration>,
    events: Vec<SupervisionEvent>,
    last_tick: Instant,
    next_probe: u64,
    shell_stopped: bool,
}

struct Worker {
    process: WorkerProcess,
    health: WorkerHealth,
    close_started: Option<Instant>,
    close_timeout_reported: bool,
    force_at: Option<Instant>,
    next_stop_attempt: Option<Instant>,
    root_stopped: bool,
    last_error: Option<String>,
}

impl Supervisor {
    pub fn new(run: RunId) -> Self {
        Self {
            run,
            workers: BTreeMap::new(),
            generations: BTreeMap::new(),
            events: Vec::new(),
            last_tick: Instant::now(),
            next_probe: 0,
            shell_stopped: false,
        }
    }

    /// Preserves the caller's stdio configuration. The guardian retains the child.
    pub fn spawn_worker(&mut self, id: WorkerId, command: &mut Command) -> io::Result<u32> {
        if id.run != self.run
            || self.shell_stopped
            || self.workers.keys().any(|worker| worker.role == id.role)
            || self
                .generations
                .get(&id.role)
                .is_some_and(|previous| id.generation <= *previous)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "worker does not belong to this run or its generation is already in use",
            ));
        }
        let process = WorkerProcess::spawn(command)?;
        let pid = process.pid();
        self.generations.insert(id.role, id.generation);
        self.workers.insert(
            id,
            Worker {
                process,
                health: WorkerHealth::new(Instant::now()),
                close_started: None,
                close_timeout_reported: false,
                force_at: None,
                next_stop_attempt: None,
                root_stopped: false,
                last_error: None,
            },
        );
        Ok(pid)
    }

    pub fn has_workers(&self) -> bool {
        !self.workers.is_empty()
    }

    pub fn state(&self, id: WorkerId) -> Option<ProcessState> {
        let worker = self.workers.get(&id)?;
        Some(
            if worker.next_stop_attempt.is_some() || worker.root_stopped {
                ProcessState::ForceStopping
            } else if worker.close_started.is_some() && !worker.close_timeout_reported {
                ProcessState::Closing
            } else if worker.health.unresponsive() || worker.close_timeout_reported {
                ProcessState::Unresponsive
            } else if worker.health.started() {
                ProcessState::Running
            } else {
                ProcessState::Starting
            },
        )
    }

    pub fn progress(&mut self, id: WorkerId, counter: u64, pong: Option<u64>) {
        let Some(worker) = self.workers.get_mut(&id) else {
            return;
        };
        if !worker.root_stopped
            && worker.next_stop_attempt.is_none()
            && worker.health.progress(counter, pong, Instant::now())
            && !worker.close_timeout_reported
        {
            self.events.push(SupervisionEvent::Recovered { worker: id });
        }
    }

    pub fn request_close(&mut self, id: WorkerId) -> bool {
        let Some(worker) = self.workers.get_mut(&id) else {
            return false;
        };
        Self::close(worker, id, Instant::now(), &mut self.events)
    }

    fn close(
        worker: &mut Worker,
        id: WorkerId,
        now: Instant,
        events: &mut Vec<SupervisionEvent>,
    ) -> bool {
        if worker.root_stopped || worker.next_stop_attempt.is_some() {
            return false;
        }
        if worker.close_started.is_none() {
            worker.close_started = Some(now);
            events.push(SupervisionEvent::RequestClose { worker: id });
        }
        true
    }

    /// Initiates OS termination before returning. Exited is emitted after reap.
    pub fn request_force_kill(&mut self, id: WorkerId) -> io::Result<()> {
        let worker = self.worker_mut(id)?;
        let first = worker.next_stop_attempt.is_none();
        let result = worker.request_stop(Instant::now());
        if first {
            self.events
                .push(SupervisionEvent::ForceStopping { worker: id });
        }
        result
    }

    pub fn register_resource(&mut self, id: WorkerId, resource: ProcessResource) -> io::Result<()> {
        let worker = self.worker_mut(id)?;
        Self::register(worker, worker.process.pid(), resource)
    }

    #[cfg(unix)]
    pub fn owns_process(&self, id: WorkerId, pid: u32) -> io::Result<bool> {
        let Some(worker) = self.workers.get(&id) else {
            return Ok(false);
        };
        if worker.root_stopped || worker.next_stop_attempt.is_some() {
            return Ok(false);
        }
        worker.process.owns_process(pid)
    }

    #[cfg(unix)]
    pub fn register_child_resource(
        &mut self,
        id: WorkerId,
        owner: u32,
        resource: ProcessResource,
    ) -> io::Result<()> {
        Self::register(self.worker_mut(id)?, owner, resource)
    }

    fn register(worker: &mut Worker, owner: u32, resource: ProcessResource) -> io::Result<()> {
        if worker.root_stopped || worker.next_stop_attempt.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "worker is stopping",
            ));
        }
        worker.process.register(owner, resource)
    }

    pub fn unregister_resource(
        &mut self,
        id: WorkerId,
        resource: ProcessResource,
    ) -> io::Result<()> {
        let worker = self.worker_mut(id)?;
        Self::unregister(worker, worker.process.pid(), resource)
    }

    #[cfg(unix)]
    pub fn unregister_child_resource(
        &mut self,
        id: WorkerId,
        owner: u32,
        resource: ProcessResource,
    ) -> io::Result<()> {
        Self::unregister(self.worker_mut(id)?, owner, resource)
    }

    fn unregister(worker: &mut Worker, owner: u32, resource: ProcessResource) -> io::Result<()> {
        if worker.next_stop_attempt.is_some() || worker.root_stopped {
            return Ok(());
        }
        worker.process.unregister(owner, resource)
    }

    fn worker_mut(&mut self, id: WorkerId) -> io::Result<&mut Worker> {
        self.workers
            .get_mut(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "worker generation has exited"))
    }

    pub fn tick(&mut self) -> Vec<SupervisionEvent> {
        let now = Instant::now();
        let resumed = now.duration_since(self.last_tick) >= MAX_TICK_GAP;
        self.last_tick = now;

        let mut shell_just_stopped = false;
        for (&id, worker) in &mut self.workers {
            if worker.root_stopped {
                continue;
            }
            match worker.process.root_exited() {
                Ok(true) => {
                    worker.root_stopped = true;
                    if id.role == WorkerRole::Shell {
                        self.shell_stopped = true;
                        shell_just_stopped = true;
                        self.events
                            .push(SupervisionEvent::ShellStopped { worker: id });
                    }
                }
                Ok(false) => {}
                Err(error) => worker.report_error(id, error, &mut self.events),
            }
        }

        for (&id, worker) in &mut self.workers {
            let mut force = worker.root_stopped;
            if shell_just_stopped && id.role != WorkerRole::Shell && !force {
                if worker.health.healthy(now)
                    && !worker.close_timeout_reported
                    && worker.next_stop_attempt.is_none()
                {
                    Self::close(worker, id, now, &mut self.events);
                    worker.force_at = worker.close_started.map(|started| started + CLOSE_TIMEOUT);
                } else {
                    force = true;
                }
            }
            if !force && worker.next_stop_attempt.is_none() {
                force = worker.force_at.is_some_and(|deadline| now >= deadline);
                if worker.force_at.is_none() {
                    force |= check_health(
                        id,
                        now,
                        resumed,
                        worker,
                        &mut self.next_probe,
                        &mut self.events,
                    );
                }
                if worker
                    .close_started
                    .is_some_and(|started| now.duration_since(started) >= CLOSE_TIMEOUT)
                    && !worker.close_timeout_reported
                    && !force
                {
                    worker.close_timeout_reported = true;
                    self.events
                        .push(SupervisionEvent::CloseTimedOut { worker: id });
                }
            }
            if force || worker.next_stop_attempt.is_some() {
                if worker.next_stop_attempt.is_none() && !worker.root_stopped {
                    self.events
                        .push(SupervisionEvent::ForceStopping { worker: id });
                }
                if worker
                    .next_stop_attempt
                    .is_none_or(|deadline| now >= deadline)
                    && let Err(error) = worker.request_stop(now)
                {
                    worker.report_error(id, error, &mut self.events);
                }
                if let Err(error) = worker.process.poll_cleanup() {
                    worker.report_error(id, error, &mut self.events);
                }
            } else if let Err(error) = worker.process.check_retired() {
                worker.report_error(id, error, &mut self.events);
            }
        }

        self.workers.retain(|&id, worker| {
            if !worker.root_stopped {
                return true;
            }
            match worker.process.try_reap() {
                Ok(Some(status)) => {
                    self.events
                        .push(SupervisionEvent::Exited { worker: id, status });
                    false
                }
                Ok(None) => true,
                Err(error) => {
                    worker.report_error(id, error, &mut self.events);
                    true
                }
            }
        });
        std::mem::take(&mut self.events)
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        for worker in self.workers.values_mut() {
            let _ = worker.process.request_stop();
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.has_workers() && Instant::now() < deadline {
            self.workers
                .retain(|_, worker| !matches!(worker.process.try_reap(), Ok(Some(_))));
            if self.has_workers() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

impl Worker {
    fn request_stop(&mut self, now: Instant) -> io::Result<()> {
        self.next_stop_attempt = Some(now + STOP_RETRY);
        self.process.request_stop()
    }

    fn report_error(&mut self, id: WorkerId, error: io::Error, events: &mut Vec<SupervisionEvent>) {
        let message = error.to_string();
        if self.last_error.as_ref() != Some(&message) {
            self.last_error = Some(message);
            events.push(SupervisionEvent::ProcessError { worker: id, error });
        }
    }
}

#[cfg(all(test, unix))]
#[allow(unsafe_code)]
#[path = "../tests/unit/supervisor.rs"]
mod tests;

fn check_health(
    id: WorkerId,
    now: Instant,
    resumed: bool,
    worker: &mut Worker,
    next_probe: &mut u64,
    events: &mut Vec<SupervisionEvent>,
) -> bool {
    match worker.health.tick(now, resumed, *next_probe) {
        Some(HealthEvent::Probe) => {
            events.push(SupervisionEvent::Probe {
                worker: id,
                nonce: *next_probe,
            });
            *next_probe = next_probe.wrapping_add(1);
        }
        Some(HealthEvent::NoResponse) => {
            events.push(SupervisionEvent::NoResponse { worker: id });
            return id.role == WorkerRole::Shell;
        }
        None => {}
    }
    false
}
