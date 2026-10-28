use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn open_command_line(&mut self) {
        self.open_command_line_at(None);
    }

    pub(in crate::session) fn open_command_line_at(&mut self, directory: Option<PathBuf>) {
        self.open_terminal_page(directory, None);
    }

    pub(in crate::session) fn open_external_terminal(
        &mut self,
        program: PathBuf,
        args: Vec<String>,
        directory: Option<PathBuf>,
    ) {
        self.open_terminal_page(directory, Some(ExternalTerminalLaunch { program, args }));
    }

    fn open_terminal_page(
        &mut self,
        directory: Option<PathBuf>,
        external: Option<ExternalTerminalLaunch>,
    ) {
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
            self.pages.common.error_message = Some(message.clone());
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

        if external.is_some() || self.active_screen() != ShellScreen::CommandLine {
            self.enter_new_page(ShellScreen::CommandLine);
            self.pages.command_line.command_line_start_directory = directory;
            self.pages.command_line.command_line_external = external;
        }
        self.pages.common.focused_component = ShellComponent::CommandLine;
        self.pages.launcher.launcher_pending_confirmation = None;
        self.pages.launcher.launcher_drag = None;
        let title = self
            .pages
            .command_line
            .command_line_external
            .as_ref()
            .map_or_else(
                || i18n::LocalizedText::from(i18n::msg!("shell-command-line")),
                |launch| i18n::LocalizedText::from(launch.title()),
            );
        self.notify_status(title);
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_command_line(&mut self) {
        self.return_from_screen(ShellScreen::CommandLine);
    }

    pub(in crate::session) fn request_command_line_close(&mut self) {
        if self.active_screen() == ShellScreen::CommandLine
            && self.interactive_overlays().is_empty()
        {
            self.command_line_close_request = Some((self.active_page_id(), false));
        }
    }

    pub(in crate::session) fn confirm_command_line_close(&mut self, id: PageInstanceId) {
        if self.active_screen() == ShellScreen::CommandLine && self.active_page_id() == id {
            self.command_line_close_request = Some((id, true));
        }
    }

    pub(in crate::session) fn show_command_line_close_confirmation(&mut self, id: PageInstanceId) {
        let external = self
            .pages
            .command_line
            .get(id)
            .and_then(|page| page.command_line_external.as_ref());
        let (title, message) = external.map_or_else(
            || {
                (
                    i18n::msg!("shell-command-line-close-title"),
                    i18n::msg!("shell-command-line-close-message"),
                )
            },
            |launch| {
                (
                    i18n::msg!("shell-terminal-close-title"),
                    i18n::msg!("shell-terminal-close-message", program = launch.title()),
                )
            },
        );
        self.notify_modal(
            title,
            message,
            ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new(
                    "continue-command-line",
                    i18n::msg!("shell-command-line-close-continue"),
                )
                .cancel(),
                ShellNotificationAction::new(
                    "terminate-command-line",
                    i18n::msg!("shell-command-line-close-confirm"),
                )
                .with_follow_up(ShellCommand::ConfirmCommandLineClose(id)),
            ],
        );
    }
}
