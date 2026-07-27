//! The isolated pseudo-terminal used by the Command Line Launcher app.
//!
//! This module deliberately never writes child output to the host terminal.
//! Child output is decoded into a `vt100` screen and the UI consumes a safe,
//! structured snapshot instead.  In particular, OSC sequences (including
//! OSC 52 clipboard requests) are discarded before they reach the parser.

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use watchdog::{AppWatchdog, ComponentId, ManagedTaskGroup, ManagedThreadHandle, TaskId, TaskSpec};

#[cfg(test)]
use crate::InputModifiers;
use crate::{InputEvent, InputKey, KeyInput};
use platform::Platform;

/// The child exit code reserved by `tundra-cli repl --embedded` for a
/// confirmed `new` request.  The shell owns the reset/restart action.
pub const EMBEDDED_RESET_EXIT_CODE: u32 = 75;
pub const DEFAULT_COLUMNS: u16 = 108;
pub const DEFAULT_ROWS: u16 = 20;

static NEXT_PTY_READER_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct CommandLinePtyConfig {
    pub program: OsString,
    pub args: Vec<OsString>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl From<vt100::Color> for TerminalColor {
    fn from(value: vt100::Color) -> Self {
        match value {
            vt100::Color::Default => Self::Default,
            vt100::Color::Idx(index) => Self::Indexed(index),
            vt100::Color::Rgb(red, green, blue) => Self::Rgb(red, green, blue),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCell {
    pub text: String,
    pub foreground: TerminalColor,
    pub background: TerminalColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub wide: bool,
    pub wide_continuation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub rows: u16,
    pub columns: u16,
    pub cells: Vec<Vec<TerminalCell>>,
    pub cursor_row: u16,
    pub cursor_column: u16,
    pub cursor_visible: bool,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub title: String,
}

impl TerminalSnapshot {
    fn from_parser(parser: &vt100::Parser) -> Self {
        let screen = parser.screen();
        let (rows, columns) = screen.size();
        let mut cells = Vec::with_capacity(usize::from(rows));
        for row in 0..rows {
            let mut snapshot_row = Vec::with_capacity(usize::from(columns));
            for column in 0..columns {
                let cell = screen.cell(row, column);
                snapshot_row.push(match cell {
                    Some(cell) => TerminalCell {
                        text: cell.contents(),
                        foreground: cell.fgcolor().into(),
                        background: cell.bgcolor().into(),
                        bold: cell.bold(),
                        italic: cell.italic(),
                        underline: cell.underline(),
                        inverse: cell.inverse(),
                        wide: cell.is_wide(),
                        wide_continuation: cell.is_wide_continuation(),
                    },
                    None => TerminalCell {
                        text: String::new(),
                        foreground: TerminalColor::Default,
                        background: TerminalColor::Default,
                        bold: false,
                        italic: false,
                        underline: false,
                        inverse: false,
                        wide: false,
                        wide_continuation: false,
                    },
                });
            }
            cells.push(snapshot_row);
        }
        let (cursor_row, cursor_column) = screen.cursor_position();
        Self {
            rows,
            columns,
            cells,
            cursor_row,
            cursor_column,
            cursor_visible: !screen.hide_cursor(),
            application_cursor: screen.application_cursor(),
            bracketed_paste: screen.bracketed_paste(),
            // OSC is filtered, so this will remain empty unless a future
            // parser API supplies a title from another safe source.
            title: screen.title().to_owned(),
        }
    }
}

/// A key-independent input representation.  The controller can use this
/// instead of leaking terminal escape sequences into UI code.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TerminalInput {
    Bytes(Vec<u8>),
    Text(String),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    PageUp,
    PageDown,
    CtrlC,
}

pub fn encode_terminal_input(input: &TerminalInput, application_cursor: bool) -> Vec<u8> {
    match input {
        TerminalInput::Bytes(bytes) => bytes.clone(),
        TerminalInput::Text(text) => text.as_bytes().to_vec(),
        TerminalInput::Enter => b"\r".to_vec(),
        TerminalInput::Backspace => vec![0x7f],
        TerminalInput::Tab => b"\t".to_vec(),
        TerminalInput::Escape => vec![0x1b],
        TerminalInput::CtrlC => vec![0x03],
        TerminalInput::Up => cursor_sequence(b'A', application_cursor),
        TerminalInput::Down => cursor_sequence(b'B', application_cursor),
        TerminalInput::Right => cursor_sequence(b'C', application_cursor),
        TerminalInput::Left => cursor_sequence(b'D', application_cursor),
        TerminalInput::Home => b"\x1b[H".to_vec(),
        TerminalInput::End => b"\x1b[F".to_vec(),
        TerminalInput::Delete => b"\x1b[3~".to_vec(),
        TerminalInput::PageUp => b"\x1b[5~".to_vec(),
        TerminalInput::PageDown => b"\x1b[6~".to_vec(),
    }
}

fn cursor_sequence(final_byte: u8, application_cursor: bool) -> Vec<u8> {
    if application_cursor {
        vec![0x1b, b'O', final_byte]
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

/// Running terminal process with a reader thread that owns the clone of the
/// PTY read handle.  The process must be killed before the reader is joined;
/// `Drop` enforces that order.
pub struct CommandLinePty {
    #[allow(dead_code)]
    master: Option<Arc<Mutex<Box<dyn MasterPty + Send>>>>,
    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    process_tree: ProcessTreeGuard,
    parser: Arc<Mutex<vt100::Parser>>,
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
        if let Some(cwd) = config.cwd {
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
                return Err(error);
            }
        };
        let reader = pair.master.try_clone_reader().map_err(portable_error)?;
        let writer = Arc::new(Mutex::new(
            pair.master.take_writer().map_err(portable_error)?,
        ));
        let parser = Arc::new(Mutex::new(vt100::Parser::new(
            config.rows.max(1),
            config.columns.max(1),
            config.scrollback_lines,
        )));
        let mut parser_for_reader = Some(Arc::clone(&parser));
        let mut writer_for_reader = Some(Arc::clone(&writer));
        let mut reader = Some(reader);
        let (reader_done_sender, reader_done) = mpsc::channel();
        let mut reader_done_sender = Some(reader_done_sender);
        let reader_task = reader_tasks
            .spawn_thread(next_reader_task_spec()?, move || {
                let (Some(reader), Some(parser), Some(writer)) = (
                    reader.take(),
                    parser_for_reader.take(),
                    writer_for_reader.take(),
                ) else {
                    return;
                };
                read_pty_output(reader, parser, writer);
                if let Some(sender) = reader_done_sender.take() {
                    let _ = sender.send(());
                }
            })
            .map_err(|error| {
                io::Error::other(format!("could not start CLI output reader: {error}"))
            })?;

        Ok(Self {
            master: Some(Arc::new(Mutex::new(pair.master))),
            writer: Some(writer),
            child: Arc::new(Mutex::new(child)),
            process_tree,
            parser,
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
        let application_cursor = self.snapshot().application_cursor;
        self.write(&encode_terminal_input(input, application_cursor))
    }

    pub fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        let size = PtySize {
            rows: rows.max(1),
            cols: columns.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        let master = self.master.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "command line PTY is closed")
        })?;
        lock_io(master)?.resize(size).map_err(portable_error)?;
        lock_io(&self.parser)?.set_size(size.rows, size.cols);
        Ok(())
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        // A poisoned parser only indicates that the reader panicked. Keep the
        // runtime usable and show the last valid state instead of panicking in
        // the shell UI.
        match self.parser.lock() {
            Ok(parser) => TerminalSnapshot::from_parser(&parser),
            Err(poisoned) => TerminalSnapshot::from_parser(&poisoned.into_inner()),
        }
    }

    pub fn try_wait(&self) -> io::Result<Option<CommandLineExitStatus>> {
        let mut child = lock_io(&self.child)?;
        child
            .try_wait()
            .map(|status| status.map(CommandLineExitStatus::from_portable))
    }

    pub fn wait(&self) -> io::Result<CommandLineExitStatus> {
        let mut child = lock_io(&self.child)?;
        child.wait().map(CommandLineExitStatus::from_portable)
    }

    /// Requests an orderly interrupt from the interactive program. This is
    /// deliberately separate from `force_terminate`, allowing the controller
    /// to offer a normal close first.
    pub fn graceful_terminate(&self) -> io::Result<()> {
        self.write(&[0x03])
    }

    /// Terminates the platform containment boundary first, then the direct
    /// PTY child as a fallback if the boundary is already gone.
    pub fn force_terminate(&self) -> io::Result<()> {
        match self.process_tree.terminate() {
            Ok(()) => Ok(()),
            Err(containment_error) => {
                let mut child = lock_io(&self.child)?;
                child.kill().map_err(|child_error| {
                    io::Error::other(format!(
                        "process-tree termination failed ({containment_error}); direct child termination also failed ({child_error})"
                    ))
                })
            }
        }
    }

    pub fn process_id(&self) -> io::Result<Option<u32>> {
        let child = lock_io(&self.child)?;
        Ok(child.process_id())
    }

    /// Joins the output reader after `try_wait` reported process completion,
    /// preserving the final bytes that may still have been buffered by the
    /// pseudo-terminal.
    fn snapshot_after_exit(mut self) -> TerminalSnapshot {
        self.close_pty_handles();
        self.join_reader_bounded(Duration::from_millis(250));
        self.snapshot()
    }

    fn close_pty_handles(&mut self) {
        self.writer.take();
        self.master.take();
    }

    fn join_reader_bounded(&mut self, timeout: Duration) {
        if self.reader_task.is_none() {
            return;
        }
        match self.reader_done.recv_timeout(timeout) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(reader_task) = self.reader_task.take() {
                    let _ = reader_task.join();
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // A broken platform PTY must never freeze the Shell. The
                // detached reader owns no host terminal and will close its
                // remaining handle whenever the operating-system read ends.
                self.reader_task.take();
            }
        }
    }
}

impl Drop for CommandLinePty {
    fn drop(&mut self) {
        // Killing first closes the slave end and lets the reader finish. It is
        // intentionally best-effort: teardown must never panic.
        let _ = self.force_terminate();
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

/// Keeps all descendants of the embedded CLI in a platform containment
/// boundary so the emergency shortcut cannot leave `/` commands behind.
#[cfg(windows)]
struct ProcessTreeGuard {
    job: windows_sys::Win32::Foundation::HANDLE,
}

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
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
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
    process_group: Option<libc::pid_t>,
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
            process_group: Some(process_group),
        })
    }

    fn terminate(&self) -> io::Result<()> {
        let Some(process_group) = self.process_group else {
            return Err(io::Error::other("child process group is unavailable"));
        };
        // A broken PTY backend must never cause the Shell to kill its own
        // foreground group.
        if process_group <= 0 || process_group == std::process::id() as libc::pid_t {
            return Err(io::Error::other("child process group is unsafe"));
        }
        if unsafe { libc::kill(-process_group, libc::SIGKILL) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
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
                        parser.screen().cursor_position()
                    }
                    Err(poisoned) => {
                        let mut parser = poisoned.into_inner();
                        parser.process(&safe);
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
            match &mut self.state {
                OscFilterState::Ground => {
                    if byte == 0x1b {
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
                    if byte == 0x07 || byte == 0x9c || (*escape_seen && byte == b'\\') {
                        self.state = OscFilterState::Ground;
                    } else {
                        *escape_seen = byte == 0x1b;
                    }
                }
            }
        }
        output
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineHostEvent {
    None,
    ExitToLauncher,
    ResetRequested,
}

#[derive(Debug)]
enum CommandLineHostState {
    Inactive,
    Running(CommandLinePty),
    Exited { code: i32 },
    Failed { message: String },
}

/// Owns the PTY only while the Command Line screen is active. The ordinary
/// Shell state remains cloneable and deterministic for controller tests.
pub struct CommandLineHost {
    state: CommandLineHostState,
    snapshot: TerminalSnapshot,
    reader_tasks: ManagedTaskGroup,
}

impl CommandLineHost {
    pub fn new(watchdog: AppWatchdog) -> Self {
        Self {
            state: CommandLineHostState::Inactive,
            snapshot: blank_terminal_snapshot(),
            reader_tasks: watchdog
                .child_component(ComponentId::from_static("command-line"))
                .task_group("pty-reader"),
        }
    }

    pub fn ensure_started(&mut self, platform: &dyn Platform) {
        if !matches!(self.state, CommandLineHostState::Inactive) {
            return;
        }

        let result = resolve_tundra_cli_program().and_then(|program| {
            let mut config = CommandLinePtyConfig::tundra_cli(program);
            if let Ok(directories) = platform.user_dirs() {
                let documents = directories.documents();
                if documents.is_dir() {
                    config.cwd = Some(documents.to_path_buf());
                }
            }
            CommandLinePty::spawn(config, &self.reader_tasks)
        });
        match result {
            Ok(pty) => {
                self.snapshot = pty.snapshot();
                self.state = CommandLineHostState::Running(pty);
            }
            Err(error) => {
                self.state = CommandLineHostState::Failed {
                    message: format!("Could not start tundra-cli: {error}"),
                };
            }
        }
    }

    pub fn resize_terminal(&mut self, columns: u16, rows: u16) {
        // The caller has already validated the outer Shell layout.  These are
        // the dimensions of the inner, bordered terminal panel, so comparing
        // them with the larger outer-terminal minimum would reject every
        // valid Command Line window (108x22 maps to 106x14).
        if columns == 0 || rows == 0 {
            return;
        }
        let needs_resize = self.snapshot.columns != columns || self.snapshot.rows != rows;
        if !needs_resize {
            return;
        }
        let result = match &self.state {
            CommandLineHostState::Running(pty) => pty.resize(columns, rows),
            _ => return,
        };
        match result {
            Ok(()) => {
                if let CommandLineHostState::Running(pty) = &self.state {
                    self.snapshot = pty.snapshot();
                }
            }
            Err(error) => {
                self.fail_running(format!("Could not resize CLI terminal: {error}"));
            }
        }
    }

    pub fn poll(&mut self) -> CommandLineHostEvent {
        let status = match &self.state {
            CommandLineHostState::Running(pty) => match pty.try_wait() {
                Ok(status) => status,
                Err(error) => {
                    self.fail_running(format!("Could not read CLI process status: {error}"));
                    return CommandLineHostEvent::None;
                }
            },
            _ => return CommandLineHostEvent::None,
        };

        let Some(status) = status else {
            if let CommandLineHostState::Running(pty) = &self.state {
                self.snapshot = pty.snapshot();
            }
            return CommandLineHostEvent::None;
        };

        let previous = std::mem::replace(&mut self.state, CommandLineHostState::Inactive);
        if let CommandLineHostState::Running(pty) = previous {
            self.snapshot = pty.snapshot_after_exit();
        }

        if status.code == EMBEDDED_RESET_EXIT_CODE {
            return CommandLineHostEvent::ResetRequested;
        }
        if status.success {
            self.snapshot = blank_terminal_snapshot();
            return CommandLineHostEvent::ExitToLauncher;
        }

        self.state = CommandLineHostState::Exited {
            code: i32::try_from(status.code).unwrap_or(i32::MAX),
        };
        CommandLineHostEvent::None
    }

    pub fn handle_input(
        &mut self,
        input: &InputEvent,
        terminal_size_accepted: bool,
    ) -> CommandLineHostEvent {
        if let InputEvent::Key(key) = input
            && key.phase.is_press_like()
            && is_emergency_termination(key)
        {
            self.terminate();
            return CommandLineHostEvent::ExitToLauncher;
        }

        match &self.state {
            CommandLineHostState::Exited { .. } | CommandLineHostState::Failed { .. } => {
                if let InputEvent::Key(key) = input
                    && key.phase.is_press_like()
                {
                    match key.key {
                        InputKey::Enter => {
                            self.snapshot = blank_terminal_snapshot();
                            self.state = CommandLineHostState::Inactive;
                        }
                        InputKey::Escape => return CommandLineHostEvent::ExitToLauncher,
                        _ => {}
                    }
                }
                return CommandLineHostEvent::None;
            }
            CommandLineHostState::Inactive | CommandLineHostState::Running(_) => {}
        }

        if !terminal_size_accepted {
            return CommandLineHostEvent::None;
        }

        let write_result = match (&self.state, input) {
            (CommandLineHostState::Running(pty), InputEvent::Key(key))
                if key.phase.is_press_like() =>
            {
                key_event_bytes(key, self.snapshot.application_cursor)
                    .map_or(Ok(()), |bytes| pty.write(&bytes))
            }
            (CommandLineHostState::Running(pty), InputEvent::Paste(text)) => {
                pty.write(&paste_bytes(text, pty.snapshot().bracketed_paste))
            }
            _ => Ok(()),
        };
        if let Err(error) = write_result {
            self.fail_running(format!("Could not write to CLI process: {error}"));
        }

        CommandLineHostEvent::None
    }

    pub fn view_model(&self) -> ui::CommandLineViewModel {
        let process_state = match &self.state {
            CommandLineHostState::Inactive | CommandLineHostState::Running(_) => {
                ui::CommandLineProcessState::Running
            }
            CommandLineHostState::Exited { code } => {
                ui::CommandLineProcessState::Exited { code: *code }
            }
            CommandLineHostState::Failed { message } => ui::CommandLineProcessState::Failed {
                message: message.clone(),
            },
        };
        ui::CommandLineViewModel {
            terminal: to_ui_snapshot(&self.snapshot),
            process_state,
            message: None,
        }
    }

    pub fn terminate(&mut self) {
        let previous = std::mem::replace(&mut self.state, CommandLineHostState::Inactive);
        if let CommandLineHostState::Running(pty) = previous {
            let _ = pty.force_terminate();
            drop(pty);
        }
        self.snapshot = blank_terminal_snapshot();
    }

    fn fail_running(&mut self, message: String) {
        let previous = std::mem::replace(&mut self.state, CommandLineHostState::Failed { message });
        if let CommandLineHostState::Running(pty) = previous {
            let _ = pty.force_terminate();
        }
    }
}

impl Drop for CommandLineHost {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn resolve_tundra_cli_program() -> io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    let parent = current.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("current executable has no parent: {}", current.display()),
        )
    })?;
    let program = parent.join(format!("tundra-cli{}", std::env::consts::EXE_SUFFIX));
    let metadata = std::fs::metadata(&program).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{} is unavailable: {error}", program.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a file", program.display()),
        ));
    }
    Ok(program)
}

fn is_emergency_termination(key: &KeyInput) -> bool {
    matches!(key.key, InputKey::Char('x' | 'X'))
        && key.modifiers.is_control()
        && key.modifiers.shift
        && !key.modifiers.alt
        && !key.modifiers.super_key
        && !key.modifiers.hyper
        && !key.modifiers.meta
}

fn key_event_bytes(key: &KeyInput, application_cursor: bool) -> Option<Vec<u8>> {
    let control = key.modifiers.is_control();
    let mut bytes = match &key.key {
        InputKey::Char(character) if control => {
            control_character(*character).map(|byte| vec![byte])?
        }
        InputKey::Char(character) => character.to_string().into_bytes(),
        InputKey::Space if control => vec![0],
        InputKey::Space => vec![b' '],
        InputKey::Enter => encode_terminal_input(&TerminalInput::Enter, application_cursor),
        InputKey::Escape => encode_terminal_input(&TerminalInput::Escape, application_cursor),
        InputKey::Backspace => encode_terminal_input(&TerminalInput::Backspace, application_cursor),
        InputKey::Tab => encode_terminal_input(&TerminalInput::Tab, application_cursor),
        InputKey::BackTab => b"\x1b[Z".to_vec(),
        InputKey::Delete => encode_terminal_input(&TerminalInput::Delete, application_cursor),
        InputKey::Insert => b"\x1b[2~".to_vec(),
        InputKey::Left => encode_terminal_input(&TerminalInput::Left, application_cursor),
        InputKey::Right => encode_terminal_input(&TerminalInput::Right, application_cursor),
        InputKey::Up => encode_terminal_input(&TerminalInput::Up, application_cursor),
        InputKey::Down => encode_terminal_input(&TerminalInput::Down, application_cursor),
        InputKey::Home => encode_terminal_input(&TerminalInput::Home, application_cursor),
        InputKey::End => encode_terminal_input(&TerminalInput::End, application_cursor),
        InputKey::PageUp => encode_terminal_input(&TerminalInput::PageUp, application_cursor),
        InputKey::PageDown => encode_terminal_input(&TerminalInput::PageDown, application_cursor),
        InputKey::F(number) => function_key_bytes(*number)?,
        InputKey::Other(_) => return None,
    };
    if key.modifiers.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn control_character(character: char) -> Option<u8> {
    let value = u8::try_from(character).ok()?;
    match value {
        b'@'..=b'_' => Some(value & 0x1f),
        b'a'..=b'z' => Some(value.to_ascii_uppercase() & 0x1f),
        b'?' => Some(0x7f),
        _ => None,
    }
}

fn function_key_bytes(number: u8) -> Option<Vec<u8>> {
    let sequence: &[u8] = match number {
        1 => b"\x1bOP",
        2 => b"\x1bOQ",
        3 => b"\x1bOR",
        4 => b"\x1bOS",
        5 => b"\x1b[15~",
        6 => b"\x1b[17~",
        7 => b"\x1b[18~",
        8 => b"\x1b[19~",
        9 => b"\x1b[20~",
        10 => b"\x1b[21~",
        11 => b"\x1b[23~",
        12 => b"\x1b[24~",
        _ => return None,
    };
    Some(sequence.to_vec())
}

fn paste_bytes(text: &str, bracketed_paste: bool) -> Vec<u8> {
    if !bracketed_paste {
        return text.as_bytes().to_vec();
    }
    let mut bytes = Vec::with_capacity(text.len().saturating_add(12));
    bytes.extend_from_slice(b"\x1b[200~");
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
}

fn blank_terminal_snapshot() -> TerminalSnapshot {
    TerminalSnapshot::from_parser(&vt100::Parser::new(DEFAULT_ROWS, DEFAULT_COLUMNS, 0))
}

fn to_ui_snapshot(snapshot: &TerminalSnapshot) -> ui::CommandLineTerminalSnapshot {
    let mut result = ui::CommandLineTerminalSnapshot::blank(snapshot.columns, snapshot.rows);
    for (row, cells) in snapshot.cells.iter().enumerate() {
        let Ok(row) = u16::try_from(row) else {
            break;
        };
        for (column, cell) in cells.iter().enumerate() {
            let Ok(column) = u16::try_from(column) else {
                break;
            };
            if cell.wide_continuation {
                continue;
            }
            result.set_cell(
                column,
                row,
                ui::CommandLineCell {
                    symbol: cell.text.clone(),
                    style: ui::CommandLineCellStyle {
                        foreground: to_ui_color(&cell.foreground),
                        background: to_ui_color(&cell.background),
                        bold: cell.bold,
                        underline: cell.underline,
                        inverse: cell.inverse,
                    },
                    cursor: snapshot.cursor_visible
                        && snapshot.cursor_row == row
                        && snapshot.cursor_column == column,
                },
            );
        }
    }
    result
}

fn to_ui_color(color: &TerminalColor) -> ui::CommandLineColor {
    match *color {
        TerminalColor::Default => ui::CommandLineColor::Default,
        TerminalColor::Indexed(index) => ui::CommandLineColor::Indexed(index),
        TerminalColor::Rgb(red, green, blue) => ui::CommandLineColor::Rgb(red, green, blue),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct CollectingWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for CollectingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            lock_io(&self.0)?.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn pty_process_runs_inside_platform_containment() {
        use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

        let root = std::env::temp_dir().join(format!(
            "tundra-command-line-pty-test-{}-{}",
            std::process::id(),
            NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let config = WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "command-line-pty-test",
            env!("CARGO_PKG_VERSION"),
        );
        let (runtime, process) =
            WatchdogRuntime::start_isolated(config).expect("isolated watchdog");
        let app = process
            .register_app(AppDescriptor::new(
                AppId::from_static("shell.command-line-test"),
                "Command Line PTY Test",
                env!("CARGO_PKG_VERSION"),
                AppCriticality::Optional,
            ))
            .expect("test app watchdog");
        let reader_tasks = app.task_group("pty-reader");

        #[cfg(windows)]
        let (program, args) = (
            OsString::from("cmd.exe"),
            vec![OsString::from("/D"), OsString::from("/Q")],
        );
        #[cfg(unix)]
        let (program, args) = (OsString::from("/bin/sh"), Vec::new());
        let pty = CommandLinePty::spawn(
            CommandLinePtyConfig {
                program,
                args,
                cwd: None,
                columns: 40,
                rows: 4,
                scrollback_lines: 0,
            },
            &reader_tasks,
        )
        .expect("contained PTY process");
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            pty.try_wait().expect("initial PTY child status").is_none(),
            "interactive PTY child exited before containment was exercised"
        );

        pty.force_terminate()
            .expect("terminate contained process tree");
        let exit_deadline = std::time::Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = pty.try_wait().expect("PTY child status") {
                break status;
            }
            assert!(
                std::time::Instant::now() < exit_deadline,
                "contained PTY child did not terminate"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let _snapshot = pty.snapshot_after_exit();

        assert!(!status.success);

        drop(reader_tasks);
        drop(app);
        drop(process);
        runtime.shutdown().expect("watchdog shutdown");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Windows ConPTY is the production backend for the embedded CLI.  A
    /// child merely staying alive does not prove that its pseudo console is
    /// usable: a broken reader or writer leaves the Shell with an apparently
    /// running, but completely blank, Command Line screen.  Keep this test
    /// interactive and bounded so it verifies the two directions separately.
    #[cfg(windows)]
    #[test]
    fn windows_conpty_receives_initial_and_typed_output() {
        use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

        let root = std::env::temp_dir().join(format!(
            "tundra-command-line-conpty-io-test-{}-{}",
            std::process::id(),
            NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let config = WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "command-line-conpty-io-test",
            env!("CARGO_PKG_VERSION"),
        );
        let (runtime, process) =
            WatchdogRuntime::start_isolated(config).expect("isolated watchdog");
        let app = process
            .register_app(AppDescriptor::new(
                AppId::from_static("shell.command-line-conpty-io-test"),
                "Command Line ConPTY I/O Test",
                env!("CARGO_PKG_VERSION"),
                AppCriticality::Optional,
            ))
            .expect("test app watchdog");
        let reader_tasks = app.task_group("pty-reader");
        let pty = CommandLinePty::spawn(
            CommandLinePtyConfig {
                program: OsString::from("cmd.exe"),
                args: vec![
                    OsString::from("/D"),
                    OsString::from("/Q"),
                    OsString::from("/K"),
                    OsString::from("echo TUNDRA_PTY_INITIAL_OUTPUT_OK"),
                ],
                cwd: None,
                columns: 80,
                rows: 12,
                scrollback_lines: 100,
            },
            &reader_tasks,
        )
        .expect("interactive ConPTY child");
        let snapshot = pty.snapshot();
        let mut host = CommandLineHost {
            state: CommandLineHostState::Running(pty),
            snapshot,
            reader_tasks,
        };

        // The smallest valid outer Shell (108x22) leaves a 106x14 inner
        // terminal.  This guards against accidentally applying the outer
        // minimum to the already-inset PTY dimensions.
        host.resize_terminal(106, 14);
        assert_eq!((host.snapshot.columns, host.snapshot.rows), (106, 14));
        let pty = match &host.state {
            CommandLineHostState::Running(pty) => pty,
            state => panic!("expected running ConPTY host, got {state:?}"),
        };

        assert_snapshot_contains(
            &pty,
            "TUNDRA_PTY_INITIAL_OUTPUT_OK",
            Duration::from_secs(5),
            "initial child output",
        );
        pty.write(b"echo TUNDRA_PTY_TYPED_INPUT_OK\r")
            .expect("write command to ConPTY");
        assert_snapshot_contains(
            &pty,
            "TUNDRA_PTY_TYPED_INPUT_OK",
            Duration::from_secs(5),
            "output from typed command",
        );

        host.terminate();
        drop(host);
        drop(app);
        drop(process);
        runtime.shutdown().expect("watchdog shutdown");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    fn assert_snapshot_contains(
        pty: &CommandLinePty,
        expected: &str,
        timeout: Duration,
        description: &str,
    ) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let snapshot = pty.snapshot();
            let screen = snapshot
                .cells
                .iter()
                .flat_map(|row| row.iter())
                .map(|cell| cell.text.as_str())
                .collect::<String>();
            if screen.contains(expected) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "ConPTY did not render {description}; last screen: {screen:?}",
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn strips_osc_52_even_when_sequence_is_split() {
        let mut filter = OscFilter::default();
        assert_eq!(filter.filter(b"safe\x1b]52;c;"), b"safe");
        assert_eq!(filter.filter(b"c2VjcmV0\x07after"), b"after");
    }

    #[test]
    fn strips_c1_osc_and_c1_string_terminator() {
        let mut filter = OscFilter::default();
        assert_eq!(
            filter.filter(b"safe\x9d52;c;payload\x9cafter"),
            b"safeafter"
        );
    }

    #[test]
    fn preserves_csi_but_discards_osc_st_terminated() {
        let mut filter = OscFilter::default();
        assert_eq!(
            filter.filter(b"\x1b[31mred\x1b]0;bad\x1b\\ok"),
            b"\x1b[31mredok"
        );
    }

    #[test]
    fn cursor_position_request_is_replied_to_across_reads_without_matching_other_csi() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(
            CollectingWriter(Arc::clone(&captured)),
        )));
        let mut responder = TerminalResponder::default();
        responder
            .respond(b"\x1b[6n", (0, 0), &writer)
            .expect("complete request is answered");
        responder
            .respond(b"\x1b[", (2, 4), &writer)
            .expect("partial request is accepted");
        responder
            .respond(b"6n", (2, 4), &writer)
            .expect("cursor-position response is written");
        responder
            .respond(b"\x1b[31m", (9, 9), &writer)
            .expect("ordinary CSI is ignored by responder");
        let output = lock_io(&captured).expect("response writer");
        assert_eq!(output.as_slice(), b"\x1b[1;1R\x1b[3;5R");
    }

    #[test]
    fn snapshot_keeps_terminal_attributes() {
        let mut parser = vt100::Parser::new(2, 8, 0);
        parser.process(b"\x1b[31;1;4;7mX");
        let snapshot = TerminalSnapshot::from_parser(&parser);
        let cell = &snapshot.cells[0][0];
        assert_eq!(cell.text, "X");
        assert_eq!(cell.foreground, TerminalColor::Indexed(1));
        assert!(cell.bold);
        assert!(cell.underline);
        assert!(cell.inverse);
    }

    #[test]
    fn cursor_keys_follow_application_cursor_mode() {
        assert_eq!(encode_terminal_input(&TerminalInput::Up, false), b"\x1b[A");
        assert_eq!(encode_terminal_input(&TerminalInput::Up, true), b"\x1bOA");
    }

    #[test]
    fn cli_config_uses_embedded_repl_contract() {
        let config = CommandLinePtyConfig::tundra_cli("tundra-cli");
        assert_eq!(
            config.args,
            [OsString::from("repl"), OsString::from("--embedded")]
        );
        assert_eq!(config.columns, DEFAULT_COLUMNS);
        assert_eq!(config.rows, DEFAULT_ROWS);
    }

    #[test]
    fn control_and_alt_keys_encode_for_the_child_terminal() {
        let ctrl_d = KeyInput::with_modifiers(InputKey::Char('d'), InputModifiers::CTRL);
        assert_eq!(key_event_bytes(&ctrl_d, false), Some(vec![0x04]));

        let alt_x = KeyInput::with_modifiers(InputKey::Char('x'), InputModifiers::ALT);
        assert_eq!(key_event_bytes(&alt_x, false), Some(b"\x1bx".to_vec()));
    }

    #[test]
    fn paste_follows_the_child_terminal_mode() {
        assert_eq!(paste_bytes("one\ntwo", false), b"one\ntwo");
        assert_eq!(paste_bytes("one\ntwo", true), b"\x1b[200~one\ntwo\x1b[201~");
    }

    #[test]
    fn emergency_shortcut_is_exact() {
        let emergency = KeyInput::with_modifiers(InputKey::Char('x'), InputModifiers::CTRL_SHIFT);
        assert!(is_emergency_termination(&emergency));
        assert!(!is_emergency_termination(&KeyInput::with_modifiers(
            InputKey::Char('x'),
            InputModifiers::CTRL,
        )));
    }
}
