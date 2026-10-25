//! Shell-script settings and document-owned debugger state.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use i18n::{LocalizedText, msg};

pub const NEW_SCRIPT: &str = "#!/usr/bin/env bash\n";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptSettings {
    pub interpreter: String,
    pub arguments: String,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLaunchSpec {
    pub path: PathBuf,
    pub interpreter: PathBuf,
    pub interpreter_args: Vec<OsString>,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
}

impl ScriptLaunchSpec {
    pub fn is_bash(&self) -> bool {
        self.interpreter
            .file_name()
            .is_some_and(|name| name == "bash")
    }
}

pub fn is_shell_script(path: Option<&Path>, source: &str) -> bool {
    path.and_then(Path::extension)
        .is_some_and(|suffix| matches!(suffix.to_str(), Some("sh" | "bash" | "zsh" | "ksh")))
        || source.lines().next().is_some_and(|line| {
            interpreter_words(line).is_some_and(|words| {
                words.first().is_some_and(|word| {
                    Path::new(word).file_name().is_some_and(|name| {
                        ["sh", "bash", "dash", "ash", "zsh", "ksh", "fish"]
                            .iter()
                            .any(|shell| name == *shell)
                    })
                })
            })
        })
}

fn interpreter_words(line: &str) -> Option<Vec<String>> {
    let body = line.strip_prefix("#!")?.trim();
    let mut words = shell_words::split(body).ok()?;
    if Path::new(words.first()?).file_name()? == "env" {
        words.remove(0);
        if words.first().is_some_and(|word| word == "-S") {
            words.remove(0);
        }
        if words.first()?.starts_with('-') || words.first()?.contains('=') {
            return None;
        }
    }
    (!words.is_empty()).then_some(words)
}

pub fn build_launch_spec(
    path: &Path,
    source: &str,
    settings: &ScriptSettings,
) -> Result<ScriptLaunchSpec, LocalizedText> {
    let mut interpreter = if settings.interpreter.trim().is_empty() {
        if source.starts_with("#!") {
            interpreter_words(source.lines().next().unwrap_or_default())
                .ok_or_else(|| LocalizedText::from(msg!("script-invalid-interpreter")))?
        } else {
            vec![if path.extension().is_some_and(|suffix| suffix == "bash") {
                "bash".to_owned()
            } else {
                "/bin/sh".to_owned()
            }]
        }
    } else {
        shell_words::split(&settings.interpreter)
            .ok()
            .filter(|words| !words.is_empty())
            .ok_or_else(|| LocalizedText::from(msg!("script-invalid-interpreter")))?
    };
    let program = PathBuf::from(interpreter.remove(0));
    let args = shell_words::split(&settings.arguments)
        .map_err(|_| LocalizedText::from(msg!("script-invalid-arguments")))?;
    let cwd = if settings.cwd.trim().is_empty() {
        path.parent().unwrap_or(Path::new("/")).to_owned()
    } else {
        PathBuf::from(&settings.cwd)
    };
    if !cwd.is_absolute() {
        return Err(msg!("script-cwd-not-absolute").into());
    }
    Ok(ScriptLaunchSpec {
        path: path.to_owned(),
        interpreter: program,
        interpreter_args: interpreter.into_iter().map(OsString::from).collect(),
        args: args.into_iter().map(OsString::from).collect(),
        cwd,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLocation {
    pub path: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptBreakpoint {
    pub path: PathBuf,
    pub line: usize,
    pub enabled: bool,
    pub verified: bool,
    pub rejected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptVariable {
    pub name: String,
    pub value: String,
    pub children: Vec<ScriptVariable>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptStackFrame {
    pub index: usize,
    pub name: String,
    pub path: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScriptStatus {
    #[default]
    Idle,
    Saving,
    Starting,
    Running,
    Paused,
    Stopping,
    Finished,
    Failed,
}

impl ScriptStatus {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Saving | Self::Starting | Self::Running | Self::Paused | Self::Stopping
        )
    }

    pub fn label(self) -> LocalizedText {
        match self {
            Self::Idle => msg!("script-status-idle"),
            Self::Saving => msg!("script-status-saving"),
            Self::Starting => msg!("script-status-starting"),
            Self::Running => msg!("script-status-running"),
            Self::Paused => msg!("script-status-paused"),
            Self::Stopping => msg!("script-status-stopping"),
            Self::Finished => msg!("script-status-finished"),
            Self::Failed => msg!("script-status-failed"),
        }
        .into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptCommand {
    Run,
    Debug,
    Continue,
    StepOver,
    StepInto,
    StepOut,
    Stop,
    Examine,
    Evaluate,
    Inspect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptEventKind {
    Started,
    Continued,
    Stopped(ScriptLocation),
    Inspected {
        variables: Vec<ScriptVariable>,
        stack: Vec<ScriptStackFrame>,
    },
    Expression(String),
    Finished(u32),
    Failed(LocalizedText),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptEvent {
    pub session_id: u64,
    pub stop_id: u64,
    pub kind: ScriptEventKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptState {
    pub settings: ScriptSettings,
    pub breakpoints: Vec<ScriptBreakpoint>,
    pub location: Option<ScriptLocation>,
    pub variables: Vec<ScriptVariable>,
    pub stack: Vec<ScriptStackFrame>,
    pub expression_log: Vec<String>,
    pub status: ScriptStatus,
    pub session_id: u64,
    pub stop_id: u64,
    pub exit_code: Option<u32>,
    pub error: Option<LocalizedText>,
    pub debugging: bool,
}

impl ScriptState {
    pub fn begin(&mut self, debugging: bool) {
        self.session_id = self.session_id.wrapping_add(1).max(1);
        self.stop_id = 0;
        self.debugging = debugging;
        self.status = ScriptStatus::Starting;
        self.location = None;
        self.variables.clear();
        self.stack.clear();
        self.expression_log.clear();
        self.exit_code = None;
        self.error = None;
        for breakpoint in &mut self.breakpoints {
            breakpoint.verified = false;
            breakpoint.rejected = false;
        }
    }

    pub fn toggle_breakpoint(&mut self, path: &Path, line: usize) {
        let line = line.max(1);
        if let Some(index) = self
            .breakpoints
            .iter()
            .position(|bp| bp.path == path && bp.line == line)
        {
            self.breakpoints.remove(index);
        } else {
            self.breakpoints.push(ScriptBreakpoint {
                path: path.to_owned(),
                line: line.max(1),
                enabled: true,
                verified: false,
                rejected: false,
            });
            self.breakpoints
                .sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        }
    }

    pub fn apply(&mut self, event: ScriptEvent) -> bool {
        if event.session_id != self.session_id || event.stop_id < self.stop_id {
            return false;
        }
        match event.kind {
            ScriptEventKind::Started | ScriptEventKind::Continued => {
                self.status = ScriptStatus::Running;
                self.location = None;
            }
            ScriptEventKind::Stopped(location) => {
                self.status = ScriptStatus::Paused;
                self.stop_id = event.stop_id;
                self.location = Some(location);
            }
            ScriptEventKind::Inspected { variables, stack }
                if self.status == ScriptStatus::Paused && event.stop_id == self.stop_id =>
            {
                self.variables = variables;
                self.stack = stack;
            }
            ScriptEventKind::Expression(value)
                if self.status == ScriptStatus::Paused && event.stop_id == self.stop_id =>
            {
                self.expression_log.push(value)
            }
            ScriptEventKind::Finished(code) => {
                self.status = ScriptStatus::Finished;
                self.exit_code = Some(code);
                self.location = None;
            }
            ScriptEventKind::Failed(error) => {
                self.status = ScriptStatus::Failed;
                self.error = Some(error);
                self.location = None;
            }
            _ => return false,
        }
        true
    }

    pub fn remap_breakpoints(&mut self, path: &Path, changes: &[crate::editor::SourceLineChange]) {
        for change in changes {
            self.breakpoints.retain_mut(|bp| {
                if bp.path != path {
                    return true;
                }
                let Some(line) = change.remap_line(bp.line) else {
                    return false;
                };
                bp.line = line;
                bp.verified = false;
                bp.rejected = false;
                true
            });
        }
        self.breakpoints
            .sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        self.breakpoints.dedup_by(|later, earlier| {
            if later.path != earlier.path || later.line != earlier.line {
                return false;
            }
            earlier.enabled |= later.enabled;
            true
        });
    }
}
