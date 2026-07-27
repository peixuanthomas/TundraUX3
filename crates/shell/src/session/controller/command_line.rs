use super::super::*;

impl ShellSession {
    pub(in crate::session) fn open_command_line(&mut self) {
        let authorization = PermissionService::new(self.debug_policy).authorize(
            self.app.auth_session(),
            PermissionAction::ExecuteCommandLine,
            Some(app::COMMAND_LINE_APPLICATION.id),
        );
        if !authorization.allowed {
            let message = match authorization.reason.as_deref() {
                Some("not_authenticated") => "Login required to use Command Line",
                _ => "Only administrators can use Command Line",
            };
            self.error_message = Some(message.to_string());
            self.notify_alert_with_tone(message, ui::NotificationTone::Error);
            return;
        }

        let (width, height) = self.terminal_size;
        if width < ui::MIN_COMMAND_LINE_TERMINAL_WIDTH
            || height < ui::MIN_COMMAND_LINE_TERMINAL_HEIGHT
        {
            let message = format!(
                "Command Line needs at least {}x{} terminal cells (current: {width}x{height})",
                ui::MIN_COMMAND_LINE_TERMINAL_WIDTH,
                ui::MIN_COMMAND_LINE_TERMINAL_HEIGHT,
            );
            self.update_launcher_state(|state| state.error = Some(message.clone()));
            self.notify_alert_with_tone(message, ui::NotificationTone::Error);
            return;
        }

        if self.active_screen() != ShellScreen::CommandLine {
            self.screen_stack.push(ShellScreen::CommandLine);
        }
        self.focused_component = ShellComponent::CommandLine;
        self.launcher_pending_confirmation = None;
        self.launcher_drag = None;
        self.notify_status("Command Line");
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_command_line(&mut self) {
        if self.active_screen() == ShellScreen::CommandLine {
            self.screen_stack.pop();
        }
        if self.active_screen() == ShellScreen::Launcher {
            self.focused_component = ShellComponent::Launcher;
            self.notify_status("Launcher");
        } else {
            self.pop_to_home();
            self.notify_status("Ready");
        }
        self.refresh_hit_map();
    }
}
