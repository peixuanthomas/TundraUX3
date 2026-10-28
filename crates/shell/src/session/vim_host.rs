//! Adapts the real Vim process to document instances and the existing save pipeline.
use super::*;
use vim_runtime::{
    BufferInput, LineEnding, SaveRequest, SaveResult, VimBuffer, VimConfig, VimHost,
};
mod install;

pub(super) struct EditorVimHost {
    watchdog: AppWatchdog,
    host: Option<VimHost>,
    installer: Option<install::VimInstaller>,
    pages: HashMap<u32, PageInstanceId>,
    imported: HashSet<PageInstanceId>,
    policies: HashMap<PageInstanceId, (bool, bool)>,
    mirrored: HashMap<u32, MirrorRevision>,
    saves: HashMap<u64, PendingSave>,
    export: Option<u64>,
    script_export_owner: Option<PageInstanceId>,
    import: Option<u64>,
    pending_input: Vec<InputEvent>,
    suspended: bool,
    selected: Option<PageInstanceId>,
    started_at: Instant,
    window_focused: bool,
    cell_size: (u16, u16),
}

#[derive(PartialEq, Eq)]
struct MirrorRevision {
    tick: i64,
    modified: bool,
    path: Option<PathBuf>,
    cursor: (usize, usize),
    encoding: String,
    bom: bool,
    ending: LineEnding,
    final_newline: bool,
}

struct PendingSave {
    page: PageInstanceId,
    configuration: bool,
    saw_form: bool,
}

impl EditorVimHost {
    pub(super) fn new(watchdog: AppWatchdog) -> Self {
        Self {
            watchdog,
            host: None,
            installer: None,
            pages: HashMap::new(),
            imported: HashSet::new(),
            policies: HashMap::new(),
            mirrored: HashMap::new(),
            saves: HashMap::new(),
            export: None,
            script_export_owner: None,
            import: None,
            pending_input: Vec::new(),
            suspended: false,
            selected: None,
            started_at: Instant::now(),
            window_focused: true,
            cell_size: (8, 16),
        }
    }

    pub(super) fn running(&self) -> bool {
        self.installer.is_some() || self.host.as_ref().is_some_and(VimHost::is_running)
    }

    pub(super) fn set_cell_size(&mut self, width: u16, height: u16) {
        self.cell_size = (width.max(1), height.max(1));
    }

    pub(super) fn poll(&mut self, state: &mut ShellSession, platform: &dyn Platform) -> bool {
        let before = state.editor_vim_terminal.clone();
        let before_install = state.editor_vim_install.clone();
        if let Err(error) = self.poll_inner(state, platform) {
            self.fail(state, error);
        }
        self.sync_focus(state);
        before != state.editor_vim_terminal || before_install != state.editor_vim_install
    }

    pub(super) fn observe_window_focus(&mut self, state: &ShellSession, focused: bool) {
        self.window_focused = focused;
        self.sync_focus(state);
    }

    fn sync_focus(&self, state: &ShellSession) {
        if let Some(host) = &self.host
            && host.is_running()
        {
            let _ = host
                .send_focus(self.window_focused && !self.suspended && self.input_visible(state));
        }
    }

    fn poll_inner(&mut self, state: &mut ShellSession, platform: &dyn Platform) -> io::Result<()> {
        if self.poll_install(state)? {
            return Ok(());
        }
        let visible = state.content_screen() == ShellScreen::Editor;
        let enabled = state.app.storage_config().editor.vim_mode_enabled
            && !state.pages.editor.script.vim_suspended;
        if visible
            && enabled
            && self.host.is_none()
            && state.pages.editor.editor_load_state.is_none()
            && state.pages.editor.editor_save_state.is_none()
        {
            let (program, runtime) = match resolve_vim(platform) {
                Ok(resolved) => resolved,
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        && std::env::var_os("TUNDRA_VIM").is_none() =>
                {
                    self.start_install(state, platform)?;
                    return Ok(());
                }
                Err(error) => return Err(error),
            };
            let mut config = VimConfig::new(program);
            config.runtime_path = runtime;
            config.user_config = std::env::var_os("TUNDRA_VIM_CLEAN").is_none();
            let area = vim_area(state);
            config.columns = area.width.max(1);
            config.rows = area.height.max(1);
            for id in state.editor_document_pages().into_iter().rev() {
                if let Some(buffer) = input_for_page(state, id) {
                    self.policies
                        .insert(id, (buffer.read_only, buffer.protected));
                    config.initial_buffers.push(buffer);
                    self.imported.insert(id);
                }
            }
            self.host = Some(VimHost::spawn(
                config,
                &self.watchdog.task_group("vim-editor"),
            )?);
            self.host
                .as_ref()
                .unwrap()
                .set_cell_size(self.cell_size.0, self.cell_size.1)?;
            self.import = Some(
                self.host
                    .as_mut()
                    .unwrap()
                    .activate_buffer(state.content_page_id().get())?,
            );
            self.selected = Some(state.content_page_id());
            self.suspended = false;
            self.started_at = Instant::now();
        }
        if self.host.is_none() {
            return Ok(());
        }
        self.host
            .as_ref()
            .unwrap()
            .set_cell_size(self.cell_size.0, self.cell_size.1)?;
        if visible && enabled && self.suspended {
            let closed = self
                .pages
                .iter()
                .filter_map(|(&number, &page)| {
                    state.page_screen(page).is_none().then_some((number, page))
                })
                .collect::<Vec<_>>();
            for (number, page) in closed {
                self.host.as_mut().unwrap().remove_buffer(number)?;
                self.pages.remove(&number);
                self.imported.remove(&page);
                self.policies.remove(&page);
                self.mirrored.remove(&number);
            }
            for id in state.editor_document_pages().into_iter().rev() {
                if let Some(buffer) = input_for_page(state, id) {
                    self.policies
                        .insert(id, (buffer.read_only, buffer.protected));
                    self.host.as_mut().unwrap().open_buffer(buffer)?;
                    self.imported.insert(id);
                }
            }
            self.import = Some(
                self.host
                    .as_mut()
                    .unwrap()
                    .activate_buffer(state.content_page_id().get())?,
            );
            self.selected = Some(state.content_page_id());
            self.suspended = false;
            self.mirrored.clear();
        }
        if visible && enabled && !self.suspended {
            let mut imported = false;
            for id in state.editor_document_pages().into_iter().rev() {
                if !self.imported.contains(&id) {
                    if let Some(buffer) = input_for_page(state, id) {
                        self.policies
                            .insert(id, (buffer.read_only, buffer.protected));
                        self.host.as_mut().unwrap().open_buffer(buffer)?;
                        self.imported.insert(id);
                        imported = true;
                    }
                }
            }
            let area = vim_area(state);
            self.host
                .as_ref()
                .unwrap()
                .resize(area.width.max(1), area.height.max(1))?;
            if imported || self.selected != Some(state.content_page_id()) {
                self.import = Some(
                    self.host
                        .as_mut()
                        .unwrap()
                        .activate_buffer(state.content_page_id().get())?,
                );
                self.selected = Some(state.content_page_id());
            }
        }
        if self.export.is_none() {
            let script_owner = state.editor_vim_script_export.take().or_else(|| {
                state.pages.editor.ids().into_iter().find(|id| {
                    state
                        .pages
                        .editor
                        .get(*id)
                        .is_some_and(|page| page.script.vim_pending)
                })
            });
            if let Some(owner) = script_owner {
                self.script_export_owner = Some(owner);
                self.export = Some(self.host.as_mut().unwrap().request_export()?);
            } else if std::mem::take(&mut state.editor_vim_toggle_requested) {
                self.script_export_owner = None;
                self.export = Some(self.host.as_mut().unwrap().request_export()?);
            }
        }
        if std::mem::take(&mut state.editor_vim_close_requested) {
            self.host.as_mut().unwrap().request_close(false)?;
        }
        self.finish_saves(state)?;
        let poll = self.host.as_mut().unwrap().poll()?;
        self.synchronize_policies(state, &poll.state.buffers)?;
        if self.import.is_some_and(|id| poll.state.last_command >= id) {
            self.import = None;
            for input in std::mem::take(&mut self.pending_input) {
                self.handle_input(state, &input)?;
            }
        }
        if !poll.state.ready
            && poll.exit.is_none()
            && self.started_at.elapsed() > Duration::from_secs(10)
        {
            return Err(io::Error::other(i18n::tr!("shell-vim-startup-failed")));
        }
        if !self.suspended {
            if self.import.is_none() {
                self.synchronize(state, &poll.state.buffers);
                for number in &poll.state.removed_buffers {
                    if let Some(page) = self.pages.remove(number) {
                        self.imported.remove(&page);
                        self.policies.remove(&page);
                        self.mirrored.remove(number);
                        state.request_page_close_after_editor_check(page, true);
                    }
                }
                if let Some(&page) = self.pages.get(&poll.state.active_buffer)
                    && visible
                    && enabled
                    && state.editor_document_pages().contains(&page)
                    && state.content_page_id() != page
                {
                    state.activate_editor_document_page(page);
                    self.selected = Some(page);
                }
            }
            for request in poll.save_requests {
                self.begin_save(state, platform, request)?;
            }
        } else {
            for request in poll.save_requests {
                self.host
                    .as_mut()
                    .unwrap()
                    .respond_save(request.id, Err(i18n::tr!("shell-vim-save-suspended")))?;
            }
        }
        if let Some(exit) = poll.exit {
            if !exit.success || !poll.state.exiting {
                return Err(io::Error::other(i18n::tr!(
                    "shell-vim-exited-unexpectedly",
                    code = exit.code
                )));
            }
            let ids = if self.suspended {
                HashSet::new()
            } else {
                self.pages.values().copied().collect::<HashSet<_>>()
            };
            state.editor_vim_terminal = None;
            for id in ids {
                state.request_page_close_after_editor_check(id, true);
            }
            self.host = None;
            self.pages.clear();
            self.imported.clear();
            self.policies.clear();
            self.mirrored.clear();
            self.saves.clear();
            self.selected = None;
            self.export = None;
            self.script_export_owner = None;
            state.editor_vim_script_export = None;
            self.import = None;
            self.pending_input.clear();
            return Ok(());
        }
        if self.export.is_some_and(|id| poll.state.last_command >= id) && self.saves.is_empty() {
            self.export = None;
            if let Some(buffer) = poll
                .state
                .buffers
                .iter()
                .find(|buffer| !gui_encoding(&buffer.encoding))
            {
                self.fail_script_export(
                    state,
                    i18n::msg!(
                        "shell-vim-unsupported-editor-encoding",
                        encoding = buffer.encoding.clone()
                    )
                    .into(),
                );
                state.notify_alert_with_tone(
                    i18n::msg!(
                        "shell-vim-unsupported-editor-encoding",
                        encoding = buffer.encoding.clone()
                    ),
                    ui::NotificationTone::Error,
                );
                return Ok(());
            }
            if poll.state.buffers.iter().any(|buffer| {
                buffer.encoding.to_ascii_lowercase().starts_with("utf-16") && !buffer.bom
            }) {
                self.fail_script_export(
                    state,
                    i18n::msg!("shell-vim-unsupported-editor-bom").into(),
                );
                state.notify_alert_with_tone(
                    i18n::msg!("shell-vim-unsupported-editor-bom"),
                    ui::NotificationTone::Error,
                );
                return Ok(());
            }
            self.suspended = true;
            if let Some(owner) = self.script_export_owner.take() {
                state.with_page_context(owner, |state| {
                    state.pages.editor.script.vim_pending = false;
                    state.pages.editor.script.vim_suspended = true;
                });
                state.editor_vim_terminal = None;
                state.refresh_hit_map();
            } else if state.set_editor_vim_enabled(false) {
                state.editor_vim_terminal = None;
                state.refresh_hit_map();
            } else {
                self.suspended = false;
            }
            return Ok(());
        }
        if visible && enabled && !self.suspended {
            state.editor_vim_terminal = Some(Arc::new(terminal_runtime::snapshot::to_ui_snapshot(
                &poll.terminal,
            )));
            state.refresh_hit_map();
        } else {
            state.editor_vim_terminal = None;
        }
        Ok(())
    }

    fn synchronize_policies(
        &mut self,
        state: &mut ShellSession,
        buffers: &[VimBuffer],
    ) -> io::Result<()> {
        for buffer in buffers {
            let Some(&page) = self.pages.get(&buffer.number) else {
                continue;
            };
            let Some(policy) = state
                .with_page_context(page, |state| {
                    state
                        .app
                        .editor_state()
                        .map(|editor| (editor.is_read_only(), state.config_editor_active()))
                })
                .flatten()
            else {
                continue;
            };
            if self
                .policies
                .get(&page)
                .is_some_and(|previous| *previous != policy)
            {
                self.import = Some(self.host.as_mut().unwrap().set_buffer_policy(
                    buffer.number,
                    policy.0,
                    policy.1,
                )?);
            }
            self.policies.insert(page, policy);
        }
        Ok(())
    }

    fn fail_script_export(&mut self, state: &mut ShellSession, error: i18n::LocalizedText) {
        if let Some(owner) = self.script_export_owner.take() {
            state.with_page_context(owner, |state| {
                state.pages.editor.script.vim_pending = false;
                state.fail_script(error);
            });
        }
    }

    fn synchronize(&mut self, state: &mut ShellSession, buffers: &[VimBuffer]) {
        let original = state.content_page_id();
        for buffer in buffers {
            if !self.pages.contains_key(&buffer.number) {
                let existing = buffer.host_id.and_then(|host| {
                    state
                        .pages
                        .editor
                        .ids()
                        .into_iter()
                        .find(|id| id.get() == host)
                });
                if existing.is_none() && state.content_screen() != ShellScreen::Editor {
                    continue;
                }
                let page = if let Some(id) = existing {
                    id
                } else {
                    state.new_edit_document();
                    let id = state.content_page_id();
                    self.imported.insert(id);
                    if let Some(path) = &buffer.path {
                        let fingerprint = platform::document_fingerprint(path).ok();
                        state.pages.editor.editor_fingerprint = fingerprint;
                    }
                    id
                };
                if buffer.host_id.is_none() {
                    let host = self.host.as_mut().unwrap();
                    let _ = host.bind_buffer(buffer.number, page.get());
                    let protected = buffer
                        .path
                        .as_deref()
                        .is_some_and(ShellSession::is_system_config_path);
                    let _ = host.set_buffer_policy(buffer.number, buffer.read_only, protected);
                }
                self.pages.insert(buffer.number, page);
            }
            let page = self.pages[&buffer.number];
            if state.page_screen(page).is_none()
                || self.saves.values().any(|save| save.page == page)
            {
                continue;
            }
            let identity = MirrorRevision {
                tick: buffer.changed_tick,
                modified: buffer.modified,
                path: buffer.path.clone(),
                cursor: (buffer.cursor.line, buffer.cursor.column),
                encoding: buffer.encoding.clone(),
                bom: buffer.bom,
                ending: buffer.line_ending,
                final_newline: buffer.end_of_line,
            };
            if self.mirrored.get(&buffer.number) == Some(&identity) {
                continue;
            }
            state.with_page_context(page, |session| mirror_buffer(session, buffer));
            self.mirrored.insert(buffer.number, identity);
        }
        if state.editor_document_pages().contains(&original) {
            state.activate_editor_document_page(original);
        }
    }

    fn begin_save(
        &mut self,
        state: &mut ShellSession,
        platform: &dyn Platform,
        request: SaveRequest,
    ) -> io::Result<()> {
        let Some(&page) = self.pages.get(&request.buffer_number) else {
            return self
                .host
                .as_mut()
                .unwrap()
                .respond_save(request.id, Err(i18n::tr!("shell-vim-buffer-unavailable")));
        };
        let Some(path) = request.path.clone() else {
            return self
                .host
                .as_mut()
                .unwrap()
                .respond_save(request.id, Err(i18n::tr!("shell-vim-file-name-required")));
        };
        let protected = ShellSession::is_system_config_path(&path)
            || state
                .with_page_context(page, |session| {
                    session.config_editor_active()
                        && session.pages.editor.editor_config.path.as_ref() == Some(&path)
                })
                .unwrap_or(false);
        let mut bytes = request.bytes;
        let mut expected = request
            .force
            .then(|| platform::document_fingerprint(&path).ok())
            .flatten();
        if request.append && path.exists() {
            match platform::read_document_bytes(&path) {
                Ok(document) => {
                    expected = Some(document.fingerprint);
                    let mut existing = document.bytes;
                    existing.extend_from_slice(&bytes);
                    bytes = existing;
                }
                Err(error) => {
                    return self
                        .host
                        .as_mut()
                        .unwrap()
                        .respond_save(request.id, Err(error.to_string()));
                }
            }
        }
        let submitted = state
            .with_page_context(page, |session| {
                session.pages.editor.editor_vim_save_result = None;
                session.pages.editor.editor_vim_write_pending = true;
                session.pages.editor.editor_vim_expected =
                    (request.force || request.append).then_some(expected);
                if session
                    .app
                    .editor_state()
                    .is_none_or(EditorState::is_read_only)
                {
                    return false;
                }
                if protected && (!request.whole_buffer || request.append) {
                    session.pages.editor.editor_message =
                        Some(i18n::msg!("shell-vim-config-whole-write").into());
                    return false;
                }
                if protected
                    && (bytes.as_slice() != request.text.as_bytes()
                        || session
                            .app
                            .editor_state()
                            .is_none_or(|editor| editor.source().as_ref() != request.text))
                {
                    session.pages.editor.editor_message =
                        Some(i18n::msg!("shell-vim-config-text-write").into());
                    return false;
                }
                let snapshot = app::editor::SaveSnapshot::external_bytes(
                    request.changed_tick.max(0) as u64,
                    bytes,
                );
                session.save_editor_document(path.clone(), snapshot, platform)
                    || session.config_editor_form_visible()
                    || session.pages.editor.editor_config.pending_action.is_some()
            })
            .unwrap_or(false);
        if submitted {
            self.saves.insert(
                request.id,
                PendingSave {
                    page,
                    configuration: protected,
                    saw_form: false,
                },
            );
        } else {
            let reason = state
                .with_page_context(page, |session| {
                    session.pages.editor.editor_vim_write_pending = false;
                    session.pages.editor.editor_vim_expected = None;
                    session
                        .pages
                        .editor
                        .editor_message
                        .as_ref()
                        .map(|s| s.render_current())
                })
                .flatten()
                .unwrap_or_else(|| i18n::tr!("shell-vim-save-failed"));
            self.host
                .as_mut()
                .unwrap()
                .respond_save(request.id, Err(reason))?;
        }
        Ok(())
    }

    fn finish_saves(&mut self, state: &mut ShellSession) -> io::Result<()> {
        let mut finished = Vec::new();
        for (&id, save) in &mut self.saves {
            let result = state
                .with_page_context(save.page, |session| {
                    let form = session.config_editor_form_visible();
                    if form {
                        save.saw_form = true;
                    }
                    let result = session
                        .pages
                        .editor
                        .editor_vim_save_result
                        .take()
                        .or_else(|| {
                            (save.configuration
                                && save.saw_form
                                && !form
                                && session.pages.editor.editor_config.pending_action.is_none())
                            .then(|| Err(i18n::tr!("shell-vim-save-cancelled")))
                        });
                    if result.is_some() {
                        session.pages.editor.editor_vim_write_pending = false;
                        session.pages.editor.editor_vim_expected = None;
                    }
                    result
                })
                .flatten();
            if let Some(result) = result {
                finished.push((id, result));
            }
        }
        for (id, result) in finished {
            self.host
                .as_mut()
                .unwrap()
                .respond_save(id, result.map(|path| SaveResult { path }))?;
            self.saves.remove(&id);
        }
        Ok(())
    }

    fn fail(&mut self, state: &mut ShellSession, error: io::Error) {
        let script_exporting =
            self.script_export_owner.is_some() || state.editor_vim_script_export.is_some();
        let message: i18n::LocalizedText =
            i18n::msg!("shell-vim-runtime-error", reason = error.to_string()).into();
        self.fail_script_export(state, message.clone());
        for id in state.pages.editor.ids() {
            if state
                .pages
                .editor
                .get(id)
                .is_some_and(|page| page.script.vim_pending)
            {
                state.with_page_context(id, |state| {
                    state.pages.editor.script.vim_pending = false;
                    state.fail_script(message.clone());
                });
            }
        }
        self.host = None;
        state.editor_vim_terminal = None;
        state.editor_vim_toggle_requested = false;
        state.editor_vim_script_export = None;
        state.editor_vim_close_requested = false;
        if !script_exporting {
            state.set_editor_vim_enabled(false);
        }
        self.pages.clear();
        self.imported.clear();
        self.policies.clear();
        self.mirrored.clear();
        self.saves.clear();
        self.export = None;
        self.script_export_owner = None;
        self.import = None;
        self.pending_input.clear();
        self.selected = None;
        state.report_editor_error(i18n::msg!(
            "shell-vim-runtime-error",
            reason = error.to_string()
        ));
    }

    fn input_visible(&self, state: &ShellSession) -> bool {
        state.active_screen() == ShellScreen::Editor
            && (!state.pages.editor.script.enabled
                || state.pages.editor.script.focus == ui::ScriptPane::Source)
            && state.editor_vim_terminal.is_some()
            && state.interactive_overlays().is_empty()
            && !state.config_editor_form_visible()
            && !state.auto_admin_visible()
    }

    pub(super) fn captures_input(&self, state: &ShellSession, input: &InputEvent) -> bool {
        if !self.input_visible(state) {
            return false;
        }
        match input {
            InputEvent::Key(key) => {
                if state.script_host_key(key) {
                    return false;
                }
                !(key.key == InputKey::F(8)
                    && !key.has_non_shift_modifier()
                    && !key.modifiers.shift)
            }
            InputEvent::Paste(_) => true,
            InputEvent::Mouse(mouse) => {
                vim_area(state).contains((mouse.position.column, mouse.position.row).into())
            }
            _ => false,
        }
    }

    pub(super) fn handle_input(
        &mut self,
        state: &ShellSession,
        input: &InputEvent,
    ) -> io::Result<()> {
        if self.import.is_some() {
            self.pending_input.push(input.clone());
            return Ok(());
        }
        let Some(host) = &self.host else {
            return Ok(());
        };
        match input {
            InputEvent::Key(key) => host.send_key(key),
            InputEvent::Paste(text) => host.paste(text),
            InputEvent::Mouse(mouse) => host.send_mouse(mouse, vim_area(state)).map(|_| ()),
            _ => Ok(()),
        }
    }

    pub(super) fn stop(&mut self, state: &mut ShellSession) {
        if !self.suspended
            && let Some(host) = &mut self.host
        {
            if let Ok(command) = host.request_export() {
                let deadline = Instant::now() + Duration::from_secs(2);
                while Instant::now() < deadline {
                    match host.poll() {
                        Ok(poll) if poll.state.last_command >= command => {
                            let buffers = poll.state.buffers.clone();
                            self.synchronize(state, &buffers);
                            break;
                        }
                        Err(_) => break,
                        _ => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
            }
        }
        let _ = state.persist_all_editor_recovery_now(Instant::now());
        self.host = None;
        state.editor_vim_terminal = None;
    }
}

fn resolve_vim(platform: &dyn Platform) -> io::Result<(PathBuf, Option<PathBuf>)> {
    let bundled = std::env::current_exe()?
        .parent()
        .ok_or_else(|| io::Error::other("executable directory unavailable"))?
        .join("vim");
    let executable = format!("vim{}", std::env::consts::EXE_SUFFIX);
    let program = if let Some(program) = std::env::var_os("TUNDRA_VIM") {
        PathBuf::from(program)
    } else if bundled.join(&executable).is_file() {
        bundled.join(&executable)
    } else {
        app::update::installed_vim_directory(platform)
            .map_err(io::Error::other)?
            .join(&executable)
    };
    if !program.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            i18n::tr!(
                "shell-vim-engine-missing",
                path = program.display().to_string()
            ),
        ));
    }
    let runtime = program
        .parent()
        .map(|p| p.join("runtime"))
        .filter(|p| p.is_dir());
    Ok((program, runtime))
}

fn vim_area(state: &ShellSession) -> Rect {
    let area = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
    let main = match state.shell_layout_for(area) {
        ui::ShellLayout::Full { main, .. } => main,
        ui::ShellLayout::Compact(main) => main,
    };
    ui::editor_vim_terminal_area(state.editor_content_area(main))
}

fn input_for_page(state: &mut ShellSession, id: PageInstanceId) -> Option<BufferInput> {
    state
        .with_page_context(id, |session| {
            session.app.dispatch_at(
                app::AppCommand::Editor(app::editor::EditorCommand::SetMode(
                    app::editor::EditorMode::Source,
                )),
                Instant::now(),
            );
            let editor = session.app.editor_state()?;
            let text = editor.export_text();
            let mut metadata =
                app::editor::TextMetadata::from_source(&text, editor.document.metadata.utf8_bom);
            metadata.encoding = editor.document.metadata.encoding;
            if !text.contains(['\r', '\n']) {
                metadata.preferred_line_ending = editor.document.metadata.preferred_line_ending;
            }
            if metadata.mixed_line_endings {
                metadata.preferred_line_ending = app::editor::LineEnding::Lf;
                metadata.has_final_newline = text.ends_with('\n');
            }
            let (line, _) = editor
                .source_position(editor.cursor.byte_offset)
                .unwrap_or((0, 0));
            let column = editor.source_line_range(line).map_or(0, |range| {
                editor.cursor.byte_offset.saturating_sub(range.start)
            });
            Some(BufferInput {
                host_id: id.get(),
                path: editor.document.path.clone(),
                text,
                encoding: encoding_name(metadata.encoding).into(),
                line_ending: ending_for(metadata.preferred_line_ending),
                bom: metadata.utf8_bom || metadata.encoding != app::editor::TextEncoding::Utf8,
                end_of_line: metadata.has_final_newline,
                modified: editor.is_dirty(),
                cursor: vim_runtime::VimCursor { line, column },
                read_only: editor.is_read_only(),
                protected: session.config_editor_active(),
            })
        })
        .flatten()
}

fn mirror_buffer(state: &mut ShellSession, buffer: &VimBuffer) {
    let Some(mut editor) = state.app.editor_state().cloned() else {
        return;
    };
    let content_changed = editor.source().as_ref() != buffer.text.as_ref();
    let path_changed = editor.document.path != buffer.path;
    let was_dirty = editor.is_dirty();
    if state.config_editor_active() && state.pages.editor.editor_config.path != buffer.path {
        state.pages.editor.editor_config = Default::default();
    }
    if path_changed || (!buffer.modified && content_changed) {
        state.pages.editor.editor_fingerprint = buffer
            .path
            .as_deref()
            .and_then(|path| platform::document_fingerprint(path).ok());
    }
    let mut metadata = app::editor::TextMetadata::from_source(&buffer.text, buffer.bom);
    metadata.encoding = match buffer.encoding.to_ascii_lowercase().as_str() {
        "utf-16le" => app::editor::TextEncoding::Utf16Le,
        "utf-16be" | "utf-16" => app::editor::TextEncoding::Utf16Be,
        _ => app::editor::TextEncoding::Utf8,
    };
    metadata.preferred_line_ending = match buffer.line_ending {
        LineEnding::Unix => app::editor::LineEnding::Lf,
        LineEnding::Dos => app::editor::LineEnding::CrLf,
        LineEnding::Mac => app::editor::LineEnding::Cr,
    };
    metadata.has_final_newline = buffer.end_of_line;
    let recovery_changed = buffer.modified
        && (content_changed || path_changed || editor.document.metadata != metadata || !was_dirty);
    let separator = if buffer.line_ending == LineEnding::Mac {
        '\r'
    } else {
        '\n'
    };
    let line_start = buffer
        .text
        .split_inclusive(separator)
        .take(buffer.cursor.line)
        .map(str::len)
        .sum::<usize>();
    let cursor = line_start
        .saturating_add(buffer.cursor.column)
        .min(buffer.text.len());
    editor.record_external_line_changes(buffer.line_changes.iter().map(|change| {
        app::editor::SourceLineChange::from_vim(
            change.start_line,
            change.end_line,
            change.added,
            change.column,
        )
    }));
    editor.synchronize_external_source(
        &buffer.text,
        buffer.path.clone(),
        metadata,
        cursor,
        buffer.modified,
    );
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(editor)),
        Instant::now(),
    );
    if recovery_changed {
        state
            .pages
            .editor
            .editor_recovery_dirty_since
            .get_or_insert_with(Instant::now);
    }
}

fn gui_encoding(encoding: &str) -> bool {
    matches!(
        encoding.to_ascii_lowercase().as_str(),
        "utf-8" | "utf8" | "utf-16le" | "utf-16be" | "utf-16"
    )
}

fn encoding_name(encoding: app::editor::TextEncoding) -> &'static str {
    match encoding {
        app::editor::TextEncoding::Utf8 => "utf-8",
        app::editor::TextEncoding::Utf16Le => "utf-16le",
        app::editor::TextEncoding::Utf16Be => "utf-16be",
    }
}

fn ending_for(ending: app::editor::LineEnding) -> LineEnding {
    match ending {
        app::editor::LineEnding::Lf => LineEnding::Unix,
        app::editor::LineEnding::CrLf => LineEnding::Dos,
        app::editor::LineEnding::Cr => LineEnding::Mac,
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "../../tests/unit/session/vim_script.rs"]
mod script_tests;
