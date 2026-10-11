//! A terminal shell gives background jobs their own process groups. Keep those
//! jobs within the terminal's session, and include children that start a session
//! of their own, when the user confirms that the whole terminal should close.
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct LinuxProcessTree {
    root: libc::pid_t,
    started: u64,
    terminated: AtomicBool,
}

#[derive(Clone, Copy)]
struct ProcessIdentity {
    pid: libc::pid_t,
    parent: libc::pid_t,
    session: libc::pid_t,
    started: u64,
    stopped: bool,
}

impl LinuxProcessTree {
    pub(super) fn attach(root: libc::pid_t) -> io::Result<Self> {
        let identity = read_identity(root)?;
        if root <= 0 || identity.session != root || identity.session == unsafe { libc::getsid(0) } {
            return Err(io::Error::other(
                "PTY child does not own a separate terminal session",
            ));
        }
        Ok(Self {
            root,
            started: identity.started,
            terminated: AtomicBool::new(false),
        })
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        if self.terminated.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut frozen = BTreeMap::<libc::pid_t, FrozenProcess>::new();
        if let Ok(root) = read_identity(self.root) {
            if root.started != self.started {
                return Err(io::Error::other(
                    "PTY process identity changed before termination",
                ));
            }
            if let Some(process) = FrozenProcess::open(root)? {
                frozen.insert(self.root, process);
            }
        }
        loop {
            let processes = std::fs::read_dir("/proc")?
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
                .filter_map(|pid| read_identity(pid).ok())
                .collect::<Vec<_>>();
            // A process killed by something outside this terminal can vanish
            // even while stopped. Never treat a reused PID as the old parent.
            for process in &processes {
                let expected_start = if process.pid == self.root {
                    Some(self.started)
                } else {
                    frozen.get(&process.pid).map(|process| process.started)
                };
                if expected_start.is_some_and(|started| process.started != started) {
                    return Err(io::Error::other(
                        "PTY process identity changed during termination",
                    ));
                }
            }
            let mut members = frozen.keys().copied().collect::<BTreeSet<_>>();
            members.insert(self.root);
            loop {
                let before = members.len();
                for process in &processes {
                    if process.started >= self.started
                        && (process.session == self.root || members.contains(&process.parent))
                    {
                        members.insert(process.pid);
                    }
                }
                if members.len() == before {
                    break;
                }
            }
            let before = frozen.len();
            for process in processes {
                if !members.contains(&process.pid) || frozen.contains_key(&process.pid) {
                    continue;
                }
                if process.pid == std::process::id() as libc::pid_t {
                    return Err(io::Error::other("PTY process tree includes its host"));
                }
                if let Some(process_handle) = FrozenProcess::open(process)? {
                    frozen.insert(process.pid, process_handle);
                }
            }
            // Stopping parents before rescanning prevents them from adding new
            // descendants while the tree is being terminated.
            if frozen.len() == before {
                break;
            }
        }
        for process in frozen.values_mut() {
            process.kill()?;
        }
        self.terminated.store(true, Ordering::Release);
        Ok(())
    }
}

fn read_identity(pid: libc::pid_t) -> io::Result<ProcessIdentity> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields = stat
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::other("invalid process identity"))?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let number = |index: usize| {
        fields
            .get(index)
            .ok_or_else(|| io::Error::other("incomplete process identity"))?
            .parse::<u64>()
            .map_err(io::Error::other)
    };
    Ok(ProcessIdentity {
        pid,
        parent: number(1)? as libc::pid_t,
        session: number(3)? as libc::pid_t,
        started: number(19)?,
        stopped: matches!(fields.first().copied(), Some("T" | "t" | "Z" | "X")),
    })
}

struct FrozenProcess {
    fd: OwnedFd,
    started: u64,
    resume: bool,
}
impl FrozenProcess {
    fn open(identity: ProcessIdentity) -> io::Result<Option<Self>> {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, identity.pid, 0) };
        if fd < 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(libc::ESRCH) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        // The process may have exited between the /proc scan and pidfd_open.
        // Check again after opening; signals through this descriptor can never
        // be delivered to a later process that happens to reuse the same PID.
        if read_identity(identity.pid)
            .ok()
            .is_none_or(|current| current.started != identity.started)
        {
            return Ok(None);
        }
        let process = Self {
            fd,
            started: identity.started,
            resume: !identity.stopped,
        };
        process.signal(libc::SIGSTOP)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while read_identity(identity.pid)
            .ok()
            .is_some_and(|current| current.started == identity.started && !current.stopped)
        {
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::other(
                    "PTY descendant did not stop before termination",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(Some(process))
    }
    fn signal(&self, signal: i32) -> io::Result<()> {
        if unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.fd.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        } == 0
        {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }
    fn kill(&mut self) -> io::Result<()> {
        self.signal(libc::SIGKILL)?;
        self.resume = false;
        Ok(())
    }
}
impl Drop for FrozenProcess {
    fn drop(&mut self) {
        if self.resume {
            let _ = self.signal(libc::SIGCONT);
        }
    }
}
