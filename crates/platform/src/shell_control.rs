//! Same-user local connections. Request contents are never logged or written to disk.
#[cfg(unix)]
use interprocess::local_socket::{Listener, ListenerNonblockingMode, ListenerOptions};
use interprocess::local_socket::{Stream, prelude::*};
use serde::de::DeserializeOwned;
use shell_control::{MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, SessionInfo};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(windows)]
use windows::Listener;

#[cfg(windows)]
mod windows;

const MAX_CLIENTS: usize = 16;
const IO_CHUNK: usize = 8192;
const IO_ROUNDS: usize = 8;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
const WATCHDOG_REQUEST_BYTES: usize = 4096;
const WATCHDOG_RESPONSE_BYTES: usize = 1024 * 1024;
const WATCHDOG_HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(200);
const WATCHDOG_RESPONSE_TIMEOUT: Duration = Duration::from_secs(1);

pub fn directory(paths: &crate::AppPaths) -> PathBuf {
    paths.cache_path().join("shell-control")
}

pub fn watchdog_directory(paths: &crate::AppPaths) -> PathBuf {
    paths.cache_path().join("shell-watchdog")
}

pub fn create_private_directory(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    crate::validate_no_follow_path(path, false).map_err(io::Error::other)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        let meta = fs::symlink_metadata(path)?;
        // SAFETY: geteuid has no preconditions.
        if !meta.is_dir()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o777 != 0o700
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    #[cfg(windows)]
    windows::private_directory(path)?;
    crate::validate_no_follow_path(path, true).map_err(io::Error::other)
}

pub struct LocalListener(Listener);

impl LocalListener {
    pub fn bind(endpoint: &str) -> io::Result<Self> {
        #[cfg(unix)]
        let listener = ListenerOptions::new()
            .name(name(endpoint)?)
            .nonblocking(ListenerNonblockingMode::Both)
            .create_sync()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(endpoint, fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(windows)]
        let listener = windows::Listener::new(endpoint)?;
        Ok(Self(listener))
    }

    pub fn accept(&self) -> io::Result<LocalConnection> {
        let connection = LocalConnection(self.0.accept()?);
        same_user(&connection.0)?;
        Ok(connection)
    }
}

/// Nonblocking stream with operating-system credentials on both ends.
pub struct LocalConnection(Stream);

impl LocalConnection {
    pub fn connect(endpoint: &str, expected_pid: u32) -> io::Result<Self> {
        connect_verified(endpoint, expected_pid).map(Self)
    }

    pub fn peer_pid(&self) -> io::Result<u32> {
        peer_pid(&self.0)?.ok_or_else(|| io::ErrorKind::PermissionDenied.into())
    }

    #[cfg(target_os = "linux")]
    pub fn peer_process(&self) -> io::Result<std::os::fd::OwnedFd> {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        let Stream::UdSocket(stream) = &self.0;
        let mut fd: libc::c_int = -1;
        let mut size = size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: for SO_PEERPIDFD, getsockopt returns an owned process descriptor.
        if unsafe {
            libc::getsockopt(
                stream.inner().as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERPIDFD,
                (&mut fd as *mut libc::c_int).cast(),
                &mut size,
            )
        } == 0
        {
            return Ok(unsafe { OwnedFd::from_raw_fd(fd) });
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOPROTOOPT) {
            return Err(error);
        }
        let pid = self.peer_pid()?;
        // Older Linux kernels can open a pidfd for the peer PID from SO_PEERCRED.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
        }
    }

    /// Gives larger payloads room to move between polls. Kernel limits may cap the requested size.
    #[cfg(unix)]
    pub fn increase_buffer_capacity(&self, bytes: libc::c_int) {
        use std::os::fd::AsRawFd;

        let Stream::UdSocket(stream) = &self.0;
        for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
            // SAFETY: the stream descriptor and integer option remain valid for the call.
            unsafe {
                libc::setsockopt(
                    stream.inner().as_raw_fd(),
                    libc::SOL_SOCKET,
                    option,
                    (&raw const bytes).cast(),
                    size_of_val(&bytes) as libc::socklen_t,
                );
            }
        }
    }

    #[cfg(unix)]
    pub fn into_unix_stream(self) -> std::os::unix::net::UnixStream {
        let Stream::UdSocket(stream) = self.0;
        let fd: std::os::fd::OwnedFd = stream.into();
        fd.into()
    }
}

impl Read for LocalConnection {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        read_nonblocking(&mut self.0, bytes)
    }
}

impl Write for LocalConnection {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        write_nonblocking(&mut self.0, bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Named pipe flush waits for the other process to consume all bytes.
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for LocalConnection {
    fn drop(&mut self) {
        windows::cancel_flush(&self.0);
    }
}

#[cfg(unix)]
fn name(endpoint: &str) -> io::Result<interprocess::local_socket::Name<'_>> {
    endpoint.to_fs_name::<interprocess::local_socket::GenericFilePath>()
}

fn same_user(stream: &Stream) -> io::Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions.
        if stream.peer_creds()?.euid() != Some(unsafe { libc::geteuid() }) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    #[cfg(windows)]
    let _ = stream; // The pipe DACL allows only the current user's SID and denies network users.
    Ok(())
}

fn peer_pid(stream: &Stream) -> io::Result<Option<u32>> {
    #[cfg(not(target_os = "macos"))]
    {
        Ok(stream.peer_creds()?.pid().map(|pid| pid as u32))
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        let Stream::UdSocket(stream) = stream;
        let mut pid: libc::pid_t = 0;
        let mut size = std::mem::size_of_val(&pid) as libc::socklen_t;
        // SAFETY: pid and size point to live, correctly sized output storage.
        if unsafe {
            libc::getsockopt(
                stream.inner().as_raw_fd(),
                0,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut size,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(pid as u32))
    }
}

/// Tries once; a busy listener returns immediately so callers can apply their own deadline.
pub fn connect(info: &SessionInfo) -> io::Result<Stream> {
    #[cfg(windows)]
    if !valid_endpoint(Path::new(""), info) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    connect_verified(&info.endpoint, info.pid)
}

fn connect_verified(endpoint: &str, expected_pid: u32) -> io::Result<Stream> {
    #[cfg(unix)]
    let stream = interprocess::local_socket::ConnectOptions::new()
        .name(name(endpoint)?)
        .wait_mode(interprocess::ConnectWaitMode::Timeout(Duration::ZERO))
        .nonblocking_stream(true)
        .connect_sync()?;
    #[cfg(windows)]
    let stream = windows::connect(endpoint)?;
    same_user(&stream)?;
    if peer_pid(&stream)? != Some(expected_pid) {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(stream)
}

pub fn sessions(directory: &Path) -> io::Result<Vec<SessionInfo>> {
    discover(directory, |info| {
        valid_endpoint(directory, info) && connect(info).is_ok()
    })
}

pub fn watchdog_sessions(directory: &Path) -> io::Result<Vec<SessionInfo>> {
    discover(directory, |info| {
        valid_watchdog_endpoint(info) && LocalConnection::connect(&info.endpoint, info.pid).is_ok()
    })
}

fn discover(
    directory: &Path,
    mut usable: impl FnMut(&SessionInfo) -> bool,
) -> io::Result<Vec<SessionInfo>> {
    if !directory.try_exists()? {
        return Ok(Vec::new());
    }
    create_private_directory(directory)?;
    let mut sessions = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let data = match crate::read_document_bytes_limited(&path, 4096) {
            Ok(data) => data,
            Err(_) => continue,
        };
        let Ok(info) = serde_json::from_slice::<SessionInfo>(&data.bytes) else {
            continue;
        };
        if path.file_stem().and_then(|stem| stem.to_str()) == Some(info.session.as_str())
            && usable(&info)
        {
            sessions.push(info);
        }
    }
    sessions.sort_by(|a, b| a.session.cmp(&b.session));
    Ok(sessions)
}

fn valid_endpoint(directory: &Path, info: &SessionInfo) -> bool {
    let valid = !info.session.is_empty()
        && info.session.len() <= 64
        && info
            .session
            .bytes()
            .all(|c| c.is_ascii_hexdigit() || c == b'-');
    #[cfg(unix)]
    let expected = directory
        .join(format!("{}.sock", info.session))
        .to_string_lossy()
        .into_owned();
    #[cfg(windows)]
    let expected = {
        let _ = directory;
        format!("tundra-shell-{}", info.session)
    };
    valid && info.pid != 0 && info.endpoint == expected
}

fn valid_watchdog_endpoint(info: &SessionInfo) -> bool {
    if info.pid == 0
        || info.session.len() != 32
        || !info
            .session
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return false;
    }
    #[cfg(windows)]
    return info.endpoint == format!("tundra-watchdog-{}", info.session);
    #[cfg(unix)]
    {
        let path = Path::new(&info.endpoint);
        path.is_absolute()
            && path.file_name().and_then(|file| file.to_str()) == Some("watchdog")
            && path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                == Some(format!(".tux-{}", info.session).as_str())
    }
}

struct Client {
    stream: LocalConnection,
    incoming: Vec<u8>,
    outgoing: Vec<u8>,
    sent: usize,
    requested: bool,
    responded: bool,
    since: Instant,
}

pub struct Server {
    listener: Option<LocalListener>,
    pub info: SessionInfo,
    registration: PathBuf,
    clients: BTreeMap<u64, Client>,
    next_client: u64,
    limits: ServerLimits,
}

struct ServerLimits {
    request_bytes: usize,
    response_bytes: usize,
    handshake_timeout: Duration,
    response_timeout: Duration,
}

impl Server {
    pub fn start(directory: &Path) -> io::Result<Self> {
        let pid = std::process::id();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let session = format!("{pid:x}-{nonce:x}");
        #[cfg(unix)]
        let endpoint = directory
            .join(format!("{session}.sock"))
            .to_string_lossy()
            .into_owned();
        #[cfg(windows)]
        let endpoint = format!("tundra-shell-{session}");
        let info = SessionInfo {
            session,
            pid,
            endpoint,
        };
        Self::start_with_limits(
            directory,
            info,
            ServerLimits {
                request_bytes: MAX_REQUEST_BYTES,
                response_bytes: MAX_RESPONSE_BYTES,
                handshake_timeout: CLIENT_TIMEOUT,
                response_timeout: CLIENT_TIMEOUT,
            },
        )
    }

    pub fn start_watchdog(directory: &Path, session: String, endpoint: String) -> io::Result<Self> {
        let info = SessionInfo {
            session,
            pid: std::process::id(),
            endpoint,
        };
        if !valid_watchdog_endpoint(&info) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Self::start_with_limits(
            directory,
            info,
            ServerLimits {
                request_bytes: WATCHDOG_REQUEST_BYTES,
                response_bytes: WATCHDOG_RESPONSE_BYTES,
                handshake_timeout: WATCHDOG_HANDSHAKE_TIMEOUT,
                response_timeout: WATCHDOG_RESPONSE_TIMEOUT,
            },
        )
    }

    fn start_with_limits(
        directory: &Path,
        info: SessionInfo,
        limits: ServerLimits,
    ) -> io::Result<Self> {
        create_private_directory(directory)?;
        let listener = LocalListener::bind(&info.endpoint)?;
        let registration = directory.join(format!("{}.json", info.session));
        let server = Self {
            listener: Some(listener),
            info,
            registration,
            clients: BTreeMap::new(),
            next_client: 0,
            limits,
        };
        crate::atomic_write_document(&server.registration, &serde_json::to_vec(&server.info)?)
            .map_err(io::Error::other)?;
        Ok(server)
    }

    /// Polls bounded nonblocking I/O, leaving UI state on the Shell thread.
    pub fn poll(&mut self) -> Vec<(u64, Vec<u8>)> {
        self.clients.retain(|_, client| {
            let timeout = if client.requested {
                self.limits.response_timeout
            } else {
                self.limits.handshake_timeout
            };
            client.since.elapsed() < timeout
        });
        for _ in 0..MAX_CLIENTS {
            let Some(listener) = &self.listener else {
                break;
            };
            let stream = match listener.accept() {
                Ok(stream) => stream,
                Err(_) => break,
            };
            if self.clients.len() >= MAX_CLIENTS {
                continue;
            }
            self.next_client += 1;
            self.clients.insert(
                self.next_client,
                Client {
                    stream,
                    incoming: Vec::new(),
                    outgoing: Vec::new(),
                    sent: 0,
                    requested: false,
                    responded: false,
                    since: Instant::now(),
                },
            );
        }
        let mut requests = Vec::new();
        self.clients.retain(|id, client| {
            let mut buffer = [0u8; IO_CHUNK];
            for _ in 0..IO_ROUNDS {
                match client.stream.read(&mut buffer) {
                    Ok(0) => return false,
                    Ok(len) if !client.requested => {
                        if client.incoming.len() + len > self.limits.request_bytes {
                            return false;
                        }
                        if let Some(end) = buffer[..len].iter().position(|b| *b == b'\n') {
                            if end + 1 != len {
                                return false;
                            }
                            client.incoming.extend_from_slice(&buffer[..end]);
                            requests.push((*id, std::mem::take(&mut client.incoming)));
                            client.requested = true;
                        } else {
                            client.incoming.extend_from_slice(&buffer[..len]);
                        }
                    }
                    Ok(_) => return false,
                    Err(error) if pending(&error) => break,
                    Err(_) => return false,
                }
            }
            if client.sent < client.outgoing.len() {
                let end = (client.sent + IO_CHUNK * IO_ROUNDS).min(client.outgoing.len());
                match client.stream.write(&client.outgoing[client.sent..end]) {
                    Ok(len) => client.sent += len,
                    Err(error) if pending(&error) => {}
                    Err(_) => return false,
                }
                if client.sent == client.outgoing.len() {
                    // Windows must keep the pipe open until the peer has consumed the response.
                    client.outgoing.clear();
                    client.sent = 0;
                }
            }
            true
        });
        requests.retain(|(id, _)| self.clients.contains_key(id));
        requests
    }

    /// Stops discovery and new connections while existing responses are drained.
    pub fn stop_accepting(&mut self) {
        self.listener = None;
        let _ = fs::remove_file(&self.registration);
    }

    pub fn connected(&self, id: u64) -> bool {
        self.clients.contains_key(&id)
    }

    pub fn has_pending_responses(&self) -> bool {
        self.clients
            .values()
            .any(|client| !client.outgoing.is_empty() || (cfg!(windows) && client.responded))
    }

    pub fn respond(&mut self, id: u64, response: &shell_control::Response) {
        self.respond_json(id, response);
    }

    pub fn respond_json(&mut self, id: u64, response: &impl serde::Serialize) {
        if let Some(client) = self.clients.get_mut(&id) {
            if !client.requested || client.responded {
                return;
            }
            match encode(response, self.limits.response_bytes) {
                Ok(bytes) => {
                    client.outgoing = bytes;
                    client.responded = true;
                }
                Err(_) => {
                    self.clients.remove(&id);
                }
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.registration);
    }
}

fn pending(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

fn read_nonblocking(stream: &mut Stream, bytes: &mut [u8]) -> io::Result<usize> {
    #[cfg(windows)]
    {
        windows::read_nonblocking(stream, bytes)
    }
    #[cfg(unix)]
    {
        stream.read(bytes)
    }
}

fn write_nonblocking(stream: &mut Stream, bytes: &[u8]) -> io::Result<usize> {
    match stream.write(bytes) {
        #[cfg(windows)]
        Ok(0) => Err(io::ErrorKind::WouldBlock.into()),
        #[cfg(not(windows))]
        Ok(0) => Err(io::ErrorKind::WriteZero.into()),
        result => result,
    }
}

struct MessageBuffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for MessageBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len() + bytes.len() >= self.limit {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode(value: &impl serde::Serialize, limit: usize) -> io::Result<Vec<u8>> {
    let mut buffer = MessageBuffer {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut buffer, value)
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    buffer.bytes.push(b'\n');
    Ok(buffer.bytes)
}

pub fn exchange(
    info: &SessionInfo,
    request: &shell_control::Request,
    timeout: Duration,
) -> io::Result<shell_control::Response> {
    exchange_with_limits(
        info,
        request,
        MAX_REQUEST_BYTES,
        MAX_RESPONSE_BYTES,
        timeout,
        connect,
    )
}

pub fn watchdog_exchange<Request: serde::Serialize, Reply: DeserializeOwned>(
    info: &SessionInfo,
    request: &Request,
    timeout: Duration,
) -> io::Result<Reply> {
    if !valid_watchdog_endpoint(info) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    exchange_with_limits(
        info,
        request,
        WATCHDOG_REQUEST_BYTES,
        WATCHDOG_RESPONSE_BYTES,
        timeout,
        |info| connect_verified(&info.endpoint, info.pid),
    )
}

fn exchange_with_limits<Reply: DeserializeOwned>(
    info: &SessionInfo,
    request: &impl serde::Serialize,
    max_request_bytes: usize,
    max_response_bytes: usize,
    timeout: Duration,
    connect: impl Fn(&SessionInfo) -> io::Result<Stream>,
) -> io::Result<Reply> {
    let start = Instant::now();
    let outgoing = encode(request, max_request_bytes)?;
    let mut stream = loop {
        if start.elapsed() >= timeout {
            return Err(io::ErrorKind::TimedOut.into());
        }
        match connect(info) {
            Ok(stream) => break stream,
            Err(error) if pending(&error) || error.kind() == io::ErrorKind::TimedOut => {
                std::thread::sleep(
                    Duration::from_millis(5).min(timeout.saturating_sub(start.elapsed())),
                );
            }
            Err(error) => return Err(error),
        }
    };
    let result = exchange_connected(&mut stream, &outgoing, max_response_bytes, start, timeout);
    #[cfg(windows)]
    windows::cancel_flush(&stream);
    result
}

fn exchange_connected<Reply: DeserializeOwned>(
    stream: &mut Stream,
    outgoing: &[u8],
    max_response_bytes: usize,
    start: Instant,
    timeout: Duration,
) -> io::Result<Reply> {
    let mut sent = 0;
    let mut incoming = Vec::new();
    loop {
        if start.elapsed() >= timeout {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if sent < outgoing.len() {
            let end = (sent + IO_CHUNK * IO_ROUNDS).min(outgoing.len());
            match write_nonblocking(stream, &outgoing[sent..end]) {
                Ok(len) => sent += len,
                Err(error) if pending(&error) => {}
                Err(error) => return Err(error),
            }
        }
        let mut buffer = [0u8; IO_CHUNK];
        match read_nonblocking(stream, &mut buffer) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(len) => {
                if incoming.len() + len > max_response_bytes {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                incoming.extend_from_slice(&buffer[..len]);
                if let Some(end) = buffer[..len].iter().position(|b| *b == b'\n') {
                    if end + 1 != len {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                    return serde_json::from_slice(&incoming)
                        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData));
                }
            }
            Err(error) if pending(&error) => {
                std::thread::sleep(
                    Duration::from_millis(5).min(timeout.saturating_sub(start.elapsed())),
                );
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/shell_control/tests.rs"]
mod tests;
