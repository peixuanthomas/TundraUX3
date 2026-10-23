use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{ManagedCommand, PlatformError};
#[cfg(target_os = "macos")]
pub(super) mod macos;

/// One stdout/stderr record, delivered while the child is still running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stderr: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    env: BTreeMap<String, String>,
    removed_env: Vec<String>,
}

impl ProcessSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
            env: BTreeMap::new(),
            removed_env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn current_dir(mut self, current_dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(current_dir.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn env_remove(mut self, key: impl Into<String>) -> Self {
        self.removed_env.push(key.into());
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn args_slice(&self) -> &[String] {
        &self.args
    }

    pub fn current_dir_path(&self) -> Option<&Path> {
        self.current_dir.as_deref()
    }

    pub fn env_map(&self) -> &BTreeMap<String, String> {
        &self.env
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessStream {
    bytes: Vec<u8>,
}

impl ProcessStream {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn utf8_lossy(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessExit {
    pub code: Option<i32>,
    pub stdout: ProcessStream,
    pub stderr: ProcessStream,
}

pub fn validate_process_spec(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
) -> Result<(), PlatformError> {
    if spec.program.as_os_str().is_empty() {
        return Err(PlatformError::InvalidInput {
            message: "process program must not be empty".to_string(),
        });
    }

    if reject_windows_scripts && is_blocked_windows_script(&spec.program) {
        return Err(PlatformError::ProcessPolicy {
            message: format!(
                "refusing to launch script file through platform process API: {}",
                spec.program.display()
            ),
        });
    }

    Ok(())
}

pub(crate) fn spawn_detached_impl(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
) -> Result<(), PlatformError> {
    validate_process_spec(spec, reject_windows_scripts)?;
    let mut command = command_from_spec(spec)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    crate::managed_process::spawn_external(&mut command).map_err(|error| PlatformError::Io {
        operation: "spawn detached process",
        path: Some(spec.program.clone()),
        message: error.to_string(),
    })?;

    Ok(())
}

pub(crate) fn spawn_wait_impl(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
) -> Result<ProcessExit, PlatformError> {
    validate_process_spec(spec, reject_windows_scripts)?;
    let output = command_from_spec(spec)?
        .output_managed()
        .map_err(|error| PlatformError::Io {
            operation: "spawn process and wait",
            path: Some(spec.program.clone()),
            message: error.to_string(),
        })?;

    Ok(ProcessExit {
        code: output.status.code(),
        stdout: ProcessStream::from_bytes(output.stdout),
        stderr: ProcessStream::from_bytes(output.stderr),
    })
}

fn command_from_spec(spec: &ProcessSpec) -> Result<Command, PlatformError> {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);

    if let Some(current_dir) = &spec.current_dir {
        command.current_dir(current_dir);
    }

    for key in &spec.removed_env {
        command.env_remove(key);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }

    #[cfg(target_os = "linux")]
    {
        let user = crate::linux::identity::LinuxUserContext::current().map_err(|error| {
            PlatformError::Native {
                operation: "resolve child process identity",
                message: error.to_string(),
            }
        })?;
        command.envs(user.environment());
    }
    Ok(command)
}

pub(crate) fn spawn_streaming_impl(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
    report: &mut dyn FnMut(ProcessOutput),
) -> Result<ProcessExit, PlatformError> {
    spawn_cancellable_impl(spec, reject_windows_scripts, &|| false, Some(report))
}

fn is_blocked_windows_script(program: &Path) -> bool {
    program
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "bat" | "cmd" | "ps1"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn cancelled_process(spec: &ProcessSpec) -> PlatformError {
    PlatformError::Interrupted {
        operation: "run process",
        path: Some(spec.program.clone()),
        message: "process cancelled; child tree stopped".into(),
    }
}

/// Both pipes are polled by the owning managed worker. No reader thread can
/// survive cancellation or hang on a descriptor inherited by a grandchild.
pub(crate) fn spawn_cancellable_impl(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
    cancelled: &dyn Fn() -> bool,
    mut report: Option<&mut dyn FnMut(ProcessOutput)>,
) -> Result<ProcessExit, PlatformError> {
    validate_process_spec(spec, reject_windows_scripts)?;
    if cancelled() {
        return Err(cancelled_process(spec));
    }
    let error = |error: std::io::Error| PlatformError::Io {
        operation: "run cancellable process",
        path: Some(spec.program.clone()),
        message: error.to_string(),
    };
    let mut command = command_from_spec(spec)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(
            windows_sys::Win32::System::Threading::CREATE_SUSPENDED
                | windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
        );
    }
    let mut child = command.spawn_managed().map_err(error)?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    make_nonblocking(&stdout).map_err(error)?;
    make_nonblocking(&stderr).map_err(error)?;
    let waiter = child.waiter();
    let mut stdout_eof = false;
    let mut stderr_eof = false;
    let streaming = report.is_some();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_pending = Vec::new();
    let mut err_pending = Vec::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        if cancelled() {
            child.kill().map_err(error)?;
            return Err(cancelled_process(spec));
        }
        // Bound work per iteration even when a process continuously fills a pipe.
        let mut made_progress = false;
        for (is_stderr, pipe, bytes, pending, eof) in [
            (
                false,
                &mut stdout as &mut dyn PollPipe,
                &mut out,
                &mut out_pending,
                &mut stdout_eof,
            ),
            (
                true,
                &mut stderr as &mut dyn PollPipe,
                &mut err,
                &mut err_pending,
                &mut stderr_eof,
            ),
        ] {
            if *eof {
                continue;
            }
            for _ in 0..16 {
                if !pipe.readable().map_err(error)? {
                    break;
                }
                let count = match pipe.read(&mut buffer) {
                    Ok(0) => {
                        *eof = true;
                        break;
                    }
                    Ok(count) => count,
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        break;
                    }
                    Err(e) => return Err(error(e)),
                };
                made_progress = true;
                bytes.extend_from_slice(&buffer[..count]);
                if streaming && bytes.len() > 65536 {
                    bytes.drain(..bytes.len() - 65536);
                }
                if let Some(report) = report.as_mut() {
                    for byte in &buffer[..count] {
                        if matches!(*byte, b'\r' | b'\n') || pending.len() >= 8192 {
                            emit_pending(pending, is_stderr, *report);
                            if matches!(*byte, b'\r' | b'\n') {
                                continue;
                            }
                        }
                        pending.push(*byte);
                    }
                }
            }
        }
        if let Some(status) = child.try_wait().map_err(error)? {
            // Stop any descendants retaining pipes before the last drain.
            child.kill().map_err(error)?;
            for (is_stderr, pipe, bytes, pending) in [
                (
                    false,
                    &mut stdout as &mut dyn PollPipe,
                    &mut out,
                    &mut out_pending,
                ),
                (
                    true,
                    &mut stderr as &mut dyn PollPipe,
                    &mut err,
                    &mut err_pending,
                ),
            ] {
                while pipe.readable().map_err(error)? {
                    match pipe.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => {
                            bytes.extend_from_slice(&buffer[..count]);
                            pending.extend_from_slice(&buffer[..count]);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(error(e)),
                    }
                }
                if let Some(report) = report.as_mut() {
                    emit_pending(pending, is_stderr, *report);
                }
                if streaming && bytes.len() > 65536 {
                    bytes.drain(..bytes.len() - 65536);
                }
            }
            if cancelled() {
                return Err(cancelled_process(spec));
            }
            return Ok(ProcessExit {
                code: status.code(),
                stdout: ProcessStream::from_bytes(out),
                stderr: ProcessStream::from_bytes(err),
            });
        }
        waiter
            .wait(&stdout, &stderr, stdout_eof, stderr_eof, made_progress)
            .map_err(error)?;
    }
}

/// Readiness wakes output immediately. The timeout bounds cancellation latency
/// because the caller supplies a predicate rather than a waitable event.
pub(super) struct ProcessWaiter {
    #[cfg(target_os = "linux")]
    exit: Option<std::os::fd::OwnedFd>,
    #[cfg(windows)]
    process: windows_sys::Win32::Foundation::HANDLE,
}

impl ProcessWaiter {
    pub(super) fn new(child: &std::process::Child) -> Self {
        let _ = child;
        Self {
            #[cfg(target_os = "linux")]
            exit: {
                use std::os::fd::FromRawFd;
                // Older kernels or restricted hosts can reject pidfd_open;
                // pipe readiness and the bounded timeout still work there.
                let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0u32) };
                (fd >= 0).then(|| unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) })
            },
            #[cfg(windows)]
            process: {
                use std::os::windows::io::AsRawHandle;
                child.as_raw_handle()
            },
        }
    }

    #[cfg(unix)]
    fn wait(
        &self,
        stdout: &std::process::ChildStdout,
        stderr: &std::process::ChildStderr,
        stdout_eof: bool,
        stderr_eof: bool,
        _made_progress: bool,
    ) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;
        #[cfg(not(target_os = "linux"))]
        let exit_fd = -1;
        #[cfg(target_os = "linux")]
        let exit_fd = self.exit.as_ref().map_or(-1, AsRawFd::as_raw_fd);
        let mut descriptors = [
            libc::pollfd {
                fd: if stdout_eof { -1 } else { stdout.as_raw_fd() },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if stderr_eof { -1 } else { stderr.as_raw_fd() },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: exit_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // Exclude EOF pipes: POLLHUP remains ready and would otherwise spin.
        if unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, 20) } < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    fn wait(
        &self,
        _stdout: &std::process::ChildStdout,
        _stderr: &std::process::ChildStderr,
        _stdout_eof: bool,
        _stderr_eof: bool,
        made_progress: bool,
    ) -> std::io::Result<()> {
        // Anonymous Windows pipes cannot be waited on for readiness. Drain
        // active producers without sleeping; an idle child exit wakes early.
        if !made_progress
            && unsafe {
                windows_sys::Win32::System::Threading::WaitForSingleObject(self.process, 20)
            } == windows_sys::Win32::Foundation::WAIT_FAILED
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

fn emit_pending(pending: &mut Vec<u8>, stderr: bool, report: &mut dyn FnMut(ProcessOutput)) {
    if !pending.is_empty() {
        report(ProcessOutput {
            stderr,
            text: String::from_utf8_lossy(pending).into_owned(),
        });
        pending.clear();
    }
}

pub(super) trait PollPipe: std::io::Read {
    fn readable(&self) -> std::io::Result<bool>;
}

#[cfg(unix)]
pub(super) fn make_nonblocking(pipe: &impl std::os::fd::AsRawFd) -> std::io::Result<()> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
impl<T: std::io::Read> PollPipe for T {
    fn readable(&self) -> std::io::Result<bool> {
        Ok(true)
    }
}

#[cfg(windows)]
pub(super) fn make_nonblocking(
    _pipe: &impl std::os::windows::io::AsRawHandle,
) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
impl<T: std::io::Read + std::os::windows::io::AsRawHandle> PollPipe for T {
    fn readable(&self) -> std::io::Result<bool> {
        let mut available = 0;
        if unsafe {
            windows_sys::Win32::System::Pipes::PeekNamedPipe(
                self.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error()
                == Some(windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE as i32)
            {
                return Ok(false);
            }
            return Err(error);
        }
        Ok(available > 0)
    }
}

pub(super) struct ChildTree {
    pub(super) child: std::process::Child,
    finished: bool,
    isolated: bool,
    pub(super) shutdown_signal: Option<Box<dyn FnMut() -> std::io::Result<()> + Send>>,
    #[cfg(target_os = "linux")]
    pub(super) descendants: Option<crate::management::termination::ProcessTree>,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}

impl ChildTree {
    pub(super) fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        #[cfg(unix)]
        if !self.finished {
            // Observe exit without reaping first. While our child is unreaped,
            // its process-group ID cannot be recycled for an unrelated process.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id() as _,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 && unsafe { info.si_pid() } != 0 {
                self.stop()?;
            } else if result != 0
                && std::io::Error::last_os_error().raw_os_error() != Some(libc::ECHILD)
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        self.child.try_wait()
    }
    pub(super) fn new(mut child: std::process::Child, isolated: bool) -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::System::JobObjects::*;
            // The process has not executed any user code yet, so no descendant
            // can escape job assignment between spawn and assignment.
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            let result = (|| {
                if job.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
                    unsafe { std::mem::zeroed() };
                limits.BasicLimitInformation.LimitFlags =
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
                if unsafe {
                    SetInformationJobObject(
                        job,
                        JobObjectExtendedLimitInformation,
                        &limits as *const _ as _,
                        std::mem::size_of_val(&limits) as _,
                    )
                } == 0
                    || unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) } == 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                #[link(name = "ntdll")]
                unsafe extern "system" {
                    fn NtResumeProcess(process: windows_sys::Win32::Foundation::HANDLE) -> i32;
                }
                if unsafe { NtResumeProcess(child.as_raw_handle()) } < 0 {
                    return Err(std::io::Error::other(
                        "could not resume isolated child process",
                    ));
                }
                Ok(())
            })();
            if let Err(error) = result {
                let _ = child.kill();
                let _ = child.wait();
                if !job.is_null() {
                    unsafe {
                        windows_sys::Win32::Foundation::CloseHandle(job);
                    }
                }
                return Err(error);
            }
            Ok(Self {
                child,
                finished: false,
                isolated,
                shutdown_signal: None,
                job,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = &mut child;
            #[allow(unused_mut)]
            let mut tree = Self {
                #[cfg(target_os = "linux")]
                descendants: None,
                child,
                finished: false,
                isolated,
                shutdown_signal: None,
            };
            #[cfg(target_os = "linux")]
            {
                // If a stable process handle cannot be opened, stop the child
                // through the guard instead of publishing untracked work.
                tree.descendants = Some(crate::management::termination::ProcessTree::child(
                    tree.child.id(),
                )?);
            }
            Ok(tree)
        }
    }

    fn stop_descendants(&self) -> std::io::Result<()> {
        #[cfg(unix)]
        if self.isolated && unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) } < 0 {
            let error = std::io::Error::last_os_error();
            #[cfg(target_os = "macos")]
            if error.raw_os_error() == Some(libc::EPERM)
                && macos::group_is_dead(self.child.id()).unwrap_or(false)
            {
                return Ok(());
            }
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::JobObjects::*;
            if unsafe { TerminateJobObject(self.job, 1) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            // TerminateJobObject is asynchronous. Keep the worker registered
            // until descendants release executable and output file handles.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            loop {
                let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION =
                    unsafe { std::mem::zeroed() };
                if unsafe {
                    QueryInformationJobObject(
                        self.job,
                        JobObjectBasicAccountingInformation,
                        &mut accounting as *mut _ as _,
                        std::mem::size_of_val(&accounting) as _,
                        std::ptr::null_mut(),
                    )
                } == 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                if accounting.ActiveProcesses == 0 {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    return Err(std::io::Error::other(
                        "child job did not finish stopping within one second",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        Ok(())
    }

    pub(super) fn stop(&mut self) -> std::io::Result<()> {
        if self.finished {
            return Ok(());
        }
        if let Some(mut signal) = self.shutdown_signal.take() {
            signal()?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
            while self.try_wait()?.is_none() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if self.finished {
                return Ok(());
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(descendants) = &mut self.descendants {
            descendants.signal(true)?;
        }
        let stopped = self.stop_descendants();
        if stopped.is_err() || !self.isolated {
            let _ = self.child.kill();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while self.child.try_wait()?.is_none() {
            if std::time::Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "child did not stop within one second",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        stopped?;
        self.finished = true;
        Ok(())
    }
}

// The job handle is owned by ChildTree and accessed only behind the child mutex.
#[cfg(windows)]
unsafe impl Send for ChildTree {}

impl Drop for ChildTree {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.stop();
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/unit/process/streaming_tests.rs"]
mod streaming_tests;

#[cfg(test)]
#[path = "../tests/unit/process/cancellation_tests.rs"]
mod cancellation_tests;
