use super::*;

/// Private editor state. Configuration contents never enter recovery drafts or task output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::session) struct ConfigEditorState {
    pub(super) document: Option<ConfigDocument>,
    pub(super) received: Option<ConfigDocument>,
    pub(in crate::session) pending_action: Option<String>,
    draft: Option<ConfigDraft>,
    path: Option<PathBuf>,
    properties: std::collections::BTreeMap<String, String>,
    service: Option<String>,
    scope: String,
    restore_id: Option<String>,
    restore_text: Option<String>,
    restore_removes: bool,
    privileged_read: bool,
    pub(super) compare_path: Option<String>,
    checked_content: Option<String>,
    pub(super) check: ConfigCheck,
    pub(super) history: ManagementSnapshot,
}

fn field(id: &str, value: String) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: management_text("field", id, id),
        value,
        required: true,
        ..Default::default()
    }
}

fn difference(before: &str, after: &str) -> String {
    if before == after {
        return i18n::tr!("config-editor-no-content-change");
    }
    let a = before.split_inclusive('\n').collect::<Vec<_>>();
    let b = after.split_inclusive('\n').collect::<Vec<_>>();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut text = format!(
        "--- {}\n+++ {}\n@@ {} @@\n",
        i18n::tr!("config-editor-original"),
        i18n::tr!("config-editor-candidate"),
        prefix + 1
    );
    for (mark, lines) in [
        ('-', &a[prefix..a.len() - suffix]),
        ('+', &b[prefix..b.len() - suffix]),
    ] {
        for line in lines {
            if text.len() > 64 * 1024 {
                text.push_str(&i18n::tr!("config-editor-diff-truncated"));
                return text;
            }
            text.push(mark);
            text.push_str(line);
            if !line.ends_with('\n') {
                text.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    text
}

impl ShellSession {
    pub(in crate::session) fn is_system_config_path(path: &std::path::Path) -> bool {
        cfg!(target_os = "linux")
            && (path.starts_with("/etc")
                || path.starts_with("/usr/lib/systemd")
                || path.starts_with("/lib/systemd")
                || path.to_string_lossy().contains("/.config/systemd/user/"))
    }
    pub(in crate::session) fn begin_config_save(&mut self, path: PathBuf) {
        if !self.authorize_editor_file(PermissionAction::ReadFile, &path) {
            return;
        }
        let draft = ConfigDraft {
            path: path.clone(),
            content: self.config_text(),
            validator: "auto".into(),
            service: None,
            scope: "system".into(),
            expected_content: None,
        };
        self.editor_config = ConfigEditorState {
            path: Some(path),
            draft: Some(draft),
            scope: "system".into(),
            ..Default::default()
        };
        self.config_operation("read", Default::default());
    }
    pub(in crate::session) fn open_editor_find(&mut self) {
        self.app.dispatch_at(
            app::AppCommand::Editor(app::editor::EditorCommand::SetMode(
                app::editor::EditorMode::Source,
            )),
            Instant::now(),
        );
        self.config_form("find", String::new(), vec![field("text", String::new())]);
    }
    pub(in crate::session) fn management_state_clear_config_form(&mut self) {
        if self
            .management_state
            .form
            .as_ref()
            .is_some_and(|f| matches!(f.purpose, FormPurpose::Configuration(_)))
        {
            self.management_state.form = None;
            self.reset_management_form_view();
        }
    }
    pub(in crate::session) fn config_editor_active(&self) -> bool {
        self.editor_config.path.is_some()
    }
    pub(in crate::session) fn config_editor_form_visible(&self) -> bool {
        self.active_screen() == ShellScreen::Editor
            && self
                .management_state
                .form
                .as_ref()
                .is_some_and(|f| matches!(f.purpose, FormPurpose::Configuration(_)))
    }
    fn config_text(&self) -> String {
        self.app
            .editor_state()
            .map(|s| s.source().to_string())
            .unwrap_or_default()
    }

    fn config_form(&mut self, action: &str, message: String, mut fields: Vec<ManagementField>) {
        if action == "result" && self.editor_config.document.is_some() {
            let mut choices = vec![
                i18n::tr!("config-editor-continue"),
                i18n::tr!("config-editor-history"),
            ];
            if self.editor_config.service.is_some() {
                choices.push(i18n::tr!("config-editor-logs"));
                choices.push(i18n::tr!("config-editor-reload"));
            }
            fields.push(ManagementField {
                id: "next".into(),
                label: i18n::tr!("config-editor-next"),
                value: choices[0].clone(),
                choices,
                ..Default::default()
            });
        }
        self.reset_management_form_view();
        self.management_state.terminal_mode = false;
        self.management_state.form = Some(ManagementEditor {
            title: i18n::tr!(format!("config-editor-{action}")),
            message,
            message_scroll: 0,
            fields,
            selected: 0,
            purpose: FormPurpose::Configuration(action.into()),
        });
        self.editor_open_menu = None;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn config_editor_action(&mut self, action: ui::EditorConfigAction) {
        if self.editor_config.pending_action.is_some() {
            self.editor_message = Some(i18n::msg!("management-operation-running").into());
            return;
        }
        self.editor_open_menu = None;
        match action {
            ui::EditorConfigAction::Menu => {
                let choices = if self.config_editor_active() {
                    vec![
                        "preview",
                        "check",
                        "properties",
                        "history",
                        "reload",
                        "logs",
                        "open",
                    ]
                } else {
                    vec!["open"]
                };
                let choices = choices
                    .into_iter()
                    .map(|key| i18n::tr!(format!("config-editor-{key}")))
                    .collect::<Vec<_>>();
                self.config_form(
                    "menu",
                    String::new(),
                    vec![ManagementField {
                        id: "action".into(),
                        value: choices[0].clone(),
                        choices,
                        ..Default::default()
                    }],
                );
                self.open_management_choice_field(0);
            }
            ui::EditorConfigAction::Open => {
                self.config_form(
                    "open",
                    String::new(),
                    vec![field(
                        "path",
                        self.app
                            .editor_state()
                            .and_then(|s| s.document.path.as_ref())
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                    )],
                );
            }
            ui::EditorConfigAction::Preview => self.config_preview(),
            ui::EditorConfigAction::Check => self.config_operation("check", Default::default()),
            ui::EditorConfigAction::Properties => {
                if let Some(d) = &self.editor_config.document {
                    self.config_form(
                        "properties",
                        i18n::tr!("config-editor-properties-help"),
                        vec![
                            field(
                                "uid",
                                self.editor_config
                                    .properties
                                    .get("uid")
                                    .cloned()
                                    .unwrap_or_else(|| d.uid.to_string()),
                            ),
                            field(
                                "gid",
                                self.editor_config
                                    .properties
                                    .get("gid")
                                    .cloned()
                                    .unwrap_or_else(|| d.gid.to_string()),
                            ),
                            field(
                                "mode",
                                self.editor_config
                                    .properties
                                    .get("mode")
                                    .cloned()
                                    .unwrap_or_else(|| format!("{:04o}", d.mode)),
                            ),
                        ],
                    );
                }
            }
            ui::EditorConfigAction::History => self.config_operation("history", Default::default()),
            ui::EditorConfigAction::Reload => {
                if self
                    .editor_config
                    .document
                    .as_ref()
                    .is_some_and(|d| d.validator == "systemd")
                {
                    self.config_form(
                        "reload",
                        i18n::tr!("config-editor-reload-definitions"),
                        vec![ManagementField {
                            id: "reload_service".into(),
                            label: i18n::tr!("config-editor-reload-service"),
                            value: "false".into(),
                            choices: vec!["false".into(), "true".into()],
                            ..Default::default()
                        }],
                    );
                } else {
                    self.config_operation("reload", Default::default());
                }
            }
            ui::EditorConfigAction::Logs => {
                if let Some(unit) = self.editor_config.service.clone() {
                    let scope = self.editor_config.scope.clone();
                    self.open_service_logs(&unit, &scope);
                } else {
                    self.editor_message = Some(i18n::msg!("config-editor-no-service").into());
                }
            }
        }
    }

    fn detach_edited_recovery(&mut self) {
        if self.editor_config.restore_id.is_some()
            && self.editor_config.restore_text.as_deref() != Some(self.config_text().as_str())
        {
            self.editor_config.restore_id = None;
            self.editor_config.restore_text = None;
            self.editor_config.restore_removes = false;
        }
    }

    pub(super) fn config_save_disabled(&self, fields: &[ManagementField]) -> bool {
        match self.editor_config.check {
            ConfigCheck::Failed(_) => true,
            ConfigCheck::Passed => false,
            _ => !fields
                .iter()
                .any(|f| f.id == "allow_unvalidated" && f.value == "true"),
        }
    }

    fn config_preview(&mut self) {
        self.detach_edited_recovery();
        let Some(d) = &self.editor_config.document else {
            return;
        };
        let mut text = difference(&d.content, &self.config_text());
        if self.editor_config.restore_removes {
            text.push_str(&format!("\n{}", i18n::tr!("config-editor-restore-absence")));
        }
        for (key, original) in [
            ("uid", d.uid.to_string()),
            ("gid", d.gid.to_string()),
            ("mode", format!("{:04o}", d.mode)),
        ] {
            if let Some(value) = self.editor_config.properties.get(key) {
                text.push_str(&format!(
                    "\n{}: {original} → {value}",
                    management_text("field", key, key)
                ));
            }
        }
        self.config_form("preview", text, vec![]);
    }

    pub(super) fn submit_config_form(&mut self, action: &str, fields: Vec<ManagementField>) {
        let values = fields
            .into_iter()
            .map(|f| (f.id, f.value))
            .collect::<std::collections::BTreeMap<_, _>>();
        match action {
            "result" => {
                if let Some(next) = values.get("next") {
                    if next == &i18n::tr!("config-editor-history") {
                        self.config_editor_action(ui::EditorConfigAction::History);
                    } else if next == &i18n::tr!("config-editor-logs") {
                        self.config_editor_action(ui::EditorConfigAction::Logs);
                    } else if next == &i18n::tr!("config-editor-reload") {
                        self.config_editor_action(ui::EditorConfigAction::Reload);
                    }
                }
            }
            "menu" => {
                use ui::EditorConfigAction as A;
                if let Some(value) = values.get("action") {
                    for (key, action) in [
                        ("open", A::Open),
                        ("preview", A::Preview),
                        ("check", A::Check),
                        ("properties", A::Properties),
                        ("history", A::History),
                        ("reload", A::Reload),
                        ("logs", A::Logs),
                    ] {
                        if value == &i18n::tr!(format!("config-editor-{key}")) {
                            self.config_editor_action(action);
                            break;
                        }
                    }
                }
            }
            "find" => {
                if let Some(needle) = values.get("text") {
                    let text = self.config_text();
                    let start = self
                        .app
                        .editor_state()
                        .map(|s| s.cursor.byte_offset)
                        .unwrap_or(0)
                        .min(text.len());
                    let found = text[start..]
                        .find(needle)
                        .map(|p| p + start)
                        .or_else(|| text[..start].find(needle));
                    if let Some(offset) = found {
                        for (byte_offset, extend_selection) in
                            [(offset, false), (offset + needle.len(), true)]
                        {
                            self.app.dispatch_at(
                                app::AppCommand::Editor(app::editor::EditorCommand::MoveTo {
                                    position: app::editor::EditorPosition::Source(byte_offset),
                                    extend_selection,
                                }),
                                Instant::now(),
                            );
                        }
                        self.reveal_source_caret();
                    } else {
                        self.editor_message = Some(i18n::msg!("config-editor-not-found").into());
                    }
                }
            }
            "open" => {
                if let Some(path) = values.get("path") {
                    self.begin_config_editor(
                        PathBuf::from(path),
                        None,
                        None,
                        None,
                        "system".into(),
                    );
                }
            }
            "reload" => self.config_operation("reload", values),
            "properties" => {
                self.editor_config.properties = values;
                self.config_preview();
            }
            "preview" => self.config_operation("check", Default::default()),
            "check" => {
                if self.editor_config.checked_content.as_deref()
                    != Some(self.config_text().as_str())
                {
                    self.config_preview();
                    return;
                }
                if matches!(self.editor_config.check, ConfigCheck::Failed(_)) {
                    return;
                }
                let unchecked = !matches!(self.editor_config.check, ConfigCheck::Passed);
                if unchecked && values.get("allow_unvalidated").map(String::as_str) != Some("true")
                {
                    return;
                }
                let command = if self.editor_config.restore_id.is_some() {
                    "restore"
                } else {
                    "apply"
                };
                self.config_operation(command, values);
            }
            "history" => {
                if let Some(id) = values.get("backup_id") {
                    self.editor_config.restore_id = Some(id.clone());
                    self.config_operation("preview_restore", Default::default());
                }
            }
            "conflict" => self.config_operation("rebase", Default::default()),
            _ => {}
        }
    }

    pub(in crate::session) fn begin_config_editor(
        &mut self,
        path: PathBuf,
        draft: Option<ConfigDraft>,
        validator: Option<String>,
        service: Option<String>,
        scope: String,
    ) {
        if !self.authorize_editor_file(PermissionAction::ReadFile, &path) {
            return;
        }
        if self.app.editor_state().is_some_and(|s| s.is_dirty())
            || self.editor_load_state.is_some()
            || self.editor_save_state.is_some()
        {
            self.notify_toast(i18n::msg!("config-editor-finish-document"));
            return;
        }
        if !path.is_absolute() {
            self.notify_toast(i18n::msg!("config-editor-absolute-path"));
            return;
        }
        self.editor_config = ConfigEditorState {
            path: Some(path),
            draft,
            service,
            scope,
            ..Default::default()
        };
        if let Some(validator) = validator {
            self.editor_config
                .properties
                .insert("validator".into(), validator);
        }
        self.config_operation("read", Default::default());
    }

    fn config_operation(
        &mut self,
        action: &str,
        extra: std::collections::BTreeMap<String, String>,
    ) {
        if matches!(action, "check" | "apply" | "restore") {
            self.detach_edited_recovery();
        }
        let Some(path) = self.editor_config.path.clone() else {
            return;
        };
        if matches!(action, "apply" | "permissions" | "restore")
            && !self.authorize_editor_file(PermissionAction::WriteFile, &path)
        {
            return;
        }
        if self.management_state.operation_job.is_some() {
            self.notify_toast(i18n::msg!("management-operation-running"));
            return;
        }
        let mut values = self.editor_config.properties.clone();
        values.insert("path".into(), path.display().to_string());
        values.insert("scope".into(), self.editor_config.scope.clone());
        if let Some(d) = &self.editor_config.document {
            values.insert("expected_version".into(), d.version.clone());
        }
        if action == "compare" {
            if let Some(path) = &self.editor_config.compare_path {
                values.insert("path".into(), path.clone());
            }
        }
        if let Some(unit) = &self.editor_config.service {
            values.insert("service".into(), unit.clone());
        }
        if let Some(id) = &self.editor_config.restore_id {
            values.insert("backup_id".into(), id.clone());
        }
        if matches!(action, "check" | "apply" | "permissions") {
            values.insert("content".into(), self.config_text());
        }
        values.extend(extra);
        self.editor_config.pending_action = Some(action.into());
        self.editor_config.received = None;
        self.management_state.problem = None;
        let user_reload = action == "reload" && self.editor_config.scope == "user";
        let privileged = !user_reload
            && (!matches!(action, "read" | "rebase" | "compare")
                || self.editor_config.privileged_read);
        self.start_management_operation(
            Some(ManagementCommand {
                kind: ManagementKind::SystemConfig,
                action: if matches!(action, "rebase" | "compare") {
                    "read"
                } else {
                    action
                }
                .into(),
                target: Some(path.display().to_string()),
                values,
                identity: Default::default(),
            }),
            privileged,
            None,
            Some(format!(
                "{}: {}",
                i18n::tr!(format!("config-editor-{action}")),
                path.display()
            )),
        );
        if self.management_state.operation_job.is_none() {
            self.editor_config.pending_action = None;
        }
    }

    fn replace_config_text(&mut self, document: &ConfigDocument, text: String) {
        let model = app::editor::EditorDocument::from_text(
            Some(document.path.clone()),
            app::editor::DocumentKind::PlainText,
            document.content.clone(),
        );
        self.app.dispatch_at(
            app::AppCommand::SetEditorState(Some(EditorState::from_document(model))),
            Instant::now(),
        );
        if text != document.content {
            self.app.dispatch_at(
                app::AppCommand::Editor(app::editor::EditorCommand::SelectAll),
                Instant::now(),
            );
            self.app.dispatch_at(
                app::AppCommand::Editor(app::editor::EditorCommand::Paste(text)),
                Instant::now(),
            );
        }
        self.editor_read_session = None;
        self.editor_fingerprint = None;
        self.editor_open_menu = None;
        self.editor_focus = ui::EditorFocus::Canvas;
        self.editor_settings_dialog = None;
        self.rebuild_editor_rich_render_cache();
        if self.active_screen() != ShellScreen::Editor {
            self.screen_stack.push(ShellScreen::Editor);
        }
        self.focused_component = ShellComponent::Editor;
        self.refresh_hit_map();
    }

    pub(super) fn finish_config_operation(&mut self) {
        let Some(action) = self.editor_config.pending_action.take() else {
            return;
        };
        self.close_auto_admin();
        if let Some(problem) = self.management_state.problem.clone() {
            if problem.exit_code == 3
                && matches!(action.as_str(), "read" | "rebase" | "compare")
                && !self.editor_config.privileged_read
            {
                self.editor_config.privileged_read = true;
                self.management_state.problem = None;
                self.config_operation(&action, Default::default());
                return;
            }
            self.editor_message = Some(i18n::tr!(problem.summary_key.clone()).into());
            if problem.exit_code == 5 {
                self.config_form("conflict", i18n::tr!("config-editor-conflict-help"), vec![]);
            } else {
                self.config_form(
                    "result",
                    format!(
                        "{}\n{}",
                        i18n::tr!(problem.summary_key.clone()),
                        problem.detail
                    ),
                    vec![],
                );
            }
            if self.editor_config.document.is_none() {
                self.editor_config.path = None;
            }
            return;
        }
        if action == "history" {
            let ids = self
                .editor_config
                .history
                .rows
                .iter()
                .map(|r| r.id.clone())
                .collect::<Vec<_>>();
            if ids.is_empty() {
                self.config_form("result", i18n::tr!("config-editor-no-history"), vec![]);
            } else {
                self.config_form(
                    "history",
                    String::new(),
                    vec![ManagementField {
                        id: "backup_id".into(),
                        label: i18n::tr!("config-editor-history"),
                        value: ids[0].clone(),
                        choices: ids,
                        required: true,
                        ..Default::default()
                    }],
                );
            }
            return;
        }
        let Some(document) = self.editor_config.received.take() else {
            self.editor_message = Some(self.management_state.status.clone().into());
            if self.editor_config.document.is_none() {
                self.editor_config.path = None;
            }
            if action == "reload" {
                self.config_form("result", self.management_state.status.clone(), vec![]);
            }
            return;
        };
        match action.as_str() {
            "read" | "rebase" => {
                self.close_auto_admin();
                let mut text = if action == "rebase" {
                    self.config_text()
                } else {
                    document.content.clone()
                };
                if let Some(draft) = self.editor_config.draft.take() {
                    if draft
                        .expected_content
                        .as_ref()
                        .is_some_and(|s| s != &document.content)
                    {
                        self.editor_config.document = Some(document.clone());
                        self.replace_config_text(&document, document.content.clone());
                        self.config_form("result", i18n::tr!("config-editor-draft-stale"), vec![]);
                        return;
                    }
                    text = draft.content;
                    self.editor_config
                        .properties
                        .insert("validator".into(), draft.validator);
                    self.editor_config.service = draft.service;
                    self.editor_config.scope = draft.scope;
                }
                self.editor_config.document = Some(document.clone());
                if self.editor_config.service.is_none() {
                    if document.validator == "sshd" {
                        self.editor_config.service =
                            Some(
                                if std::path::Path::new("/usr/lib/systemd/system/sshd.service")
                                    .exists()
                                {
                                    "sshd.service"
                                } else {
                                    "ssh.service"
                                }
                                .into(),
                            );
                    } else if document.validator == "systemd" {
                        self.editor_config.service = document
                            .path
                            .components()
                            .filter_map(|c| c.as_os_str().to_str())
                            .find_map(|s| {
                                s.strip_suffix(".service.d").map(|n| format!("{n}.service"))
                            })
                            .or_else(|| {
                                document
                                    .path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .filter(|n| n.ends_with(".service"))
                                    .map(String::from)
                            });
                        if document.path.to_string_lossy().contains("/systemd/user/") {
                            self.editor_config.scope = "user".into();
                        }
                    }
                }
                self.replace_config_text(&document, text);
                self.editor_message = Some(i18n::msg!("config-editor-loaded").into());
                if action == "read" && self.editor_config.compare_path.is_some() {
                    self.config_operation("compare", Default::default());
                }
                if action == "rebase" {
                    self.config_preview();
                }
            }
            "compare" => {
                self.close_auto_admin();
                if let Some(original) = self.editor_config.document.clone() {
                    self.replace_config_text(&original, document.content);
                    self.config_preview();
                }
            }
            "preview_restore" => {
                self.editor_config.restore_text = Some(document.content.clone());
                self.editor_config.restore_removes = !document.existed;
                self.editor_config.properties.extend([
                    ("uid".into(), document.uid.to_string()),
                    ("gid".into(), document.gid.to_string()),
                    ("mode".into(), format!("{:04o}", document.mode)),
                ]);
                if let Some(original) = self.editor_config.document.clone() {
                    self.replace_config_text(&original, document.content);
                    self.config_preview();
                }
            }
            "check" => {
                self.editor_config.checked_content = Some(document.content);
                self.editor_config.check = document.check.clone();
                let (message, fields) = match document.check {
                    ConfigCheck::Passed => (i18n::tr!("config-editor-check-passed"), vec![]),
                    ConfigCheck::Failed(reason) => (
                        format!("{}\n{reason}", i18n::tr!("config-editor-check-failed")),
                        vec![],
                    ),
                    other => (
                        format!(
                            "{}\n{}",
                            i18n::tr!("config-editor-unchecked"),
                            if let ConfigCheck::Unavailable(s) = other {
                                s
                            } else {
                                String::new()
                            }
                        ),
                        vec![ManagementField {
                            id: "allow_unvalidated".into(),
                            label: i18n::tr!("config-editor-allow-unvalidated"),
                            value: "false".into(),
                            choices: vec!["false".into(), "true".into()],
                            ..Default::default()
                        }],
                    ),
                };
                let preview = self
                    .editor_config
                    .document
                    .as_ref()
                    .map(|d| difference(&d.content, &self.config_text()))
                    .unwrap_or_default();
                self.config_form("check", format!("{message}\n\n{preview}"), fields);
            }
            "apply" | "permissions" | "restore" => {
                let old = self.editor_config.document.as_ref();
                let content_changed = old.is_none_or(|old| {
                    old.content != document.content || old.existed != document.existed
                });
                let properties_changed = old.is_none_or(|old| {
                    (old.uid, old.gid, old.mode) != (document.uid, document.gid, document.mode)
                });
                let state = |changed| {
                    if changed {
                        i18n::tr!("config-editor-applied")
                    } else {
                        i18n::tr!("config-editor-unchanged")
                    }
                };
                let report = format!(
                    "{}: {}\n{}: {}\n{}",
                    i18n::tr!("config-editor-content"),
                    state(content_changed),
                    i18n::tr!("config-editor-properties"),
                    state(properties_changed),
                    i18n::tr!("config-editor-saved")
                );
                self.replace_config_text(&document, document.content.clone());
                self.editor_config.document = Some(document);
                self.editor_config.restore_id = None;
                self.editor_config.restore_text = None;
                self.editor_config.restore_removes = false;
                self.editor_config
                    .properties
                    .retain(|k, _| k == "validator");
                self.editor_message = Some(i18n::msg!("config-editor-saved").into());
                self.config_form("result", report, vec![]);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> ShellSession {
        let mut session = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (100, 36),
            ShellHomeMode::User,
        );
        session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
        session.screen_stack.push(ShellScreen::Management);
        session.management_state.kind = Some(ManagementKind::Services);
        session.management_state.query = Some(ManagementQuery::new(ManagementKind::Services));
        session
    }
    fn document(content: &str) -> ConfigDocument {
        ConfigDocument {
            path: PathBuf::from("/etc/test.conf"),
            content: content.into(),
            version: "version-1".into(),
            existed: true,
            uid: 0,
            gid: 0,
            mode: 0o640,
            validator: "none".into(),
            check: ConfigCheck::NotChecked,
            backup_id: None,
        }
    }
    fn loaded() -> ShellSession {
        let mut s = session();
        s.editor_config.path = Some(PathBuf::from("/etc/test.conf"));
        s.editor_config.pending_action = Some("read".into());
        s.editor_config.received = Some(document("original\n"));
        s.finish_config_operation();
        s
    }
    #[test]
    fn diff_keeps_newline_changes_and_bounds_output() {
        assert!(difference("a\n", "a").contains("No newline"));
        assert!(difference("same\nold\nend\n", "same\nnew\nend\n").contains("-old\n+new"));
        assert!(difference("", &"a\n".repeat(200_000)).len() < 70_000);
    }
    #[test]
    fn private_configuration_uses_editor_without_ordinary_recovery_and_returns_to_services() {
        let mut s = loaded();
        assert_eq!(s.active_screen(), ShellScreen::Editor);
        assert!(s.to_editor_view_model().configuration);
        assert_eq!(s.config_text(), "original\n");
        assert!(s.editor_recovery_context().is_none());
        s.finish_editor_close(false);
        assert_eq!(s.active_screen(), ShellScreen::Management);
        assert!(!s.config_editor_active());
    }
    #[test]
    fn failed_check_blocks_save_and_keeps_candidate_text() {
        let mut s = loaded();
        let mut d = document("invalid\n");
        d.check = ConfigCheck::Failed("bad line".into());
        s.replace_config_text(&document("original\n"), "invalid\n".into());
        s.editor_config.pending_action = Some("check".into());
        s.editor_config.received = Some(d);
        s.finish_config_operation();
        assert!(s.to_management_view_model().form.unwrap().submit_disabled);
        s.submit_management_form();
        assert!(s.management_state.operation_job.is_none());
        assert_eq!(s.config_text(), "invalid\n");
        assert!(s.app.editor_state().unwrap().is_dirty());
    }
    #[test]
    fn failed_reload_keeps_saved_content_and_offers_recovery_and_logs() {
        let mut s = loaded();
        s.editor_config.service = Some("sshd.service".into());
        s.editor_config.pending_action = Some("reload".into());
        s.management_state.problem = Some(problem::OperationProblem::from_error(
            &ManagementError::Failed("reload failed".into()),
        ));
        s.finish_config_operation();
        assert_eq!(s.config_text(), "original\n");
        let choices = &s.management_state.form.as_ref().unwrap().fields[0].choices;
        assert!(choices.contains(&i18n::tr!("config-editor-history")));
        assert!(choices.contains(&i18n::tr!("config-editor-logs")));
    }
    #[test]
    fn comparison_keeps_original_for_preview_and_marks_editor_dirty() {
        let mut s = loaded();
        s.editor_config.pending_action = Some("compare".into());
        s.editor_config.received = Some(document("package version\n"));
        s.finish_config_operation();
        assert_eq!(
            s.editor_config.document.as_ref().unwrap().content,
            "original\n"
        );
        assert_eq!(s.config_text(), "package version\n");
        assert!(
            s.management_state
                .form
                .as_ref()
                .unwrap()
                .message
                .contains("-original")
        );
        assert!(s.app.editor_state().unwrap().is_dirty());
    }
    #[test]
    fn find_selects_text_and_config_menu_is_keyboard_accessible() {
        let mut s = loaded();
        s.open_editor_find();
        s.submit_config_form("find", vec![field("text", "original".into())]);
        assert_eq!(s.app.editor_state().unwrap().selection.unwrap().focus, 8);
        s.config_editor_action(ui::EditorConfigAction::Menu);
        assert_eq!(s.management_state.choice_field, Some(0));
        assert!(s.to_editor_view_model().config_form.is_some());
    }

    #[test]
    fn edited_recovery_becomes_a_normal_candidate_and_unchecked_save_needs_consent() {
        let mut s = loaded();
        s.editor_config.restore_id = Some("1-2".into());
        s.editor_config.pending_action = Some("preview_restore".into());
        s.editor_config.received = Some(document("recovered\n"));
        s.finish_config_operation();
        assert_eq!(s.editor_config.restore_id.as_deref(), Some("1-2"));
        s.replace_config_text(&document("original\n"), "edited recovery\n".into());
        s.config_preview();
        assert!(s.editor_config.restore_id.is_none());
        assert!(!s.editor_config.restore_removes);
        assert_eq!(s.config_text(), "edited recovery\n");
        s.editor_config.check = ConfigCheck::NotChecked;
        assert!(s.config_save_disabled(&[]));
        assert!(!s.config_save_disabled(&[field("allow_unvalidated", "true".into())]));
        s.editor_config.check = ConfigCheck::Failed("invalid".into());
        assert!(s.config_save_disabled(&[field("allow_unvalidated", "true".into())]));
    }
}
