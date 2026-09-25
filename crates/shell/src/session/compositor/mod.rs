//! The only normal Shell frame assembler. Pages paint content, never shell chrome.
use super::spring_progress::SpringProgress;
use super::*;
use ratatui::Frame;

pub(super) enum ScreenViewModel {
    Home(Box<ui::HomeViewModel>),
    Setup(Box<ui::SetupViewModel>),
    Login(Box<ui::LoginViewModel>),
    BootstrapAdmin(Box<ui::BootstrapAdminViewModel>),
    UserManagement(Box<ui::UserManagementViewModel>),
    Explorer(Box<ui::ExplorerViewModel>),
    Launcher(Box<ui::LauncherViewModel>),
    CommandLine(Box<ui::CommandLineViewModel>),
    Editor(Box<ui::EditorViewModel>),
    Settings(Box<ui::SettingsViewModel>),
    Logs(Box<ui::LogsViewModel>),
    Diagnostics(Box<ui::DiagnosticsViewModel>),
    SystemStatus(Box<ui::SystemStatusViewModel>),
    Clock(Box<ui::ClockViewModel>),
}
impl ScreenViewModel {
    fn from_session(
        state: &ShellSession,
        command_line: &CommandLineHost,
        now: Instant,
        aspect: ui::TerminalCellAspectRatio,
    ) -> Self {
        match state.content_screen() {
            ShellScreen::Home | ShellScreen::ExitConfirm => {
                Self::Home(Box::new(state.to_home_view_model()))
            }
            ShellScreen::FirstRunSetup => Self::Setup(Box::new(state.to_setup_view_model())),
            ShellScreen::Login => Self::Login(Box::new(state.to_login_view_model_at(now))),
            ShellScreen::BootstrapAdmin => {
                Self::BootstrapAdmin(Box::new(state.to_bootstrap_admin_view_model()))
            }
            ShellScreen::UserManagement => {
                Self::UserManagement(Box::new(state.to_user_management_view_model()))
            }
            ShellScreen::Explorer => Self::Explorer(Box::new(state.to_explorer_view_model())),
            ShellScreen::Launcher => Self::Launcher(Box::new(state.to_launcher_view_model())),
            ShellScreen::CommandLine => Self::CommandLine(Box::new({
                let mut model = command_line.view_model();
                if let Some(username) = state.current_home_username() {
                    model = model.with_prompt_username(username);
                }
                model
            })),
            ShellScreen::Editor => Self::Editor(Box::new(state.to_editor_view_model())),
            ShellScreen::Settings => Self::Settings(Box::new(
                state
                    .to_settings_view_model()
                    .expect("Settings requires an authenticated session"),
            )),
            ShellScreen::Logs => Self::Logs(Box::new(state.to_logs_view_model())),
            ShellScreen::Diagnostics => {
                Self::Diagnostics(Box::new(state.to_diagnostics_view_model()))
            }
            ShellScreen::SystemStatus => Self::SystemStatus(Box::new(
                state
                    .to_system_status_view_model()
                    .expect("System Status requires an authenticated session"),
            )),
            ShellScreen::Clock => Self::Clock(Box::new(
                state
                    .to_clock_view_model_at(&state.app.snapshot().clock, now)
                    .with_terminal_cell_aspect_ratio(aspect),
            )),
        }
    }
    fn content(&self) -> ui::ScreenContent<'_> {
        match self {
            Self::Home(model) => ui::ScreenContent::Home(model),
            Self::Setup(model) => ui::ScreenContent::Setup(model),
            Self::Login(model) => ui::ScreenContent::Login(model),
            Self::BootstrapAdmin(model) => ui::ScreenContent::BootstrapAdmin(model),
            Self::UserManagement(model) => ui::ScreenContent::UserManagement(model),
            Self::Explorer(model) => ui::ScreenContent::Explorer(model),
            Self::Launcher(model) => ui::ScreenContent::Launcher(model),
            Self::CommandLine(model) => ui::ScreenContent::CommandLine(model),
            Self::Editor(model) => ui::ScreenContent::Editor(model),
            Self::Settings(model) => ui::ScreenContent::Settings(model),
            Self::Logs(model) => ui::ScreenContent::Logs(model),
            Self::Diagnostics(model) => ui::ScreenContent::Diagnostics(model),
            Self::SystemStatus(model) => ui::ScreenContent::SystemStatus(model),
            Self::Clock(model) => ui::ScreenContent::Clock(model),
        }
    }
}

pub(super) struct PreparedFrame {
    page: ScreenViewModel,
    chrome: ui::ShellChromeViewModel,
    context: ui::RenderContext,
    notification: Option<ui::NotificationViewModel>,
    time_sync: Option<ui::TimeSyncDialogViewModel>,
    progress_running: bool,
}

#[derive(Default)]
pub(super) struct ScreenCompositor {
    motion: ShellMotionEffects,
    progress: SpringProgress,
    toast: Option<ui::components::Toast>,
    appearance: Option<(ui::ThemeTokens, u64)>,
}

impl ScreenCompositor {
    pub(super) fn prepare(
        &mut self,
        state: &ShellSession,
        command_line: &CommandLineHost,
        now: Instant,
        aspect: ui::TerminalCellAspectRatio,
        context: ui::RenderContext,
        icons: Option<&mut LauncherIconRuntime>,
    ) -> PreparedFrame {
        let mut page = ScreenViewModel::from_session(state, command_line, now, aspect);
        let context = page.content().render_context(&context);
        self.synchronize_appearance(state, &context);
        let chrome = state.to_shell_chrome_view_model();
        self.synchronize_toast(&chrome, context.motion);
        let progress_running = match &mut page {
            ScreenViewModel::Settings(model) => {
                self.progress.update(Some(model), None, context.motion)
            }
            ScreenViewModel::SystemStatus(model) => {
                self.progress.update(None, Some(model), context.motion)
            }
            _ => self.progress.update(None, None, context.motion),
        };
        let layout = ui::ShellFrameLayout::new(
            Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1),
            chrome.status.time_button_label.as_deref(),
            &context,
        );
        if state.graphical_icons_enabled()
            && !layout.is_compact()
            && let Some(icons) = icons
        {
            match &page {
                ScreenViewModel::Home(model) => icons.sync_home(model, layout.main),
                ScreenViewModel::Launcher(model) => icons.sync(model, layout.main),
                _ => {}
            }
        }
        let notification = (state.content_screen() != ShellScreen::CommandLine
            || state.active_screen() == ShellScreen::ExitConfirm)
            .then(|| state.to_notification_view_model())
            .flatten();
        let time_sync = (state.content_screen() != ShellScreen::CommandLine)
            .then(|| state.to_time_sync_dialog_view_model())
            .flatten();
        PreparedFrame {
            page,
            chrome,
            context,
            notification,
            time_sync,
            progress_running,
        }
    }

    /// Returns animation demand; the runtime retains ownership of scheduling and I/O.
    pub(super) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        state: &mut ShellSession,
        prepared: &PreparedFrame,
        icons: Option<&LauncherIconRuntime>,
    ) -> bool {
        let bounds = frame.area();
        let context = &prepared.context;
        let mut chrome = prepared.chrome.clone();
        chrome.terminal_size = (bounds.width, bounds.height);
        let layout =
            ui::ShellFrameLayout::new(bounds, chrome.status.time_button_label.as_deref(), context);
        // Use the actual frame bounds, including a resize arriving just before draw.
        state.terminal_size = (bounds.width, bounds.height);
        state.refresh_hit_map_with_frame_layout(context.transitions, layout);
        self.motion
            .update_layout(state, &layout, context.theme, context.motion.reduced_motion);
        ui::components::Surface::new().render_frame(frame, bounds, context);
        let content = prepared.page.content();
        let visible_content = !layout.is_compact() || content.renders_in_compact();
        let icons = icons.filter(|_| state.graphical_icons_enabled());
        if visible_content {
            content.render_content(
                frame,
                &layout,
                context,
                icons.map(|v| v as &dyn ui::HomeIconRenderer),
                icons.map(|v| v as &dyn ui::LauncherIconRenderer),
            );
        } else {
            ui::render_compact_home(frame, bounds, &chrome, &context.compatibility_theme());
        }
        let shell_modal = self.motion.needs_shell_modal_base();
        if !shell_modal {
            self.motion.capture_base(frame.buffer_mut(), state);
        }
        if visible_content {
            content.render_overlay(frame, &layout, context);
        }
        ui::render_shell_chrome(frame, &layout, &chrome, context);
        if prepared.notification.is_none()
            && prepared.chrome.status.error.is_none()
            && let (Some(toast), Some(area)) = (&self.toast, layout.status_message)
        {
            toast.render_frame(frame, area, context);
        }
        if shell_modal {
            self.motion.capture_base(frame.buffer_mut(), state);
        }
        if let Some(notification) = &prepared.notification {
            ui::render_notification_overlay_with_context(frame, bounds, notification, context);
        } else if let Some(dialog) = &prepared.time_sync {
            ui::render_time_sync_failure_dialog_with_context(frame, bounds, dialog, context);
        }
        self.motion.capture_overlay(frame.buffer_mut(), state);
        self.motion
            .process(context.motion.scaled_delta(), frame.buffer_mut(), state);
        prepared.progress_running
            || self.motion.is_running()
            || self
                .toast
                .as_ref()
                .is_some_and(|toast| toast.requests_redraw(context.motion))
    }

    fn synchronize_toast(&mut self, chrome: &ui::ShellChromeViewModel, motion: ui::MotionFrame) {
        if chrome.status.error.is_some() {
            // A disappearing toast must not paint over a higher-priority alert.
            self.toast = None;
        } else {
            sync_shell_toast(&mut self.toast, chrome.status.toast.as_deref(), motion);
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| !toast.is_visible(motion))
        {
            self.toast = None;
        }
    }

    fn synchronize_appearance(&mut self, state: &ShellSession, context: &ui::RenderContext) {
        let identity = (context.theme, state.language.generation());
        if self
            .appearance
            .as_ref()
            .is_some_and(|previous| previous != &identity)
        {
            self.motion.cancel_for_bounds_change();
        }
        self.appearance = Some(identity);
    }

    pub(super) fn synchronize_after_input(
        &mut self,
        state: &ShellSession,
        context: &ui::RenderContext,
    ) {
        let context = if state.content_screen() == ShellScreen::FirstRunSetup {
            ui::setup_render_context(&state.to_setup_view_model(), context)
        } else {
            *context
        };
        self.synchronize_appearance(state, &context);
        let chrome = state.to_shell_chrome_view_model();
        let layout = ui::ShellFrameLayout::new(
            Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1),
            chrome.status.time_button_label.as_deref(),
            &context,
        );
        self.motion
            .update_layout(state, &layout, context.theme, context.motion.reduced_motion);
    }
    pub(super) fn cancel_for_suspend(&mut self, state: &ShellSession) -> Option<RoutedEvent> {
        self.motion.cancel_for_suspend(state)
    }
    pub(super) fn cancel_for_bounds_change(&mut self) {
        self.motion.cancel_for_bounds_change();
    }
    pub(super) fn take_deferred_close(&mut self, state: &ShellSession) -> Option<RoutedEvent> {
        self.motion.take_deferred_close(state)
    }
    pub(super) fn dispatch_input(
        &mut self,
        state: &mut ShellSession,
        input: InputEvent,
        platform: &dyn Platform,
        received_at: Instant,
    ) -> (ShellAction, bool) {
        dispatch_motion_aware_input(state, &mut self.motion, input, platform, received_at)
    }
}

pub(super) fn sync_shell_toast(
    toast: &mut Option<ui::components::Toast>,
    visible_message: Option<&str>,
    frame: ui::MotionFrame,
) {
    match visible_message {
        Some(message) => match toast.as_mut() {
            Some(toast) if toast.message == message => {
                if toast.dismiss_at.is_some() {
                    toast.resume(frame);
                }
            }
            _ => {
                *toast = Some(ui::components::Toast::new(
                    message,
                    ui::components::ToastTone::Info,
                    frame,
                ));
            }
        },
        None => {
            if let Some(toast) = toast.as_mut()
                && toast.dismiss_at.is_none()
            {
                toast.dismiss(frame);
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/session/compositor.rs"]
mod tests;
