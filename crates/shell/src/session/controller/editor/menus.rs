use crate::session::*;
use platform::management::ManagementField;

impl ShellSession {
    pub(in crate::session) fn new_edit_document(&mut self) {
        if self.config_editor_active() {
            self.notify_toast(i18n::msg!("config-editor-finish-document"));
            return;
        }
        let script_workspace =
            self.active_screen() == ShellScreen::Editor && self.pages.editor.script.enabled;
        self.enter_new_page(ShellScreen::Editor);
        self.app.dispatch_at(
            app::AppCommand::SetEditorState(Some(EditorState::new())),
            Instant::now(),
        );
        if script_workspace {
            self.app.dispatch_at(
                app::AppCommand::Editor(app::editor::EditorCommand::InsertText(
                    app::script::NEW_SCRIPT.into(),
                )),
                Instant::now(),
            );
            self.enable_script_workspace();
        }
        self.pages.common.focused_component = ShellComponent::Editor;
        self.refresh_hit_map();
    }
    pub(in crate::session) fn activate_editor_menu_action(
        &mut self,
        action: ui::EditorMenuAction,
        platform: &dyn Platform,
    ) {
        use app::editor::EditorCommand as Command;
        use ui::EditorMenuAction as Action;
        self.pages.editor.editor_open_menu = None;
        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
        let command = match action {
            Action::ScriptWorkspace => {
                self.enable_script_workspace();
                return;
            }
            Action::ScriptSettings => {
                self.enable_script_workspace();
                self.activate_script_action(ui::ScriptAction::Settings, platform);
                return;
            }
            Action::ToggleVim => {
                self.toggle_editor_vim();
                return;
            }
            Action::VimHelp => {
                self.notify_modal(
                    i18n::msg!("ui-editor-vim-help"),
                    i18n::msg!("ui-editor-vim-help-text"),
                    ui::NotificationTone::Info,
                    vec![
                        ShellNotificationAction::new("close", i18n::msg!("shell-close"))
                            .with_shortcut(InputKey::Escape)
                            .cancel(),
                    ],
                );
                return;
            }
            Action::SaveAs => Command::RequestSaveAs,
            Action::Close => Command::RequestClose,
            Action::Exit => {
                self.pages.editor.editor_exit_group = true;
                Command::RequestClose
            }
            Action::Cut => Command::Cut,
            Action::Copy => Command::Copy,
            Action::Paste => Command::RequestPaste,
            Action::SelectAll => Command::SelectAll,
            Action::Replace => {
                self.open_editor_replace();
                return;
            }
            Action::ToggleWrap => {
                self.toggle_editor_wrap();
                return;
            }
            Action::FormatJson => Command::FormatJson,
            Action::Toolbar(action) => {
                self.activate_editor_toolbar(action, platform);
                return;
            }
            Action::Mode(mode) => Command::SetMode(mode),
            Action::Config(action) => {
                self.config_editor_action(action);
                return;
            }
            Action::Preferences => {
                self.open_editor_settings();
                return;
            }
            Action::FocusStatus => {
                self.pages.editor.editor_focus = ui::EditorFocus::StatusBar;
                self.pages.editor.editor_menu_index = 0;
                return;
            }
            Action::GoToLine
            | Action::GoToFile
            | Action::About
            | Action::Language
            | Action::LineEnding
            | Action::Encoding
            | Action::Indentation => {
                self.open_edit_dialog(action);
                return;
            }
        };
        self.apply_editor_command(command, platform);
    }

    /// Menu navigation consumes its keys before they can insert document text.
    pub(in crate::session) fn handle_edit_menu_key(
        &mut self,
        key: &KeyInput,
        platform: &dyn Platform,
    ) -> bool {
        if key.phase == InputPhase::Repeat
            && (key.modifiers.alt
                || matches!(
                    key.key,
                    InputKey::Enter | InputKey::Char(' ') | InputKey::F(10)
                ))
        {
            return true;
        }
        let menu_key = if key.modifiers.alt {
            match key.key {
                InputKey::Char('f' | 'F') => Some(ui::EditorMenu::File),
                InputKey::Char('e' | 'E') => Some(ui::EditorMenu::Edit),
                InputKey::Char('v' | 'V') => Some(ui::EditorMenu::View),
                InputKey::Char('s' | 'S') if self.script_available() => {
                    Some(ui::EditorMenu::Script)
                }
                InputKey::Char('h' | 'H') => Some(ui::EditorMenu::Help),
                _ => None,
            }
        } else {
            None
        };
        if let Some(menu) = menu_key {
            self.pages.editor.editor_selected_menu = menu;
            self.pages.editor.editor_open_menu = Some(menu);
            self.pages.editor.editor_focus = ui::EditorFocus::MenuBar;
            self.pages.editor.editor_menu_index = 0;
            return true;
        }
        if key.key == InputKey::F(10) {
            self.pages.editor.editor_focus = ui::EditorFocus::MenuBar;
            self.pages.editor.editor_open_menu = None;
            self.pages.editor.editor_menu_index = 0;
            return true;
        }
        if self.pages.editor.editor_focus == ui::EditorFocus::StatusBar {
            let actions = self.to_editor_view_model().status_actions;
            let len = actions.len().max(1);
            match key.key {
                InputKey::Tab | InputKey::Right => {
                    self.pages.editor.editor_menu_index =
                        (self.pages.editor.editor_menu_index + 1) % len
                }
                InputKey::BackTab | InputKey::Left => {
                    self.pages.editor.editor_menu_index =
                        (self.pages.editor.editor_menu_index + len - 1) % len
                }
                InputKey::Enter | InputKey::Char(' ') => {
                    if let Some((action, _)) = actions.get(self.pages.editor.editor_menu_index) {
                        self.activate_editor_menu_action(*action, platform);
                    }
                }
                InputKey::Escape | InputKey::F(6) => {
                    self.pages.editor.editor_focus = ui::EditorFocus::Canvas
                }
                _ => return false,
            }
            return true;
        }
        if self.pages.editor.editor_focus != ui::EditorFocus::MenuBar {
            return false;
        }
        let mut menus = vec![
            ui::EditorMenu::File,
            ui::EditorMenu::Edit,
            ui::EditorMenu::View,
            ui::EditorMenu::Help,
        ];
        if self.script_available() {
            menus.insert(3, ui::EditorMenu::Script);
        }
        let index = menus
            .iter()
            .position(|m| *m == self.pages.editor.editor_selected_menu)
            .unwrap_or(0);
        match key.key {
            InputKey::Left | InputKey::Right | InputKey::Tab | InputKey::BackTab => {
                let backwards = matches!(key.key, InputKey::Left | InputKey::BackTab);
                let menu =
                    menus[(index + if backwards { menus.len() - 1 } else { 1 }) % menus.len()];
                self.pages.editor.editor_selected_menu = menu;
                self.pages.editor.editor_open_menu =
                    self.pages.editor.editor_open_menu.map(|_| menu);
                self.pages.editor.editor_menu_index = 0;
            }
            InputKey::Down | InputKey::Up | InputKey::Home | InputKey::End => {
                let menu = self.pages.editor.editor_selected_menu;
                let len = ui::editor_menu_actions(menu).len();
                if self.pages.editor.editor_open_menu.is_none() {
                    self.pages.editor.editor_open_menu = Some(menu);
                    self.pages.editor.editor_menu_index = 0;
                } else if len > 0 {
                    let index = self.pages.editor.editor_menu_index.min(len - 1);
                    self.pages.editor.editor_menu_index = match key.key {
                        InputKey::Home => 0,
                        InputKey::End => len - 1,
                        InputKey::Up => (index + len - 1) % len,
                        _ => (index + 1) % len,
                    };
                }
            }
            InputKey::Enter | InputKey::Char(' ') => {
                if let Some(menu) = self.pages.editor.editor_open_menu {
                    let actions = ui::editor_menu_actions(menu);
                    if let Some(action) = actions.get(self.pages.editor.editor_menu_index) {
                        let layout = self.editor_current_layout();
                        if layout
                            .menu_items
                            .iter()
                            .any(|item| item.action == *action && item.enabled)
                        {
                            self.activate_editor_menu_action(*action, platform);
                        }
                    }
                } else {
                    self.pages.editor.editor_open_menu =
                        Some(self.pages.editor.editor_selected_menu);
                }
            }
            InputKey::Escape | InputKey::F(6) => {
                self.pages.editor.editor_open_menu = None;
                self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
            }
            _ => return false,
        }
        true
    }

    fn open_edit_dialog(&mut self, action: ui::EditorMenuAction) {
        use ui::EditorMenuAction as Action;
        let Some(state) = self.app.editor_state() else {
            return;
        };
        let (id, label, value, choices, message) = match action {
            Action::GoToLine => (
                "goto",
                i18n::tr!("ui-editor-go-to-line"),
                state
                    .source_display_position(state.cursor.byte_offset)
                    .map_or("1:1".into(), |(l, c)| format!("{}:{}", l + 1, c + 1)),
                vec![],
                i18n::tr!("ui-editor-goto-help"),
            ),
            Action::GoToFile => {
                let choices = self
                    .editor_document_pages()
                    .into_iter()
                    .filter_map(|id| {
                        let doc = self.app.editor_state_for_page(id)?;
                        Some(format!(
                            "{}: {}{}",
                            id.get(),
                            doc.document.display_name(),
                            if doc.is_dirty() { " *" } else { "" }
                        ))
                    })
                    .collect::<Vec<_>>();
                (
                    "file",
                    i18n::tr!("ui-editor-go-to-file"),
                    choices.first().cloned().unwrap_or_default(),
                    choices,
                    String::new(),
                )
            }
            Action::Language => (
                "language",
                i18n::tr!("ui-editor-language"),
                self.pages
                    .editor
                    .editor_language
                    .clone()
                    .unwrap_or("Auto".into()),
                vec![
                    "Auto",
                    "Text",
                    "Rust",
                    "C",
                    "C++",
                    "JSON",
                    "Python",
                    "JavaScript",
                    "TypeScript",
                    "Markdown",
                    "TOML",
                    "YAML",
                    "HTML",
                    "CSS",
                    "Shell",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
                String::new(),
            ),
            Action::LineEnding => (
                "newline",
                i18n::tr!("ui-editor-line-ending"),
                editor_line_ending_label(state.document.metadata),
                vec!["LF".into(), "CRLF".into(), "CR".into()],
                String::new(),
            ),
            Action::Encoding => (
                "encoding",
                i18n::tr!("ui-editor-encoding"),
                state
                    .document
                    .metadata
                    .encoding
                    .label(state.document.metadata.utf8_bom)
                    .into(),
                vec![
                    "UTF-8".into(),
                    "UTF-8 BOM".into(),
                    "UTF-16LE".into(),
                    "UTF-16BE".into(),
                ],
                String::new(),
            ),
            Action::Indentation => (
                "indent",
                i18n::tr!("ui-editor-indentation"),
                format!(
                    "{}:{}",
                    if self.pages.editor.editor_indent_tabs {
                        "Tab"
                    } else {
                        "Space"
                    },
                    self.pages.editor.editor_indent_width
                ),
                (1..=8)
                    .flat_map(|width| [format!("Space:{width}"), format!("Tab:{width}")])
                    .collect(),
                String::new(),
            ),
            _ => (
                "about",
                i18n::tr!("ui-editor-about"),
                String::new(),
                vec![],
                i18n::tr!("ui-editor-about-text"),
            ),
        };
        let fields = if id == "about" {
            vec![]
        } else {
            vec![ManagementField {
                id: "value".into(),
                label,
                value,
                choices,
                required: false,
                ..Default::default()
            }]
        };
        self.config_form(&format!("edit-{id}"), message, fields);
    }

    pub(in crate::session) fn submit_edit_dialog(
        &mut self,
        action: &str,
        fields: Vec<ManagementField>,
    ) {
        if action == "edit-script-settings" {
            self.submit_script_settings(&fields);
            return;
        }
        let value = fields
            .iter()
            .find(|f| f.id == "value")
            .map_or("", |f| f.value.as_str());
        let platform = platform::native_platform();
        match action {
            "edit-goto" => {
                let mut parts = value.split(':');
                let line = parts
                    .next()
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .filter(|n| *n > 0);
                let column = parts
                    .next()
                    .map_or(Some(1), |v| v.trim().parse::<usize>().ok())
                    .filter(|n| *n > 0);
                if let (Some(line), Some(column), None) = (line, column, parts.next()) {
                    if let Some(offset) = self
                        .app
                        .editor_state()
                        .and_then(|s| s.source_offset(line - 1, column - 1))
                    {
                        self.apply_editor_command(
                            app::editor::EditorCommand::MoveTo {
                                position: app::editor::EditorPosition::Source(offset),
                                extend_selection: false,
                            },
                            platform.as_ref(),
                        );
                    }
                } else {
                    self.config_form(action, i18n::tr!("ui-editor-goto-help"), fields);
                }
            }
            "edit-file" if !value.trim().is_empty() => {
                if let Some(id) = self
                    .editor_document_pages()
                    .into_iter()
                    .find(|id| value.starts_with(&format!("{}:", id.get())))
                {
                    self.activate_editor_document_page(id);
                }
            }
            "edit-indent" => {
                if let Some((kind, width)) = value.split_once(':')
                    && let Ok(width @ 1..=8) = width.parse::<usize>()
                {
                    self.pages.editor.editor_indent_tabs = kind == "Tab";
                    self.pages.editor.editor_indent_width = width;
                }
            }
            "edit-language" => {
                self.pages.editor.editor_language = (value != "Auto").then(|| value.to_owned())
            }
            "edit-newline" => {
                let ending = match value {
                    "CRLF" => app::editor::LineEnding::CrLf,
                    "CR" => app::editor::LineEnding::Cr,
                    _ => app::editor::LineEnding::Lf,
                };
                self.apply_editor_command(
                    app::editor::EditorCommand::NormalizeLineEndings(ending),
                    platform.as_ref(),
                );
            }
            "edit-encoding" => {
                use app::editor::{EditorCommand, TextEncoding};
                let encoding = match value {
                    "UTF-16LE" => TextEncoding::Utf16Le,
                    "UTF-16BE" => TextEncoding::Utf16Be,
                    _ => TextEncoding::Utf8,
                };
                self.apply_editor_command(
                    EditorCommand::SetEncoding {
                        encoding,
                        utf8_bom: value == "UTF-8 BOM",
                    },
                    platform.as_ref(),
                );
            }
            _ => {}
        }
    }
}
