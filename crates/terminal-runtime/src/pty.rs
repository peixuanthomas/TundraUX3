//! The isolated pseudo-terminal used by the Command Line Launcher app.
//!
//! This module deliberately never writes child output to the host terminal.
//! Child output is decoded into a `vt100` screen and the UI consumes a safe,
//! structured snapshot instead.  In particular, OSC sequences (including
//! OSC 52 clipboard requests) are discarded before they reach the parser.

pub use crate::input::{
    MouseProtocolEncoding, MouseProtocolMode, TerminalInput, encode_terminal_input,
    key_event_bytes, mouse_event_bytes, paste_bytes,
};
pub use crate::snapshot::{
    TerminalCell, TerminalColor, TerminalCursorStyle, TerminalSnapshot, to_ui_snapshot,
};
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

/// The child exit code reserved by `tundra-cli repl --embedded` for a
/// confirmed `new` request.  The shell owns the reset/restart action.
pub const EMBEDDED_RESET_EXIT_CODE: u32 = 75;
pub const DEFAULT_COLUMNS: u16 = 108;
pub const DEFAULT_ROWS: u16 = 20;

/// Private environment contract with the embedded `tundra-cli` REPL.
const COMMAND_LINE_USERNAME_ENV: &str = "TUNDRA_COMMAND_LINE_USERNAME";
const COMMAND_LINE_ACCENT_ENV: &str = "TUNDRA_COMMAND_LINE_ACCENT";

static NEXT_PTY_READER_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct CommandLinePtyConfig {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub cwd: Option<PathBuf>,
    pub columns: u16,
    pub rows: u16,
    pub scrollback_lines: usize,
}

impl CommandLinePtyConfig {
    /// Creates the exact command used by the embedded Command Line app.
    pub fn tundra_cli(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: vec![OsString::from("repl"), OsString::from("--embedded")],
            env: Vec::new(),
            cwd: None,
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
            scrollback_lines: 2_000,
        }
    }

    pub fn with_username(mut self, username: &str) -> Self {
        self.env.push((
            OsString::from(COMMAND_LINE_USERNAME_ENV),
            OsString::from(username),
        ));
        self
    }

    pub fn with_accent_color(mut self, color: ratatui::style::Color) -> Self {
        self.env.push((
            OsString::from(COMMAND_LINE_ACCENT_ENV),
            OsString::from(crate::ansi_foreground(color)),
        ));
        self
    }

    fn size(&self) -> PtySize {
        PtySize {
            rows: self.rows.max(1),
            cols: self.columns.max(1),
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandLineExitStatus {
    pub code: u32,
    pub success: bool,
}

impl CommandLineExitStatus {
    fn from_portable(status: portable_pty::ExitStatus) -> Self {
        Self {
            code: status.exit_code(),
            success: status.success(),
        }
    }
}

/// Retained terminal contents after the process, PTY and output reader close.
pub struct TerminalOutput {
    parser: Arc<Mutex<vt100::Parser>>,
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
        if parser.screen().size() != size {
            parser.set_size(size.0, size.1);
        }
        Ok(())
    }

    pub fn set_scrollback(&self, rows: usize) -> io::Result<bool> {
        let mut parser = lock_io(&self.parser)?;
        let previous = parser.screen().scrollback();
        parser.set_scrollback(rows);
        Ok(parser.screen().scrollback() != previous)
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        let mut parser = self
            .parser
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        TerminalSnapshot::from_parser(&mut parser)
    }
}

/// Running terminal process with a reader thread that owns the clone of the
/// PTY read handle.  The process must be killed before the reader is joined;
/// `Drop` enforces that order.
pub struct CommandLinePty {
    #[allow(dead_code)]
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    process_registration: Option<watchdog::ManagedResource>,
    parser: Arc<Mutex<vt100::Parser>>,
    output_revision: Arc<AtomicU64>,
    reader_task: Option<ManagedThreadHandle<()>>,
    reader_done: mpsc::Receiver<()>,
}

impl std::fmt::Debug for CommandLinePty {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandLinePty")
            .finish_non_exhaustive()
    }
}

#[allow(dead_code)]
impl CommandLinePty {
    pub fn spawn(
        config: CommandLinePtyConfig,
        reader_tasks: &ManagedTaskGroup,
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
                        "Could not open Command Line directory {}: {error}",
                        cwd.display()
                    ),
                )
            })?;
            if !metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    format!("Command Line directory is not a folder: {}", cwd.display()),
                ));
            }
            command.cwd(cwd);
        }
        // The child needs a color-capable, non-host terminal.  The in-memory
        // parser, rather than the outer shell, renders all escape sequences.
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
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
        let process_registration = reader_tasks.manage_resource("terminal-process", move || {
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
        let (reader, writer) = {
            let master = lock_io(&master)?;
            let master = master.as_ref().ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "command line PTY is closed")
            })?;
            (
                master.try_clone_reader().map_err(portable_error)?,
                Arc::new(Mutex::new(master.take_writer().map_err(portable_error)?)),
            )
        };
        let parser = Arc::new(Mutex::new(vt100::Parser::new(
            config.rows.max(1),
            config.columns.max(1),
            config.scrollback_lines,
        )));
        let output_revision = Arc::new(AtomicU64::new(0));
        let mut parser_for_reader = Some(Arc::clone(&parser));
        let mut output_revision_for_reader = Some(Arc::clone(&output_revision));
        let mut writer_for_reader = Some(Arc::clone(&writer));
        let mut reader = Some(reader);
        let (reader_done_sender, reader_done) = mpsc::channel();
        let mut reader_done_sender = Some(reader_done_sender);
        let reader_task = reader_tasks
            .spawn_thread(next_reader_task_spec()?, move || {
                let (Some(reader), Some(parser), Some(output_revision), Some(writer)) = (
                    reader.take(),
                    parser_for_reader.take(),
                    output_revision_for_reader.take(),
                    writer_for_reader.take(),
                ) else {
                    return;
                };
                read_pty_output(reader, parser, output_revision, writer);
                if let Some(sender) = reader_done_sender.take() {
                    let _ = sender.send(());
                }
            })
            .map_err(|error| {
                io::Error::other(format!("could not start CLI output reader: {error}"))
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
        })
    }

    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let writer = self.writer.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "command line PTY is closed")
        })?;
        let mut writer = lock_io(writer)?;
        writer.write_all(bytes)?;
        writer.flush()
    }

    pub fn send(&self, input: &TerminalInput) -> io::Result<()> {
        let application_cursor = self.input_modes().0;
        self.write(&encode_terminal_input(input, application_cursor))
    }

    /// Reads input encoding modes without cloning terminal cells or scrollback.
    pub fn input_modes(&self) -> (bool, bool) {
        let parser = self
            .parser
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let screen = parser.screen();
        (screen.application_cursor(), screen.bracketed_paste())
    }

    /// Returns false when the child has not requested mouse input or the event
    /// belongs to the host controls outside its terminal content.
    pub fn send_mouse(
        &self,
        mouse: &ui::MouseEvent,
        content_area: ratatui::layout::Rect,
    ) -> io::Result<bool> {
        let bytes = {
            let parser = lock_io(&self.parser)?;
            let screen = parser.screen();
            if screen.mouse_protocol_mode() == MouseProtocolMode::None
                || mouse.column() < content_area.x
                || mouse.column() >= content_area.right()
                || mouse.row() < content_area.y
                || mouse.row() >= content_area.bottom()
            {
                return Ok(false);
            }
            mouse_event_bytes(
                mouse,
                content_area,
                screen.mouse_protocol_mode(),
                screen.mouse_protocol_encoding(),
            )
        };
        if let Some(bytes) = bytes {
            self.write(&bytes)?;
        }
        Ok(true)
    }

    pub fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        let size = PtySize {
            rows: rows.max(1),
            cols: columns.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        let master = lock_io(&self.master)?;
        let master = master.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "command line PTY is closed")
        })?;
        if lock_io(&self.parser)?.screen().size() == (size.rows, size.cols) {
            return Ok(());
        }
        master.resize(size).map_err(portable_error)?;
        let mut parser = lock_io(&self.parser)?;
        parser.set_size(size.rows, size.cols);
        self.output_revision.fetch_add(1, Ordering::Release);
        Ok(())
    }

    pub fn set_scrollback(&self, rows: usize) -> io::Result<bool> {
        let mut parser = lock_io(&self.parser)?;
        let previous = parser.screen().scrollback();
        parser.set_scrollback(rows);
        let changed = parser.screen().scrollback() != previous;
        if changed {
            self.output_revision.fetch_add(1, Ordering::Release);
        }
        Ok(changed)
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        self.snapshot_with_revision().0
    }

    pub fn snapshot_if_changed(&self, previous_revision: u64) -> Option<(TerminalSnapshot, u64)> {
        if self.output_revision.load(Ordering::Acquire) == previous_revision {
            return None;
        }
        Some(self.snapshot_with_revision())
    }

    pub fn snapshot_with_revision(&self) -> (TerminalSnapshot, u64) {
        // A poisoned parser only indicates that the reader panicked. Keep the
        // runtime usable and show the last valid state instead of panicking in
        // the shell UI.
        match self.parser.lock() {
            Ok(mut parser) => (
                TerminalSnapshot::from_parser(&mut parser),
                self.output_revision.load(Ordering::Acquire),
            ),
            Err(poisoned) => (
                TerminalSnapshot::from_parser(&mut poisoned.into_inner()),
                self.output_revision.load(Ordering::Acquire),
            ),
        }
    }

    pub fn try_wait(&self) -> io::Result<Option<CommandLineExitStatus>> {
        let mut child = lock_io(&self.child)?;
        child
            .try_wait()
            .map(|status| status.map(CommandLineExitStatus::from_portable))
    }

    pub fn wait(&self) -> io::Result<CommandLineExitStatus> {
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
        self.snapshot()
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
        Ok(TerminalOutput {
            parser: Arc::clone(&self.parser),
        })
    }

    fn close_pty_handles(&mut self) {
        self.writer.take();
        self.master.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    fn join_reader_bounded(&mut self, timeout: Duration) -> bool {
        if self.reader_task.is_none() {
            return true;
        }
        match self.reader_done.recv_timeout(timeout) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(reader_task) = self.reader_task.take() {
                    let _ = reader_task.join();
                }
                true
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Keep the worker in watchdog's registry. A broken PTY must
                // not freeze the UI; watchdog reports a shutdown failure if
                // closing our process tree does not unblock the reader.
                self.reader_task.take();
                false
            }
        }
    }
}

impl Drop for CommandLinePty {
    fn drop(&mut self) {
        // Killing first closes the slave end and lets the reader finish. It is
        // intentionally best-effort: teardown must never panic.
        let _ = self.force_terminate();
        self.process_registration.take();
        self.close_pty_handles();
        self.join_reader_bounded(Duration::from_millis(500));
    }
}

fn next_reader_task_spec() -> io::Result<TaskSpec> {
    let sequence = NEXT_PTY_READER_TASK_ID
        .fetch_add(1, Ordering::Relaxed)
        .max(1);
    let id = TaskId::new(format!("pty-reader-{sequence}"))
        .map_err(|error| io::Error::other(format!("invalid CLI reader task id: {error}")))?;
    Ok(TaskSpec::one_shot(id))
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

/// Keeps all descendants of the embedded CLI in a platform containment
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

fn read_pty_output(
    mut reader: Box<dyn Read + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
    output_revision: Arc<AtomicU64>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
) {
    let mut buffer = [0_u8; 8_192];
    let mut osc_filter = OscFilter::default();
    let mut terminal_responder = TerminalResponder::default();
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                let safe = osc_filter.filter(&buffer[..count]);
                if safe.is_empty() {
                    continue;
                }
                let cursor_position = match parser.lock() {
                    Ok(mut parser) => {
                        parser.process(&safe);
                        output_revision.fetch_add(1, Ordering::Release);
                        parser.screen().cursor_position()
                    }
                    Err(poisoned) => {
                        let mut parser = poisoned.into_inner();
                        parser.process(&safe);
                        output_revision.fetch_add(1, Ordering::Release);
                        parser.screen().cursor_position()
                    }
                };
                let _ = terminal_responder.respond(&safe, cursor_position, &writer);
            }
        }
    }
}

/// Implements the minimum terminal-query response required by Windows
/// ConPTY.  At startup it sends CSI 6 n and waits for the terminal emulator
/// to return a cursor-position report before it emits the interactive prompt.
#[derive(Default)]
struct TerminalResponder {
    dsr_prefix_len: usize,
}

impl TerminalResponder {
    fn respond(
        &mut self,
        bytes: &[u8],
        cursor_position: (u16, u16),
        writer: &Arc<Mutex<Box<dyn Write + Send>>>,
    ) -> io::Result<()> {
        const CURSOR_POSITION_QUERY: &[u8] = b"\x1b[6n";
        for &byte in bytes {
            if byte == CURSOR_POSITION_QUERY[self.dsr_prefix_len] {
                self.dsr_prefix_len += 1;
                if self.dsr_prefix_len == CURSOR_POSITION_QUERY.len() {
                    self.dsr_prefix_len = 0;
                    let (row, column) = cursor_position;
                    let reply = format!(
                        "\x1b[{};{}R",
                        row.saturating_add(1),
                        column.saturating_add(1)
                    );
                    let mut writer = lock_io(writer)?;
                    writer.write_all(reply.as_bytes())?;
                    writer.flush()?;
                }
            } else {
                self.dsr_prefix_len = usize::from(byte == CURSOR_POSITION_QUERY[0]);
            }
        }
        Ok(())
    }
}

fn lock_io<T>(mutex: &Mutex<T>) -> io::Result<std::sync::MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|_| io::Error::other("command line runtime lock was poisoned"))
}

fn portable_error(error: impl ToString) -> io::Error {
    io::Error::other(error.to_string())
}

#[derive(Default)]
struct OscFilter {
    state: OscFilterState,
    utf8_continuations: u8,
}

#[derive(Default)]
enum OscFilterState {
    #[default]
    Ground,
    Escape,
    Osc {
        escape_seen: bool,
    },
}

impl OscFilter {
    /// Removes OSC control strings, including split sequences.  This protects
    /// against clipboard / hyperlink / title side effects while preserving all
    /// ordinary ANSI CSI and printable output for `vt100`.
    fn filter(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            let utf8_continuation = self.advance_utf8(byte);
            match &mut self.state {
                OscFilterState::Ground => {
                    if utf8_continuation {
                        output.push(byte);
                    } else if byte == 0x1b {
                        self.state = OscFilterState::Escape;
                    } else if byte == 0x9d {
                        self.state = OscFilterState::Osc { escape_seen: false };
                    } else {
                        output.push(byte);
                    }
                }
                OscFilterState::Escape => {
                    if byte == b']' {
                        self.state = OscFilterState::Osc { escape_seen: false };
                    } else {
                        output.push(0x1b);
                        output.push(byte);
                        self.state = OscFilterState::Ground;
                    }
                }
                OscFilterState::Osc { escape_seen } => {
                    if !utf8_continuation
                        && (byte == 0x07 || byte == 0x9c || (*escape_seen && byte == b'\\'))
                    {
                        self.state = OscFilterState::Ground;
                    } else if !utf8_continuation {
                        *escape_seen = byte == 0x1b;
                    }
                }
            }
        }
        output
    }

    /// Keeps C1 control bytes distinct from identical byte values inside a
    /// UTF-8 character. For example, `保` ends in `0x9d`; only a standalone
    /// `0x9d` starts an eight-bit OSC sequence.
    fn advance_utf8(&mut self, byte: u8) -> bool {
        if self.utf8_continuations > 0 && (0x80..=0xbf).contains(&byte) {
            self.utf8_continuations -= 1;
            return true;
        }

        self.utf8_continuations = match byte {
            0xc2..=0xdf => 1,
            0xe0..=0xef => 2,
            0xf0..=0xf4 => 3,
            _ => 0,
        };
        false
    }
}

#[cfg(test)]
#[path = "../tests/unit/pty.rs"]
mod tests;
