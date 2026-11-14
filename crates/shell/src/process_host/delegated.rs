use super::{HANDSHAKE_TIMEOUT, POLL_INTERVAL};
use platform::shell_control::{LocalConnection, LocalListener};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use watchdog::Supervisor;
use workspace_protocol::transport::Framed;
use workspace_protocol::{
    GuardianToResource, Hello, MAX_RESOURCE_FRAME_BYTES, PROTOCOL_VERSION, ProcessResource,
    ResourceToGuardian, WorkerId, WorkerRole,
};

const RESOURCE_ENV: &str = "TUNDRA_EMBEDDED_RESOURCE_OWNER";
const ENDPOINT: &str = "resources";
const MAX_PENDING: usize = 16;

#[derive(Serialize, Deserialize)]
struct Address {
    worker: WorkerId,
    pid: u32,
    endpoint: PathBuf,
}

pub(super) fn environment(
    worker: WorkerId,
    pid: u32,
    directory: &Path,
) -> io::Result<(OsString, OsString)> {
    let address = Address {
        worker,
        pid,
        endpoint: directory.join(ENDPOINT),
    };
    Ok((RESOURCE_ENV.into(), serde_json::to_string(&address)?.into()))
}

/// Installs the limited guardian registration connection of an embedded CLI.
///
/// # Safety
/// Call at single-threaded startup, before the CLI captures its environment.
pub unsafe fn install_delegated_worker_resources() -> io::Result<()> {
    let Some(address) = std::env::var_os(RESOURCE_ENV) else {
        return Ok(());
    };
    // SAFETY: callers keep process startup single-threaded until after this call returns.
    unsafe { std::env::remove_var(RESOURCE_ENV) };
    let address = address.to_str().ok_or(io::ErrorKind::InvalidInput)?;
    install_from_address(address)
}

fn install_from_address(address: &str) -> io::Result<()> {
    let Address {
        worker,
        pid,
        endpoint,
    } = serde_json::from_str(address)?;
    let endpoint = endpoint.to_str().ok_or(io::ErrorKind::InvalidInput)?;
    let client = LocalConnection::connect(endpoint, pid)?;
    let mut channel = Framed::new(client, MAX_RESOURCE_FRAME_BYTES);
    channel.queue(&ResourceToGuardian::Hello(Hello::new(worker)))?;
    wait_for_reply(&mut channel, |reply| match reply {
        GuardianToResource::Welcome { worker: ready } if worker == ready => Some(Ok(())),
        _ => Some(Err(io::ErrorKind::PermissionDenied.into())),
    })?;

    let channel = Arc::new(Mutex::new((channel, 0u64)));
    let registers = Arc::clone(&channel);
    watchdog::set_process_resource_registrar(
        move |resource| {
            let mut registers = registers.lock().map_err(|_| io::ErrorKind::BrokenPipe)?;
            let (channel, next_request) = &mut *registers;
            *next_request += 1;
            let expected = *next_request;
            channel.queue(&ResourceToGuardian::Register {
                request: expected,
                resource,
            })?;
            wait_for_reply(channel, |reply| match reply {
                GuardianToResource::Registered { request, accepted } if request == expected => {
                    Some(if accepted {
                        Ok(())
                    } else {
                        Err(io::ErrorKind::PermissionDenied.into())
                    })
                }
                GuardianToResource::Registered { .. } => None,
                GuardianToResource::Welcome { .. } => Some(Err(io::ErrorKind::InvalidData.into())),
            })
        },
        move |resource| {
            if let Ok(mut channel) = channel.lock() {
                let _ = channel.0.queue(&ResourceToGuardian::Unregister(resource));
                let _ = channel.0.poll::<GuardianToResource>();
            }
        },
    )
}

fn wait_for_reply(
    channel: &mut Framed<LocalConnection>,
    mut decide: impl FnMut(GuardianToResource) -> Option<io::Result<()>>,
) -> io::Result<()> {
    let started = Instant::now();
    while started.elapsed() < HANDSHAKE_TIMEOUT {
        for reply in channel.poll()? {
            if let Some(result) = decide(reply) {
                return result;
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Err(io::ErrorKind::TimedOut.into())
}

pub(super) struct DelegatedResources {
    listener: LocalListener,
    peers: Vec<Peer>,
}

struct Peer {
    pid: u32,
    pidfd: OwnedFd,
    connected: Instant,
    owner: Option<WorkerId>,
    channel: Framed<LocalConnection>,
}

impl DelegatedResources {
    pub(super) fn bind(directory: &Path) -> io::Result<Self> {
        let endpoint = directory.join(ENDPOINT);
        let path = endpoint.to_str().ok_or(io::ErrorKind::InvalidInput)?;
        Ok(Self {
            listener: LocalListener::bind(path)?,
            peers: Vec::new(),
        })
    }

    pub(super) fn poll(&mut self, supervisor: &mut Supervisor) {
        self.peers
            .retain_mut(|peer| peer.poll(supervisor).unwrap_or(false));
        let pending = self
            .peers
            .iter()
            .filter(|peer| peer.owner.is_none())
            .count();
        for _ in pending..MAX_PENDING {
            match self.listener.accept() {
                Ok(connection) => {
                    if let Ok(peer) = Peer::new(connection)
                        && !self.peers.iter().any(|existing| existing.pid == peer.pid)
                    {
                        self.peers.push(peer);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue,
                Err(_) => break,
            }
        }
    }
}

impl Peer {
    fn new(connection: LocalConnection) -> io::Result<Self> {
        let pid = connection.peer_pid()?;
        let pidfd = connection.peer_process()?;
        Ok(Self {
            pid,
            pidfd,
            connected: Instant::now(),
            owner: None,
            channel: Framed::new(connection, MAX_RESOURCE_FRAME_BYTES),
        })
    }

    fn poll(&mut self, supervisor: &mut Supervisor) -> io::Result<bool> {
        if self.owner.is_none() && self.connected.elapsed() >= HANDSHAKE_TIMEOUT {
            return Ok(false);
        }
        let mut process = libc::pollfd {
            fd: self.pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: process is live and initializes exactly one pollfd for the call.
        if unsafe { libc::poll(&mut process, 1, 0) } != 0 {
            return Ok(false);
        }
        let messages = self.channel.poll()?;
        if let Some(owner) = self.owner
            && !messages.is_empty()
            && !supervisor.owns_process(owner, self.pid)?
        {
            return Ok(false);
        }
        for message in messages {
            match (self.owner, message) {
                (
                    None,
                    ResourceToGuardian::Hello(Hello {
                        worker,
                        pid,
                        protocol,
                    }),
                ) if matches!(worker.role, WorkerRole::App(_))
                    && pid == self.pid
                    && protocol == PROTOCOL_VERSION
                    && supervisor.owns_process(worker, pid)? =>
                {
                    self.channel
                        .queue(&GuardianToResource::Welcome { worker })?;
                    self.owner = Some(worker);
                }
                (Some(owner), ResourceToGuardian::Register { request, resource })
                    if valid_resource(resource) =>
                {
                    let accepted = supervisor
                        .register_child_resource(owner, self.pid, resource)
                        .is_ok();
                    self.channel
                        .queue(&GuardianToResource::Registered { request, accepted })?;
                }
                (Some(owner), ResourceToGuardian::Unregister(resource))
                    if valid_resource(resource) =>
                {
                    supervisor.unregister_child_resource(owner, self.pid, resource)?;
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
    }
}

fn valid_resource(resource: ProcessResource) -> bool {
    matches!(
        resource,
        ProcessResource::UnixSession { .. }
            | ProcessResource::UnixGroup { .. }
            | ProcessResource::LinuxDelegatedSession { .. }
    )
}

#[cfg(test)]
#[path = "../../tests/unit/process_host/delegated_tests.rs"]
mod tests;
