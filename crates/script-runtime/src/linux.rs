use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    collections::{BTreeSet, VecDeque},
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use terminal_runtime::pty::PtyConfig;
use watchdog::ManagedTaskGroup;
include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

struct Debugger {
    _directory: tempfile::TempDir,
    input: File,
    outbound: VecDeque<u8>,
    output: File,
    pending: Vec<u8>,
    location: Option<ScriptLocation>,
    variables: Vec<ScriptVariable>,
    stack: Vec<ScriptStackFrame>,
    breakpoints: Vec<ScriptBreakpoint>,
    breakpoints_dirty: bool,
    observed_lines: BTreeSet<(PathBuf, usize)>,
    paused: bool,
    stop_id: u64,
    evaluation: Option<(String, String, bool)>,
    next_request: u64,
    started: Instant,
}

pub struct ScriptSession {
    pty: Option<PtyProcess>,
    debugger: Option<Debugger>,
    session_id: u64,
    exited: bool,
    events: VecDeque<RuntimeEvent>,
    cwd: PathBuf,
}

impl ScriptSession {
    pub fn spawn(
        spec: &ScriptLaunchSpec,
        debug: bool,
        session_id: u64,
        tasks: &ManagedTaskGroup,
        columns: u16,
        rows: u16,
    ) -> io::Result<Self> {
        Self::spawn_recorded(spec, debug, session_id, tasks, columns, rows, None)
    }

    pub fn spawn_recorded(
        spec: &ScriptLaunchSpec,
        debug: bool,
        session_id: u64,
        tasks: &ManagedTaskGroup,
        columns: u16,
        rows: u16,
        history: Option<terminal_runtime::pty::TaskRecorder>,
    ) -> io::Result<Self> {
        if debug && !spec.is_bash() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                i18n::tr!("script-bash-required"),
            ));
        }
        let mut env = Vec::new();
        let debugger = if debug {
            let directory = tempfile::Builder::new()
                .prefix("tundra-bashdb-")
                .tempdir()?;
            for (name, contents) in BASHDB_FILES {
                let path = directory.path().join(name);
                fs::create_dir_all(path.parent().unwrap())?;
                fs::write(path, contents)?;
            }
            let bootstrap = directory.path().join("bootstrap.sh");
            fs::write(&bootstrap, include_str!("bridge.sh").replace("\r\n", "\n"))?;
            let input_path = directory.path().join("control.in");
            let c_path = std::ffi::CString::new(input_path.as_os_str().as_bytes())?;
            if unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let input = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(input_path)?;
            let output_path = directory.path().join("control.out");
            let output = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(output_path)?;
            env.push((OsString::from("BASH_ENV"), bootstrap.into_os_string()));
            env.push((
                OsString::from("TUNDRA_SCRIPT_DEBUG_ROOT"),
                directory.path().as_os_str().to_owned(),
            ));
            Some(Debugger {
                _directory: directory,
                input,
                outbound: VecDeque::new(),
                output,
                pending: Vec::new(),
                location: None,
                variables: Vec::new(),
                stack: Vec::new(),
                breakpoints: Vec::new(),
                breakpoints_dirty: false,
                observed_lines: BTreeSet::new(),
                paused: false,
                stop_id: 0,
                evaluation: None,
                next_request: 0,
                started: Instant::now(),
            })
        } else {
            None
        };
        let mut args = spec.interpreter_args.clone();
        args.push(spec.path.as_os_str().to_owned());
        args.extend(spec.args.iter().cloned());
        let pty = PtyProcess::spawn_recorded(
            PtyConfig {
                program: spec.interpreter.as_os_str().to_owned(),
                args,
                env,
                cwd: Some(spec.cwd.clone()),
                columns,
                rows,
                scrollback_lines: 2_000,
            },
            tasks,
            history,
        )?;
        Ok(Self {
            pty: Some(pty),
            debugger,
            session_id,
            exited: false,
            events: VecDeque::from([RuntimeEvent::Started, RuntimeEvent::Running]),
            cwd: spec.cwd.clone(),
        })
    }

    pub fn terminal(&self) -> &PtyProcess {
        self.pty.as_ref().unwrap()
    }
    pub fn session_id(&self) -> u64 {
        self.session_id
    }
    pub fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        self.terminal().resize(columns, rows)
    }

    pub fn stop(&mut self) -> io::Result<()> {
        let Some(pty) = self.pty.as_ref() else {
            return Ok(());
        };
        pty.force_terminate()?;
        if !self.exited {
            let code = pty.try_wait()?.map_or(1, |status| status.code);
            self.exited = true;
            self.events.push_back(RuntimeEvent::Exited { code });
        }
        Ok(())
    }

    pub fn into_output(mut self) -> io::Result<terminal_runtime::pty::TerminalOutput> {
        self.stop()?;
        let output = self.pty.take().unwrap().into_output()?;
        self.debugger.take();
        Ok(output)
    }

    pub fn into_final_snapshot(self) -> io::Result<terminal_runtime::pty::TerminalSnapshot> {
        Ok(self.into_output()?.snapshot())
    }

    pub fn command(&mut self, command: RuntimeCommand) -> io::Result<()> {
        if matches!(command, RuntimeCommand::Stop) {
            return self.stop();
        }
        let debugger = self.debugger.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                i18n::tr!("script-runtime-not-debugging"),
            )
        })?;
        match command {
            RuntimeCommand::SetBreakpoints(mut breakpoints) => {
                for breakpoint in &mut breakpoints {
                    breakpoint.rejected = false;
                    breakpoint.verified = debugger
                        .observed_lines
                        .contains(&(breakpoint.path.clone(), breakpoint.line));
                }
                debugger.breakpoints = breakpoints;
                debugger.breakpoints_dirty = true;
                if debugger.paused {
                    debugger.send_breakpoints()?;
                    debugger.send("tundra inspect\n")?;
                }
            }
            RuntimeCommand::Evaluate { input, execute } => {
                if input.trim().is_empty() {
                    return Err(io::ErrorKind::InvalidInput.into());
                }
                if !debugger.paused || debugger.evaluation.is_some() {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                debugger.next_request += 1;
                debugger.send(&format!(
                    "tundra {} {} {}\n",
                    if execute { "eval" } else { "examine" },
                    STANDARD.encode(input.as_bytes()),
                    debugger.next_request
                ))?;
                debugger.evaluation = Some((input, String::new(), false));
            }
            RuntimeCommand::Continue
            | RuntimeCommand::StepOver
            | RuntimeCommand::StepIn
            | RuntimeCommand::StepOut => {
                if !debugger.paused || debugger.evaluation.is_some() {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                debugger.send(match command {
                    RuntimeCommand::Continue => "continue\n",
                    RuntimeCommand::StepOver => "next\n",
                    RuntimeCommand::StepIn => "step\n",
                    _ => "finish\n",
                })?;
                debugger.paused = false;
                self.events.push_back(RuntimeEvent::Running);
            }
            RuntimeCommand::Start { .. } | RuntimeCommand::Stop => {
                return Err(io::ErrorKind::InvalidInput.into());
            }
        }
        Ok(())
    }

    pub fn poll(&mut self) -> io::Result<Vec<RuntimeEvent>> {
        if let Some(debugger) = &mut self.debugger {
            debugger.flush()?;
            let mut chunk = vec![0; 64 * 1024];
            for _ in 0..16 {
                let count = debugger.output.read(&mut chunk)?;
                if count == 0 {
                    break;
                }
                debugger.pending.extend_from_slice(&chunk[..count]);
            }
            while let Some(newline) = debugger.pending.iter().position(|byte| *byte == b'\n') {
                let line = String::from_utf8_lossy(&debugger.pending[..newline])
                    .trim_end_matches('\r')
                    .to_owned();
                debugger.pending.drain(..=newline);
                if let Some(frame) = line.strip_prefix("__TUNDRA__\t") {
                    debugger.frame(frame, &self.cwd, &mut self.events)?;
                } else if let Some((_, output, true)) = &mut debugger.evaluation {
                    output.push_str(&line);
                    output.push('\n');
                }
            }
            debugger.flush()?;
            if debugger.pending.len() > 8 * 1024 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    i18n::tr!("script-runtime-response-limit"),
                ));
            }
            if debugger.stop_id == 0 && debugger.started.elapsed() > Duration::from_secs(15) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    i18n::tr!("script-runtime-start-timeout"),
                ));
            }
        }
        if !self.exited
            && let Some(status) = self.terminal().try_wait()?
        {
            self.exited = true;
            self.events
                .push_back(RuntimeEvent::Exited { code: status.code });
        }
        Ok(self.events.drain(..).collect())
    }
}

impl Drop for ScriptSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl Debugger {
    fn send(&mut self, command: &str) -> io::Result<()> {
        self.outbound.extend(command.bytes());
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut budget = 64 * 1024;
        while budget > 0 && !self.outbound.is_empty() {
            let (bytes, _) = self.outbound.as_slices();
            match self.input.write(&bytes[..bytes.len().min(budget)]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => {
                    self.outbound.drain(..count);
                    budget -= count;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn send_breakpoints(&mut self) -> io::Result<()> {
        if !self.breakpoints_dirty {
            return Ok(());
        }
        self.send("tundra clear\n")?;
        let commands = self
            .breakpoints
            .iter()
            .filter(|breakpoint| breakpoint.enabled)
            .map(|breakpoint| {
                format!(
                    "tundra bp {} {}\n",
                    STANDARD.encode(breakpoint.path.as_os_str().as_bytes()),
                    breakpoint.line
                )
            })
            .collect::<Vec<_>>();
        for command in commands {
            self.send(&command)?;
        }
        self.breakpoints_dirty = false;
        Ok(())
    }

    fn frame(
        &mut self,
        frame: &str,
        cwd: &Path,
        events: &mut VecDeque<RuntimeEvent>,
    ) -> io::Result<()> {
        let fields = frame.split_whitespace().collect::<Vec<_>>();
        let Some(kind) = fields.first().copied() else {
            return Ok(());
        };
        match kind {
            "STOP" if fields.len() == 3 => {
                self.stop_id += 1;
                self.location = Some(ScriptLocation {
                    path: decode_path(fields[1], cwd)?,
                    line: fields[2].parse().map_err(invalid)?,
                });
                if let Some(location) = &self.location {
                    self.observed_lines
                        .insert((location.path.clone(), location.line));
                    for breakpoint in &mut self.breakpoints {
                        if breakpoint.path == location.path && breakpoint.line == location.line {
                            breakpoint.verified = true;
                            breakpoint.rejected = false;
                        }
                    }
                }
                self.paused = true;
                self.variables.clear();
                self.stack.clear();
                self.send_breakpoints()?;
                self.send("tundra inspect\n")?;
            }
            "VARS" if fields.len() == 2 => {
                self.variables = crate::variables::parse(&decode(fields[1])?);
                self.stack.clear();
            }
            "FRAME" if fields.len() == 4 => {
                self.stack.push(ScriptStackFrame {
                    index: self.stack.len(),
                    name: decode(fields[1])?,
                    path: decode_path(fields[2], cwd)?,
                    line: fields[3].parse().map_err(invalid)?,
                });
            }
            "BP" if fields.len() == 4 => {
                let path = decode_path(fields[1], cwd)?;
                let line = fields[2].parse::<usize>().map_err(invalid)?;
                if let Some(breakpoint) = self
                    .breakpoints
                    .iter_mut()
                    .find(|bp| bp.path == path && bp.line == line)
                {
                    breakpoint.rejected = fields[3] == "0";
                    breakpoint.verified =
                        fields[3] == "1" && self.observed_lines.contains(&(path, line));
                }
            }
            "INSPECTED" => {
                if self.paused
                    && let Some(location) = self.location.clone()
                {
                    events.push_back(RuntimeEvent::Stopped {
                        stop_id: self.stop_id,
                        location,
                        variables: self.variables.clone(),
                        stack: self.stack.clone(),
                        breakpoints: self.breakpoints.clone(),
                    });
                }
            }
            "EVAL_BEGIN" => {
                if let Some((_, output, active)) = &mut self.evaluation {
                    output.clear();
                    *active = true;
                }
            }
            "EVAL_END" => {
                if let Some((input, output, _)) = self.evaluation.take() {
                    events.push_back(RuntimeEvent::Evaluation {
                        stop_id: self.stop_id,
                        input,
                        output: output.trim_end_matches('\n').to_owned(),
                    });
                    self.send("tundra inspect\n")?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
fn decode(value: &str) -> io::Result<String> {
    String::from_utf8(STANDARD.decode(value).map_err(invalid)?).map_err(invalid)
}
fn decode_path(value: &str, cwd: &Path) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let path = PathBuf::from(OsString::from_vec(STANDARD.decode(value).map_err(invalid)?));
    Ok(if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    })
}
