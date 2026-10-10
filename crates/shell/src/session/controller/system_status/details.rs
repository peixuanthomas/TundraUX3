use crate::session::*;

const STATUS_DETAILS_NOTIFICATION_KEY: &str = "shell.status-details";

impl ShellSession {
    pub(in crate::session) fn status_details_visible(&self) -> bool {
        self.app
            .notification_center()
            .active_modal()
            .is_some_and(|modal| modal.key.as_deref() == Some(STATUS_DETAILS_NOTIFICATION_KEY))
    }

    pub(in crate::session) fn open_status_details(&mut self) {
        // Chrome must never replace or jump ahead of an existing shell modal.
        if self.auto_admin_visible()
            || self.notification_has_active_modal()
            || self.time_sync_dialog_visible
            || self.active_screen() == ShellScreen::ExitConfirm
        {
            return;
        }
        let message = self
            .displayed_status
            .clone()
            .unwrap_or_else(|| self.to_shell_chrome_view_model().status.full_message());
        self.notify_modal_with_options(
            ShellNotification::modal(
                i18n::msg!("shell-status-details-title"),
                message,
                ui::NotificationTone::Info,
                vec![
                    ShellNotificationAction::new("close", i18n::msg!("shell-status-details-close"))
                        .cancel(),
                ],
            )
            .with_key(STATUS_DETAILS_NOTIFICATION_KEY),
        );
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/system_status/details/tests.rs"]
mod tests;
