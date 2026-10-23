//! Runs the real Vim editor without interpreting Vim editing commands.

mod protocol;

pub use protocol::*;
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use terminal_runtime::pty::{
    CommandLineExitStatus, CommandLinePty, CommandLinePtyConfig, TerminalSnapshot, key_event_bytes,
    paste_bytes,
};
use watchdog::ManagedTaskGroup;

const BRIDGE: &str = include_str!("bridge.vim");

#[derive(Debug, Clone)]
pub struct VimConfig {
    pub program: PathBuf,
    pub runtime_path: Option<PathBuf>,
    pub columns: u16,
    pub rows: u16,
    pub user_config: bool,
    pub initial_buffers: Vec<BufferInput>,
}

impl VimConfig {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            runtime_path: None,
            columns: 108,
            rows: 20,
            user_config: true,
            initial_buffers: Vec::new(),
        }
    }
}

pub struct VimPoll {
    pub terminal: TerminalSnapshot,
    pub state: Arc<VimState>,
    pub save_requests: Vec<SaveRequest>,
    pub exit: Option<CommandLineExitStatus>,
}

/// Process resources stay outside cloneable application/page state.
pub struct VimHost {
    pty: Option<CommandLinePty>,
    _directory: tempfile::TempDir,
    command: u64,
    revision: u64,
    state: Arc<VimState>,
    texts: HashMap<u32, (i64, Arc<str>)>,
    pending_saves: HashSet<u64>,
    listener: TcpListener,
    stream: Option<TcpStream>,
    authenticated: bool,
    token: String,
    incoming: Vec<u8>,
    outgoing: VecDeque<Vec<u8>>,
    written: usize,
    exit: Option<CommandLineExitStatus>,
    final_terminal: Option<TerminalSnapshot>,
    size: Cell<(u16, u16)>,
}

impl std::fmt::Debug for VimHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VimHost")
            .field("command", &self.command)
            .field("revision", &self.revision)
            .field("exit", &self.exit)
            .finish_non_exhaustive()
    }
}

impl VimHost {
    pub fn spawn(config: VimConfig, tasks: &ManagedTaskGroup) -> io::Result<Self> {
        let directory = tempfile::Builder::new().prefix("tundra-vim-").tempdir()?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let endpoint = listener.local_addr()?.to_string();
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let token = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let script = directory.path().join("bridge.vim");
        fs::write(&script, BRIDGE.replace("\r\n", "\n"))?;
        let mut args: Vec<OsString> = ["-N", "-n", "-i", "NONE"]
            .into_iter()
            .map(OsString::from)
            .collect();
        if !config.user_config {
            args.extend(["-u", "NONE", "--noplugin"].map(OsString::from));
        }
        args.extend([OsString::from("-S"), script.into_os_string()]);
        let mut env = vec![
            (OsString::from("TUNDRA_VIM_ENDPOINT"), endpoint.into()),
            (OsString::from("TUNDRA_VIM_TOKEN"), token.clone().into()),
        ];
        if let Some(runtime) = config.runtime_path {
            env.push((OsString::from("VIMRUNTIME"), runtime.into_os_string()));
        }
        let pty = CommandLinePty::spawn(
            CommandLinePtyConfig {
                program: config.program.into_os_string(),
                args,
                env,
                cwd: None,
                columns: config.columns,
                rows: config.rows,
                scrollback_lines: 0,
            },
            tasks,
        )?;
        let mut host = Self {
            pty: Some(pty),
            _directory: directory,
            command: 0,
            revision: 0,
            state: Arc::new(VimState::default()),
            texts: HashMap::new(),
            pending_saves: HashSet::new(),
            listener,
            stream: None,
            authenticated: false,
            token,
            incoming: Vec::new(),
            outgoing: VecDeque::new(),
            written: 0,
            exit: None,
            final_terminal: None,
            size: Cell::new((config.columns.max(1), config.rows.max(1))),
        };
        for buffer in config.initial_buffers {
            host.open_buffer(buffer)?;
        }
        Ok(host)
    }

    pub fn open_buffer(&mut self, buffer: BufferInput) -> io::Result<u64> {
        let normalized = match buffer.line_ending {
            LineEnding::Unix => buffer.text.clone(),
            LineEnding::Dos => buffer.text.replace("\r\n", "\n"),
            LineEnding::Mac => buffer.text.replace('\r', "\n"),
        };
        let mut lines = normalized
            .split('\n')
            .map(|line| line.replace('\0', "\n"))
            .collect::<Vec<_>>();
        if buffer.end_of_line && lines.len() > 1 && normalized.ends_with('\n') {
            lines.pop();
        }
        let mut message = serde_json::json!({"buffer": buffer});
        message["buffer"]["lines"] = serde_json::to_value(lines).map_err(io::Error::other)?;
        message["buffer"]["empty"] = normalized.is_empty().into();
        self.command("open", message)
    }

    pub fn activate_buffer(&mut self, host_id: u64) -> io::Result<u64> {
        self.command("activate", serde_json::json!({"host_id": host_id}))
    }

    pub fn activate_buffer_number(&mut self, number: u32) -> io::Result<u64> {
        self.command("activate", serde_json::json!({"number": number}))
    }

    pub fn bind_buffer(&mut self, number: u32, host_id: u64) -> io::Result<u64> {
        self.command(
            "bind",
            serde_json::json!({"number": number, "host_id": host_id}),
        )
    }

    /// Wipes a document that the host has already closed, including its undo history.
    pub fn remove_buffer(&mut self, number: u32) -> io::Result<u64> {
        self.command("remove", serde_json::json!({"number": number}))
    }

    pub fn set_buffer_policy(
        &mut self,
        number: u32,
        read_only: bool,
        protected: bool,
    ) -> io::Result<u64> {
        self.command(
            "policy",
            serde_json::json!({
                "number": number, "read_only": read_only, "protected": protected,
            }),
        )
    }

    pub fn request_export(&mut self) -> io::Result<u64> {
        self.command("export", serde_json::json!({}))
    }

    pub fn request_close(&mut self, force: bool) -> io::Result<u64> {
        self.command("close", serde_json::json!({"force": force}))
    }

    pub fn send_key(&self, key: &ui::KeyEvent) -> io::Result<()> {
        let pty = self.running()?;
        if key.phase.is_press_like()
            && let Some(bytes) = key_event_bytes(key, pty.input_modes().0)
        {
            pty.write(&bytes)?;
        }
        Ok(())
    }

    pub fn paste(&self, text: &str) -> io::Result<()> {
        let pty = self.running()?;
        pty.write(&paste_bytes(text, pty.input_modes().1))
    }

    pub fn send_bytes(&self, bytes: &[u8]) -> io::Result<()> {
        self.running()?.write(bytes)
    }

    pub fn send_mouse(
        &self,
        mouse: &ui::MouseEvent,
        content_area: ratatui::layout::Rect,
    ) -> io::Result<bool> {
        self.running()?.send_mouse(mouse, content_area)
    }

    pub fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        let size = (columns.max(1), rows.max(1));
        if self.size.get() != size {
            self.running()?.resize(size.0, size.1)?;
            self.size.set(size);
        }
        Ok(())
    }

    pub fn state(&self) -> &VimState {
        &self.state
    }

    pub fn is_running(&self) -> bool {
        self.pty.is_some() && self.exit.is_none()
    }

    pub fn terminal_snapshot(&self) -> TerminalSnapshot {
        match &self.pty {
            Some(pty) => pty.snapshot(),
            None => self
                .final_terminal
                .clone()
                .expect("a stopped Vim retains its last terminal snapshot"),
        }
    }

    pub fn poll(&mut self) -> io::Result<VimPoll> {
        let saves = self.read_messages()?;
        if let Some(pty) = &self.pty
            && let Some(exit) = pty.try_wait()?
        {
            self.exit = Some(exit);
            self.final_terminal = self.pty.take().map(CommandLinePty::snapshot_after_exit);
            let _ = self.read_messages()?;
        }
        if self.authenticated && self.stream.is_none() && self.exit.is_none() && !self.state.exiting
        {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Vim bridge disconnected",
            ));
        }
        Ok(VimPoll {
            terminal: self.terminal_snapshot(),
            state: self.state.clone(),
            save_requests: saves,
            exit: self.exit,
        })
    }

    pub fn respond_save(
        &mut self,
        request: u64,
        result: Result<SaveResult, String>,
    ) -> io::Result<()> {
        if !self.pending_saves.contains(&request) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "unknown Vim save request",
            ));
        }
        let response = match result {
            Ok(saved) => {
                serde_json::json!({"op": "save_response", "request": request, "ok": true, "path": saved.path})
            }
            Err(error) => {
                serde_json::json!({"op": "save_response", "request": request, "ok": false, "error": error})
            }
        };
        self.queue(response)?;
        self.pending_saves.remove(&request);
        Ok(())
    }

    pub fn force_terminate(&mut self) -> io::Result<()> {
        if let Some(pty) = self.pty.take() {
            pty.force_terminate()?;
            self.exit = Some(pty.wait()?);
            self.final_terminal = Some(pty.snapshot_after_exit());
        }
        let _ = self.read_messages()?;
        Ok(())
    }

    fn running(&self) -> io::Result<&CommandLinePty> {
        self.pty
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Vim has already exited"))
    }

    fn command(&mut self, operation: &str, mut fields: serde_json::Value) -> io::Result<u64> {
        self.running()?;
        self.command += 1;
        fields["id"] = self.command.into();
        fields["op"] = operation.into();
        self.queue(fields)?;
        Ok(self.command)
    }

    fn queue(&mut self, message: serde_json::Value) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(&message).map_err(io::Error::other)?;
        bytes.push(b'\n');
        self.outgoing.push_back(bytes);
        self.flush_messages()
    }

    fn flush_messages(&mut self) -> io::Result<()> {
        if !self.authenticated {
            return Ok(());
        }
        let Some(stream) = &mut self.stream else {
            return Ok(());
        };
        while let Some(message) = self.outgoing.front() {
            match stream.write(&message[self.written..]) {
                Ok(0) => return Err(io::ErrorKind::BrokenPipe.into()),
                Ok(count) => {
                    self.written += count;
                    if self.written == message.len() {
                        self.outgoing.pop_front();
                        self.written = 0;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn read_messages(&mut self) -> io::Result<Vec<SaveRequest>> {
        if self.stream.is_none() {
            if self.authenticated {
                return Ok(Vec::new());
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true)?;
                    stream.set_nodelay(true)?;
                    self.stream = Some(stream);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(Vec::new()),
                Err(error) => return Err(error),
            }
        }
        let mut bytes = [0u8; 65536];
        let mut disconnected = false;
        if let Some(stream) = &mut self.stream {
            loop {
                match stream.read(&mut bytes) {
                    Ok(0) => {
                        disconnected = true;
                        break;
                    }
                    Ok(count) => self.incoming.extend_from_slice(&bytes[..count]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                        ) =>
                    {
                        disconnected = true;
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        let mut saves = Vec::new();
        while let Some(newline) = self.incoming.iter().position(|byte| *byte == b'\n') {
            let line = self.incoming.drain(..=newline).collect::<Vec<_>>();
            let message: serde_json::Value =
                serde_json::from_slice(&line).map_err(io::Error::other)?;
            if !self.authenticated {
                if message.get("kind").and_then(|v| v.as_str()) != Some("hello")
                    || message.get("token").and_then(|v| v.as_str()) != Some(self.token.as_str())
                {
                    self.stream = None;
                    self.incoming.clear();
                    break;
                }
                self.authenticated = true;
                continue;
            }
            match message.get("kind").and_then(|value| value.as_str()) {
                Some("state") => {
                    let state: StateRecord =
                        serde_json::from_value(message).map_err(io::Error::other)?;
                    self.install_state(state);
                }
                Some("save") => {
                    let mut save: SaveRequest =
                        serde_json::from_value(message).map_err(io::Error::other)?;
                    if let Some(lines) = save.text_lines.take() {
                        save.text = decode_lines(lines, save.line_ending, save.end_of_line);
                    }
                    if self.pending_saves.insert(save.id) {
                        saves.push(save);
                    }
                }
                _ => {}
            }
        }
        if disconnected {
            self.stream = None;
        }
        self.flush_messages()?;
        Ok(saves)
    }

    fn install_state(&mut self, raw: StateRecord) {
        if raw.revision <= self.revision {
            return;
        }
        let mut buffers = Vec::with_capacity(raw.buffers.len());
        for mut buffer in raw.buffers {
            if let Some(lines) = buffer.text_lines.take() {
                buffer.text = decode_lines(lines, buffer.line_ending, buffer.end_of_line).into();
            }
            if buffer.text_changed
                || self
                    .texts
                    .get(&buffer.number)
                    .is_none_or(|(tick, _)| *tick != buffer.changed_tick)
            {
                self.texts
                    .insert(buffer.number, (buffer.changed_tick, buffer.text.clone()));
            } else {
                buffer.text = self.texts[&buffer.number].1.clone();
            }
            buffers.push(buffer);
        }
        self.texts
            .retain(|number, _| buffers.iter().any(|buffer| buffer.number == *number));
        self.revision = raw.revision;
        self.state = Arc::new(VimState {
            ready: raw.ready,
            revision: raw.revision,
            last_command: raw.last_command,
            active_buffer: raw.active_buffer,
            mode: raw.mode,
            buffers,
            removed_buffers: raw.removed_buffers,
            exiting: raw.exiting,
            discard_modified_on_exit: raw.discard_modified_on_exit,
            error: raw.error,
        });
    }
}

fn decode_lines(lines: Vec<String>, ending: LineEnding, final_newline: bool) -> String {
    let separator = match ending {
        LineEnding::Unix => "\n",
        LineEnding::Dos => "\r\n",
        LineEnding::Mac => "\r",
    };
    let empty = lines.is_empty();
    let mut text = lines
        .into_iter()
        .map(|line| line.replace('\n', "\0"))
        .collect::<Vec<_>>()
        .join(separator);
    if final_newline && !empty {
        text.push_str(separator);
    }
    text
}
