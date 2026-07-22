use super::super::*;
impl ShellSession {
    pub fn current_time_label(&self) -> String {
        clock_display_label(self.app.current_clock_display())
    }

    pub fn time_sync_failure_dialog_visible(&self) -> bool {
        self.time_sync_dialog_visible
    }

    pub fn time_sync_failure_message(&self) -> Option<&str> {
        self.time_sync_failure_message.as_deref()
    }

    pub fn apply_time_sync_result(&mut self, result: TimeSyncResult) {
        self.time_sync_attempted = true;
        match result {
            Ok(utc) => self.apply_time_sync_success_utc(utc),
            Err(error) => {
                self.app
                    .dispatch_at(app::AppCommand::ApplyTimeSync(Err(error)), Instant::now());
                self.show_time_sync_failure_dialog("联网校准时间失败".to_string());
            }
        }
        self.restore_clock_profile_after_initial_sync();
    }

    #[doc(hidden)]
    pub fn apply_time_sync_utc_for_test(&mut self, utc: DateTime<Utc>) {
        self.apply_time_sync_utc(utc);
    }

    pub(in crate::session) fn apply_time_sync_utc(&mut self, utc: DateTime<Utc>) {
        self.time_sync_attempted = true;
        self.apply_time_sync_success_utc(utc);
        self.restore_clock_profile_after_initial_sync();
    }

    #[doc(hidden)]
    pub fn apply_time_sync_failure_for_test(&mut self, message: &str) {
        self.apply_time_sync_failure_message(message);
    }

    pub(in crate::session) fn apply_time_sync_failure_message(&mut self, message: &str) {
        self.time_sync_attempted = true;
        self.last_time_sync_utc = None;
        let app_snapshot = self.app.snapshot();
        let timezone_id = app_snapshot.clock_timezone_id.map(str::to_owned);
        self.app.dispatch_at(
            app::AppCommand::SetClockTimezone {
                timezone_id,
                synchronized_utc: None,
            },
            Instant::now(),
        );
        self.show_time_sync_failure_dialog(message.to_string());
        self.restore_clock_profile_after_initial_sync();
    }

    pub(in crate::session) fn apply_time_sync_success_utc(&mut self, utc: DateTime<Utc>) {
        self.last_time_sync_utc = Some(utc);
        self.app
            .dispatch_at(app::AppCommand::ApplyTimeSync(Ok(utc)), Instant::now());

        if self.clock_scheduler.is_some() && self.app.auth_session().is_some() {
            let snapshot = self.app.snapshot().clock;
            match self.persist_clock_scheduler_at(&snapshot, Instant::now()) {
                Ok(()) => {
                    self.clock_persist_pending = false;
                    self.clock_pending_due_summary = None;
                    self.resolve_notification_alert(CLOCK_STORAGE_ALERT_KEY);
                }
                Err(error) => {
                    self.clock_persist_pending = true;
                    self.report_clock_storage_error(error);
                }
            }
        }

        if self.time_sync_dialog_visible {
            self.time_sync_dialog_visible = false;
            self.time_sync_failure_message = None;
            self.notification_dismiss_modal_by_key(TIME_SYNC_NOTIFICATION_KEY);
            self.notify_status("Ready");
        }

        self.finish_modal_focus_transition();
        if self.modal_focus_context.is_none() {
            self.refresh_hit_map();
        }
    }

    pub(in crate::session) fn show_time_sync_failure_dialog(&mut self, message: String) {
        self.time_sync_dialog_visible = true;
        self.time_sync_failure_message = Some(message.clone());
        self.active_popup = None;
        self.notify_status(message.clone());
        self.notify_modal_with_options(
            ShellNotification::modal(
                "Time Sync",
                message,
                ui::NotificationTone::Error,
                vec![
                    ShellNotificationAction::new("ok", "OK")
                        .with_shortcut(InputKey::Escape)
                        .cancel()
                        .with_follow_up(ShellCommand::CloseTimeSyncDialog),
                ],
            )
            .with_key(TIME_SYNC_NOTIFICATION_KEY)
            .with_component(ShellComponent::TimeSyncDialog),
        );
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_time_sync_dialog(&mut self) {
        self.time_sync_dialog_visible = false;
        self.time_sync_failure_message = None;
        self.notification_dismiss_modal_by_key(TIME_SYNC_NOTIFICATION_KEY);
        self.notify_status("Ready");
        self.refresh_hit_map();
    }

    pub(in crate::session) fn status_time_button_label(&self) -> Option<String> {
        clock_button_active_for_screen(self.content_screen()).then(|| self.current_time_label())
    }

    pub(in crate::session) fn time_button_selected(&self) -> bool {
        self.focused_component == ShellComponent::ClockButton
            || self.content_screen() == ShellScreen::Clock
    }

    pub(in crate::session) fn replace_storage_config(&mut self, config: storage::StorageConfig) {
        self.app.dispatch_at(
            app::AppCommand::SetStorageConfig {
                config,
                synchronized_utc: self.last_time_sync_utc,
            },
            Instant::now(),
        );
    }
}
