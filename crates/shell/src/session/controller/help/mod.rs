mod input;
use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn open_page_help(&mut self) -> ShellAction {
        if self.help_open
            || !self.help_button_available()
            || self.auto_admin_visible()
            || self.input_keyboard_open()
            || self
                .interactive_overlays()
                .iter()
                .any(|overlay| overlay.target.is_some())
        {
            return ShellAction::Redraw;
        }
        self.capture_modal_focus_context();
        self.help_open = true;
        self.help_scroll = 0;
        self.help_scrollbar_drag = None;
        self.synchronize_overlay_focus();
        self.refresh_hit_map();
        ShellAction::Redraw
    }

    pub(in crate::session) fn close_page_help(&mut self) -> ShellAction {
        self.help_open = false;
        self.help_scroll = 0;
        self.help_scrollbar_drag = None;
        self.synchronize_overlay_focus();
        self.refresh_hit_map();
        ShellAction::Redraw
    }

    pub(in crate::session) fn to_page_help_view_model(&self) -> ui::PageHelpViewModel {
        let screen = match self.content_screen() {
            ShellScreen::Home | ShellScreen::ExitConfirm => ui::PageHelpScreen::Home,
            ShellScreen::FirstRunSetup => ui::PageHelpScreen::Setup,
            ShellScreen::Login => ui::PageHelpScreen::Login,
            ShellScreen::BootstrapAdmin => ui::PageHelpScreen::BootstrapAdmin,
            ShellScreen::UserManagement => ui::PageHelpScreen::UserManagement,
            ShellScreen::Explorer => ui::PageHelpScreen::Explorer,
            ShellScreen::Launcher => ui::PageHelpScreen::Launcher,
            ShellScreen::CommandLine => ui::PageHelpScreen::CommandLine,
            ShellScreen::Editor if self.pages.editor.script.enabled => ui::PageHelpScreen::Script,
            ShellScreen::Editor => ui::PageHelpScreen::Editor,
            ShellScreen::Settings => ui::PageHelpScreen::Settings,
            ShellScreen::Logs => ui::PageHelpScreen::Logs,
            ShellScreen::Management => self.management_help_screen(),
            ShellScreen::Diagnostics => ui::PageHelpScreen::Diagnostics,
            ShellScreen::SystemStatus => ui::PageHelpScreen::SystemStatus,
            ShellScreen::Clock => ui::PageHelpScreen::Clock,
        };
        ui::page_help(screen)
    }

    pub(in crate::session) fn page_help_layout(&self) -> ui::NotificationLayout {
        let model = ui::page_help_notification(&self.to_page_help_view_model(), self.help_scroll);
        ui::notification_layout(self.shell_modal_area(), &model)
    }
}
