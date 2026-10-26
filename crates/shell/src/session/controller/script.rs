//! Connects script controls to the existing document and save workflow.
use crate::session::*;
use app::script::{ScriptState, ScriptStatus};
use script_runtime::RuntimeCommand;
use ui::{ScriptAction as Action, ScriptHitTarget as Hit, ScriptPane as Pane};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::session) struct ScriptPageState {
    pub enabled: bool,
    pub process_running: bool,
    pub state: ScriptState,
    pub focus: Pane,
    pub detail: Pane,
    pub detail_motion: ui::PanelMotion,
    pub closing_detail: Option<Pane>,
    pub motion_area: Rect,
    pub expression: ui::components::TextInput,
    pub output: Option<Arc<ui::CommandLineTerminalSnapshot>>,
    pub selected_variable: usize,
    pub selected_stack: usize,
    pub selected_breakpoint: usize,
    pub scrolls: [usize; 6],
    pub expanded_variables: std::collections::BTreeSet<String>,
    pub selected_action: Option<Action>,
    pub pointer: Option<Hit>,
    pub drag: Option<(Pane, u16)>,
    pub source_horizontal_drag: bool,
    pub pending_launch: Option<bool>,
    pub save_requested: bool,
    pub vim_pending: bool,
    pub vim_suspended: bool,
    pub source_view: Option<EditorState>,
}

impl Default for ScriptPageState {
    fn default() -> Self {
        Self {
            enabled: false,
            process_running: false,
            state: ScriptState::default(),
            focus: Pane::Source,
            detail: Pane::Output,
            detail_motion: ui::PanelMotion::default(),
            closing_detail: None,
            motion_area: Rect::default(),
            expression: ui::components::TextInput::new("script.expression")
                .with_horizontal_scroll(true),
            output: None,
            selected_variable: 0,
            selected_stack: 0,
            selected_breakpoint: 0,
            scrolls: [0; 6],
            expanded_variables: Default::default(),
            selected_action: None,
            pointer: None,
            drag: None,
            source_horizontal_drag: false,
            pending_launch: None,
            save_requested: false,
            vim_pending: false,
            vim_suspended: false,
            source_view: None,
        }
    }
}

impl ShellSession {
    pub(in crate::session) fn advance_script_details(&mut self, frame: ui::MotionFrame) -> bool {
        if self.content_screen() != ShellScreen::Editor || !self.pages.editor.script.enabled {
            return false;
        }
        let area = self.script_main_area();
        let frame = self.interaction_motion_frame(frame);
        let phases = ui::MotionPhases::from_millis(self.animation_timings().dialog);
        let page = &mut self.pages.editor.script;
        let target = if matches!(page.detail, Pane::Source | Pane::Output) {
            0
        } else {
            1_000
        };
        if page.motion_area != area || area.width < 140 || area.height < 36 {
            page.detail_motion = ui::PanelMotion::new(target);
            page.motion_area = area;
        }
        let before = page.detail_motion.value();
        let running = page.detail_motion.update_with_phases(target, frame, phases);
        if before != page.detail_motion.value() {
            if !matches!(page.pointer, Some(Hit::Tab(_))) {
                page.pointer = None;
            }
            page.drag = None;
        }
        if page.detail_motion.value() == 0 {
            page.closing_detail = None;
        }
        running
    }

    pub(in crate::session) fn script_available(&self) -> bool {
        cfg!(target_os = "linux")
            && !self.config_editor_active()
            && self.app.editor_state().is_some_and(|editor| {
                self.pages.editor.script.enabled
                    || app::script::is_shell_script(
                        editor.document.path.as_deref(),
                        &editor
                            .source_rope_snapshot()
                            .map(|source| {
                                source
                                    .chars()
                                    .take(512)
                                    .take_while(|character| !matches!(character, '\n' | '\r'))
                                    .collect::<String>()
                            })
                            .unwrap_or_default(),
                    )
            })
    }

    pub(in crate::session) fn open_script_editor(&mut self) {
        if !cfg!(target_os = "linux") {
            return;
        }
        self.new_edit_document();
        self.app.dispatch_at(
            app::AppCommand::Editor(app::editor::EditorCommand::InsertText(
                app::script::NEW_SCRIPT.into(),
            )),
            Instant::now(),
        );
        self.enable_script_workspace();
    }

    pub(in crate::session) fn enable_script_workspace(&mut self) {
        if !self.script_available() {
            return;
        }
        self.pages.editor.script.enabled = true;
        if self
            .app
            .editor_state()
            .is_some_and(|editor| editor.document.path.is_none())
        {
            self.pages.editor.editor_language = Some("Shell".into());
        }
        self.app.set_editor_source_change_tracking(true);
        self.pages.editor.editor_open_menu = None;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn script_main_area(&self) -> Rect {
        let bounds = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        self.input_keyboard_content_layout(
            ui::ShellFrameLayout::new(bounds, None, &ui::RenderContext::default())
                .with_help_button(self.help_button_available()),
        )
        .main
    }

    pub(in crate::session) fn editor_content_area(&self, main: Rect) -> Rect {
        if self.pages.editor.script.enabled {
            let page = &self.pages.editor.script;
            let dirty = self
                .app
                .editor_state()
                .is_some_and(|editor| editor.is_dirty() || editor.document.path.is_none());
            ui::script_editor_area_with_state(
                main,
                page.state.status,
                dirty,
                page.closing_detail
                    .filter(|_| page.detail == Pane::Output)
                    .unwrap_or(page.detail),
                Some(page.detail_motion.value()),
            )
        } else {
            main
        }
    }

    pub(in crate::session) fn to_script_view_model(&self) -> ui::ScriptViewModel {
        let page = &self.pages.editor.script;
        let editor = if let Some(source) = &page.source_view {
            let mut model = ui::EditorViewModel::source(
                source.document.display_name(),
                source.source().to_string(),
            );
            model.path_hint = source
                .document
                .path
                .as_ref()
                .map(|path| path.display().to_string());
            model.read_only = true;
            model.scroll_line = source.viewport.top_line;
            model.horizontal_scroll = source.viewport.left_column;
            model.cursor = source
                .source_display_position(source.cursor.byte_offset)
                .map(|(line, column)| ui::EditorTextPosition::new(line, column));
            model.cursor_offset = Some(source.cursor.byte_offset);
            model.syntax_highlights = source.source_syntax_highlights_for_path(
                &source
                    .document
                    .path
                    .as_ref()
                    .map_or_else(String::new, |path| path.display().to_string()),
                source.viewport.top_line
                    ..source.viewport.top_line + usize::from(self.terminal_size.1),
            );
            model
        } else {
            self.to_editor_view_model()
        };
        let mut model = ui::ScriptViewModel::new(editor, page.state.clone());
        model.editor.dirty = self
            .app
            .editor_state()
            .is_some_and(|editor| editor.is_dirty() || editor.document.path.is_none());
        model.output = page.output.clone();
        model.expression = page.expression.clone();
        model.focus = page.focus;
        model.detail = page.detail;
        if model.detail == Pane::Output {
            model.detail = page.closing_detail.unwrap_or(model.detail);
        }
        model.detail_expansion = Some(page.detail_motion.value());
        model.selected_variable = page.selected_variable;
        model.selected_stack = page.selected_stack;
        model.selected_breakpoint = page.selected_breakpoint;
        model.scrolls = page.scrolls;
        model.expanded_variables = page.expanded_variables.clone();
        model.selected_action = page.selected_action;
        model
    }

    pub(in crate::session) fn script_layout(&self) -> ui::ScriptLayout {
        ui::script_layout(self.script_main_area(), &self.to_script_view_model())
    }

    pub(in crate::session) fn request_script_launch(
        &mut self,
        debug: bool,
        platform: &dyn Platform,
    ) {
        if !self.script_available()
            || self.pages.editor.script.state.status.is_active()
            || self.pages.editor.script.process_running
        {
            return;
        }
        self.enable_script_workspace();
        if !PermissionService::new(self.debug_policy)
            .authorize(
                self.app.auth_session(),
                PermissionAction::ExecuteCommandLine,
                Some("builtin.script-editor"),
            )
            .allowed
        {
            return;
        }
        let script = &mut self.pages.editor.script;
        script.state.begin(debug);
        script.source_view = None;
        script.pending_launch = Some(debug);
        script.save_requested = false;
        if self.editor_vim_terminal.is_some() {
            self.pages.editor.script.vim_pending = true;
            self.editor_vim_script_export = Some(self.active_page_id_for_editor());
        }
        self.poll_script_launch(platform);
    }

    pub(in crate::session) fn poll_script_launch(&mut self, platform: &dyn Platform) {
        let Some(debug) = self.pages.editor.script.pending_launch else {
            return;
        };
        if self.pages.editor.script.vim_pending {
            return;
        }
        if self.pages.editor.editor_save_state.is_some()
            || self.pages.editor.editor_load_state.is_some()
        {
            return;
        }
        let Some(editor) = self.app.editor_state() else {
            return;
        };
        if editor.is_dirty() || editor.document.path.is_none() {
            if self.pages.editor.script.save_requested {
                if self.active_screen() == ShellScreen::Editor {
                    self.pages.editor.script.pending_launch = None;
                    self.pages.editor.script.state.status = ScriptStatus::Idle;
                    self.pages.editor.script.vim_suspended = false;
                }
                return;
            }
            self.pages.editor.script.save_requested = true;
            self.pages.editor.script.state.status = ScriptStatus::Saving;
            self.apply_editor_command(app::editor::EditorCommand::RequestSave, platform);
            return;
        }
        let result = app::script::build_launch_spec(
            editor.document.path.as_ref().unwrap(),
            &editor.source().to_string(),
            &self.pages.editor.script.state.settings,
        );
        self.pages.editor.script.pending_launch = None;
        match result {
            Ok(spec) if !debug || spec.is_bash() => {
                if self
                    .pages
                    .editor
                    .editor_fingerprint
                    .as_ref()
                    .is_some_and(|expected| {
                        platform::document_fingerprint(&spec.path).ok().as_ref() != Some(expected)
                    })
                {
                    self.fail_script(i18n::msg!("script-external-change").into());
                    return;
                }
                let id = self.active_page_id_for_editor();
                let session_id = self.pages.editor.script.state.session_id;
                self.pages.editor.script.state.status = ScriptStatus::Starting;
                self.script_requests.push_back((
                    id,
                    RuntimeCommand::Start {
                        spec,
                        debug,
                        session_id,
                    },
                ));
                self.app.dispatch_at(
                    app::AppCommand::Editor(app::editor::EditorCommand::SetExecutionReadOnly(true)),
                    Instant::now(),
                );
            }
            Ok(_) => self.fail_script(i18n::msg!("script-bash-required").into()),
            Err(error) => self.fail_script(error),
        }
    }

    fn active_page_id_for_editor(&self) -> PageInstanceId {
        self.pages
            .editor
            .selected_id()
            .unwrap_or(self.content_page_id())
    }

    pub(in crate::session) fn fail_script(&mut self, error: i18n::LocalizedText) {
        let script = &mut self.pages.editor.script;
        script.state.status = ScriptStatus::Failed;
        script.state.error = Some(error);
        script.pending_launch = None;
        if !script.process_running {
            script.vim_suspended = false;
            self.app.dispatch_at(
                app::AppCommand::Editor(app::editor::EditorCommand::SetExecutionReadOnly(false)),
                Instant::now(),
            );
        }
    }

    fn queue_script_command(&mut self, command: RuntimeCommand) {
        let id = self.active_page_id_for_editor();
        self.script_requests.push_back((id, command));
    }

    pub(in crate::session) fn activate_script_action(
        &mut self,
        action: Action,
        platform: &dyn Platform,
    ) {
        use ScriptStatus as Status;
        match action {
            Action::Run => self.request_script_launch(false, platform),
            Action::Debug => self.request_script_launch(true, platform),
            Action::Continue | Action::Next | Action::Step | Action::Finish
                if self.pages.editor.script.state.status == Status::Paused =>
            {
                let command = match action {
                    Action::Continue => RuntimeCommand::Continue,
                    Action::Next => RuntimeCommand::StepOver,
                    Action::Step => RuntimeCommand::StepIn,
                    _ => RuntimeCommand::StepOut,
                };
                self.pages.editor.script.state.status = Status::Running;
                self.pages.editor.script.source_view = None;
                self.queue_script_command(command);
            }
            Action::Stop if self.pages.editor.script.state.status.is_active() => {
                self.pages.editor.script.pending_launch = None;
                self.pages.editor.script.state.status = Status::Stopping;
                self.queue_script_command(RuntimeCommand::Stop);
            }
            Action::Examine | Action::Evaluate
                if self.pages.editor.script.state.status == Status::Paused =>
            {
                let input = self.pages.editor.script.expression.value().to_owned();
                if !input.trim().is_empty() {
                    self.pages.editor.script.state.status = Status::Running;
                    self.queue_script_command(RuntimeCommand::Evaluate {
                        input,
                        execute: action == Action::Evaluate,
                    });
                }
            }
            Action::Settings if !self.pages.editor.script.state.status.is_active() => {
                self.open_script_settings()
            }
            Action::ToggleBreakpoint | Action::DeleteBreakpoint | Action::LocateBreakpoint => {
                let index = self.pages.editor.script.selected_breakpoint;
                if let Some(bp) = self
                    .pages
                    .editor
                    .script
                    .state
                    .breakpoints
                    .get(index)
                    .cloned()
                {
                    match action {
                        Action::ToggleBreakpoint => {
                            self.pages.editor.script.state.breakpoints[index].enabled = !bp.enabled
                        }
                        Action::DeleteBreakpoint => {
                            self.pages.editor.script.state.breakpoints.remove(index);
                            self.pages.editor.script.selected_breakpoint = index.min(
                                self.pages
                                    .editor
                                    .script
                                    .state
                                    .breakpoints
                                    .len()
                                    .saturating_sub(1),
                            );
                        }
                        _ => self.locate_script_source(&bp.path, bp.line),
                    }
                    self.push_script_breakpoints();
                }
            }
            Action::InspectVariable => {
                let rows = self.to_script_view_model().variable_rows();
                if let Some(row) = rows.get(self.pages.editor.script.selected_variable) {
                    self.notify_modal(
                        i18n::msg!("script-variables"),
                        format!("{}\n{}", row.variable.name, row.variable.value),
                        ui::NotificationTone::Info,
                        vec![
                            ShellNotificationAction::new("close", i18n::msg!("shell-close"))
                                .cancel(),
                        ],
                    );
                }
            }
            _ => {}
        }
    }

    fn open_script_settings(&mut self) {
        use platform::management::ManagementField;
        let settings = &self.pages.editor.script.state.settings;
        let fields = [
            (
                "interpreter",
                "script-interpreter",
                settings.interpreter.clone(),
            ),
            ("arguments", "script-arguments", settings.arguments.clone()),
            ("cwd", "script-working-directory", settings.cwd.clone()),
        ]
        .into_iter()
        .map(|(id, key, value)| ManagementField {
            id: id.into(),
            label: i18n::tr!(key),
            value,
            required: false,
            ..Default::default()
        })
        .collect();
        self.config_form("edit-script-settings", String::new(), fields);
    }

    pub(in crate::session) fn submit_script_settings(
        &mut self,
        fields: &[platform::management::ManagementField],
    ) {
        for field in fields {
            let settings = &mut self.pages.editor.script.state.settings;
            match field.id.as_str() {
                "interpreter" => settings.interpreter = field.value.clone(),
                "arguments" => settings.arguments = field.value.clone(),
                "cwd" => settings.cwd = field.value.clone(),
                _ => {}
            }
        }
    }

    fn push_script_breakpoints(&mut self) {
        if self.pages.editor.script.state.debugging
            && self.pages.editor.script.state.status.is_active()
        {
            self.queue_script_command(RuntimeCommand::SetBreakpoints(
                self.pages.editor.script.state.breakpoints.clone(),
            ));
        }
    }

    fn toggle_script_line(&mut self, line: usize) {
        let path = self
            .pages
            .editor
            .script
            .source_view
            .as_ref()
            .or(self.app.editor_state())
            .and_then(|editor| editor.document.path.clone());
        if let Some(path) = path {
            self.pages
                .editor
                .script
                .state
                .toggle_breakpoint(&path, line);
            self.push_script_breakpoints();
        }
    }

    pub(in crate::session) fn locate_script_source(&mut self, path: &std::path::Path, line: usize) {
        if self
            .app
            .editor_state()
            .and_then(|editor| editor.document.path.as_deref())
            == Some(path)
        {
            self.pages.editor.script.source_view = None;
            if let Some(offset) = self
                .app
                .editor_state()
                .and_then(|editor| editor.source_offset(line.saturating_sub(1), 0))
            {
                let platform = platform::native_platform();
                self.apply_editor_command(
                    app::editor::EditorCommand::MoveTo {
                        position: app::editor::EditorPosition::Source(offset),
                        extend_selection: false,
                    },
                    platform.as_ref(),
                );
            }
        } else {
            let source = platform::read_document_bytes_limited(path, platform::MAX_DOCUMENT_BYTES)
                .map_err(|error| error.to_string())
                .and_then(|document| {
                    EditorState::open_read_only(path, &document.bytes)
                        .map_err(|error| error.to_string())
                });
            if let Ok(mut source) = source {
                source.cursor.byte_offset =
                    source.source_offset(line.saturating_sub(1), 0).unwrap_or(0);
                source.viewport.top_line = line.saturating_sub(3);
                self.pages.editor.script.source_view = Some(source);
            } else if let Err(reason) = source {
                self.pages.editor.script.state.error = Some(
                    i18n::msg!(
                        "script-source-unavailable",
                        path = path.display().to_string(),
                        reason = reason
                    )
                    .into(),
                );
            }
        }
    }

    pub(in crate::session) fn handle_script_key(
        &mut self,
        key: &KeyInput,
        platform: &dyn Platform,
    ) -> bool {
        if !self.script_available()
            || !self.interactive_overlays().is_empty()
            || self.current_vim_install().is_some()
            || self.pages.editor.editor_focus == ui::EditorFocus::MenuBar
        {
            return false;
        }
        if !key.phase.is_press_like() {
            return self.pages.editor.script.enabled
                && (self.pages.editor.script.focus != Pane::Source
                    || matches!(key.key, InputKey::F(5 | 6 | 7 | 9 | 11)));
        }
        let action = match key.key {
            InputKey::F(5) if key.modifiers.shift => Some(Action::Stop),
            InputKey::F(5) if key.modifiers.control => Some(Action::Run),
            InputKey::F(5) => Some(
                if self.pages.editor.script.state.status == ScriptStatus::Paused {
                    Action::Continue
                } else {
                    Action::Debug
                },
            ),
            InputKey::F(7) => Some(Action::Next),
            InputKey::F(11) => Some(if key.modifiers.shift {
                Action::Finish
            } else {
                Action::Step
            }),
            _ => None,
        };
        if let Some(action) = action {
            if key.phase == InputPhase::Press {
                self.activate_script_action(action, platform);
            }
            return true;
        }
        if !self.pages.editor.script.enabled {
            return false;
        }
        if key.key == InputKey::Escape {
            return false;
        }
        if key.key == InputKey::F(10) {
            self.pages.editor.script.focus = Pane::Source;
            self.pages.editor.script.selected_action = None;
            return false;
        }
        if key.key == InputKey::F(9) {
            if key.phase == InputPhase::Press {
                let line = self
                    .pages
                    .editor
                    .script
                    .source_view
                    .as_ref()
                    .or(self.app.editor_state())
                    .and_then(|editor| editor.source_display_position(editor.cursor.byte_offset))
                    .map_or(1, |(line, _)| line + 1);
                self.toggle_script_line(line);
            }
            return true;
        }
        if key.key == InputKey::F(6) {
            let pane = self.pages.editor.script.focus.next(key.modifiers.shift);
            self.pages.editor.script.focus = pane;
            if pane != Pane::Source {
                self.pages.editor.script.detail = pane;
            }
            self.pages.editor.script.selected_action = None;
            return true;
        }
        if key.key == InputKey::F(8) && self.pages.editor.script.state.status.is_active() {
            return true;
        }
        if key.key == InputKey::F(8) {
            self.pages.editor.script.source_view = None;
            return false;
        }
        let pane = self.pages.editor.script.focus;
        if pane == Pane::Source && self.pages.editor.script.source_view.is_some() {
            use app::editor::{CursorMove, EditorCommand};
            let movement = match key.key {
                InputKey::Up => Some(CursorMove::Up),
                InputKey::Down => Some(CursorMove::Down),
                InputKey::PageUp => Some(CursorMove::Up),
                InputKey::PageDown => Some(CursorMove::Down),
                InputKey::Left => Some(CursorMove::Left),
                InputKey::Right => Some(CursorMove::Right),
                InputKey::Home => Some(CursorMove::LineStart),
                InputKey::End => Some(CursorMove::LineEnd),
                _ => None,
            };
            if let Some(movement) = movement {
                let capacity = self.script_layout().editor.visible_capacity;
                let source = self.pages.editor.script.source_view.as_mut().unwrap();
                let steps = if matches!(key.key, InputKey::PageUp | InputKey::PageDown) {
                    capacity.saturating_sub(2).max(1)
                } else {
                    1
                };
                for _ in 0..steps {
                    app::editor::EditorController.apply(
                        source,
                        EditorCommand::MoveCursor {
                            movement,
                            extend_selection: key.modifiers.shift,
                        },
                    );
                }
                let line = source
                    .source_display_position(source.cursor.byte_offset)
                    .map_or(0, |(line, _)| line);
                if line < source.viewport.top_line {
                    source.viewport.top_line = line;
                } else if line >= source.viewport.top_line + capacity {
                    source.viewport.top_line = line.saturating_sub(capacity.saturating_sub(1));
                }
                return true;
            }
        }
        if pane == Pane::Source && self.pages.editor.script.source_view.is_none() {
            return false;
        }
        if key.key == InputKey::Tab || key.key == InputKey::BackTab {
            let model = self.to_script_view_model();
            let actions = self
                .script_layout()
                .actions
                .into_iter()
                .filter(|(action, area)| {
                    !area.is_empty()
                        && model.action_enabled(*action)
                        && match pane {
                            Pane::Variables => *action == Action::InspectVariable,
                            Pane::Breakpoints => matches!(
                                action,
                                Action::LocateBreakpoint
                                    | Action::ToggleBreakpoint
                                    | Action::DeleteBreakpoint
                            ),
                            Pane::Expression => {
                                matches!(action, Action::Examine | Action::Evaluate)
                            }
                            Pane::Output => matches!(
                                action,
                                Action::Run
                                    | Action::Debug
                                    | Action::Continue
                                    | Action::Next
                                    | Action::Step
                                    | Action::Finish
                                    | Action::Stop
                                    | Action::Settings
                            ),
                            _ => false,
                        }
                })
                .collect::<Vec<_>>();
            if !actions.is_empty() {
                let current = self
                    .pages
                    .editor
                    .script
                    .selected_action
                    .and_then(|selected| {
                        actions.iter().position(|(action, _)| *action == selected)
                    });
                let reverse = key.key == InputKey::BackTab || key.modifiers.shift;
                let next = match current {
                    None => Some(if reverse { actions.len() - 1 } else { 0 }),
                    Some(0) if reverse => None,
                    Some(index) if !reverse && index + 1 == actions.len() => None,
                    Some(index) => Some(if reverse { index - 1 } else { index + 1 }),
                };
                self.pages.editor.script.selected_action = next.map(|index| actions[index].0);
            } else {
                self.pages.editor.script.selected_action = None;
            }
            return true;
        }
        if matches!(
            key.key,
            InputKey::Enter | InputKey::Space | InputKey::Char(' ')
        ) && self.pages.editor.script.selected_action.is_some()
        {
            if key.phase == InputPhase::Press {
                self.activate_script_action(
                    self.pages.editor.script.selected_action.unwrap(),
                    platform,
                );
            }
            return true;
        }
        if pane == Pane::Output {
            return !self.pages.editor.script.process_running;
        }
        if pane == Pane::Expression {
            if self.pages.editor.script.state.status != ScriptStatus::Paused
                || self.pages.editor.script.selected_action.is_some()
            {
                return true;
            }
            if key.key == InputKey::Enter {
                if key.phase == InputPhase::Press {
                    self.activate_script_action(
                        if key.modifiers.control {
                            Action::Evaluate
                        } else {
                            Action::Examine
                        },
                        platform,
                    );
                }
            } else if key.modifiers.control
                && matches!(key.key, InputKey::Char('c' | 'C' | 'x' | 'X'))
            {
                if let Some(text) = self.pages.editor.script.expression.selected_text() {
                    match platform.write_clipboard_text(&text) {
                        Ok(()) if matches!(key.key, InputKey::Char('x' | 'X')) => {
                            self.pages.editor.script.expression.handle_event_borderless(
                                InputEvent::key(InputKey::Backspace),
                                Rect::default(),
                            );
                        }
                        Ok(()) => {}
                        Err(error) => self.notify_status(i18n::msg!(
                            "screen-keyboard-clipboard-error",
                            error = error.to_string()
                        )),
                    }
                }
            } else if key.modifiers.control && matches!(key.key, InputKey::Char('v' | 'V')) {
                if let Ok(value) = platform.read_clipboard_text() {
                    self.script_paste(value);
                }
            } else {
                self.pages.editor.script.expression.set_focused(true);
                self.pages
                    .editor
                    .script
                    .expression
                    .handle_event_borderless(InputEvent::Key(key.clone()), Rect::default());
            }
            return true;
        }
        let delta = match key.key {
            InputKey::Up => -1,
            InputKey::Down => 1,
            InputKey::PageUp => -5,
            InputKey::PageDown => 5,
            _ => 0,
        };
        if delta != 0 {
            let count = match pane {
                Pane::Variables => self.to_script_view_model().variable_rows().len(),
                Pane::Stack => self.pages.editor.script.state.stack.len(),
                Pane::Breakpoints => self.pages.editor.script.state.breakpoints.len(),
                _ => usize::MAX,
            };
            let capacity = self
                .script_layout()
                .rows
                .iter()
                .filter(|(row_pane, _, _)| *row_pane == pane)
                .count()
                .max(1);
            let script = &mut self.pages.editor.script;
            script.selected_action = None;
            let selected = match pane {
                Pane::Variables => &mut script.selected_variable,
                Pane::Stack => &mut script.selected_stack,
                Pane::Breakpoints => &mut script.selected_breakpoint,
                _ => &mut script.scrolls[pane as usize],
            };
            *selected = selected
                .saturating_add_signed(delta)
                .min(count.saturating_sub(1));
            let index = *selected;
            let scroll = &mut script.scrolls[pane as usize];
            if index < *scroll {
                *scroll = index;
            } else if index >= scroll.saturating_add(capacity) {
                *scroll = index.saturating_add(1).saturating_sub(capacity);
            }
        } else if matches!(key.key, InputKey::Enter | InputKey::Char(' '))
            && key.phase == InputPhase::Press
        {
            self.activate_script_row(pane, platform);
        }
        true
    }

    fn activate_script_row(&mut self, pane: Pane, platform: &dyn Platform) {
        match pane {
            Pane::Variables => {
                let rows = self.to_script_view_model().variable_rows();
                if let Some(row) = rows.get(self.pages.editor.script.selected_variable) {
                    if !self.pages.editor.script.expanded_variables.remove(&row.key) {
                        self.pages
                            .editor
                            .script
                            .expanded_variables
                            .insert(row.key.clone());
                    }
                }
            }
            Pane::Stack => {
                if let Some(frame) = self
                    .pages
                    .editor
                    .script
                    .state
                    .stack
                    .get(self.pages.editor.script.selected_stack)
                    .cloned()
                {
                    self.locate_script_source(&frame.path, frame.line);
                }
            }
            Pane::Breakpoints => self.activate_script_action(Action::LocateBreakpoint, platform),
            _ => {}
        }
    }

    pub(in crate::session) fn script_paste(&mut self, value: String) -> bool {
        if !self.interactive_overlays().is_empty() {
            return false;
        }
        if self.pages.editor.script.enabled && self.pages.editor.script.focus == Pane::Expression {
            if self.pages.editor.script.state.status != ScriptStatus::Paused
                || self.pages.editor.script.selected_action.is_some()
            {
                return true;
            }
            self.pages.editor.script.expression.set_focused(true);
            self.pages
                .editor
                .script
                .expression
                .handle_event_borderless(InputEvent::Paste(value), Rect::default());
            true
        } else {
            self.pages.editor.script.enabled
                && (self.pages.editor.script.focus != Pane::Source
                    || self.pages.editor.script.source_view.is_some())
        }
    }

    pub(in crate::session) fn handle_script_pointer(
        &mut self,
        mouse: MouseInput,
        platform: &dyn Platform,
    ) -> bool {
        if !self.pages.editor.script.enabled || !self.interactive_overlays().is_empty() {
            return false;
        }
        let layout = self.script_layout();
        let hit = layout.hit_test(mouse.column(), mouse.row());
        if self.pages.editor.script.source_view.is_some()
            && matches!(hit,Some(Hit::Editor(target)) if !matches!(target,ui::EditorHitTarget::Canvas(_)|ui::EditorHitTarget::VerticalScrollbar|ui::EditorHitTarget::HorizontalScrollbar))
        {
            return false;
        }
        if matches!(hit, Some(Hit::Editor(_)))
            && self.pages.editor.script.source_view.is_none()
            && self.pages.editor.script.pointer.is_none()
        {
            self.pages.editor.script.focus = Pane::Source;
            self.pages.editor.script.selected_action = None;
            return false;
        }
        match mouse.kind {
            ui::MouseEventKind::Down(PointerButton::Left) => {
                self.pages.editor.script.pointer = hit.clone();
                if let Some(Hit::Editor(
                    target @ (ui::EditorHitTarget::VerticalScrollbar
                    | ui::EditorHitTarget::HorizontalScrollbar),
                )) = hit
                {
                    let horizontal = target == ui::EditorHitTarget::HorizontalScrollbar;
                    if let Some(bar) = if horizontal {
                        layout.editor.horizontal_scrollbar
                    } else {
                        layout.editor.vertical_scrollbar
                    } {
                        self.pages.editor.script.source_horizontal_drag = horizontal;
                        let grab = if horizontal {
                            mouse.column().saturating_sub(bar.thumb.x)
                        } else {
                            mouse.row().saturating_sub(bar.thumb.y)
                        };
                        self.pages.editor.script.drag = Some((Pane::Source, grab));
                    }
                }
                if hit == Some(Hit::ExpressionInput) {
                    self.pages.editor.script.focus = Pane::Expression;
                    self.pages.editor.script.detail = Pane::Expression;
                    self.pages.editor.script.selected_action = None;
                    self.pages.editor.script.expression.set_focused(true);
                    self.pages
                        .editor
                        .script
                        .expression
                        .handle_event_borderless(InputEvent::Mouse(mouse), layout.expression_input);
                }
                if let Some(Hit::Scrollbar(pane)) = hit {
                    if let Some((_, bar)) = layout
                        .scrollbars
                        .iter()
                        .find(|(candidate, _)| *candidate == pane)
                    {
                        self.pages.editor.script.drag =
                            Some((pane, mouse.row().saturating_sub(bar.thumb.y)));
                    }
                }
            }
            ui::MouseEventKind::Drag(PointerButton::Left) => {
                self.pages.editor.script.pointer = None;
                if let Some((pane, grab)) = self.pages.editor.script.drag {
                    if pane == Pane::Source {
                        let horizontal = self.pages.editor.script.source_horizontal_drag;
                        if let Some(bar) = if horizontal {
                            layout.editor.horizontal_scrollbar
                        } else {
                            layout.editor.vertical_scrollbar
                        } {
                            let (pointer, start, length, thumb, total, capacity) = if horizontal {
                                (
                                    mouse.column(),
                                    bar.track.x,
                                    bar.track.width,
                                    bar.thumb.width,
                                    layout.editor.horizontal_content_width,
                                    usize::from(layout.editor.canvas.width),
                                )
                            } else {
                                (
                                    mouse.row(),
                                    bar.track.y,
                                    bar.track.height,
                                    bar.thumb.height,
                                    layout.editor.document_line_count,
                                    layout.editor.visible_capacity,
                                )
                            };
                            let position = scrollbar_window_start(
                                pointer, grab, start, length, thumb, total, capacity,
                            );
                            if let Some(source) = &mut self.pages.editor.script.source_view {
                                if horizontal {
                                    source.viewport.left_column = position;
                                } else {
                                    source.viewport.top_line = position;
                                }
                            }
                        }
                        return true;
                    }
                    if let Some((_, bar)) = layout
                        .scrollbars
                        .iter()
                        .find(|(candidate, _)| *candidate == pane)
                    {
                        let history = self
                            .pages
                            .editor
                            .script
                            .output
                            .as_ref()
                            .map_or(0, |output| output.scrollback_rows);
                        let total = match pane {
                            Pane::Variables => self.to_script_view_model().variable_rows().len(),
                            Pane::Stack => self.pages.editor.script.state.stack.len(),
                            Pane::Breakpoints => self.pages.editor.script.state.breakpoints.len(),
                            Pane::Expression => layout.expression_lines.len(),
                            Pane::Output => history + usize::from(bar.track.height),
                            _ => 0,
                        };
                        let position = scrollbar_window_start(
                            mouse.row(),
                            grab,
                            bar.track.y,
                            bar.track.height,
                            bar.thumb.height,
                            total,
                            usize::from(bar.track.height),
                        );
                        self.pages.editor.script.scrolls[pane as usize] = if pane == Pane::Output {
                            history.saturating_sub(position)
                        } else {
                            position
                        };
                    }
                }
            }
            ui::MouseEventKind::Up(PointerButton::Left) => {
                self.pages.editor.script.drag = None;
                let pressed = self.pages.editor.script.pointer.take();
                if pressed.is_some() && pressed == hit {
                    match hit {
                        Some(Hit::Editor(_)) if self.pages.editor.script.source_view.is_some() => {
                            if let Some(hit) =
                                layout.editor.hit_test_source(mouse.column(), mouse.row())
                            {
                                self.pages
                                    .editor
                                    .script
                                    .source_view
                                    .as_mut()
                                    .unwrap()
                                    .cursor
                                    .byte_offset = hit.byte_offset;
                                self.pages.editor.script.focus = Pane::Source;
                            }
                        }
                        Some(Hit::Action(action)) => self.activate_script_action(action, platform),
                        Some(Hit::BreakpointLine(line)) => self.toggle_script_line(line),
                        Some(Hit::Tab(pane)) => {
                            if layout.wide && self.pages.editor.script.detail == pane {
                                self.pages.editor.script.closing_detail = Some(pane);
                            }
                            let pane = if layout.wide && self.pages.editor.script.detail == pane {
                                Pane::Output
                            } else {
                                pane
                            };
                            self.pages.editor.script.selected_action = None;
                            self.pages.editor.script.focus = pane;
                            self.pages.editor.script.detail = pane;
                        }
                        Some(Hit::Pane(pane)) => {
                            self.pages.editor.script.selected_action = None;
                            self.pages.editor.script.focus = pane;
                            if !layout.wide || pane != Pane::Output {
                                self.pages.editor.script.detail = pane;
                            }
                        }
                        Some(Hit::ExpressionInput) => {
                            self.pages.editor.script.focus = Pane::Expression;
                            self.pages.editor.script.detail = Pane::Expression;
                        }
                        Some(Hit::Row(pane, index)) => {
                            self.pages.editor.script.selected_action = None;
                            self.pages.editor.script.focus = pane;
                            match pane {
                                Pane::Variables => {
                                    self.pages.editor.script.selected_variable = index
                                }
                                Pane::Stack => self.pages.editor.script.selected_stack = index,
                                Pane::Breakpoints => {
                                    self.pages.editor.script.selected_breakpoint = index
                                }
                                _ => {}
                            }
                            self.activate_script_row(pane, platform);
                        }
                        _ => {}
                    }
                }
            }
            ui::MouseEventKind::Scroll(direction) => {
                self.pages.editor.script.pointer = None;
                let pane = if layout.editor_area.contains(mouse.coordinates().into()) {
                    Some(Pane::Source)
                } else {
                    layout
                        .panes
                        .iter()
                        .find(|(_, area)| area.contains(mouse.coordinates().into()))
                        .map(|(pane, _)| *pane)
                };
                if let Some(pane) = pane {
                    let delta = if matches!(direction, ScrollDirection::Up | ScrollDirection::Left)
                    {
                        -3
                    } else {
                        3
                    };
                    let index = pane as usize;
                    if pane == Pane::Output {
                        let history = self
                            .pages
                            .editor
                            .script
                            .output
                            .as_ref()
                            .map_or(0, |output| output.scrollback_rows);
                        self.pages.editor.script.scrolls[index] = self.pages.editor.script.scrolls
                            [index]
                            .saturating_add_signed(-delta)
                            .min(history);
                    } else {
                        self.pages.editor.script.scrolls[index] =
                            self.pages.editor.script.scrolls[index].saturating_add_signed(delta);
                    }
                    if pane == Pane::Source {
                        if let Some(source) = &mut self.pages.editor.script.source_view {
                            if matches!(direction, ScrollDirection::Left | ScrollDirection::Right) {
                                source.viewport.left_column =
                                    source.viewport.left_column.saturating_add_signed(delta);
                            } else {
                                source.viewport.top_line =
                                    source.viewport.top_line.saturating_add_signed(delta);
                            }
                        }
                    }
                }
            }
            ui::MouseEventKind::Moved => {
                if self
                    .pages
                    .editor
                    .script
                    .pointer
                    .as_ref()
                    .is_some_and(|pressed| Some(pressed) != hit.as_ref())
                {
                    self.pages.editor.script.pointer = None;
                }
            }
            _ => {}
        }
        true
    }

    pub(in crate::session) fn script_host_key(&self, key: &KeyInput) -> bool {
        self.script_available() && matches!(key.key, InputKey::F(5 | 6 | 7 | 9 | 11))
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "../../../tests/unit/session/controller/script.rs"]
mod tests;
