//! Signals only processes owned by one operation. Keep pidfds so a recycled PID
//! can never turn a delayed force-stop into a signal to an unrelated process.
use std::collections::BTreeMap;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

struct Process {
    fd: OwnedFd,
    signalled: i32,
}

impl Process {
    fn open(pid: u32) -> io::Result<Self> {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            signalled: 0,
        })
    }

    fn alive(&self) -> bool {
        let mut fd = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // An interrupted poll is not evidence that the process exited.
        unsafe { libc::poll(&mut fd, 1, 0) <= 0 }
    }

    fn signal(&mut self, signal: i32) -> io::Result<()> {
        if self.signalled == signal || !self.alive() {
            return Ok(());
        }
        if unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.fd.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        } < 0
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        self.signalled = signal;
        Ok(())
    }
}

/// The root must be our own process or an unreaped child owned by the caller.
/// The helper excludes itself, keeping its control loop responsive while its
/// operation thread waits for the child programs to exit.
pub struct ProcessTree {
    root: u32,
    include_root: bool,
    processes: BTreeMap<u32, Process>,
}

impl ProcessTree {
    pub fn child(pid: u32) -> io::Result<Self> {
        Self::new(pid, true)
    }

    pub(super) fn helper_children() -> io::Result<Self> {
        Self::new(std::process::id(), false)
    }

    fn new(root: u32, include_root: bool) -> io::Result<Self> {
        if root <= 1 {
            return Err(io::Error::other("Invalid task process"));
        }
        Ok(Self {
            root,
            include_root,
            processes: BTreeMap::from([(root, Process::open(root)?)]),
        })
    }

    fn refresh(&mut self) -> io::Result<()> {
        let mut parents: Vec<_> = self.processes.keys().copied().collect();
        while let Some(parent) = parents.pop() {
            if !self.processes[&parent].alive() {
                continue;
            }
            let tasks = match std::fs::read_dir(format!("/proc/{parent}/task")) {
                Ok(tasks) => tasks,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            for task in tasks {
                let children = match std::fs::read_to_string(task?.path().join("children")) {
                    Ok(children) => children,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                };
                for pid in children
                    .split_whitespace()
                    .filter_map(|p| p.parse::<u32>().ok())
                {
                    if self.processes.get(&pid).is_some_and(Process::alive) {
                        continue;
                    }
                    let process = match Process::open(pid) {
                        Ok(process) => process,
                        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => continue,
                        Err(error) => return Err(error),
                    };
                    // Recheck the relationship after opening the stable handle.
                    // A child may have exited between reading children and open.
                    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                        Ok(stat) => stat,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(error) => return Err(error),
                    };
                    let ppid = stat.rsplit_once(')').and_then(|(_, fields)| {
                        fields.split_whitespace().nth(1)?.parse::<u32>().ok()
                    });
                    if ppid == Some(parent) && process.alive() && self.processes[&parent].alive() {
                        self.processes.insert(pid, process);
                        parents.push(pid);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn signal(&mut self, force: bool) -> io::Result<()> {
        self.refresh()?;
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        let mut failure = None;
        // Retain descendant handles even when their parent exits so a later
        // force-stop still reaches them. Each signal uses the stable handle.
        for (&pid, process) in self.processes.iter_mut().rev() {
            if (pid != self.root || self.include_root)
                && let Err(error) = process.signal(signal)
            {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub fn has_live_children(&self) -> bool {
        self.processes
            .iter()
            .any(|(&pid, process)| (pid != self.root || self.include_root) && process.alive())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/management/termination_tests.rs"]
mod tests;
