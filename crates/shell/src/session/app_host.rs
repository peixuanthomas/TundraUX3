use super::window_launch::{WindowApplication, WindowOwner};
use super::*;
use crate::process_host::WorkerConnection;
use crate::session_services::SessionServicesClient;
use ratatui::{Terminal, backend::TestBackend};
use std::thread;
use watchdog::{AppCriticality, AppDescriptor, AppId};
use workspace_protocol::{
    AppToShell, Cursor, CursorShape, LayoutGeneration, MAX_IMAGE_BYTES, MIN_CONTENT_SIZE,
    ShellAction as HostAction, ShellToApp, Size, SupervisorToWorker, WindowFrame, WindowInit,
    WindowStatus, WorkerToSupervisor,
};

const HOST_TICK: Duration = Duration::from_millis(16);
const SERVICE_TICK: Duration = Duration::from_millis(250);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const WINDOW_CLOSE_TASK_KEY: &str = "workspace.window-close-task";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum WindowClose {
    #[default]
    Idle,
    Checking,
    Confirmed,
    Cancelled,
}

struct AppHostSession {
    state: ShellSession,
    platform: Arc<dyn Platform>,
    watchdog: AppWatchdog,
    services: Arc<SessionServicesClient>,
    compositor: ScreenCompositor,
    launcher_icons: Option<LauncherIconRuntime>,
    command_lines: CommandLineHosts,
    vim: vim_host::EditorVimHost,
    scripts: script_host::ScriptHosts,
    terminal: Terminal<TestBackend>,
    theme: ui::TundraTheme,
    theme_reloader: UserThemeReloader,
    redraw: RedrawScheduler,
    next_service_sync: Instant,
    size: Size,
    layout: LayoutGeneration,
    visible: bool,
    focused: bool,
    frozen: bool,
    close_status: WindowClose,
    last_status: Option<WindowStatus>,
    pending_explorer_selection: Option<PathBuf>,
    output: VecDeque<AppToShell>,
    input_fences: Vec<u64>,
    sent_images: std::collections::HashSet<u64>,
}

impl AppHostSession {
    fn new(
        init: WindowInit,
        process: ProcessWatchdog,
        watchdog: AppWatchdog,
        services: Arc<SessionServicesClient>,
    ) -> io::Result<Self> {
        let launch: WindowLaunch = serde_json::from_value(init.launch)?;
        let application = launch.application();
        let auth: Option<AuthSession> = init.profile.map(serde_json::from_value).transpose()?;
        let platform: Arc<dyn Platform> = Arc::from(platform::native_platform());
        let mut startup = ShellStartupState::clean(platform.kind(), platform.capabilities());
        startup.storage_manager = Some(StorageManager::from_layout(
            storage::StorageLayout::from_app_paths(
                &platform.app_paths().map_err(io::Error::other)?,
            ),
        ));
        startup.debug_policy = DebugPolicy::current_build();
        if cfg!(target_os = "linux") {
            startup.identity_backend = identity::IdentityBackend::Linux;
        }
        let storage = startup.storage_manager.clone();
        let explorer = storage.as_ref().map(|storage| {
            ShellExplorerTaskRuntime::new_managed(storage.clone(), watchdog.clone())
        });
        let diagnostics = storage.as_ref().map(|storage| {
            ShellDiagnosticsTaskRuntime::new_managed(
                storage.clone(),
                process.clone(),
                watchdog.clone(),
            )
        });
        let settings_config = system_services_config_for_startup(&startup);
        let ascii_assets = ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
        let size = Size::new(
            init.size.width.max(MIN_CONTENT_SIZE.width),
            init.size
                .height
                .max(MIN_CONTENT_SIZE.height)
                .min(u16::MAX - 2),
        );
        let terminal_size = (size.width, size.height + 2);
        let mut state = ShellSession::new_with_runtime_services(
            ShellLaunchConfig::default(),
            terminal_size,
            startup,
            ascii_assets,
            ShellRuntimeServices {
                language: None,
                terminal_image_support: init.image_cell_size.is_some(),
                explorer,
                diagnostics,
                editor: ShellEditorTaskRuntime::new_managed(watchdog.clone()),
                settings: ShellSettingsTaskRuntime::new_managed_with_system_services(
                    watchdog.clone(),
                    None,
                    settings_config,
                    Some(platform.clone()),
                ),
                window_owner: WindowOwner::App(application),
                inherited_auth: auth,
            },
        );
        state.set_terminal_text_sizing_support(init.text_sizing);
        if application == WindowApplication::Launcher {
            state.launcher_task_runtime = Some(ShellLauncherTaskRuntime::new_managed(
                platform.clone(),
                watchdog.clone(),
            ));
        }
        services.sync_session(&mut state);
        let _language = i18n::enter_snapshot(state.language.clone());
        let select = match &launch {
            WindowLaunch::ExplorerPath { select, .. } => select.clone().map(PathBuf::from),
            _ => None,
        };
        launch.activate(&mut state, platform.as_ref());
        if state.content_screen() == ShellScreen::Home {
            let message = state.pages.common.error_message.as_ref().map_or_else(
                || state.to_shell_chrome_view_model().status.full_message(),
                i18n::LocalizedText::render_current,
            );
            return Err(io::Error::other(message));
        }
        let now = Instant::now();
        let redraw = RedrawScheduler::new(
            now,
            RedrawIdentity::from_session(&state),
            reduced_motion_enabled(&state),
        );
        let mut host = Self {
            state,
            platform,
            services,
            compositor: ScreenCompositor::default(),
            launcher_icons: None,
            command_lines: CommandLineHosts::new(watchdog.clone()),
            vim: vim_host::EditorVimHost::new(watchdog.clone()),
            scripts: script_host::ScriptHosts::new(watchdog.clone()),
            terminal: Terminal::new(TestBackend::new(terminal_size.0, terminal_size.1))
                .unwrap_or_else(|error| match error {}),
            theme: ui::TundraTheme::default_dark(),
            theme_reloader: UserThemeReloader::new(storage, now),
            redraw,
            watchdog,
            next_service_sync: now,
            size,
            layout: init.layout,
            visible: true,
            focused: init.focused,
            frozen: false,
            close_status: WindowClose::Idle,
            last_status: None,
            pending_explorer_selection: select,
            output: VecDeque::new(),
            input_fences: Vec::new(),
            sent_images: std::collections::HashSet::new(),
        };
        if let Some([width, height]) = init.image_cell_size {
            host.command_lines.set_cell_size(width, height);
            host.vim.set_cell_size(width, height);
            host.scripts.set_cell_size(width, height);
            host.compositor.capture_workspace_images = true;
            if application == WindowApplication::Launcher {
                host.launcher_icons = LauncherIconRuntime::spawn(
                    host.platform.clone(),
                    ui::EditorImagePicker::for_workspace(width, height),
                    &host.watchdog,
                )
                .ok();
            }
        }
        host.compositor.capture_enabled = true;
        if !init.focused {
            host.set_focused(false);
        }
        Ok(host)
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.command_lines.observe_window_focus(focused);
        self.vim.observe_window_focus(&self.state, focused);
        self.scripts.observe_window_focus(&self.state, focused);
        self.state.apply_input_with_platform(
            if focused {
                InputEvent::FocusGained
            } else {
                InputEvent::FocusLost
            },
            self.platform.as_ref(),
        );
    }

    fn update_viewport(
        &mut self,
        layout: LayoutGeneration,
        size: Option<Size>,
        visible: bool,
        focused: bool,
    ) {
        if layout < self.layout {
            return;
        }
        self.frozen = size.is_none();
        self.visible = visible;
        if self.layout != layout {
            self.state.cancel_pointer_gestures_for_modal();
            self.compositor.cancel_for_bounds_change();
            self.layout = layout;
            self.redraw.request_redraw();
        }
        if self.focused != focused {
            self.set_focused(focused);
        }
        if let Some(size) = size
            && visible
            && size != self.size
        {
            self.size = Size::new(size.width, size.height.min(u16::MAX - 2));
            let width = self.size.width;
            let height = self.size.height + 2;
            self.terminal.backend_mut().resize(width, height);
            self.state.apply_input_with_platform(
                InputEvent::Resize { width, height },
                self.platform.as_ref(),
            );
            self.compositor.cancel_for_bounds_change();
            self.redraw.request_redraw();
        }
    }

    fn escape(&mut self, key: KeyInput, from_chrome: bool) {
        let press = key.phase == InputPhase::Press;
        let input = InputEvent::Key(key);
        let screen = self.state.active_screen();
        let modal = !self.state.interactive_overlays().is_empty()
            || self.state.current_vim_install().is_some()
            || (screen == ShellScreen::Explorer
                && (self.state.pages.explorer.explorer_input_mode != ExplorerInputMode::Browse
                    || matches!(
                        self.state.pages.explorer.explorer_purpose,
                        ExplorerPurpose::EditorOpen | ExplorerPurpose::EditorSaveAs { .. }
                    )))
            || (screen == ShellScreen::Editor
                && self.state.pages.editor.editor_search_bar.is_some());
        let native = self.vim.captures_input(&self.state, &input)
            || screen == ShellScreen::CommandLine
            || self.scripts.captures_terminal_input(&self.state)
            || (screen == ShellScreen::Management
                && self.state.to_management_view_model().terminal);
        if (!from_chrome && native) || (modal && press) {
            self.dispatch(input);
        } else if !press {
            return;
        } else if from_chrome && screen == ShellScreen::CommandLine {
            self.request_close();
        } else if from_chrome && self.state.screen_stack().len() > 2 {
            if self.vim.captures_input(&self.state, &input) {
                self.state.request_editor_close(self.platform.as_ref());
            } else {
                self.dispatch(input);
            }
        } else if screen == ShellScreen::SystemStatus
            && matches!(
                self.state
                    .route_back_key(&KeyInput::new(InputKey::Escape))
                    .1,
                ShellCommand::SystemStatusBack | ShellCommand::SystemStatusRequestCancelEdit
            )
        {
            self.dispatch(input);
        } else {
            self.output.push_back(AppToShell::BackgroundRequested);
        }
    }

    fn dispatch(&mut self, input: InputEvent) {
        let received_at = Instant::now();
        let _language = i18n::enter_snapshot(self.state.language.clone());
        let before = RedrawIdentity::from_session(&self.state);
        let Some(input) = self.compositor.prepare_input(
            &mut self.state,
            input,
            self.platform.as_ref(),
            received_at,
        ) else {
            self.redraw.request_input_redraw();
            return;
        };
        let action = if self.scripts.handle_input(&self.state, &input) {
            ShellAction::Redraw
        } else if self.vim.captures_input(&self.state, &input) {
            if let Err(error) = self.vim.handle_input(&self.state, &input) {
                self.state.report_editor_error(i18n::msg!(
                    "shell-vim-runtime-error",
                    reason = error.to_string()
                ));
            }
            ShellAction::Redraw
        } else if command_line_captures_input(&self.state, &input) {
            let terminal_area = self.state.command_line_input_area();
            let event = self
                .command_lines
                .handle_input(&self.state, &input, terminal_area);
            self.command_line_event(event);
            ShellAction::Redraw
        } else {
            self.compositor
                .dispatch_input(&mut self.state, input, self.platform.as_ref(), received_at)
                .0
        };
        synchronize_motion_hit_map_after_input(
            &mut self.state,
            &mut self.redraw,
            before,
            received_at,
        );
        let context = self.render_context(received_at);
        self.compositor
            .synchronize_after_input(&self.state, &context);
        self.observe_action(action);
        self.redraw.request_input_redraw();
    }

    fn command_line_event(&mut self, event: CommandLineHostEvent) {
        match event {
            CommandLineHostEvent::PanicRequested => trigger_command_line_panic(),
            CommandLineHostEvent::ExitToCaller => {
                self.state.close_page_instance(self.state.active_page_id());
            }
            CommandLineHostEvent::ResetRequested => {
                self.output
                    .push_back(AppToShell::ShellAction(HostAction::Reset));
            }
            CommandLineHostEvent::None => {}
        }
    }

    fn observe_action(&mut self, action: ShellAction) {
        let request = match action {
            ShellAction::PowerOff => Some(HostAction::PowerOff),
            ShellAction::Reboot => Some(HostAction::Reboot),
            ShellAction::Exit if self.state.return_to_lockscreen_requested() => {
                Some(HostAction::Logout)
            }
            ShellAction::Exit if self.state.restart_requested() => Some(HostAction::Restart),
            ShellAction::Exit => Some(HostAction::Exit),
            ShellAction::Redraw => None,
        };
        if let Some(request) = request {
            self.output.push_back(AppToShell::ShellAction(request));
        }
    }

    fn request_close(&mut self) {
        if self.state.window_close == WindowClose::Idle {
            self.state.window_close = WindowClose::Checking;
            self.output.push_back(AppToShell::CloseDeferred);
            self.redraw.request_redraw();
        }
    }

    fn check_close(&mut self) {
        if self.state.window_close != WindowClose::Checking
            || !self.state.interactive_overlays().is_empty()
        {
            return;
        }
        let dirty = self.state.pages.editor.ids().into_iter().find(|&id| {
            self.state
                .with_page_context(id, |state| {
                    state.app.editor_state().is_some_and(|editor| {
                        editor.is_dirty()
                            && !state.approved_editor_closes.get(&id).is_some_and(
                                |&(revision, discard)| {
                                    discard && revision == Some(editor.revision())
                                },
                            )
                    })
                })
                .unwrap_or(false)
        });
        if let Some(id) = dirty {
            if self
                .state
                .pages
                .editor
                .get(id)
                .is_some_and(|page| page.editor_close_after_save)
            {
                return;
            }
            self.state.activate_task_page(id);
            self.state.confirm_editor_close();
            self.redraw.request_redraw();
            return;
        }
        if self.state.tasks.has_running_operations()
            || self.command_lines.has_running_processes()
            || self.scripts.running()
            || self.state.auto_admin_running()
        {
            self.state.notify_modal_with_options(
                ShellNotification::modal(
                    i18n::msg!("workspace-close-task-title"),
                    i18n::msg!("workspace-close-task-message"),
                    ui::NotificationTone::Warning,
                    vec![
                        ShellNotificationAction::new(
                            "stop-and-close",
                            i18n::msg!("workspace-close-task-confirm"),
                        )
                        .with_follow_up(ShellCommand::ConfirmWindowClose),
                        ShellNotificationAction::new("cancel", i18n::msg!("shell-cancel"))
                            .with_shortcut(InputKey::Escape)
                            .with_follow_up(ShellCommand::CancelWindowClose)
                            .cancel(),
                    ],
                )
                .with_key(WINDOW_CLOSE_TASK_KEY),
            );
            self.redraw.request_redraw();
        } else {
            self.state.window_close = WindowClose::Confirmed;
        }
    }

    fn receive(&mut self, message: ShellToApp) -> io::Result<()> {
        match message {
            ShellToApp::Viewport {
                layout,
                size,
                visible,
                focused,
            } => {
                self.update_viewport(layout, size, visible, focused);
            }
            ShellToApp::Input { layout, event }
                if layout == self.layout && self.visible && !self.frozen =>
            {
                let mut input = serde_json::from_value(event)?;
                if let InputEvent::Mouse(mouse) = &mut input {
                    mouse.position.row = mouse.position.row.saturating_add(1);
                }
                match input {
                    InputEvent::Key(key)
                        if key.key == InputKey::Escape && !key.has_non_shift_modifier() =>
                    {
                        self.escape(key, false);
                    }
                    input => self.dispatch(input),
                }
            }
            ShellToApp::Back { layout }
                if layout == self.layout && self.focused && !self.frozen =>
            {
                self.escape(KeyInput::new(InputKey::Escape), true)
            }
            ShellToApp::Keyboard { layout }
                if layout == self.layout && self.focused && !self.frozen =>
            {
                self.state.toggle_input_keyboard();
                self.redraw.request_redraw();
            }
            ShellToApp::Help { layout }
                if layout == self.layout && self.focused && !self.frozen =>
            {
                self.state.open_page_help();
                self.redraw.request_redraw();
            }
            ShellToApp::CancelPointer { layout } if layout == self.layout => {
                self.state.cancel_pointer_gestures_for_modal();
                self.redraw.request_redraw();
            }
            ShellToApp::RequestClose => self.request_close(),
            ShellToApp::InputFence { serial } => {
                self.input_fences.push(serial);
                self.redraw.request_input_redraw();
            }
            ShellToApp::TaskAction { id, cancel } => {
                if let Some((domain, request, page)) = self.state.tasks.history_task(&id) {
                    if cancel {
                        self.state.tasks.cancel(domain, request);
                    } else if let Some(page) = page {
                        self.state.activate_task_page(page);
                    }
                    self.redraw.request_redraw();
                }
            }
            ShellToApp::TaskCatalog { ids } => {
                self.state.window_task_ids = ids.into_iter().collect();
                self.redraw.request_redraw();
            }
            _ => {}
        }
        Ok(())
    }

    fn poll(&mut self, now: Instant) -> io::Result<()> {
        let _language = i18n::enter_snapshot(self.state.language.clone());
        let mut before = self.state.clone();
        let previous_theme = self.theme.clone();
        if now >= self.next_service_sync {
            self.services.sync_session(&mut self.state);
            self.next_service_sync = now + SERVICE_TICK;
            if self.state.auth_session().is_none() {
                self.state.window_close = WindowClose::Confirmed;
                self.close_status = WindowClose::Confirmed;
                return Ok(());
            }
        }
        if self
            .launcher_icons
            .as_mut()
            .is_some_and(LauncherIconRuntime::poll_results)
        {
            self.redraw.request_redraw();
        }
        let action = self
            .state
            .apply_input_with_platform(InputEvent::Tick, self.platform.as_ref());
        self.observe_action(action);
        self.state.poll_input_keyboard(now);
        self.theme_reloader
            .poll_at(now, &mut self.theme, &mut self.state);
        let (changed, event) = self.command_lines.poll(
            &mut self.state,
            self.platform.as_ref(),
            self.theme.tokens().accent,
        );
        self.command_line_event(event);
        let changed = self.vim.poll(&mut self.state, self.platform.as_ref())
            | self.scripts.poll(&mut self.state, self.platform.as_ref())
            | changed;
        if let Some(path) = &self.pending_explorer_selection
            && self.state.select_explorer_path(path)
        {
            self.pending_explorer_selection = None;
        }
        if let Some(routed) = self.compositor.take_deferred_close(&self.state) {
            let action = self
                .state
                .apply_routed_event(routed, self.platform.as_ref(), now);
            self.observe_action(action);
            self.redraw.request_redraw();
        }
        if self.state.content_screen() == ShellScreen::Home
            && self.state.window_close == WindowClose::Idle
        {
            self.state.window_close = WindowClose::Confirmed;
        }
        self.check_close();
        if self.state.window_close == WindowClose::Cancelled {
            self.state.window_close = WindowClose::Idle;
            self.output.push_back(AppToShell::CloseCancelled);
        }
        if let Some(request) = self.state.window_global_request.take() {
            self.output
                .push_back(AppToShell::ShellAction(match request {
                    ShellCommand::Logout | ShellCommand::LogoutToLockscreen => HostAction::Logout,
                    ShellCommand::Restart => HostAction::Restart,
                    _ => HostAction::Exit,
                }));
        }
        for launch in self.state.window_launches.drain(..) {
            self.output
                .push_back(AppToShell::OpenApp(serde_json::to_value(launch)?));
        }
        if let Some((id, cancel)) = self.state.window_task_action.take() {
            self.output.push_back(AppToShell::TaskAction { id, cancel });
        }
        if std::mem::take(&mut self.state.window_tasks_requested) {
            self.output.push_back(AppToShell::ToggleTasks);
        }
        if before.app.storage_config() != self.state.app.storage_config() {
            self.output
                .push_back(AppToShell::ShellAction(HostAction::SettingsChanged));
        }
        if let Some(manifest) = self.state.update_apply_manifest() {
            self.output
                .push_back(AppToShell::ShellAction(HostAction::UpdatePrepared(
                    manifest.to_owned(),
                )));
            self.state.window_close = WindowClose::Confirmed;
        }
        self.close_status = self.state.window_close;
        if self.close_status == WindowClose::Confirmed {
            return Ok(());
        }
        if changed
            || previous_theme != self.theme
            || session_render_state_changed(&mut before, &self.state)
        {
            self.redraw.request_redraw();
        }
        self.redraw.observe(
            now,
            RedrawIdentity::from_session(&self.state),
            reduced_motion_enabled(&self.state),
        );
        let minimum = [
            self.command_lines.minimum_content_size(),
            self.vim.minimum_content_size(),
            self.scripts.minimum_content_size(),
        ]
        .into_iter()
        .flatten()
        .fold(MIN_CONTENT_SIZE, |minimum, (width, height)| {
            Size::new(minimum.width.max(width), minimum.height.max(height))
        });
        let dirty = self.state.pages.editor.ids().into_iter().any(|id| {
            self.state
                .with_page_context(id, |state| {
                    state.app.editor_state().is_some_and(EditorState::is_dirty)
                })
                .unwrap_or(false)
        });
        let status = WindowStatus {
            minimum,
            busy: self.state.tasks.has_running()
                || self.state.auto_admin_running()
                || self.command_lines.has_running_processes()
                || self.vim.running()
                || self.scripts.running(),
            dirty,
            task_ids: self.state.tasks.window_task_ids(),
            running_operations: self.state.tasks.has_running_operations(),
            awaiting_approval: self.state.auto_admin.waiting_for_approval(),
            unresponsive: self.vim.unresponsive(),
        };
        if self.last_status.as_ref() != Some(&status) {
            self.last_status = Some(status.clone());
            self.output.push_back(AppToShell::Status(status));
        }
        Ok(())
    }

    fn render_context(&self, now: Instant) -> ui::RenderContext {
        ui::RenderContext::from_theme_with_transitions(
            &self.theme,
            self.redraw.frame(now, animation_speed_percent(&self.state)),
            self.redraw.transitions(now),
            ui::RenderCapabilities::default(),
        )
    }

    fn frame(&mut self, now: Instant) -> io::Result<Option<WindowFrame>> {
        if self.frozen || !self.visible || !self.redraw.is_due(now) {
            return Ok(None);
        }
        let _language = i18n::enter_snapshot(self.state.language.clone());
        let context = self.render_context(now);
        let prepared = self.compositor.prepare(
            &self.state,
            self.command_lines.visible_host(&self.state),
            now,
            ui::TerminalCellAspectRatio::default(),
            context,
            self.launcher_icons.as_mut(),
        );
        let content = Rect::new(0, 1, self.size.width, self.size.height);
        let headings = ui::components::WorkspaceHeadingCapture::begin(content);
        let icons = ui::WorkspaceIconCapture::begin(content);
        let mut animating = false;
        self.terminal
            .draw(|frame| {
                animating = self.compositor.render(
                    frame,
                    &mut self.state,
                    &prepared,
                    self.launcher_icons.as_ref(),
                    None,
                );
            })
            .map_err(io::Error::other)?;
        let completed = Instant::now();
        self.redraw.did_draw_finished(now, completed);
        if prepared.rainbow_running() {
            self.redraw.request_continuous_frame(completed);
        }
        if animating && let Some(interval) = self.compositor.next_redraw_in() {
            self.redraw.request_animation_frame_in(completed, interval);
        }
        let backend = self.terminal.backend();
        let cursor = backend.cursor_position();
        let cursor =
            (self.focused && backend.cursor_visible() && content.contains(cursor)).then(|| {
                Cursor {
                    x: cursor.x,
                    y: cursor.y - content.y,
                    shape: self.cursor_shape(),
                }
            });
        let mut frame = ui::capture_workspace_frame(self.layout, backend.buffer(), content, cursor);
        let mut images = self.compositor.take_workspace_images();
        frame.modal = !self.state.interactive_overlays().is_empty();
        if !frame.modal && !self.state.input_keyboard_open() && !self.compositor.toast_visible() {
            frame.headings = headings.finish();
            images.extend(icons.finish());
        }
        let mut next_ids = std::collections::HashSet::new();
        let mut total_bytes = 0;
        images.retain_mut(|image| {
            let repeated = next_ids.contains(&image.image_id);
            if !repeated {
                let next_bytes = total_bytes + image.rgba.len();
                if next_bytes > MAX_IMAGE_BYTES {
                    return false;
                }
                total_bytes = next_bytes;
                next_ids.insert(image.image_id);
            }
            if repeated || self.sent_images.contains(&image.image_id) {
                image.rgba = Default::default();
            }
            true
        });
        self.sent_images = next_ids;
        frame.images = images;
        if let Some(snapshot) = self.compositor.snapshot.take() {
            frame.page = snapshot.page;
            frame.animation_running = snapshot.animation_running;
            ui::capture_workspace_controls(&mut frame, content, &snapshot.nodes);
        }
        frame.keyboard_available = self.state.keyboard_launcher_area().is_some();
        frame.status = self.state.displayed_status.clone().unwrap_or_default();
        if !frame.modal {
            let native_area = if self.state.active_screen() == ShellScreen::CommandLine {
                self.state.command_line_input_area()
            } else if self.scripts.captures_terminal_input(&self.state) {
                Some(self.state.script_layout().output_area)
            } else if self
                .vim
                .captures_input(&self.state, &InputEvent::key(InputKey::Escape))
            {
                Some(content)
            } else {
                None
            };
            if let Some(area) = native_area
                .map(|area| area.intersection(content))
                .filter(|area| !area.is_empty())
            {
                frame.native_escape.push(workspace_protocol::Rect {
                    x: area.x - content.x,
                    y: area.y - content.y,
                    width: area.width,
                    height: area.height,
                });
            }
        }
        Ok(Some(frame))
    }

    fn cursor_shape(&self) -> CursorShape {
        let style = if self.state.active_screen() == ShellScreen::CommandLine {
            self.command_lines
                .visible_host(&self.state)
                .view_model()
                .terminal
                .cursor_style
        } else {
            self.state
                .editor_vim_terminal
                .as_ref()
                .map_or(ui::CommandLineCursorStyle::Default, |terminal| {
                    terminal.cursor_style
                })
        };
        use ui::CommandLineCursorStyle as Native;
        match style {
            Native::Default => CursorShape::Default,
            Native::BlinkingBlock => CursorShape::BlinkingBlock,
            Native::SteadyBlock => CursorShape::SteadyBlock,
            Native::BlinkingUnderline => CursorShape::BlinkingUnderline,
            Native::SteadyUnderline => CursorShape::SteadyUnderline,
            Native::BlinkingBar => CursorShape::BlinkingBar,
            Native::SteadyBar => CursorShape::SteadyBar,
        }
    }

    fn shutdown(&mut self) -> io::Result<()> {
        let _language = i18n::enter_snapshot(self.state.language.clone());
        self.vim.stop(&mut self.state);
        self.scripts.stop_all();
        if self.state.window_close == WindowClose::Confirmed {
            for id in std::mem::take(&mut self.state.approved_editor_closes).into_keys() {
                self.state
                    .with_page_context(id, ShellSession::clear_editor_recovery);
            }
        }
        let preserve_update = self.state.update_apply_manifest().is_some();
        stop_fullscreen_session_tasks(
            &mut self.state,
            &mut self.command_lines,
            self.platform.as_ref(),
            &self.watchdog,
            preserve_update,
        )
    }
}

pub fn run_app_host(
    connection: &'static WorkerConnection,
    process: ProcessWatchdog,
) -> io::Result<()> {
    let endpoint = connection.endpoint().ok_or(io::ErrorKind::InvalidInput)?;
    let listener = platform::shell_control::LocalListener::bind(endpoint)?;
    connection.send(WorkerToSupervisor::Ready)?;
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    let mut stream = loop {
        connection.progress()?;
        if let Ok(stream) = connection.accept_shell(&listener) {
            break stream;
        }
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        thread::sleep(HOST_TICK);
    };
    let (shell, shell_pid) = connection.shell_peer().ok_or(io::ErrorKind::InvalidInput)?;
    let mut hello_received = false;
    let (init, pending) = 'receive_init: loop {
        connection.progress()?;
        let mut messages = stream.poll::<ShellToApp>()?.into_iter();
        while let Some(message) = messages.next() {
            match message {
                ShellToApp::Hello(hello)
                    if !hello_received
                        && hello.worker == shell
                        && hello.pid == shell_pid
                        && hello.protocol == workspace_protocol::PROTOCOL_VERSION =>
                {
                    hello_received = true;
                    stream.queue(&AppToShell::Welcome {
                        worker: connection.worker(),
                    })?;
                }
                ShellToApp::Init(init) if hello_received => break 'receive_init (init, messages),
                _ => return Err(io::ErrorKind::InvalidData.into()),
            }
        }
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        thread::sleep(HOST_TICK);
    };
    let services = SessionServicesClient::connect(connection)?;
    services.install();
    let watchdog = process
        .register_app(AppDescriptor::new(
            AppId::from_static("app-host"),
            "Tundra AppHost",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::ProcessCritical,
        ))
        .map_err(io::Error::other)?;
    let mut host = match AppHostSession::new(init, process, watchdog, services) {
        Ok(host) => host,
        Err(error) => {
            stream.queue(&AppToShell::LaunchFailed(error.to_string()))?;
            return flush_app_messages(&mut stream);
        }
    };
    for message in pending {
        host.receive(message)?;
    }

    loop {
        connection.progress()?;
        while let Some(command) = connection.try_recv()? {
            if matches!(
                command,
                SupervisorToWorker::Close | SupervisorToWorker::Exit
            ) {
                host.shutdown()?;
                return Ok(());
            }
        }
        for message in stream.poll::<ShellToApp>()? {
            host.receive(message)?;
        }
        let now = Instant::now();
        host.poll(now)?;
        while let Some(message) = host.output.pop_front() {
            if let Err(error) = stream.queue(&message) {
                if error.kind() == io::ErrorKind::WouldBlock {
                    host.output.push_front(message);
                    break;
                }
                return Err(error);
            }
        }
        if host.close_status == WindowClose::Confirmed
            && host.output.is_empty()
            && !stream.pending_write()
        {
            connection.send(WorkerToSupervisor::AppClosing)?;
            stream.queue(&AppToShell::CloseStarted)?;
            flush_app_messages(&mut stream)?;
            host.shutdown()?;
            stream.queue(&AppToShell::Closed)?;
            return flush_app_messages(&mut stream);
        }
        if host.output.is_empty() && !stream.pending_write() {
            let frame = host.frame(now)?;
            let rendered = frame.is_some();
            if let Some(frame) = frame {
                stream.queue(&AppToShell::Frame(frame))?;
            }
            if rendered || host.frozen || !host.visible {
                for serial in host.input_fences.drain(..) {
                    host.output.push_back(AppToShell::InputFence { serial });
                }
            }
        }
        thread::sleep(HOST_TICK);
    }
}

fn flush_app_messages(stream: &mut crate::process_host::WindowConnection) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(1);
    while stream.pending_write() {
        let _ = stream.poll::<ShellToApp>()?;
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if stream.pending_write() {
            thread::sleep(HOST_TICK);
        }
    }
    Ok(())
}
