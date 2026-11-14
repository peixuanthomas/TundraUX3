use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

#[derive(Clone, Copy)]
pub(super) struct ProcessIdentity {
    pub(super) pid: libc::pid_t,
    pub(super) parent: libc::pid_t,
    pub(super) group: libc::pid_t,
    pub(super) session: libc::pid_t,
    pub(super) birth: u64,
    pub(super) dead: bool,
}

pub(super) fn read_identity(pid: libc::pid_t) -> io::Result<ProcessIdentity> {
    let stat = std::fs::read(format!("/proc/{pid}/stat"))?;
    let end = stat
        .iter()
        .rposition(|&byte| byte == b')')
        .ok_or_else(invalid_stat)?;
    let fields = std::str::from_utf8(&stat[end + 1..])
        .map_err(io::Error::other)?
        .split_whitespace()
        .take(20)
        .collect::<Vec<_>>();
    let number = |index: usize| {
        fields
            .get(index)
            .ok_or_else(invalid_stat)?
            .parse::<u64>()
            .map_err(io::Error::other)
    };
    Ok(ProcessIdentity {
        pid,
        parent: number(1)? as libc::pid_t,
        group: number(2)? as libc::pid_t,
        session: number(3)? as libc::pid_t,
        birth: number(19)?,
        dead: matches!(fields[0], "Z" | "X" | "x"),
    })
}

fn invalid_stat() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid process identity")
}

pub(super) fn list_process_ids(group: Option<libc::pid_t>) -> io::Result<Vec<libc::pid_t>> {
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        if group.is_none_or(|group| unsafe { libc::getpgid(pid) } == group) {
            pids.push(pid);
        }
    }
    Ok(pids)
}

impl ProcessIdentity {
    pub(super) fn has_parent(&self, parent: &Self) -> bool {
        self.parent == parent.pid && self.birth >= parent.birth
    }

    pub(super) fn kill(&self) -> io::Result<()> {
        let fd = match observe(self) {
            Ok(fd) => fd,
            Err(error) if super::vanished(&error) => return Ok(()),
            Err(error) => return Err(error),
        };
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if super::vanished(&error) {
            Ok(())
        } else {
            Err(error)
        }
    }
}

pub(super) fn observe(process: &ProcessIdentity) -> io::Result<OwnedFd> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, process.pid, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    if read_identity(process.pid)?.birth != process.birth {
        return Err(io::Error::from_raw_os_error(libc::ESRCH));
    }
    Ok(fd)
}

pub(super) fn exited(handle: &OwnedFd) -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd: handle.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
    if result >= 0 {
        Ok(descriptor.revents & (libc::POLLIN | libc::POLLHUP) != 0)
    } else {
        Err(io::Error::last_os_error())
    }
}
