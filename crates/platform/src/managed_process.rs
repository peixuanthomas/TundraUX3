//! Owned child processes: stop the tree and reap the child on every exit path.
use crate::process::{ChildTree, PollPipe, make_nonblocking};
use std::io;
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use watchdog::ManagedResource;

/// External desktop applications have an explicitly independent lifetime.
/// Our reaper stops on cancellation without terminating the application.
pub(crate) fn spawn_external(command: &mut Command) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    use watchdog::{
        AppCriticality, AppDescriptor, AppId, AppWatchdog, ProcessWatchdog, TaskId, TaskSpec,
    };
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let app = AppWatchdog::current()
        .or_else(|| {
            ProcessWatchdog::global()?
                .register_app(AppDescriptor::new(
                    AppId::from_static("external-applications"),
                    "External applications",
                    env!("CARGO_PKG_VERSION"),
                    AppCriticality::Optional,
                ))
                .ok()
        })
        .ok_or_else(|| io::Error::other("the process watchdog is not active"))?;
    let child = Arc::new(Mutex::new(Some(spawn_external_child(command)?)));
    let worker_child = child.clone();
    let group = app.task_group("external-application-reapers");
    group.reap_finished();
    let result = group.spawn_thread_with_cancellation(
        TaskSpec::one_shot(
            TaskId::new(format!("external-{}", NEXT.fetch_add(1, Ordering::Relaxed)))
                .map_err(io::Error::other)?,
        ),
        move |cancel| {
            if let Some(mut child) = worker_child
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                while !cancel.is_cancelled() {
                    match child.try_wait() {
                        Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                        _ => break,
                    }
                }
            }
        },
    );
    if let Err(error) = result {
        if let Some(mut child) = child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = child.kill();
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            while matches!(child.try_wait(), Ok(None)) && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        return Err(io::Error::other(error));
    }
    Ok(())
}

pub(crate) fn spawn_external_child(command: &mut Command) -> io::Result<std::process::Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Separate from the launcher terminal's session and process group.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::JobObjects::*;
        // Hosts such as a test runner can put UX in their own non-breakaway
        // job. Only our child jobs opt into breakaway; requesting it from an
        // unrelated host job would make every external launch fail with EACCES.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        let can_break_away = unsafe {
            QueryInformationJobObject(
                std::ptr::null_mut(),
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as _,
                std::mem::size_of_val(&limits) as _,
                std::ptr::null_mut(),
            )
        } != 0
            && limits.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_BREAKAWAY_OK != 0;
        command.creation_flags(if can_break_away {
            windows_sys::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB
        } else {
            0
        });
    }
    command.spawn()
}

pub struct ManagedChild {
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
    tree: Arc<Mutex<ChildTree>>,
    registration: Option<ManagedResource>,
}

pub trait ManagedCommand {
    fn spawn_managed(&mut self) -> io::Result<ManagedChild>;
    /// Preserve terminal/session setup supplied by the caller.
    fn spawn_managed_inherited(&mut self) -> io::Result<ManagedChild>;
    fn status_managed(&mut self) -> io::Result<ExitStatus>;
    fn output_managed(&mut self) -> io::Result<Output>;
    /// A synchronous replacement launched after the old runtime has stopped.
    /// Its caller retains ownership until the new application exits.
    fn spawn_successor(&mut self) -> io::Result<ManagedChild>;
    /// The one-operation helper explicitly transfers its work to a process
    /// with its own watchdog. Only its short-lived launcher is owned here.
    fn spawn_supervised_helper(&mut self) -> io::Result<ManagedChild>;
}

impl ManagedCommand for Command {
    fn spawn_managed(&mut self) -> io::Result<ManagedChild> {
        ManagedChild::spawn(self, true, true)
    }
    fn spawn_managed_inherited(&mut self) -> io::Result<ManagedChild> {
        ManagedChild::spawn(self, false, true)
    }
    fn spawn_successor(&mut self) -> io::Result<ManagedChild> {
        ManagedChild::spawn(self, false, false)
    }
    fn spawn_supervised_helper(&mut self) -> io::Result<ManagedChild> {
        let child = ManagedChild::spawn(self, false, true)?;
        #[cfg(target_os = "linux")]
        {
            child
                .tree
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .descendants = None;
        }
        Ok(child)
    }
    fn status_managed(&mut self) -> io::Result<ExitStatus> {
        self.spawn_managed_inherited()?.wait()
    }
    fn output_managed(&mut self) -> io::Result<Output> {
        self.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn_managed()?
            .wait_with_output()
    }
}

impl ManagedChild {
    /// Privileged helpers stop through their private control connection before
    /// force-stop is attempted, since their caller may not have signal rights.
    pub fn shutdown_via(&mut self, signal: impl FnMut() -> io::Result<()> + Send + 'static) {
        self.tree
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shutdown_signal = Some(Box::new(signal));
    }
    pub(super) fn waiter(&self) -> crate::process::ProcessWaiter {
        crate::process::ProcessWaiter::new(
            &self.tree.lock().unwrap_or_else(|e| e.into_inner()).child,
        )
    }
    fn spawn(command: &mut Command, isolated: bool, register: bool) -> io::Result<Self> {
        #[cfg(unix)]
        if isolated {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
            command.creation_flags(CREATE_SUSPENDED | if isolated { CREATE_NO_WINDOW } else { 0 });
        }
        let child = command.spawn()?;
        Self::track(
            child,
            isolated,
            register,
            format!("child:{}", command.get_program().to_string_lossy()),
        )
    }

    fn track(
        child: std::process::Child,
        isolated: bool,
        register: bool,
        label: String,
    ) -> io::Result<Self> {
        let mut tree = ChildTree::new(child, isolated)?;
        let stdin = tree.child.stdin.take();
        let stdout = tree.child.stdout.take();
        let stderr = tree.child.stderr.take();
        let tree = Arc::new(Mutex::new(tree));
        let cleanup = tree.clone();
        let registration = if register {
            ManagedResource::register_current(label, move || {
                cleanup.lock().unwrap_or_else(|e| e.into_inner()).stop()
            })
            .map_err(io::Error::other)?
        } else {
            None
        };
        Ok(Self {
            stdin,
            stdout,
            stderr,
            tree,
            registration,
        })
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn spawn_pty(
        slave: &dyn portable_pty::SlavePty,
        command: portable_pty::CommandBuilder,
    ) -> io::Result<Self> {
        let child: Box<dyn portable_pty::Child> =
            slave.spawn_command(command).map_err(io::Error::other)?;
        let child = child
            .downcast::<std::process::Child>()
            .expect("the native Unix PTY returns std::process::Child");
        Self::track(*child, true, true, "pty".into())
    }

    pub fn id(&self) -> u32 {
        self.tree
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .child
            .id()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.tree
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_wait()
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.tree.lock().unwrap_or_else(|e| e.into_inner()).stop()
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn terminate(&mut self) -> io::Result<()> {
        let mut tree = self.tree.lock().unwrap_or_else(|e| e.into_inner());
        if tree.try_wait()?.is_none()
            && unsafe { libc::kill(-(tree.child.id() as i32), libc::SIGTERM) } < 0
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EPERM)
                && crate::process::macos::group_is_dead(tree.child.id()).unwrap_or(false)
            {
                return Ok(());
            }
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.stdin.take();
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            // Never hold the child lock across a blocking wait: watchdog must
            // be able to stop it even while its owner is waiting for completion.
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn wait_with_output(mut self) -> io::Result<Output> {
        self.stdin.take();
        let mut stdout = self.stdout.take();
        let mut stderr = self.stderr.take();
        if let Some(pipe) = &stdout {
            make_nonblocking(pipe)?;
        }
        if let Some(pipe) = &stderr {
            make_nonblocking(pipe)?;
        }
        let mut out = Vec::new();
        let mut err = Vec::new();
        loop {
            let status = self.try_wait()?;
            if status.is_some() {
                self.tree.lock().unwrap_or_else(|e| e.into_inner()).stop()?;
            }
            let mut progress = false;
            if let Some(pipe) = &mut stdout {
                progress |= drain(pipe, &mut out)?;
            }
            if let Some(pipe) = &mut stderr {
                progress |= drain(pipe, &mut err)?;
            }
            if let Some(status) = status {
                // Keep draining until both pipes are empty, including output
                // larger than a single bounded read batch.
                if !progress {
                    return Ok(Output {
                        status,
                        stdout: out,
                        stderr: err,
                    });
                }
            }
            if !progress {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn drain(pipe: &mut dyn PollPipe, bytes: &mut Vec<u8>) -> io::Result<bool> {
    let mut progress = false;
    let mut buffer = [0; 16 * 1024];
    for _ in 0..16 {
        if !pipe.readable()? {
            break;
        }
        match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                progress = true;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(progress)
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        self.stdin.take();
        if let Some(registration) = self.registration.take() {
            // The registration reports cleanup failures and unregisters.
            drop(registration);
        } else {
            let _ = self.tree.lock().unwrap_or_else(|e| e.into_inner()).stop();
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/process/managed_tests.rs"]
mod tests;
