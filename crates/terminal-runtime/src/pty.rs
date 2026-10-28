//! Isolated child terminals shared by Command Line, Vim and scripts.
//!
//! This module deliberately never writes child output to the host terminal.
//! Child output is decoded into an isolated terminal model. The UI receives
//! text and images; host-side operations such as OSC 52 remain blocked.

use crate::emulator::Emulator;
pub use crate::input::{
    MouseProtocolEncoding, MouseProtocolMode, TerminalInput, encode_terminal_input,
    key_event_bytes, mouse_event_bytes, mouse_to_terminal, paste_bytes,
};
use crate::input_queue::InputQueue;
pub use crate::snapshot::{
    TerminalCell, TerminalColor, TerminalCursorStyle, TerminalSnapshot, to_ui_snapshot,
};
pub use platform::task_history::TaskRecorder;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use watchdog::{ManagedTaskGroup, ManagedThreadHandle, TaskId, TaskSpec};
#[cfg(target_os = "linux")]
#[path = "process_tree_linux.rs"]
mod process_tree_linux;
#[cfg(target_os = "macos")]
#[path = "process_tree_macos.rs"]
mod process_tree_macos;

#[cfg(test)]
use ui::KeyModifiers as InputModifiers;
#[cfg(test)]
use ui::{Key as InputKey, KeyEvent as KeyInput};

pub const DEFAULT_COLUMNS: u16 = 108;
pub const DEFAULT_ROWS: u16 = 20;

static NEXT_PTY_READER_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct PtyConfig {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub cwd: Option<PathBuf>,
    pub columns: u16,
    pub rows: u16,
    pub scrollback_lines: usize,
}

impl PtyConfig {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
            scrollback_lines: 2_000,
        }
    }

    fn size(&self) -> PtySize {
        PtySize {
            rows: self.rows.max(1),
            cols: self.columns.max(1),
            pixel_width: self.columns.saturating_mul(8),
            pixel_height: self.rows.saturating_mul(16),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyExitStatus {
    pub code: u32,
    pub success: bool,
}

impl PtyExitStatus {
    fn from_portable(status: portable_pty::ExitStatus) -> Self {
        Self {
            code: status.exit_code(),
            success: status.success(),
        }
    }
}

/// Retained terminal contents after the process, PTY and output reader close.
pub struct TerminalOutput {
    parser: Arc<Mutex<Emulator>>,
}

impl std::fmt::Debug for TerminalOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalOutput")
            .finish_non_exhaustive()
    }
}

impl TerminalOutput {
    pub fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        let mut parser = lock_io(&self.parser)?;
        let size = (rows.max(1), columns.max(1));
        if parser.size() != size {
            parser.resize(columns, rows);
        }
        Ok(())
    }

    pub fn set_scrollback(&self, rows: usize) -> io::Result<bool> {
        let mut parser = lock_io(&self.parser)?;
        Ok(parser.set_scrollback(rows))
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        let mut parser = self
            .parser
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        parser.snapshot()
    }

    pub fn ui_snapshot(&self) -> ui::CommandLineTerminalSnapshot {
        let mut parser = self
            .parser
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        parser.ui_snapshot()
    }
}

/// Running terminal process with a reader thread that owns the clone of the
/// PTY read handle.  The process must be killed before the reader is joined;
/// `Drop` enforces that order.
pub struct PtyProcess {
    #[allow(dead_code)]
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    process_registration: Option<watchdog::ManagedResource>,
    parser: Arc<Mutex<Emulator>>,
    output_revision: Arc<AtomicU64>,
    reader_task: Option<ManagedThreadHandle<()>>,
    reader_done: mpsc::Receiver<()>,
    writer_task: Option<ManagedThreadHandle<()>>,
    writer_done: mpsc::Receiver<()>,
    input: Arc<InputQueue>,
}

impl std::fmt::Debug for PtyProcess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PtyProcess").finish_non_exhaustive()
    }
}

#[allow(dead_code)]
impl PtyProcess {
    pub fn spawn(config: PtyConfig, reader_tasks: &ManagedTaskGroup) -> io::Result<Self> {
        Self::spawn_recorded(config, reader_tasks, None)
    }

    pub fn spawn_recorded(
        config: PtyConfig,
        reader_tasks: &ManagedTaskGroup,
        history: Option<platform::task_history::TaskRecorder>,
    ) -> io::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(config.size()).map_err(portable_error)?;
        let mut command = CommandBuilder::new(config.program);
        command.args(config.args);
        for (name, value) in config.env {
            command.env(name, value);
        }
        #[cfg(target_os = "linux")]
        {
            let user = platform::linux::identity::LinuxUserContext::current()?;
            for (name, value) in user.environment() {
                command.env(name, value);
            }
        }
        if let Some(cwd) = config.cwd {
            // portable-pty falls back to the user's home for an invalid cwd.
            // An explicitly requested folder must instead fail visibly.
            let metadata = std::fs::metadata(&cwd).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "Could not open terminal directory {}: {error}",
                        cwd.display()
                    ),
                )
            })?;
            if !metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    format!("Terminal directory is not a folder: {}", cwd.display()),
                ));
            }
            command.cwd(cwd);
        }
        // The child needs a color-capable, non-host terminal.  The in-memory
        // parser, rather than the outer shell, renders all escape sequences.
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "TundraUX3");
        command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        for name in [
            "KITTY_WINDOW_ID",
            "KITTY_LISTEN_ON",
            "WEZTERM_PANE",
            "WEZTERM_UNIX_SOCKET",
            "ITERM_SESSION_ID",
            "WT_SESSION",
            "WT_PROFILE_ID",
            "TMUX",
            "STY",
        ] {
            command.env_remove(name);
        }
        let mut child = pair.slave.spawn_command(command).map_err(portable_error)?;
        let process_tree = match ProcessTreeGuard::attach(pair.master.as_ref(), child.as_ref()) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = child.kill();
                let _ = wait_stopped(child.as_mut());
                return Err(error);
            }
        };
        let child = Arc::new(Mutex::new(child));
        let process_tree = Arc::new(Mutex::new(process_tree));
        // The slave and master share the Windows console. Release the slave
        // before registering cleanup so closing the master really closes it.
        drop(pair.slave);
        let master = Arc::new(Mutex::new(Some(pair.master)));
        let cleanup_child = child.clone();
        let cleanup_tree = process_tree.clone();
        let cleanup_master = master.clone();
        let input = Arc::new(InputQueue::default());
        let cleanup_input = input.clone();
        let process_registration = reader_tasks.manage_resource("terminal-process", move || {
            cleanup_input.close();
            let stopped = cleanup_tree
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .terminate();
            let mut child = cleanup_child.lock().unwrap_or_else(|e| e.into_inner());
            if stopped.is_err() {
                let _ = child.kill();
            }
            let waited = wait_stopped(child.as_mut());
            drop(child);
            // A view may outlive watchdog shutdown. Close the console here
            // so its reader can reach EOF before watchdog waits for workers.
            cleanup_master
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            waited.and(stopped)
        });
        let process_registration = match process_registration {
            Ok(registration) => registration,
            Err(error) => {
                let _ = process_tree
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .terminate();
                let mut child = child.lock().unwrap_or_else(|e| e.into_inner());
                let _ = child.kill();
                let _ = wait_stopped(child.as_mut());
                return Err(io::Error::other(error));
            }
        };
        let (reader, physical_writer) = {
            let master = lock_io(&master)?;
            let master = master.as_ref().ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "terminal PTY is closed")
            })?;
            (
                master.try_clone_reader().map_err(portable_error)?,
                master.take_writer().map_err(portable_error)?,
            )
        };
        let writer = Arc::new(Mutex::new(input.writer()));
        let worker_input = input.clone();
        let (writer_done_sender, writer_done) = mpsc::channel();
        let mut physical_writer = Some(physical_writer);
        let mut writer_done_sender = Some(writer_done_sender);
        let writer_task = reader_tasks
            .spawn_thread(next_terminal_task_spec("pty-writer")?, move || {
                if let Some(writer) = physical_writer.take() {
                    worker_input.run(writer);
                }
                if let Some(sender) = writer_done_sender.take() {
                    let _ = sender.send(());
                }
            })
            .map_err(|error| {
                io::Error::other(format!("could not start terminal input writer: {error}"))
            })?;
        let parser = Arc::new(Mutex::new(Emulator::new(
            config.rows.max(1),
            config.columns.max(1),
            config.scrollback_lines,
            Arc::clone(&writer),
        )));
        let output_revision = Arc::new(AtomicU64::new(0));
        let mut parser_for_reader = Some(Arc::clone(&parser));
        let mut output_revision_for_reader = Some(Arc::clone(&output_revision));
        let mut reader = Some(reader);
        let (reader_done_sender, reader_done) = mpsc::channel();
        let mut reader_done_sender = Some(reader_done_sender);
        let reader_task = reader_tasks
            .spawn_thread(next_reader_task_spec()?, move || {
                let (Some(reader), Some(parser), Some(output_revision)) = (
                    reader.take(),
                    parser_for_reader.take(),
                    output_revision_for_reader.take(),
                ) else {
                    return;
                };
                read_terminal_output_recorded(reader, parser, output_revision, history.as_ref());
                if let Some(sender) = reader_done_sender.take() {
                    let _ = sender.send(());
                }
            })
            .map_err(|error| {
                io::Error::other(format!("could not start terminal output reader: {error}"))
            })?;

        Ok(Self {
            master,
            writer: Some(writer),
            child,
            process_registration: Some(process_registration),
            parser,
            output_revision,
            reader_task: Some(reader_task),
            reader_done,
            writer_task: Some(writer_task),
            writer_done,
            input,
        })
    }

    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "terminal PTY is closed"))?;
        let mut writer = lock_io(writer)?;
        writer.write_all(bytes)?;
        writer.flush()
    }

    pub fn send(&self, input: &TerminalInput) -> io::Result<()> {
        let application_cursor = self.input_modes().0;
        self.write(&encode_terminal_input(input, application_cursor))
    }

    pub fn send_key(&self, key: &ui::KeyEvent) -> io::Result<()> {
        lock_io(&self.parser)?.send_key(key)
    }

    pub fn send_paste(&self, text: &str) -> io::Result<()> {
        lock_io(&self.parser)?.send_paste(text)
    }

    pub fn send_focus(&self, focused: bool) -> io::Result<()> {
        lock_io(&self.parser)?.send_focus(focused);
        Ok(())
    }

    pub fn set_cell_size(&self, width: u16, height: u16) -> io::Result<()> {
        let mut parser = lock_io(&self.parser)?;
        let cell_size = (width.max(1), height.max(1));
        if parser.cell_size() == cell_size {
            return Ok(());
        }
        parser.set_cell_size(cell_size.0, cell_size.1);
        let (rows, columns) = parser.size();
        drop(parser);
        let master = lock_io(&self.master)?;
        if let Some(master) = master.as_ref() {
            master
                .resize(PtySize {
                    rows,
                    cols: columns,
                    pixel_width: columns.saturating_mul(cell_size.0),
                    pixel_height: rows.saturating_mul(cell_size.1),
                })
                .map_err(portable_error)?;
        }
        self.output_revision.fetch_add(1, Ordering::Release);
        Ok(())
    }

    /// Reads input encoding modes without cloning terminal cells or scrollback.
    pub fn input_modes(&self) -> (bool, bool) {
        let parser = self
            .parser
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        parser.input_modes()
    }

    /// Returns false when the child has not requested mouse input or the event
    /// belongs to the host controls outside its terminal content.
    pub fn send_mouse(
        &self,
        mouse: &ui::MouseEvent,
        content_area: ratatui::layout::Rect,
    ) -> io::Result<bool> {
        let Some(mouse) = mouse_to_terminal(mouse, content_area) else {
            return Ok(false);
        };
        lock_io(&self.parser)?.send_mouse(mouse)
    }

    pub fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        let cell_size = lock_io(&self.parser)?.cell_size();
        let size = PtySize {
            rows: rows.max(1),
            cols: columns.max(1),
            pixel_width: columns.saturating_mul(cell_size.0),
            pixel_height: rows.saturating_mul(cell_size.1),
        };
        let master = lock_io(&self.master)?;
        let master = master
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "terminal PTY is closed"))?;
        if lock_io(&self.parser)?.size() == (size.rows, size.cols) {
            return Ok(());
        }
        master.resize(size).map_err(portable_error)?;
        let mut parser = lock_io(&self.parser)?;
        parser.resize(size.cols, size.rows);
        self.output_revision.fetch_add(1, Ordering::Release);
        Ok(())
    }

    pub fn set_scrollback(&self, rows: usize) -> io::Result<bool> {
        let mut parser = lock_io(&self.parser)?;
        let changed = parser.set_scrollback(rows);
        if changed {
            self.output_revision.fetch_add(1, Ordering::Release);
        }
        Ok(changed)
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        self.snapshot_with_revision().0
    }

    pub fn snapshot_if_changed(&self, previous_revision: u64) -> Option<(TerminalSnapshot, u64)> {
        self.refresh_synchronized_output();
        if self.output_revision.load(Ordering::Acquire) == previous_revision {
            return None;
        }
        Some(self.snapshot_with_revision())
    }

    pub fn ui_snapshot_if_changed(
        &self,
        previous_revision: u64,
    ) -> Option<(ui::CommandLineTerminalSnapshot, u64)> {
        self.refresh_synchronized_output();
        if self.output_revision.load(Ordering::Acquire) == previous_revision {
            return None;
        }
        Some(self.ui_snapshot_with_revision())
    }

    fn refresh_synchronized_output(&self) {
        if self
            .parser
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .refresh_sync()
        {
            self.output_revision.fetch_add(1, Ordering::Release);
        }
    }

    pub fn ui_snapshot_with_revision(&self) -> (ui::CommandLineTerminalSnapshot, u64) {
        let mut parser = self
            .parser
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        (
            parser.ui_snapshot(),
            self.output_revision.load(Ordering::Acquire),
        )
    }

    pub fn snapshot_with_revision(&self) -> (TerminalSnapshot, u64) {
        // A poisoned parser only indicates that the reader panicked. Keep the
        // runtime usable and show the last valid state instead of panicking in
        // the shell UI.
        match self.parser.lock() {
            Ok(mut parser) => (
                parser.snapshot(),
                self.output_revision.load(Ordering::Acquire),
            ),
            Err(poisoned) => (
                poisoned.into_inner().snapshot(),
                self.output_revision.load(Ordering::Acquire),
            ),
        }
    }

    pub fn try_wait(&self) -> io::Result<Option<PtyExitStatus>> {
        let mut child = lock_io(&self.child)?;
        let status = child.try_wait()?.map(PtyExitStatus::from_portable);
        if status.is_none() {
            self.input.check_error()?;
        }
        Ok(status)
    }

    pub fn wait(&self) -> io::Result<PtyExitStatus> {
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Requests an orderly interrupt from the interactive program. This is
    /// deliberately separate from `force_terminate`, allowing the controller
    /// to offer a normal close first.
    pub fn graceful_terminate(&self) -> io::Result<()> {
        self.write(&[0x03])
    }

    /// Terminates all processes belonging to this terminal. On Linux a tree
    /// inspection or permission failure is reported instead of silently killing
    /// only the direct child and leaving its other jobs behind.
    pub fn force_terminate(&self) -> io::Result<()> {
        self.process_registration.as_ref().unwrap().stop()
    }

    pub fn process_id(&self) -> io::Result<Option<u32>> {
        let child = lock_io(&self.child)?;
        Ok(child.process_id())
    }

    /// Joins the output reader after `try_wait` reported process completion,
    /// preserving the final bytes that may still have been buffered by the
    /// pseudo-terminal.
    pub fn snapshot_after_exit(mut self) -> TerminalSnapshot {
        self.close_pty_handles();
        self.join_reader_bounded(Duration::from_millis(250));
        self.join_writer_bounded(Duration::from_millis(250));
        self.snapshot()
    }

    pub fn ui_snapshot_after_exit(mut self) -> ui::CommandLineTerminalSnapshot {
        self.close_pty_handles();
        self.join_reader_bounded(Duration::from_millis(250));
        self.join_writer_bounded(Duration::from_millis(250));
        self.ui_snapshot_with_revision().0
    }

    /// Cleans up the process tree and drains output before retaining its parser.
    pub fn into_output(mut self) -> io::Result<TerminalOutput> {
        self.force_terminate()?;
        self.close_pty_handles();
        if !self.join_reader_bounded(Duration::from_millis(500)) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "terminal output reader did not stop after closing its PTY",
            ));
        }
        if !self.join_writer_bounded(Duration::from_millis(500)) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "terminal input writer did not stop after closing its PTY",
            ));
        }
        Ok(TerminalOutput {
            parser: Arc::clone(&self.parser),
        })
    }

    fn close_pty_handles(&mut self) {
        self.input.close();
        self.writer.take();
        self.master.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    fn join_reader_bounded(&mut self, timeout: Duration) -> bool {
        join_worker(&mut self.reader_task, &self.reader_done, timeout)
    }

    fn join_writer_bounded(&mut self, timeout: Duration) -> bool {
        join_worker(&mut self.writer_task, &self.writer_done, timeout)
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        // Killing first closes the slave end and lets the reader finish. It is
        // intentionally best-effort: teardown must never panic.
        let _ = self.force_terminate();
        self.process_registration.take();
        self.close_pty_handles();
        self.join_reader_bounded(Duration::from_millis(500));
        self.join_writer_bounded(Duration::from_millis(500));
    }
}

fn next_reader_task_spec() -> io::Result<TaskSpec> {
    next_terminal_task_spec("pty-reader")
}

fn next_terminal_task_spec(prefix: &str) -> io::Result<TaskSpec> {
    let sequence = NEXT_PTY_READER_TASK_ID
        .fetch_add(1, Ordering::Relaxed)
        .max(1);
    let id = TaskId::new(format!("{prefix}-{sequence}"))
        .map_err(|error| io::Error::other(format!("invalid terminal reader task id: {error}")))?;
    Ok(TaskSpec::one_shot(id))
}

fn join_worker(
    worker: &mut Option<ManagedThreadHandle<()>>,
    done: &mpsc::Receiver<()>,
    timeout: Duration,
) -> bool {
    if worker.is_none() {
        return true;
    }
    match done.recv_timeout(timeout) {
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
            true
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Watchdog retains the worker and reports a cleanup failure.
            worker.take();
            false
        }
    }
}

fn wait_stopped(child: &mut (dyn Child + Send + Sync)) -> io::Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while child.try_wait()?.is_none() {
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "terminal child did not stop within one second",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

/// Keeps all descendants of the child terminal in a platform containment
/// boundary so closing a terminal cannot leave commands running behind it.
#[cfg(windows)]
struct ProcessTreeGuard {
    job: windows_sys::Win32::Foundation::HANDLE,
}

// The owned Windows job is used only while holding process_tree's mutex.
#[cfg(windows)]
unsafe impl Send for ProcessTreeGuard {}

#[cfg(windows)]
impl ProcessTreeGuard {
    fn attach(_master: &(dyn MasterPty + Send), child: &dyn Child) -> io::Result<Self> {
        use std::mem::size_of;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let guard = Self { job };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .unwrap_or(u32::MAX),
            )
        };
        if configured == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = child.as_raw_handle().ok_or_else(|| {
            io::Error::other("portable PTY did not expose a Windows child process handle")
        })? as HANDLE;
        if unsafe { AssignProcessToJobObject(job, process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(guard)
    }

    fn terminate(&self) -> io::Result<()> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        if unsafe { TerminateJobObject(self.job, 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        let _ = self.terminate();
        let _ = unsafe { CloseHandle(self.job) };
    }
}

#[cfg(unix)]
struct ProcessTreeGuard {
    #[cfg(not(target_os = "linux"))]
    process_group: Option<libc::pid_t>,
    #[cfg(target_os = "linux")]
    linux: process_tree_linux::LinuxProcessTree,
}

#[cfg(unix)]
impl ProcessTreeGuard {
    fn attach(master: &(dyn MasterPty + Send), child: &dyn Child) -> io::Result<Self> {
        let process_group = master.process_group_leader().or_else(|| {
            child
                .process_id()
                .and_then(|process_id| libc::pid_t::try_from(process_id).ok())
        });
        let Some(process_group) = process_group else {
            return Err(io::Error::other(
                "portable PTY did not expose a child process group",
            ));
        };
        if process_group <= 0 || process_group == std::process::id() as libc::pid_t {
            return Err(io::Error::other(
                "portable PTY returned an unsafe child process group",
            ));
        }
        Ok(Self {
            #[cfg(not(target_os = "linux"))]
            process_group: Some(process_group),
            #[cfg(target_os = "linux")]
            linux: process_tree_linux::LinuxProcessTree::attach(process_group)?,
        })
    }

    fn terminate(&self) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        return self.linux.terminate();
        #[cfg(not(target_os = "linux"))]
        {
            let Some(process_group) = self.process_group else {
                return Err(io::Error::other("child process group is unavailable"));
            };
            // A broken PTY backend must never cause the Shell to kill its own
            // foreground group.
            if process_group <= 0 || process_group == std::process::id() as libc::pid_t {
                return Err(io::Error::other("child process group is unsafe"));
            }
            #[cfg(target_os = "macos")]
            process_tree_macos::terminate_background_groups(process_group)?;
            if unsafe { libc::kill(-process_group, libc::SIGKILL) } == 0 {
                Ok(())
            } else {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH) {
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }
}

#[cfg(unix)]
impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

fn read_terminal_output_recorded(
    mut reader: Box<dyn Read + Send>,
    parser: Arc<Mutex<Emulator>>,
    output_revision: Arc<AtomicU64>,
    history: Option<&platform::task_history::TaskRecorder>,
) {
    let mut buffer = [0_u8; 8_192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => {
                if parser
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .finish_output()
                {
                    output_revision.fetch_add(1, Ordering::Release);
                }
                return;
            }
            Ok(count) => {
                let recorded = parser
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .process(&buffer[..count]);
                if let Some(history) = history {
                    history.output(&recorded);
                }
                output_revision.fetch_add(1, Ordering::Release);
            }
        }
    }
}

fn lock_io<T>(mutex: &Mutex<T>) -> io::Result<std::sync::MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|_| io::Error::other("terminal runtime lock was poisoned"))
}

fn portable_error(error: impl ToString) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
#[path = "../tests/unit/pty.rs"]
mod tests;
