use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn open_command_line(&mut self) {
        self.open_command_line_at(None);
    }

    pub(in crate::session) fn open_command_line_at(&mut self, directory: Option<PathBuf>) {
        let authorization = PermissionService::new(self.debug_policy).authorize(
            self.app.auth_session(),
            PermissionAction::ExecuteCommandLine,
            Some(app::COMMAND_LINE_APPLICATION.id),
        );
        if !authorization.allowed {
            let message = match authorization.reason.as_deref() {
                Some("not_authenticated") => i18n::LocalizedText::from(i18n::msg!(
                    "shell-login-required-to-use-command-line"
                )),
                _ => i18n::LocalizedText::from(i18n::msg!(
                    "shell-only-administrators-can-use-command-line"
                )),
            };
            self.error_message = Some(message.clone());
            self.notify_alert_with_tone(message, ui::NotificationTone::Error);
            return;
        }

        let (width, height) = self.terminal_size;
        if width < ui::MIN_COMMAND_LINE_TERMINAL_WIDTH
            || height < ui::MIN_COMMAND_LINE_TERMINAL_HEIGHT
        {
            let message = i18n::LocalizedText::from(i18n::msg!(
                "shell-command-line-needs-at-least-arg1xarg2-terminal-cells-current-widthxheight",
                arg1 = ui::MIN_COMMAND_LINE_TERMINAL_WIDTH,
                arg2 = ui::MIN_COMMAND_LINE_TERMINAL_HEIGHT,
                width = width,
                height = height
            ));
            self.update_launcher_state(|state| state.error = Some(message.clone()));
            self.notify_alert_with_tone(message, ui::NotificationTone::Error);
            return;
        }

        if self.active_screen() != ShellScreen::CommandLine {
            self.command_line_start_directory = directory;
            self.enter_screen(ShellScreen::CommandLine);
        }
        self.focused_component = ShellComponent::CommandLine;
        self.launcher_pending_confirmation = None;
        self.launcher_drag = None;
        self.notify_status(i18n::LocalizedText::from(i18n::msg!("shell-command-line")));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_command_line(&mut self) {
        self.command_line_start_directory = None;
        self.return_from_screen(ShellScreen::CommandLine);
    }
}
