//! Keep daemonized embedded commands attached to this CLI until Shell closes it.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

pub(super) fn enable() -> io::Result<()> {
    // Only the embedded CLI becomes a subreaper. Orphaned grandchildren then
    // become its children even after double-fork and setsid, so Shell can still
    // find the whole tree without adopting another terminal's processes.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn finish(shutdown: impl FnOnce()) {
    // Protect the root during watchdog shutdown too, then reap only after all
    // workers have stopped creating children. It must outlive its descendants.
    let mut signals = None;
    let mut shutdown = Some(shutdown);
    let mut signalled = BTreeMap::<libc::pid_t, OwnedFd>::new();
    let mut previous_error = None;
    let mut announced = false;
    loop {
        let result = (|| -> io::Result<bool> {
            if signals.is_none() {
                signals = Some(CleanupSignals::install()?);
            }
            if let Some(shutdown) = shutdown.take() {
                shutdown();
            }
            if reap_finished_children()? {
                return Ok(true);
            }
            if !announced {
                let _ = writeln!(
                    std::io::stderr(),
                    "Waiting for Command Line child processes to stop. Use Shell's terminate action if they do not exit."
                );
                announced = true;
            }
            // An exited child can be reaped and its PID reused before the next
            // pass. Its pidfd must not suppress TERM for a new child with that PID.
            signalled.retain(|_, fd| !process_exited(fd));
            for pid in direct_children()? {
                if signalled.contains_key(&pid) {
                    continue;
                }
                if let Some(fd) = open_child(pid)? {
                    signal(&fd, libc::SIGTERM)?;
                    signalled.insert(pid, fd);
                }
            }
            Ok(false)
        })();
        match result {
            Ok(true) => return,
            Ok(false) => previous_error = None,
            Err(error) => {
                let error = error.to_string();
                if previous_error.as_ref() != Some(&error) {
                    let _ = writeln!(
                        std::io::stderr(),
                        "Could not finish Command Line child cleanup: {error}. Keeping this Command Line open so Shell can terminate its process tree."
                    );
                    previous_error = Some(error);
                }
            }
        }
        // There is deliberately no automatic KILL timeout. The existing Shell
        // termination action retains control of children that ignore TERM.
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Returns true only once there are no children left, including adopted zombies.
fn reap_finished_children() -> io::Result<bool> {
    loop {
        let result = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        if result > 0 {
            continue;
        }
        if result == 0 {
            return Ok(false);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ECHILD) => return Ok(true),
            Some(libc::EINTR) => continue,
            _ => return Err(error),
        }
    }
}

fn direct_children() -> io::Result<BTreeSet<libc::pid_t>> {
    let mut children = BTreeSet::new();
    for task in std::fs::read_dir("/proc/self/task")? {
        let path = task?.path().join("children");
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for word in contents.split_whitespace() {
            let pid = word.parse::<libc::pid_t>().map_err(io::Error::other)?;
            if pid > 0 && pid != unsafe { libc::getpid() } {
                children.insert(pid);
            }
        }
    }
    Ok(children)
}

fn open_child(pid: libc::pid_t) -> io::Result<Option<OwnedFd>> {
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if raw < 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw as i32) };
    // Verify parentage after opening the stable descriptor. If another waiter
    // reaped this child meanwhile, never signal an unrelated replacement PID.
    let status = match std::fs::read_to_string(format!("/proc/{pid}/status")) {
        Ok(status) => status,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let parent = status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .ok_or_else(|| io::Error::other("child process has no parent identity"))?
        .trim()
        .parse::<libc::pid_t>()
        .map_err(io::Error::other)?;
    if parent != unsafe { libc::getpid() } || process_exited(&fd) {
        Ok(None)
    } else {
        Ok(Some(fd))
    }
}

fn process_exited(fd: &OwnedFd) -> bool {
    let mut poll = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut poll, 1, 0) > 0 }
}

fn signal(fd: &OwnedFd, number: libc::c_int) -> io::Result<()> {
    if unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            number,
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

struct CleanupSignals(Vec<(libc::c_int, libc::sigaction)>);

impl CleanupSignals {
    fn install() -> io::Result<Self> {
        let mut saved = Self(Vec::new());
        for number in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
            let mut ignore: libc::sigaction = unsafe { std::mem::zeroed() };
            ignore.sa_sigaction = libc::SIG_IGN;
            unsafe { libc::sigemptyset(&mut ignore.sa_mask) };
            let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
            if unsafe { libc::sigaction(number, &ignore, &mut previous) } != 0 {
                return Err(io::Error::last_os_error());
            }
            saved.0.push((number, previous));
        }
        Ok(saved)
    }
}

impl Drop for CleanupSignals {
    fn drop(&mut self) {
        for (number, previous) in self.0.iter().rev() {
            unsafe { libc::sigaction(*number, previous, std::ptr::null_mut()) };
        }
    }
}
