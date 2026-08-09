use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::PlatformError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    env: BTreeMap<String, String>,
}

impl ProcessSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
            env: BTreeMap::new(),
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

/// A child process owned by the current application lifetime.
///
/// This deliberately has a much narrower surface than [`std::process::Child`]:
/// callers can wait for it or stop it, but cannot detach it accidentally.  It
/// is appropriate for a GUI host such as the Tundra launcher, where leaving a
/// terminal process behind is always a bug.
#[derive(Debug)]
pub struct SupervisedChild {
    child: Child,
    program: PathBuf,
    #[cfg(unix)]
    process_group: i32,
    #[cfg(windows)]
    job: JobHandle,
}

impl SupervisedChild {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn try_wait(&mut self) -> Result<Option<ProcessStatus>, PlatformError> {
        self.child
            .try_wait()
            .map(|status| status.map(ProcessStatus::from))
            .map_err(|error| child_error("query supervised child", &self.program, error))
    }

    pub fn wait(&mut self) -> Result<ProcessStatus, PlatformError> {
        self.child
            .wait()
            .map(ProcessStatus::from)
            .map_err(|error| child_error("wait for supervised child", &self.program, error))
    }

    /// Requests graceful shutdown, waits at most two seconds, then forcibly
    /// kills the contained process tree and reaps the GUI process.
    pub fn terminate_and_wait(&mut self) -> Result<ProcessStatus, PlatformError> {
        self.request_graceful_termination()?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            thread::sleep(Duration::from_millis(25));
        }
        self.force_kill_tree()?;
        self.wait()
    }

    fn request_graceful_termination(&mut self) -> Result<(), PlatformError> {
        #[cfg(unix)]
        return send_signal_to_group(self.process_group, SIGTERM, &self.program);
        #[cfg(windows)]
        {
            // A GUI child can decline CTRL_BREAK; the bounded force-kill below
            // remains authoritative for all Job Object members.
            unsafe {
                let _ = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, self.child.id());
            }
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        Ok(())
    }

    fn force_kill_tree(&mut self) -> Result<(), PlatformError> {
        #[cfg(unix)]
        return send_signal_to_group(self.process_group, SIGKILL, &self.program);
        #[cfg(windows)]
        return self.job.terminate(&self.program);
        #[cfg(not(any(unix, windows)))]
        match self.child.kill() {
            Ok(()) | Err(error) if error.kind() == io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(child_error(
                "terminate supervised child",
                &self.program,
                error,
            )),
        }
    }
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        // The direct GUI process may have exited while an inherited child is
        // still alive in the group/job.  Always close/kill the containment
        // boundary; ESRCH is treated as success on Unix.
        let _ = self.force_kill_tree();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.wait();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessStatus {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub success: bool,
}

impl From<ExitStatus> for ProcessStatus {
    fn from(status: ExitStatus) -> Self {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;

        Self {
            code: status.code(),
            #[cfg(unix)]
            signal: status.signal(),
            #[cfg(not(unix))]
            signal: None,
            success: status.success(),
        }
    }
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

/// Spawns a non-detached child whose lifecycle remains owned by the caller.
/// No shell is used and all executable resolution is delegated to the
/// absolute program path supplied in [`ProcessSpec`].
pub fn spawn_supervised(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
) -> Result<SupervisedChild, PlatformError> {
    validate_process_spec(spec, reject_windows_scripts)?;
    let mut command = command_from_spec(spec);
    configure_supervised_command(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| child_error("spawn supervised child", &spec.program, error))?;
    #[cfg(windows)]
    let job = match JobHandle::create_and_assign(&child, &spec.program) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    #[cfg(windows)]
    if let Err(error) = resume_suspended_process(&child, &spec.program) {
        let _ = job.terminate(&spec.program);
        let _ = child.wait();
        return Err(error);
    }
    Ok(SupervisedChild {
        #[cfg(unix)]
        process_group: child.id() as i32,
        #[cfg(windows)]
        job,
        child,
        program: spec.program.clone(),
    })
}

#[cfg(unix)]
const SIGKILL: i32 = 9;
#[cfg(unix)]
const SIGTERM: i32 = 15;

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

#[cfg(unix)]
fn configure_supervised_command(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // Descendants inherit this group, allowing reliable tree termination
    // without PID enumeration races.
    command.process_group(0);

    #[cfg(target_os = "linux")]
    {
        const PR_SET_PDEATHSIG: i32 = 1;
        let expected_parent = std::process::id() as i32;
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(PR_SET_PDEATHSIG, SIGTERM) != 0 {
                    return Err(io::Error::last_os_error());
                }
                // Avoid the fork/exec race where the launcher disappears just
                // before the parent-death signal is configured.
                if libc::getppid() != expected_parent {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "launcher parent exited before child setup",
                    ));
                }
                Ok(())
            });
        }
    }
}

#[cfg(unix)]
fn send_signal_to_group(
    process_group: i32,
    signal: i32,
    program: &Path,
) -> Result<(), PlatformError> {
    let result = unsafe { kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // ESRCH: all members were already reaped, so containment succeeded.
    if error.raw_os_error() == Some(3) {
        return Ok(());
    }
    Err(child_error(
        "signal supervised process group",
        program,
        error,
    ))
}

#[cfg(windows)]
const CREATE_SUSPENDED: u32 = 0x0000_0004;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const CTRL_BREAK_EVENT: u32 = 1;
#[cfg(windows)]
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
#[cfg(windows)]
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

#[cfg(windows)]
#[repr(C)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[cfg(windows)]
#[repr(C)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[cfg(windows)]
#[repr(C)]
struct JobObjectExtendedLimitInformation {
    basic_limit_information: JobObjectBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(
        attributes: *const std::ffi::c_void,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        class: i32,
        info: *const std::ffi::c_void,
        length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut std::ffi::c_void, process: *mut std::ffi::c_void) -> i32;
    fn TerminateJobObject(job: *mut std::ffi::c_void, exit_code: u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn GenerateConsoleCtrlEvent(control_type: u32, process_group_id: u32) -> i32;
}

#[cfg(windows)]
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtResumeProcess(process: *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
#[derive(Debug)]
struct JobHandle(usize);

#[cfg(windows)]
impl JobHandle {
    fn create_and_assign(child: &Child, program: &Path) -> Result<Self, PlatformError> {
        use std::os::windows::io::AsRawHandle;

        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(child_error(
                "create supervised Job Object",
                program,
                io::Error::last_os_error(),
            ));
        }
        let job = Self(handle as usize);
        let mut limits: JobObjectExtendedLimitInformation = unsafe { std::mem::zeroed() };
        limits.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&raw const limits).cast(),
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            )
        };
        if configured == 0 {
            return Err(child_error(
                "configure supervised Job Object",
                program,
                io::Error::last_os_error(),
            ));
        }
        let assigned = unsafe { AssignProcessToJobObject(handle, child.as_raw_handle().cast()) };
        if assigned == 0 {
            return Err(child_error(
                "assign supervised process to Job Object",
                program,
                io::Error::last_os_error(),
            ));
        }
        Ok(job)
    }

    fn terminate(&self, program: &Path) -> Result<(), PlatformError> {
        let success = unsafe { TerminateJobObject(self.0 as *mut std::ffi::c_void, 1) };
        if success != 0 {
            Ok(())
        } else {
            Err(child_error(
                "terminate supervised Job Object",
                program,
                io::Error::last_os_error(),
            ))
        }
    }
}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0 as *mut std::ffi::c_void);
        }
    }
}

#[cfg(windows)]
fn configure_supervised_command(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(windows)]
fn resume_suspended_process(child: &Child, program: &Path) -> Result<(), PlatformError> {
    use std::os::windows::io::AsRawHandle;

    let status = unsafe { NtResumeProcess(child.as_raw_handle().cast()) };
    if status >= 0 {
        Ok(())
    } else {
        Err(PlatformError::Native {
            operation: "resume supervised process",
            message: format!(
                "NtResumeProcess failed with NTSTATUS 0x{:08x} for {}",
                status as u32,
                program.display()
            ),
        })
    }
}

#[cfg(not(any(unix, windows)))]
fn configure_supervised_command(_command: &mut Command) {}

pub(crate) fn spawn_detached_impl(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
) -> Result<(), PlatformError> {
    validate_process_spec(spec, reject_windows_scripts)?;
    let mut command = command_from_spec(spec);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.spawn().map_err(|error| PlatformError::Io {
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
    let output = command_from_spec(spec)
        .output()
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

fn command_from_spec(spec: &ProcessSpec) -> Command {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);

    if let Some(current_dir) = &spec.current_dir {
        command.current_dir(current_dir);
    }

    for (key, value) in &spec.env {
        command.env(key, value);
    }

    command
}

fn child_error(operation: &'static str, program: &Path, error: io::Error) -> PlatformError {
    PlatformError::Io {
        operation,
        path: Some(program.to_path_buf()),
        message: error.to_string(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervised_child_is_waitable_and_reports_its_exit_status() {
        let test_binary = std::env::current_exe().unwrap();
        let mut child =
            spawn_supervised(&ProcessSpec::new(test_binary).arg("--list"), cfg!(windows)).unwrap();
        assert!(child.id() > 0);
        #[cfg(windows)]
        assert_ne!(
            child.job.0, 0,
            "Windows child must be attached to a Job Object"
        );
        #[cfg(unix)]
        assert_eq!(child.process_group, child.id() as i32);
        assert!(child.wait().unwrap().success);
    }

    #[cfg(unix)]
    #[test]
    fn unix_exit_status_preserves_terminating_signal() {
        let mut child = spawn_supervised(
            &ProcessSpec::new("/bin/sh").args(["-c", "kill -TERM $$"]),
            false,
        )
        .unwrap();
        let status = child.wait().unwrap();
        assert_eq!(status.code, None);
        assert_eq!(status.signal, Some(SIGTERM));
    }
}
