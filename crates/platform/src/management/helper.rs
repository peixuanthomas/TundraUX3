//! A one-operation, reconnectable helper. Closing the TUI never kills a package transaction.
use super::*;
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use watchdog::{
    AppCriticality, AppDescriptor, AppId, TaskId, TaskSpec, WatchdogConfig, WatchdogRuntime,
};

const MAX_PACKET: usize = 128 * 1024;
const MAX_REPLAY: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct OperationMetadata {
    kind: ManagementKind,
}

fn write_operation_metadata(
    directory: &std::path::Path,
    kind: ManagementKind,
) -> Result<(), ManagementError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(directory.join("metadata.json"))
        .map_err(failure)?;
    // The actor can read this label, but a root task's directory remains root-owned.
    file.set_permissions(fs::Permissions::from_mode(0o644))
        .map_err(failure)?;
    serde_json::to_writer(&mut file, &OperationMetadata { kind }).map_err(failure)?;
    file.sync_all().map_err(failure)
}

fn failure(e: impl std::fmt::Display) -> ManagementError {
    ManagementError::Failed(e.to_string())
}

fn checked_directory(path: &std::path::Path, owner: u32, mode: u32) -> Result<(), ManagementError> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(failure)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(failure(e)),
    }
    let m = fs::symlink_metadata(path).map_err(failure)?;
    if !m.is_dir() || m.uid() != owner || m.mode() & 0o022 != 0 {
        return Err(ManagementError::Conflict(
            "Unsafe operation directory ownership or permissions".into(),
        ));
    }
    Ok(())
}

fn base_directory(euid: u32, actor: u32) -> PathBuf {
    if euid == 0 {
        PathBuf::from("/run/tundraux3-management").join(actor.to_string())
    } else {
        std::env::temp_dir().join(format!("tundraux3-management-{actor}"))
    }
}

/// Used only by the CLI's early helper dispatch, before its ordinary startup/runtime.
pub fn entry(actor: u32) -> Result<(), ManagementError> {
    let uid = unsafe { libc::getuid() };
    let euid = unsafe { libc::geteuid() };
    if uid != euid || unsafe { libc::getgid() } != unsafe { libc::getegid() } {
        return Err(ManagementError::PermissionDenied(
            "Set-ID helper invocation is forbidden".into(),
        ));
    }
    if euid != 0 && actor != euid {
        return Err(ManagementError::PermissionDenied(
            "The operation belongs to a different user".into(),
        ));
    }
    if euid == 0 && actor != 0 {
        let sudo_actor = std::env::var("SUDO_UID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok());
        if sudo_actor != Some(actor) {
            return Err(ManagementError::PermissionDenied(
                "Administrator operation must be started through sudo by its owner".into(),
            ));
        }
    }
    let mut request = String::new();
    std::io::stdin()
        .lock()
        .take(MAX_PACKET as u64 + 1)
        .read_line(&mut request)
        .map_err(failure)?;
    if request.len() > MAX_PACKET {
        return Err(ManagementError::InvalidInput(
            "Operation request is too large".into(),
        ));
    }
    let command: ManagementCommand = serde_json::from_str(&request).map_err(failure)?;
    // No request text is retained in diagnostics or placed on a command line.
    request.clear();
    if euid == 0 {
        checked_directory(std::path::Path::new("/run/tundraux3-management"), 0, 0o755)?;
    }
    let base = base_directory(euid, actor);
    checked_directory(&base, euid, 0o755)?;
    let id = format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(failure)?
            .as_nanos(),
        std::process::id()
    );
    let directory = base.join(id);
    checked_directory(&directory, euid, 0o755)?;
    write_operation_metadata(&directory, command.kind)?;
    let private = directory.join("private");
    checked_directory(&private, euid, 0o700)?;
    let mut helper_path = std::env::current_exe().map_err(failure)?;
    if euid == 0 && needs_stable_recovery_binary(&command) {
        let staged = private.join("tundra-cli");
        let mut target = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o500)
            .open(&staged)
            .map_err(failure)?;
        std::io::copy(
            &mut File::open("/proc/self/exe").map_err(failure)?,
            &mut target,
        )
        .map_err(failure)?;
        target.sync_all().map_err(failure)?;
        helper_path = staged;
    }
    let socket = directory.join("control.sock");
    let listener = UnixListener::bind(&socket).map_err(failure)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).map_err(failure)?;
    if euid == 0 {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(socket.as_os_str().as_bytes()).map_err(failure)?;
        if unsafe { libc::chown(name.as_ptr(), actor, u32::MAX) } != 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
    }
    listener.set_nonblocking(true).map_err(failure)?;
    // The early CLI entry has not started threads; fork occurs before watchdog initialization.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    if pid > 0 {
        println!(
            "{}",
            serde_json::to_string(&HelperReady {
                socket,
                process_id: pid as u32
            })
            .map_err(failure)?
        );
        return Ok(());
    }
    unsafe {
        libc::setsid();
    }
    if let Ok(null) = OpenOptions::new().read(true).write(true).open("/dev/null") {
        for fd in 0..=2 {
            unsafe {
                libc::dup2(null.as_raw_fd(), fd);
            }
        }
    }
    let result = serve(
        listener,
        command,
        ExecutionContext {
            actor_uid: actor,
            helper_path,
        },
        &private,
    );
    // Remove only this helper's own directory; no caller can write inside it.
    let _ = fs::remove_file(&socket);
    let _ = fs::remove_dir_all(&private);
    let _ = fs::remove_file(directory.join("metadata.json"));
    let _ = fs::remove_dir(&directory);
    unsafe {
        libc::_exit(if result.is_ok() { 0 } else { 1 });
    }
}

struct WorkerInteraction {
    events: mpsc::SyncSender<OperationEvent>,
    inputs: mpsc::Receiver<OperationInput>,
    cancelled: Arc<AtomicBool>,
    terminal: VecDeque<Vec<u8>>,
    size: Option<(u16, u16)>,
}
impl WorkerInteraction {
    fn other(&mut self, input: OperationInput) {
        match input {
            OperationInput::Terminal { bytes } => {
                if self.terminal.len() < 64 {
                    self.terminal.push_back(bytes);
                }
            }
            OperationInput::Resize { columns, rows } => {
                self.size = Some((columns.max(1), rows.max(1)))
            }
            OperationInput::Cancel => self.cancelled.store(true, Ordering::Relaxed),
            _ => {}
        }
    }
}
impl OperationInteraction for WorkerInteraction {
    fn emit(&mut self, event: OperationEvent) {
        let _ = self.events.send(event);
    }
    fn ask(
        &mut self,
        id: &str,
        prompt: &str,
        choices: &[String],
        secret: bool,
    ) -> Result<String, ManagementError> {
        self.emit(OperationEvent::Question {
            id: id.into(),
            prompt: prompt.into(),
            choices: choices.to_vec(),
            secret,
        });
        let started = Instant::now();
        loop {
            if id == "network-confirm-120" && started.elapsed() >= Duration::from_secs(120) {
                return Err(ManagementError::Cancelled);
            }
            if self.cancelled.load(Ordering::Relaxed) && !id.starts_with("package-config-") {
                return Err(ManagementError::Cancelled);
            }
            match self.inputs.recv_timeout(Duration::from_millis(100)) {
                Ok(OperationInput::Answer {
                    id: answer_id,
                    value,
                }) if answer_id == id => return Ok(value),
                Ok(input) => self.other(input),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(ManagementError::Cancelled),
            }
        }
    }
    fn terminal_input(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, ManagementError> {
        if let Some(bytes) = self.terminal.pop_front() {
            return Ok(Some(bytes));
        }
        match self.inputs.recv_timeout(timeout) {
            Ok(OperationInput::Terminal { bytes }) => Ok(Some(bytes)),
            Ok(input) => {
                self.other(input);
                Ok(None)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(_) => Ok(None),
        }
    }
    fn terminal_size(&mut self) -> Option<(u16, u16)> {
        self.size.take()
    }
}

struct Peer {
    stream: UnixStream,
    incoming: Vec<u8>,
    outgoing: VecDeque<Vec<u8>>,
    offset: usize,
}

#[derive(Clone)]
struct PendingQuestion {
    id: String,
    event: OperationEvent,
}

fn needs_stable_recovery_binary(command: &ManagementCommand) -> bool {
    command.kind == ManagementKind::Network
        && matches!(
            command.action.as_str(),
            "configure" | "wifi-connect" | "wifi-disconnect" | "wifi-forget"
        )
}

fn encode_record(sequence: u64, event: OperationEvent) -> Result<Vec<u8>, ManagementError> {
    let mut bytes = serde_json::to_vec(&OperationRecord { sequence, event }).map_err(failure)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn append_replay(replay: &mut VecDeque<Vec<u8>>, total: &mut usize, bytes: Vec<u8>) {
    *total = total.saturating_add(bytes.len());
    replay.push_back(bytes);
    while *total > MAX_REPLAY {
        if let Some(old) = replay.pop_front() {
            *total = total.saturating_sub(old.len());
        } else {
            break;
        }
    }
}

fn record_event(
    event: OperationEvent,
    sequence: &mut u64,
    replay: &mut VecDeque<Vec<u8>>,
    replay_bytes: &mut usize,
    pending: &mut Option<PendingQuestion>,
) -> Result<Vec<u8>, ManagementError> {
    *sequence += 1;
    let bytes = encode_record(*sequence, event.clone())?;
    match &event {
        OperationEvent::Question { id, .. } => {
            // Questions are current state, not output history. Replaying an
            // already answered question would show an obsolete dialog.
            *pending = Some(PendingQuestion {
                id: id.clone(),
                event,
            });
        }
        OperationEvent::Completed { .. } | OperationEvent::Failed { .. } => {
            *pending = None;
            append_replay(replay, replay_bytes, bytes.clone());
        }
        _ => append_replay(replay, replay_bytes, bytes.clone()),
    }
    Ok(bytes)
}

fn reconnect_outgoing(
    replay: &VecDeque<Vec<u8>>,
    pending: &Option<PendingQuestion>,
    sequence: &mut u64,
    started: Option<&[u8]>,
) -> Result<VecDeque<Vec<u8>>, ManagementError> {
    let mut outgoing = replay.clone();
    if let Some(started) = started {
        // Metadata has its original first sequence and survives output history
        // truncation. It contains no command values or entered passwords.
        outgoing.push_front(started.to_vec());
    }
    if let Some(question) = pending {
        // A fresh sequence also reaches clients that retained their cursor
        // while disconnected. The current question is sent exactly once.
        *sequence += 1;
        outgoing.push_back(encode_record(*sequence, question.event.clone())?);
    }
    Ok(outgoing)
}

/// False means channel backpressure: leave the input packet in the peer's
/// buffer and retry later. Answers and terminal bytes must not disappear.
fn deliver_input(
    input: OperationInput,
    sender: &mpsc::SyncSender<OperationInput>,
    cancelled: &AtomicBool,
    pending: &mut Option<PendingQuestion>,
) -> bool {
    if matches!(input, OperationInput::Cancel) {
        cancelled.store(true, Ordering::Relaxed);
        // Cancellation is observed through the atomic flag even when a
        // worker is not currently reading its interaction channel.
        let _ = sender.try_send(input);
        return true;
    }
    let answer = if let OperationInput::Answer { id, .. } = &input {
        if pending.as_ref().is_none_or(|question| question.id != *id) {
            return true;
        }
        true
    } else {
        false
    };
    match sender.try_send(input) {
        Ok(()) => {
            if answer {
                *pending = None;
            }
            true
        }
        Err(mpsc::TrySendError::Full(_)) => false,
        Err(mpsc::TrySendError::Disconnected(_)) => true,
    }
}

fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(credentials.uid)
}

fn serve(
    listener: UnixListener,
    command: ManagementCommand,
    context: ExecutionContext,
    private: &std::path::Path,
) -> Result<(), ManagementError> {
    let started_record = encode_record(
        1,
        OperationEvent::Started {
            kind: command.kind,
            action: command.action.clone(),
            target: command.target.clone(),
        },
    )?;
    let config = WatchdogConfig::new(
        private.join("crashes"),
        private.join("fallback"),
        private.join("state"),
        "system-operation",
        env!("CARGO_PKG_VERSION"),
    );
    let (runtime, process) = WatchdogRuntime::start(config).map_err(failure)?;
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("system-operation"),
            "System operation",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::ProcessCritical,
        ))
        .map_err(failure)?;
    let (event_tx, event_rx) = mpsc::sync_channel(64);
    let (input_tx, input_rx) = mpsc::sync_channel(32);
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let actor = context.actor_uid;
    let mut worker_io = Some(WorkerInteraction {
        events: event_tx,
        inputs: input_rx,
        cancelled: worker_cancelled.clone(),
        terminal: VecDeque::new(),
        size: None,
    });
    let worker = app
        .task_group("operation")
        .spawn_thread(
            TaskSpec::one_shot(TaskId::from_static("execute")),
            move || {
                // Managed closures implement FnMut; move the channels exactly once.
                // This operation must never be replayed automatically after a panic.
                let Some(mut io) = worker_io.take() else {
                    return;
                };
                let result = execute(&command, &context, &mut io, &worker_cancelled);
                io.emit(match result {
                    Ok(message) => OperationEvent::Completed { message },
                    Err(error) => OperationEvent::Failed {
                        message: error.to_string(),
                    },
                });
            },
        )
        .map_err(failure)?;
    let mut peer: Option<Peer> = None;
    let mut replay = VecDeque::<Vec<u8>>::new();
    let mut replay_bytes = 0;
    let mut pending_question: Option<PendingQuestion> = None;
    let mut sequence = 1;
    let mut finished: Option<Instant> = None;
    loop {
        if let Ok((stream, _)) = listener.accept() {
            if peer_uid(&stream).is_ok_and(|uid| uid == actor || uid == 0) {
                stream.set_nonblocking(true).map_err(failure)?;
                let outgoing = reconnect_outgoing(
                    &replay,
                    &pending_question,
                    &mut sequence,
                    Some(&started_record),
                )?;
                peer = Some(Peer {
                    stream,
                    incoming: Vec::new(),
                    outgoing,
                    offset: 0,
                });
            }
        }
        loop {
            match event_rx.try_recv() {
                Ok(event) => {
                    let terminal = matches!(
                        event,
                        OperationEvent::Completed { .. } | OperationEvent::Failed { .. }
                    );
                    let bytes = record_event(
                        event,
                        &mut sequence,
                        &mut replay,
                        &mut replay_bytes,
                        &mut pending_question,
                    )?;
                    if terminal {
                        finished = Some(Instant::now());
                    }
                    if let Some(p) = &mut peer {
                        if p.outgoing.iter().map(Vec::len).sum::<usize>() < MAX_REPLAY * 2 {
                            p.outgoing.push_back(bytes);
                        } else {
                            peer = None;
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if finished.is_none() {
                        let bytes = record_event(OperationEvent::Failed { message: "Operation worker ended without a verified result; refresh system state before retrying".into() }, &mut sequence, &mut replay, &mut replay_bytes, &mut pending_question)?;
                        if let Some(p) = &mut peer {
                            p.outgoing.push_back(bytes);
                        }
                        finished = Some(Instant::now());
                        pending_question = None;
                    }
                    break;
                }
            }
        }
        let mut disconnected = false;
        if let Some(p) = &mut peer {
            let mut buffer = [0u8; 8192];
            match p.stream.read(&mut buffer) {
                Ok(0) => disconnected = true,
                Ok(n) => p.incoming.extend_from_slice(&buffer[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => disconnected = true,
            }
            if p.incoming.len() > MAX_PACKET {
                disconnected = true;
            }
            while let Some(end) = p.incoming.iter().position(|b| *b == b'\n') {
                match serde_json::from_slice::<OperationInput>(&p.incoming[..=end]) {
                    Ok(input) => {
                        if !deliver_input(input, &input_tx, &cancelled, &mut pending_question) {
                            break;
                        }
                        p.incoming.drain(..=end);
                    }
                    Err(_) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            if let Some(bytes) = p.outgoing.front() {
                match p.stream.write(&bytes[p.offset..]) {
                    Ok(0) => disconnected = true,
                    Ok(n) => {
                        p.offset += n;
                        if p.offset == bytes.len() {
                            p.outgoing.pop_front();
                            p.offset = 0;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => disconnected = true,
                }
            }
        }
        if disconnected {
            peer = None;
        }
        if finished.is_some_and(|at| at.elapsed() > Duration::from_secs(600)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(worker);
    let _ = runtime.shutdown();
    Ok(())
}

/// Enumerate only sockets under the two fixed current-user roots. Connecting verifies peers.
pub fn recoverable_operations(actor: u32) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for owner in [actor, 0] {
        let root = base_directory(owner, actor);
        let Ok(meta) = fs::symlink_metadata(&root) else {
            continue;
        };
        if !meta.is_dir() || meta.uid() != owner || meta.mode() & 0o022 != 0 {
            continue;
        }
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten().take(256) {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    result.push(entry.path().join("control.sock"));
                }
            }
        }
    }
    result.sort();
    result.dedup();
    result
}

/// Read the nonsecret application label used to filter reconnect choices.
pub fn operation_kind(socket: &std::path::Path) -> Option<ManagementKind> {
    if socket.file_name()? != "control.sock" {
        return None;
    }
    let directory = socket.parent()?;
    let owner = fs::symlink_metadata(directory).ok()?;
    let actor = unsafe { libc::getuid() };
    if !owner.is_dir() || (owner.uid() != 0 && owner.uid() != actor) || owner.mode() & 0o022 != 0 {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join("metadata.json"))
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file()
        || metadata.uid() != owner.uid()
        || metadata.mode() & 0o022 != 0
        || metadata.len() > 256
    {
        return None;
    }
    serde_json::from_reader::<_, OperationMetadata>(file.take(257))
        .ok()
        .map(|metadata| metadata.kind)
}

pub fn connect(path: &std::path::Path, actor: u32) -> Result<UnixStream, ManagementError> {
    let stream = UnixStream::connect(path).map_err(failure)?;
    let uid = peer_uid(&stream).map_err(failure)?;
    if uid != 0 && uid != actor {
        return Err(ManagementError::PermissionDenied(
            "Unexpected operation helper identity".into(),
        ));
    }
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(failure)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(failure)?;
    Ok(stream)
}

#[cfg(test)]
#[path = "../../tests/unit/management/helper_tests.rs"]
mod tests;
