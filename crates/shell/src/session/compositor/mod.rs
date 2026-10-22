//! The only normal Shell frame assembler. Pages paint content, never shell chrome.
use super::spring_progress::SpringProgress;
use super::*;
use ratatui::Frame;

mod screen_keyboard;

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
    Management(Box<ui::ManagementViewModel>),
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
            ShellScreen::CommandLine => Self::CommandLine(Box::new(command_line.view_model())),
            ShellScreen::Editor => Self::Editor(Box::new(state.to_editor_view_model())),
            ShellScreen::Settings => Self::Settings(Box::new(
                state
                    .to_settings_view_model()
                    .expect("Settings requires an authenticated session"),
            )),
            ShellScreen::Logs => Self::Logs(Box::new(state.to_logs_view_model())),
            ShellScreen::Management => Self::Management(Box::new(state.to_management_view_model())),
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
            Self::Management(model) => ui::ScreenContent::Management(model),
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
    help: Option<ui::PageHelpViewModel>,
    time_sync: Option<ui::TimeSyncDialogViewModel>,
    progress_running: bool,
}

#[derive(Default)]
pub(crate) struct ScreenCompositor {
    motion: ShellMotionEffects,
    progress: SpringProgress,
    toast: Option<ui::components::Toast>,
    appearance: Option<(ui::ThemeTokens, u64)>,
    keyboard_open: bool,
    pub(super) capture_enabled: bool,
    pub(super) snapshot: Option<shell_control::Snapshot>,
    sensitive_frame: bool,
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
        self.synchronize_toast(&chrome, state.interaction_motion_frame(context.motion));
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
        )
        .with_back_shortcut(chrome.back_shortcut)
        .with_help_button(state.help_button_available());
        let layout = state.input_keyboard_content_layout(layout);
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
        let notification = state.to_notification_view_model();
        let time_sync = (state.content_screen() != ShellScreen::CommandLine)
            .then(|| state.to_time_sync_dialog_view_model())
            .flatten();
        PreparedFrame {
            page,
            chrome,
            context,
            notification,
            help: state.help_open.then(|| state.to_page_help_view_model()),
            time_sync,
            progress_running,
        }
    }

    pub(super) fn render_size_notice(
        &mut self,
        frame: &mut Frame<'_>,
        error: crate::startup::terminal_size::ShellTerminalSizeError,
        theme: &ui::TundraTheme,
    ) {
        let capture = self
            .capture_enabled
            .then(|| ui::automation::FrameCapture::begin(frame.area(), "terminal_size"));
        super::runtime::render_terminal_size_notice(frame, error, theme);
        self.snapshot = capture.map(|capture| capture.finish(frame.buffer_mut()));
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
        let mut capture = self.capture_enabled.then(|| {
            ui::automation::FrameCapture::begin(bounds, format!("{:?}", state.content_screen()))
        });
        let overlays = state.interactive_overlays();
        let top_overlay = overlays.last();
        let modal = top_overlay.is_some_and(|overlay| {
            overlay.category == super::overlays::ShellOverlayCategory::ShellModal
        });
        let page_overlay = top_overlay.is_some_and(|overlay| {
            overlay.category != super::overlays::ShellOverlayCategory::ShellModal
        });
        if let Some(capture) = &mut capture {
            capture.set_group("page", "page", bounds, modal || page_overlay);
        }
        if state.terminal_size != (bounds.width, bounds.height) {
            state.details_motion = None;
            state.screen_keyboard.pressed = None;
            state.screen_keyboard.physical.clear();
        }
        state.terminal_size = (bounds.width, bounds.height);
        state.sync_input_keyboard();
        let previous_keyboard = state.input_keyboard_layout().keyboard;
        let keyboard_animating = state.advance_input_keyboard(prepared.context.motion);
        let details_animating = state.advance_management_details(prepared.context.motion);
        if self.keyboard_open != state.input_keyboard_open() {
            self.keyboard_open = state.input_keyboard_open();
            self.motion.cancel_for_bounds_change();
        }
        if previous_keyboard != state.input_keyboard_layout().keyboard {
            self.motion.cancel_for_bounds_change();
        }
        let mut frame_context = prepared.context.clone();
        frame_context.motion = state.interaction_motion_frame(frame_context.motion);
        let hovered = state
            .interaction
            .hover_position()
            .and_then(|point| state.button_at(point));
        let pressed = if let Some((region, _)) = &state.screen_keyboard.pressed {
            Some(region.clone())
        } else if state.auto_admin_visible() {
            state.auto_admin_pressed_button()
        } else {
            state
                .interaction
                .button_capture
                .as_ref()
                .map(|capture| capture.region.clone())
        };
        let mut buttons = ui::components::ButtonFrame::new(
            hovered,
            pressed,
            &frame_context.compatibility_theme(),
        );
        buttons.keyboard_focus_visible = state.interaction.keyboard_focus_visible();
        frame_context.buttons = Some(buttons.clone());
        let context = &frame_context;
        let mut chrome = prepared.chrome.clone();
        chrome.terminal_size = (bounds.width, bounds.height);
        let layout =
            ui::ShellFrameLayout::new(bounds, chrome.status.time_button_label.as_deref(), context)
                .with_back_shortcut(chrome.back_shortcut)
                .with_help_button(state.help_button_available());
        let layout = state.input_keyboard_content_layout(layout);
        // Use the actual frame bounds, including a resize arriving just before draw.
        state.terminal_size = (bounds.width, bounds.height);
        state.refresh_hit_map_with_frame_layout(context.transitions, layout);
        self.motion
            .update_layout(state, &layout, context.theme, context.motion.reduced_motion);
        ui::components::Surface::new().render_frame(frame, bounds, context);
        let management = matches!(prepared.page, ScreenViewModel::Management(_))
            .then(|| state.to_management_view_model());
        let content = management
            .as_ref()
            .map_or_else(|| prepared.page.content(), ui::ScreenContent::Management);
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
            ui::render_compact_home(frame, layout.main, &chrome, &context.compatibility_theme());
        }
        let shell_modal = self.motion.needs_shell_modal_base();
        if !shell_modal {
            self.motion.capture_base(frame.buffer_mut(), state);
        }
        if let Some(capture) = &mut capture
            && overlays.iter().any(|overlay| {
                overlay.category != super::overlays::ShellOverlayCategory::ShellModal
            })
        {
            capture.set_group("page-overlay", "dialog", layout.main, modal);
        }
        if visible_content {
            content.render_overlay(frame, &layout, context);
        }
        if let Some(capture) = &mut capture {
            capture.set_group("chrome", "toolbar", bounds, false);
        }
        ui::render_shell_chrome_with_status_focus(
            frame,
            &layout,
            &chrome,
            context,
            state.pages.common.focused_component == ShellComponent::StatusBar,
        );
        let mut displayed_status = chrome.status.clone();
        if prepared.notification.is_none()
            && prepared.chrome.status.error.is_none()
            && let (Some(toast), Some(area)) = (&self.toast, layout.status_message)
        {
            // Reuse the status button's resolved colors, including hover/press,
            // while keeping the existing Toast timing and semantic border style.
            let mut toast_context = context.clone();
            if area.width > 2 && area.height > 2 {
                toast_context.theme.border = frame.buffer_mut()[(area.x, area.y + 1)].fg;
                toast_context.theme.text = frame.buffer_mut()[(area.x + 1, area.y + 1)].fg;
            }
            toast.render_frame(frame, area, &toast_context);
            if toast.is_visible(context.motion) {
                displayed_status.toast = Some(toast.message.clone());
            }
        }
        state.displayed_status = Some(displayed_status.full_message());
        if shell_modal {
            self.motion.capture_base(frame.buffer_mut(), state);
        }
        if let Some(capture) = &mut capture
            && (prepared.notification.is_some()
                || prepared.help.is_some()
                || prepared.time_sync.is_some())
        {
            capture.set_group(
                "modal",
                "dialog",
                layout.modal_area(),
                state.auto_admin_visible(),
            );
        }
        if let Some(notification) = &prepared.notification {
            ui::render_notification_overlay_with_context(
                frame,
                layout.modal_area(),
                notification,
                context,
            );
        } else if let Some(help) = &prepared.help {
            ui::render_page_help_overlay(
                frame,
                layout.modal_area(),
                help,
                state.help_scroll,
                context,
            );
        } else if let Some(dialog) = &prepared.time_sync {
            ui::render_time_sync_failure_dialog_with_context(
                frame,
                layout.modal_area(),
                dialog,
                context,
            );
        }
        if let Some(auto_admin) = state.auto_admin_view() {
            if let Some(capture) = &mut capture {
                capture.set_group(
                    "authorization",
                    "dialog",
                    state.auto_admin_available_area(),
                    false,
                );
            }
            ui::render_auto_admin_in(
                frame,
                bounds,
                state.auto_admin_available_area(),
                &auto_admin,
                context,
            );
        }
        self.motion.capture_overlay(frame.buffer_mut(), state);
        self.motion
            .process(context.motion.scaled_delta(), frame.buffer_mut(), state);
        if let Some(capture) = &mut capture
            && let Some(launcher) = state.keyboard_launcher_area()
        {
            let keyboard = state.input_keyboard_layout().keyboard;
            let area = if keyboard.is_empty() {
                launcher
            } else {
                keyboard.union(launcher)
            };
            capture.set_group("keyboard", "keyboard", area, false);
        }
        self.render_input_keyboard(frame, state, context);
        state.button_regions = buttons.regions();
        let animation_running = prepared.progress_running
            || (!context.motion.reduced_motion
                && matches!(&prepared.page, ScreenViewModel::Editor(model) if model.vim_mode.is_some()))
            || keyboard_animating
            || details_animating
            || self.motion.is_running()
            || self
                .toast
                .as_ref()
                .is_some_and(|toast| toast.requests_redraw(context.motion));
        self.snapshot = capture.map(|capture| {
            let mut snapshot = capture.finish(frame.buffer_mut());
            snapshot.animation_running = animation_running;
            snapshot.background_tasks_running =
                state.tasks.has_running() || state.auto_admin_running();
            for node in snapshot
                .nodes
                .iter_mut()
                .filter(|node| node.id.starts_with("chrome/") && node.role == "button")
            {
                let point = (
                    node.rect.x + node.rect.width / 2,
                    node.rect.y + node.rect.height / 2,
                );
                node.actionable &= state.button_at(point).is_some_and(|button| {
                    node.id.strip_prefix("chrome/") == Some(button.id.as_str()) && !button.disabled
                });
                if !node.actionable {
                    node.actions.clear();
                }
            }
            let layout_animating =
                keyboard_animating || details_animating || self.motion.is_running();
            let sensitive = snapshot.nodes.iter().any(|node| node.sensitive);
            if layout_animating {
                for node in &mut snapshot.nodes {
                    node.actionable = false;
                }
                if sensitive || self.sensitive_frame {
                    snapshot.lines.iter_mut().for_each(String::clear);
                    for node in &mut snapshot.nodes {
                        node.text.clear();
                        if node.role == "text" {
                            node.name.clear();
                        }
                    }
                }
            }
            self.sensitive_frame = sensitive || (self.sensitive_frame && layout_animating);
            snapshot
        });
        animation_running
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
            context.clone()
        };
        self.synchronize_appearance(state, &context);
        let chrome = state.to_shell_chrome_view_model();
        let layout = ui::ShellFrameLayout::new(
            Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1),
            chrome.status.time_button_label.as_deref(),
            &context,
        )
        .with_back_shortcut(chrome.back_shortcut)
        .with_help_button(state.help_button_available());
        self.motion.update_layout(
            state,
            &state.input_keyboard_content_layout(layout),
            context.theme,
            state
                .interaction_motion_frame(context.motion)
                .reduced_motion,
        );
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
    pub(super) fn prepare_input(
        &mut self,
        state: &mut ShellSession,
        input: InputEvent,
        platform: &dyn Platform,
        received_at: Instant,
    ) -> Option<InputEvent> {
        if matches!(
            input,
            InputEvent::Key(_) | InputEvent::Paste(_) | InputEvent::FocusLost
        ) {
            if let Some(close) = self.motion.cancel_for_suspend(state) {
                state.apply_routed_event(close, platform, received_at);
            }
            self.motion.remember(
                state,
                Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1),
            );
        }
        // AA consumes pointer events before ordinary routing, so gate them here.
        if input.is_mouse() && self.motion.blocks_before_route(&input) {
            return None;
        }
        state.prepare_input(input, platform, received_at)
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
