use super::{ProcessResourceGuard, process_session_registration_enabled, register_process_group};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;

/// Launch a process group, holding exec until its guardian recognizes the group.
pub fn spawn_registered_process_group(
    command: &mut Command,
) -> io::Result<(Child, Option<ProcessResourceGuard>)> {
    command.process_group(0);
    if !process_session_registration_enabled() {
        return command.spawn().map(|child| (child, None));
    }

    let (mut monitor, gate) = UnixStream::pair()?;
    gate.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
    let armed = Arc::new(AtomicI32::new(gate.as_raw_fd()));
    let child_armed = armed.clone();
    // The forked child only uses native I/O. The parent thread registers the group.
    unsafe {
        command.pre_exec(move || {
            let socket = child_armed.load(Ordering::Relaxed) as RawFd;
            if socket < 0 {
                return Ok(());
            }
            let pid = libc::getpid() as u32;
            let mut decision = [0_u8];
            write_gate(socket, &pid.to_ne_bytes())?;
            read_gate(socket, &mut decision)?;
            if decision != [1] {
                return Err(io::Error::from_raw_os_error(libc::EPERM));
            }
            Ok(())
        });
    }
    let registration = thread::Builder::new()
        .name("watchdog-child-registration".into())
        .spawn(move || {
            let mut bytes = [0; std::mem::size_of::<u32>()];
            let result = monitor
                .read_exact(&mut bytes)
                .and_then(|()| register_process_group(u32::from_ne_bytes(bytes)));
            let _ = monitor.write_all(&[u8::from(result.is_ok())]);
            result
        });
    let registration = match registration {
        Ok(registration) => registration,
        Err(error) => {
            armed.store(-1, Ordering::Relaxed);
            return Err(error);
        }
    };
    let result = command.spawn();
    armed.store(-1, Ordering::Relaxed);
    // If spawn failed before pre_exec, waking the registrar prevents a stuck join.
    let _ = gate.shutdown(Shutdown::Both);
    let registration = registration
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("child registration failed")));
    match result {
        Ok(mut child) => match registration {
            Ok(registration) => Ok((child, registration)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        },
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
            Err(registration.err().unwrap_or(error))
        }
        Err(error) => Err(error),
    }
}

unsafe fn write_gate(socket: RawFd, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let count = unsafe { libc::write(socket, bytes.as_ptr().cast(), bytes.len()) };
        if count > 0 {
            bytes = &bytes[count as usize..];
        } else {
            check_transfer(count)?;
        }
    }
    Ok(())
}

unsafe fn read_gate(socket: RawFd, mut bytes: &mut [u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let count = unsafe { libc::read(socket, bytes.as_mut_ptr().cast(), bytes.len()) };
        if count > 0 {
            bytes = &mut bytes[count as usize..];
        } else {
            check_transfer(count)?;
        }
    }
    Ok(())
}

fn check_transfer(count: libc::ssize_t) -> io::Result<()> {
    if count == 0 {
        return Err(io::Error::from_raw_os_error(libc::EPIPE));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EINTR) {
        Ok(())
    } else {
        Err(error)
    }
}
