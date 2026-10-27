//! Linux script execution and a private bashdb connection, sharing the existing PTY.
use app::script::{
    ScriptBreakpoint, ScriptLaunchSpec, ScriptLocation, ScriptStackFrame, ScriptVariable,
};
use std::io;
use terminal_runtime::pty::PtyProcess;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCommand {
    Start {
        spec: ScriptLaunchSpec,
        debug: bool,
        session_id: u64,
    },
    Stop,
    Continue,
    StepOver,
    StepIn,
    StepOut,
    SetBreakpoints(Vec<ScriptBreakpoint>),
    Evaluate {
        input: String,
        execute: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    Started,
    Running,
    Stopped {
        stop_id: u64,
        location: ScriptLocation,
        variables: Vec<ScriptVariable>,
        stack: Vec<ScriptStackFrame>,
        breakpoints: Vec<ScriptBreakpoint>,
    },
    Evaluation {
        stop_id: u64,
        input: String,
        output: String,
    },
    Exited {
        code: u32,
    },
    Failed(String),
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::ScriptSession;

#[cfg(not(target_os = "linux"))]
pub struct ScriptSession;
#[cfg(not(target_os = "linux"))]
impl ScriptSession {
    pub fn spawn_recorded(
        spec: &ScriptLaunchSpec,
        debug: bool,
        session_id: u64,
        tasks: &watchdog::ManagedTaskGroup,
        columns: u16,
        rows: u16,
        _: Option<terminal_runtime::pty::TaskRecorder>,
    ) -> io::Result<Self> {
        Self::spawn(spec, debug, session_id, tasks, columns, rows)
    }
    pub fn spawn(
        _: &ScriptLaunchSpec,
        _: bool,
        _: u64,
        _: &watchdog::ManagedTaskGroup,
        _: u16,
        _: u16,
    ) -> io::Result<Self> {
        Err(io::ErrorKind::Unsupported.into())
    }
    pub fn terminal(&self) -> &PtyProcess {
        unreachable!("Linux-only script session")
    }
    pub fn poll(&mut self) -> io::Result<Vec<RuntimeEvent>> {
        Ok(Vec::new())
    }
    pub fn command(&mut self, _: RuntimeCommand) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    pub fn stop(&mut self) -> io::Result<()> {
        Ok(())
    }
    pub fn resize(&self, _: u16, _: u16) -> io::Result<()> {
        Ok(())
    }
    pub fn session_id(&self) -> u64 {
        0
    }
    pub fn into_final_snapshot(self) -> io::Result<terminal_runtime::pty::TerminalSnapshot> {
        Err(io::ErrorKind::Unsupported.into())
    }
    pub fn into_output(self) -> io::Result<terminal_runtime::pty::TerminalOutput> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(target_os = "linux")]
mod variables;
