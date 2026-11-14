use super::state::ServiceState;
use crate::process_host::WorkerConnection;
use platform::shell_control::{LocalConnection, LocalListener};
use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};
use workspace_protocol::transport::Framed;
use workspace_protocol::{
    MAX_SERVICE_FRAME_BYTES, PROTOCOL_VERSION, ServiceClientMessage, ServiceHostMessage,
    ServiceReply, SupervisorToWorker, WorkerId, WorkerToSupervisor,
};

struct Peer {
    wire: Framed<LocalConnection>,
    pid: u32,
    worker: Option<WorkerId>,
    accepted: Instant,
    last_request: Option<u64>,
}

pub(crate) fn run_session_services(
    connection: &'static WorkerConnection,
    _watchdog: watchdog::ProcessWatchdog,
) -> io::Result<()> {
    let platform = platform::native_platform();
    let storage = storage::StorageManager::open_from_platform(platform.as_ref())
        .map_err(io::Error::other)?
        .manager;
    let mut state = ServiceState::new(storage).map_err(io::Error::other)?;
    let listener = LocalListener::bind(
        connection
            .endpoint()
            .ok_or_else(|| io::Error::other("missing workspace services endpoint"))?,
    )?;
    let mut allowed = HashMap::<WorkerId, u32>::new();
    let mut peers = Vec::<Peer>::new();
    connection.send(WorkerToSupervisor::Ready)?;
    loop {
        while let Some(message) = connection.try_recv()? {
            match message {
                SupervisorToWorker::RegisterWorker { worker, pid } => {
                    allowed.insert(worker, pid);
                    state.register(worker);
                    connection.send(WorkerToSupervisor::Registered { worker })?;
                }
                SupervisorToWorker::UnregisterWorker { worker } => {
                    allowed.remove(&worker);
                    state.unregister(worker);
                    peers.retain(|peer| peer.worker != Some(worker));
                }
                SupervisorToWorker::Close | SupervisorToWorker::Exit => {
                    connection.send(WorkerToSupervisor::Exiting { code: 0 })?;
                    return Ok(());
                }
                _ => {}
            }
        }
        for _ in 0..16 {
            match listener.accept() {
                Ok(stream) => {
                    if peers.len() >= allowed.len().saturating_mul(2).max(8) {
                        continue;
                    }
                    let Ok(pid) = stream.peer_pid() else { continue };
                    if !allowed.values().any(|allowed_pid| *allowed_pid == pid) {
                        continue;
                    }
                    peers.push(Peer {
                        wire: Framed::new(stream, MAX_SERVICE_FRAME_BYTES),
                        pid,
                        worker: None,
                        accepted: Instant::now(),
                        last_request: None,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue,
                Err(error) => return Err(error),
            }
        }
        peers.retain_mut(|peer| {
            if peer.worker.is_none() && peer.accepted.elapsed() > Duration::from_secs(2) {
                return false;
            }
            let Ok(messages) = peer.wire.poll::<ServiceClientMessage>() else {
                return false;
            };
            for message in messages {
                let reply = match message {
                    ServiceClientMessage::Hello(hello)
                        if peer.worker.is_none()
                            && hello.protocol == PROTOCOL_VERSION
                            && hello.pid == peer.pid
                            && allowed.get(&hello.worker) == Some(&peer.pid) =>
                    {
                        peer.worker = Some(hello.worker);
                        ServiceHostMessage::Welcome {
                            worker: connection.worker(),
                        }
                    }
                    ServiceClientMessage::Request(request) if peer.worker.is_some() => {
                        let worker = peer.worker.unwrap();
                        if allowed.get(&worker) != Some(&peer.pid) {
                            return false;
                        }
                        if peer.last_request.is_some_and(|last| request.id <= last) {
                            return false;
                        }
                        peer.last_request = Some(request.id);
                        ServiceHostMessage::Reply(ServiceReply {
                            id: request.id,
                            result: state.execute(worker, request.operation, platform.as_ref()),
                        })
                    }
                    _ => return false,
                };
                if peer.wire.queue(&reply).is_err() {
                    return false;
                }
            }
            true
        });
        connection.progress()?;
        std::thread::sleep(Duration::from_millis(5));
    }
}
