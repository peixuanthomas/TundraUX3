//! Fedora's text agent owns terminal input directly; Tundra never pipes its prompts.
use crate::service::ServiceError;
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub struct TextAgent {
    child: Child,
}
impl TextAgent {
    pub fn register() -> Result<Self, ServiceError> {
        // Fedora 43 accepts agent registration for unix-process/session subjects only.
        // Bind our own live process, whose stable D-Bus connection makes the request.
        let stat = std::fs::read_to_string("/proc/self/stat")
            .map_err(|_| ServiceError::ServiceUnavailable)?;
        let started = process_start_time(&stat)?;
        let process = format!("{},{}", std::process::id(), started);
        let tty = controlling_terminal()?;
        let mut descriptors = [-1; 2];
        // The registration pipe contains only pkttyagent's readiness notification.
        if unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0
        {
            return Err(ServiceError::ServiceUnavailable);
        }
        let reader = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
        let writer = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
        let notify_fd = writer.as_raw_fd();
        let mut command = Command::new("/usr/bin/pkttyagent");
        command
            .args([
                "--process",
                &process,
                "--notify-fd",
                &notify_fd.to_string(),
                "--fallback",
            ])
            .stdin(Stdio::from(
                tty.try_clone()
                    .map_err(|_| ServiceError::ServiceUnavailable)?,
            ))
            .stdout(Stdio::from(
                tty.try_clone()
                    .map_err(|_| ServiceError::ServiceUnavailable)?,
            ))
            .stderr(Stdio::from(tty));
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(notify_fd, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut agent = Self {
            child: command
                .spawn()
                .map_err(|_| ServiceError::ServiceUnavailable)?,
        };
        drop(writer);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if !agent.alive()? {
                return Err(ServiceError::AuthorizationCancelled);
            }
            let mut descriptor = libc::pollfd {
                fd: reader.as_raw_fd(),
                events: libc::POLLIN | libc::POLLHUP,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut descriptor, 1, 50) };
            if ready < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                return Err(ServiceError::ServiceUnavailable);
            }
            if ready > 0 {
                let mut byte = 0_u8;
                let count =
                    unsafe { libc::read(reader.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) };
                if count == 0 && agent.alive()? {
                    return Ok(agent);
                }
                if count > 0 {
                    return Err(ServiceError::Unknown);
                }
            }
            if Instant::now() >= deadline {
                return Err(ServiceError::Timeout);
            }
        }
    }
    pub fn alive(&mut self) -> Result<bool, ServiceError> {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|_| ServiceError::Unknown)
    }
}
impl Drop for TextAgent {
    fn drop(&mut self) {
        if self.child.try_wait().is_ok_and(|status| status.is_some()) {
            return;
        }
        // Reap only our authentication agent. PackageKit/RPM processes are never touched.
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
        }
        for _ in 0..20 {
            if self.child.try_wait().is_ok_and(|status| status.is_some()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Read/write access is passed to the agent only. This function never reads terminal bytes.
pub fn controlling_terminal() -> Result<File, ServiceError> {
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| ServiceError::ServiceUnavailable)?;
    let safe = unsafe {
        libc::isatty(0) == 1
            && libc::isatty(1) == 1
            && libc::tcgetsid(0) == libc::getsid(0)
            && libc::tcgetsid(1) == libc::getsid(0)
            && libc::tcgetpgrp(0) == libc::getpgrp()
            && libc::tcgetpgrp(1) == libc::getpgrp()
            && libc::tcgetpgrp(tty.as_raw_fd()) == libc::getpgrp()
    };
    if !safe {
        return Err(ServiceError::PermissionDenied);
    }
    Ok(tty)
}

fn process_start_time(stat: &str) -> Result<u64, ServiceError> {
    // comm (field 2) can contain spaces and parentheses; starttime is field 22.
    stat.rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .and_then(|value| value.parse().ok())
        .filter(|value| *value != 0)
        .ok_or(ServiceError::Unknown)
}

#[cfg(test)]
#[path = "../../tests/unit/linux/authorization_tty/tests.rs"]
mod tests;
