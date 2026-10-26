use std::collections::BTreeSet;
use std::sync::Arc;

use app::script::{ScriptState, ScriptStatus, ScriptVariable};
use ratatui::layout::Rect;

use crate::components::TextInput;
use crate::{CommandLineTerminalSnapshot, EditorViewModel};

mod layout;
mod render;
pub use layout::*;
pub use render::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptPane {
    #[default]
    Source,
    Output,
    Variables,
    Stack,
    Expression,
    Breakpoints,
}

impl ScriptPane {
    pub const ALL: [Self; 6] = [
        Self::Source,
        Self::Output,
        Self::Variables,
        Self::Stack,
        Self::Expression,
        Self::Breakpoints,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|pane| *pane == self).unwrap()
    }

    pub fn next(self, reverse: bool) -> Self {
        Self::ALL[(self.index() + if reverse { 5 } else { 1 }) % Self::ALL.len()]
    }

    pub fn title(self) -> String {
        i18n::tr!(match self {
            Self::Source => "script-source",
            Self::Output => "script-output",
            Self::Variables => "script-variables",
            Self::Stack => "script-stack",
            Self::Expression => "script-expression",
            Self::Breakpoints => "script-breakpoints",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptAction {
    Run,
    Debug,
    Continue,
    Next,
    Step,
    Finish,
    Stop,
    Settings,
    Examine,
    Evaluate,
    ToggleBreakpoint,
    DeleteBreakpoint,
    LocateBreakpoint,
    InspectVariable,
}

impl ScriptAction {
    pub fn label(self, dirty: bool) -> String {
        let key = match self {
            Self::Run if dirty => "script-save-run",
            Self::Run => "script-run",
            Self::Debug if dirty => "script-save-debug",
            Self::Debug => "script-debug",
            Self::Continue => "script-continue",
            Self::Next => "script-next",
            Self::Step => "script-step",
            Self::Finish => "script-finish",
            Self::Stop => "script-stop",
            Self::Settings => "script-settings",
            Self::Examine => "script-examine",
            Self::Evaluate => "script-evaluate",
            Self::ToggleBreakpoint => "script-toggle-breakpoint",
            Self::DeleteBreakpoint => "script-delete-breakpoint",
            Self::LocateBreakpoint => "script-locate-breakpoint",
            Self::InspectVariable => "script-inspect-variable",
        };
        let label = i18n::tr!(key);
        let shortcut = match self {
            Self::Run => "Ctrl+F5",
            Self::Debug | Self::Continue => "F5",
            Self::Next => "F7",
            Self::Step => "F11",
            Self::Finish => "Shift+F11",
            Self::Stop => "Shift+F5",
            Self::Examine => "Enter",
            Self::Evaluate => "Ctrl+Enter",
            _ => "",
        };
        if shortcut.is_empty() {
            label
        } else {
            format!("{label} {shortcut}")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptVariableRow {
    pub key: String,
    pub depth: usize,
    pub variable: ScriptVariable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptViewModel {
    pub editor: EditorViewModel,
    pub state: ScriptState,
    pub output: Option<Arc<CommandLineTerminalSnapshot>>,
    pub expression: TextInput,
    pub focus: ScriptPane,
    pub detail: ScriptPane,
    pub detail_expansion: Option<u16>,
    pub selected_variable: usize,
    pub selected_stack: usize,
    pub selected_breakpoint: usize,
    pub scrolls: [usize; 6],
    pub expanded_variables: BTreeSet<String>,
    pub selected_action: Option<ScriptAction>,
}

impl ScriptViewModel {
    pub fn new(editor: EditorViewModel, state: ScriptState) -> Self {
        Self {
            editor,
            state,
            output: None,
            expression: TextInput::new("script.expression.input").with_horizontal_scroll(true),
            focus: ScriptPane::Source,
            detail: ScriptPane::Output,
            detail_expansion: None,
            selected_variable: 0,
            selected_stack: 0,
            selected_breakpoint: 0,
            scrolls: [0; 6],
            expanded_variables: BTreeSet::new(),
            selected_action: None,
        }
    }

    pub fn variable_rows(&self) -> Vec<ScriptVariableRow> {
        fn append(
            variables: &[ScriptVariable],
            parent: &str,
            depth: usize,
            expanded: &BTreeSet<String>,
            rows: &mut Vec<ScriptVariableRow>,
        ) {
            for (index, variable) in variables.iter().enumerate() {
                let key = format!("{parent}/{index}:{}", variable.name);
                rows.push(ScriptVariableRow {
                    key: key.clone(),
                    depth,
                    variable: variable.clone(),
                });
                if expanded.contains(&key) {
                    append(&variable.children, &key, depth + 1, expanded, rows);
                }
            }
        }
        let mut rows = Vec::new();
        append(
            &self.state.variables,
            "",
            0,
            &self.expanded_variables,
            &mut rows,
        );
        rows
    }

    pub fn action_enabled(&self, action: ScriptAction) -> bool {
        let active = matches!(
            self.state.status,
            ScriptStatus::Saving
                | ScriptStatus::Starting
                | ScriptStatus::Running
                | ScriptStatus::Paused
                | ScriptStatus::Stopping
        );
        let paused = self.state.status == ScriptStatus::Paused;
        match action {
            ScriptAction::Run | ScriptAction::Debug | ScriptAction::Settings => !active,
            ScriptAction::Continue
            | ScriptAction::Next
            | ScriptAction::Step
            | ScriptAction::Finish => paused,
            ScriptAction::Examine | ScriptAction::Evaluate => {
                paused && !self.expression.value().trim().is_empty()
            }
            ScriptAction::Stop => active && self.state.status != ScriptStatus::Stopping,
            ScriptAction::InspectVariable => !self.state.variables.is_empty(),
            ScriptAction::ToggleBreakpoint
            | ScriptAction::DeleteBreakpoint
            | ScriptAction::LocateBreakpoint => !self.state.breakpoints.is_empty(),
        }
    }

    pub fn decorated_editor(&self) -> EditorViewModel {
        let mut editor = self.editor.clone();
        if self.focus != ScriptPane::Source {
            editor.cursor = None;
        }
        editor.script_available = true;
        editor.script_settings_available = self.action_enabled(ScriptAction::Settings);
        editor.script_breakpoints = Some(
            self.state
                .breakpoints
                .iter()
                .filter(|point| {
                    editor
                        .path_hint
                        .as_deref()
                        .is_some_and(|path| std::path::Path::new(path) == point.path)
                })
                .map(|point| crate::EditorScriptBreakpoint {
                    line: point.line,
                    enabled: point.enabled,
                    verified: point.verified,
                    rejected: point.rejected,
                })
                .collect::<Vec<_>>()
                .into(),
        );
        editor.script_execution_line = self.state.location.as_ref().and_then(|location| {
            editor
                .path_hint
                .as_deref()
                .filter(|path| std::path::Path::new(path) == location.path)
                .map(|_| location.line)
        });
        editor
    }
}

pub fn script_editor_area(area: Rect) -> Rect {
    script_editor_area_with_state(area, ScriptStatus::Idle, false, ScriptPane::Output, None)
}

pub fn script_editor_area_with_state(
    area: Rect,
    status: ScriptStatus,
    dirty: bool,
    detail: ScriptPane,
    detail_expansion: Option<u16>,
) -> Rect {
    workspace_areas(area, status, dirty, detail, detail_expansion).0
}
