//! TUI-side client. Disconnecting cancels this reader, never the system operation.
use platform::management::*;
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, mpsc},
};
#[cfg(target_os = "linux")]
use std::{sync::atomic::Ordering, time::Duration};

#[cfg(target_os = "linux")]
pub(super) fn run(
    command: Option<ManagementCommand>,
    privileged: bool,
    socket: Option<PathBuf>,
    inputs: mpsc::Receiver<OperationInput>,
    detached: &AtomicBool,
    emit: &dyn Fn(OperationEvent),
) -> Result<(), ManagementError> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use zeroize::Zeroizing;
    let failure = |e: std::io::Error| ManagementError::Failed(e.to_string());
    let actor = unsafe { libc::getuid() };
    let mut terminal_size = None;
    let socket = if let Some(socket) = socket {
        socket
    } else {
        let command =
            command.ok_or_else(|| ManagementError::InvalidInput("Missing operation".into()))?;
        let executable = std::env::current_exe()
            .map_err(failure)?
            .with_file_name("tundra-cli");
        if !executable.is_file() {
            return Err(ManagementError::Unavailable(
                "tundra-cli must be beside tundra-shell".into(),
            ));
        }
        if privileged && actor != 0 {
            if !std::path::Path::new("/usr/bin/sudo").is_file() {
                return Err(ManagementError::Unavailable(
                    "This operation requires sudo; it is not installed".into(),
                ));
            }
            let probe = Command::new("/usr/bin/sudo")
                .args(["-n", "-v"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(failure)?;
            let authorized = wait_for_launcher(probe, detached)?.status.success();
            if !authorized {
                let mut success = false;
                for _ in 0..3 {
                    emit(OperationEvent::Question {
                        id: "sudo-password".into(),
                        prompt: i18n::tr!("management-auth-prompt"),
                        choices: Vec::new(),
                        secret: true,
                    });
                    let password = loop {
                        if detached.load(Ordering::Relaxed) {
                            return Err(ManagementError::Cancelled);
                        }
                        match inputs.recv_timeout(Duration::from_millis(100)) {
                            Ok(OperationInput::Answer { id, value }) if id == "sudo-password" => {
                                break Zeroizing::new(value);
                            }
                            Ok(OperationInput::Cancel) => return Err(ManagementError::Cancelled),
                            Ok(OperationInput::Resize { columns, rows }) => {
                                terminal_size = Some((columns, rows));
                            }
                            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(_) => return Err(ManagementError::Cancelled),
                        }
                    };
                    let mut auth = Command::new("/usr/bin/sudo")
                        .args(["-S", "-p", "", "-v"])
                        .stdin(Stdio::piped())
                        .stdout(Stdio::null())
                        .stderr(Stdio::piped())
                        .spawn()
                        .map_err(failure)?;
                    if let Some(mut stdin) = auth.stdin.take() {
                        stdin.write_all(password.as_bytes()).map_err(failure)?;
                        stdin.write_all(b"\n").map_err(failure)?;
                    }
                    drop(password);
                    let result = wait_for_launcher(auth, detached)?;
                    if result.status.success() {
                        success = true;
                        break;
                    }
                    emit(OperationEvent::Progress {
                        message: i18n::tr!("management-auth-failed"),
                        percent: None,
                    });
                }
                if !success {
                    return Err(ManagementError::PermissionDenied(
                        "System authorization failed".into(),
                    ));
                }
            }
        }
        let mut launch = if privileged && actor != 0 {
            let mut c = Command::new("/usr/bin/sudo");
            c.args(["-n", "--"]).arg(&executable);
            c
        } else {
            Command::new(&executable)
        };
        launch
            .arg("__system-helper")
            .arg(actor.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = launch.spawn().map_err(failure)?;
        let mut request = Zeroizing::new(
            serde_json::to_vec(&command)
                .map_err(|e| ManagementError::InvalidInput(e.to_string()))?,
        );
        request.push(b'\n');
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&request).map_err(failure)?;
        }
        drop(request);
        let output = wait_for_launcher(child, detached)?;
        if !output.status.success() {
            return Err(ManagementError::Failed(runtime_log::sanitize_text(
                &String::from_utf8_lossy(&output.stderr),
            )));
        }
        let ready: HelperReady = serde_json::from_slice(&output.stdout).map_err(|_| {
            ManagementError::Failed("The operation helper did not report a valid connection".into())
        })?;
        ready.socket
    };
    let mut stream = platform::management::helper::connect(&socket, actor)?;
    if let Some((columns, rows)) = terminal_size {
        let mut bytes = serde_json::to_vec(&OperationInput::Resize { columns, rows })
            .map_err(|e| ManagementError::Failed(e.to_string()))?;
        bytes.push(b'\n');
        stream.write_all(&bytes).map_err(failure)?;
    }
    let mut buffer = Vec::new();
    let mut last_sequence = 0;
    while !detached.load(Ordering::Relaxed) {
        for input in inputs.try_iter() {
            let mut bytes = Zeroizing::new(
                serde_json::to_vec(&input).map_err(|e| ManagementError::Failed(e.to_string()))?,
            );
            bytes.push(b'\n');
            stream.write_all(&bytes).map_err(failure)?;
        }
        let mut chunk = [0u8; 16384];
        match stream.read(&mut chunk) {
            Ok(0) => {
                return Err(ManagementError::Failed(
                    "Operation connection ended without a verified result; refresh before retrying"
                        .into(),
                ));
            }
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(e) => return Err(failure(e)),
        }
        if buffer.len() > 2 * 1024 * 1024 {
            return Err(ManagementError::Failed(
                "Oversized operation response".into(),
            ));
        }
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let packet = buffer.drain(..=end).collect::<Vec<_>>();
            let record: OperationRecord = serde_json::from_slice(&packet)
                .map_err(|e| ManagementError::Failed(e.to_string()))?;
            if record.sequence <= last_sequence {
                continue;
            }
            last_sequence = record.sequence;
            let done = matches!(
                record.event,
                OperationEvent::Completed { .. } | OperationEvent::Failed { .. }
            );
            emit(record.event);
            if done {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Only the short-lived authorization/launcher process is bounded. The
/// independent operation has its own lifetime and is never killed here.
#[cfg(target_os = "linux")]
fn wait_for_launcher(
    mut child: std::process::Child,
    detached: &AtomicBool,
) -> Result<std::process::Output, ManagementError> {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    let failure = |e: std::io::Error| ManagementError::Failed(e.to_string());
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    for fd in [
        stdout.as_ref().map(AsRawFd::as_raw_fd),
        stderr.as_ref().map(AsRawFd::as_raw_fd),
    ]
    .into_iter()
    .flatten()
    {
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure(std::io::Error::last_os_error()));
            }
        }
    }
    let started = std::time::Instant::now();
    let mut out = Vec::new();
    let mut err = Vec::new();
    loop {
        let status = child.try_wait().map_err(failure)?;
        for (reader, destination) in [
            (stdout.as_mut().map(|r| r as &mut dyn Read), &mut out),
            (stderr.as_mut().map(|r| r as &mut dyn Read), &mut err),
        ] {
            if let Some(reader) = reader {
                let mut chunk = [0u8; 8192];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(count) => {
                            if destination.len() < 128 * 1024 {
                                destination.extend_from_slice(
                                    &chunk[..count.min(128 * 1024 - destination.len())],
                                );
                            }
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                            ) =>
                        {
                            break;
                        }
                        Err(e) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Err(failure(e));
                        }
                    }
                }
            }
        }
        if let Some(status) = status {
            return Ok(std::process::Output {
                status,
                stdout: out,
                stderr: err,
            });
        }
        if detached.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ManagementError::Failed("Authorization or task startup was interrupted. Reconnect to any background operation before retrying.".into()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run(
    _command: Option<ManagementCommand>,
    _privileged: bool,
    _socket: Option<PathBuf>,
    _inputs: mpsc::Receiver<OperationInput>,
    _detached: &AtomicBool,
    _emit: &dyn Fn(OperationEvent),
) -> Result<(), ManagementError> {
    Err(ManagementError::Unavailable(
        "Linux management is unavailable on this platform".into(),
    ))
}
