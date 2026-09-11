use super::super::*;
use i18n::LocalizedText;
impl ShellSession {
    pub fn status_text(&self) -> &LocalizedText {
        self.app.notification_center().status()
    }

    /// Render on demand using the current presentation's scoped locale snapshot.
    pub fn status(&self) -> String {
        self.status_text().render_current()
    }

    pub fn notify_status(&mut self, message: impl Into<LocalizedText>) {
        self.dispatch_notification(
            app::NotificationCommand::SetStatus(message.into()),
            Instant::now(),
        );
    }

    pub fn notify_toast(&mut self, message: impl Into<LocalizedText>) {
        self.dispatch_notification(
            app::NotificationCommand::ShowToast(message.into()),
            Instant::now(),
        );
    }

    pub fn notify_alert(&mut self, message: impl Into<LocalizedText>) {
        self.notify_alert_with_tone(message, ui::NotificationTone::Warning);
    }

    pub fn notify_alert_with_tone(
        &mut self,
        message: impl Into<LocalizedText>,
        tone: ui::NotificationTone,
    ) {
        self.notify_alert_with_key(DEFAULT_ALERT_KEY, message, tone);
    }

    pub fn notify_alert_with_key(
        &mut self,
        key: impl Into<String>,
        message: impl Into<LocalizedText>,
        tone: ui::NotificationTone,
    ) {
        self.dispatch_notification(
            app::NotificationCommand::ShowAlert {
                key: key.into(),
                message: message.into(),
                tone,
            },
            Instant::now(),
        );
    }

    pub fn resolve_notification_alert(&mut self, key: &str) {
        self.resolve_notification_alert_at(key, Instant::now());
    }

    pub fn clear_notification_alert(&mut self) {
        self.dispatch_notification(app::NotificationCommand::ClearAlerts, Instant::now());
    }

    pub fn notify_modal(
        &mut self,
        title: impl Into<LocalizedText>,
        message: impl Into<LocalizedText>,
        tone: ui::NotificationTone,
        actions: Vec<ShellNotificationAction>,
    ) -> u64 {
        self.notify_modal_with_options(
            ShellNotification::modal(title, message, tone, actions)
                .with_component(ShellComponent::NotificationDialog),
        )
    }

    pub fn notify_critical_modal(
        &mut self,
        title: impl Into<LocalizedText>,
        message: impl Into<LocalizedText>,
        actions: Vec<ShellNotificationAction>,
    ) -> u64 {
        self.capture_modal_focus_context();
        let notification =
            ShellNotification::modal(title, message, ui::NotificationTone::Critical, actions)
                .with_component(ShellComponent::NotificationDialog);
        let app_notification = notification.to_app_notification();
        let id = self.app.push_critical_notification_modal(app_notification);
        self.ui.notification_bindings.bind(id, &notification);
        self.active_popup = None;
        self.notification_pointer_capture = None;
        self.modal_focus_prepared_for_follow_up = false;
        if let Some(component) = self.notification_active_modal_component() {
            self.focused_component = component;
        }
        self.refresh_hit_map();
        id
    }

    pub fn take_notification_response(&mut self) -> Option<ShellNotificationResponse> {
        self.app.take_notification_response().map(Into::into)
    }

    pub fn to_notification_view_model(&self) -> Option<ui::NotificationViewModel> {
        self.notification_active_modal_view_model()
    }

    pub(in crate::session) fn dispatch_notification(
        &mut self,
        command: app::NotificationCommand,
        at: Instant,
    ) {
        // Record alert lifecycle metadata only. The producing operation owns the
        // failure record; notification text may contain user content.
        let transition = match &command {
            app::NotificationCommand::ShowAlert { key, message, .. }
                if self.app.notification_center().alert_message_for_key(key) != Some(message) =>
            {
                Some((key.clone(), "alert_shown"))
            }
            app::NotificationCommand::ResolveAlert(key)
                if self
                    .app
                    .notification_center()
                    .alert_message_for_key(key)
                    .is_some() =>
            {
                Some((key.clone(), "alert_resolved"))
            }
            app::NotificationCommand::ClearAlerts
                if self.app.notification_center().alert_count() > 0 =>
            {
                Some((String::new(), "alerts_cleared"))
            }
            _ => None,
        };
        self.app
            .dispatch_at(app::AppCommand::Notification(command), at);
        if let Some((key, operation)) = transition {
            let mut event = runtime_log::RuntimeLogEvent::new(
                self.operation_log_context("ux.notifications", operation),
                runtime_log::LogLevel::Info,
                runtime_log::LogPhase::Observed,
                "Notification state changed",
            );
            event.alert_key = (!key.is_empty()).then_some(key);
            record_shell_runtime_event(event);
        }
    }

    pub(in crate::session) fn notification_expire(&mut self, now: Instant) {
        self.app.dispatch_at(app::AppCommand::Tick, now);
    }

    pub(in crate::session) fn notification_tick(&mut self) {
        self.notification_expire(Instant::now());
    }

    pub(in crate::session) fn notification_poll_timeout(
        &self,
        now: Instant,
        maximum: Duration,
    ) -> Duration {
        self.app.notification_center().poll_timeout(now, maximum)
    }

    pub(in crate::session) fn notification_has_active_modal(&self) -> bool {
        self.app.notification_center().active_modal().is_some()
    }

    pub(in crate::session) fn notification_active_modal_id(&self) -> Option<u64> {
        self.app.notification_center().active_modal_id()
    }

    pub(in crate::session) fn notification_active_modal_component(&self) -> Option<ShellComponent> {
        self.ui
            .notification_bindings
            .active_component(self.app.notification_center())
    }

    pub(in crate::session) fn notification_active_modal_view_model(
        &self,
    ) -> Option<ui::NotificationViewModel> {
        let mut model = self
            .ui
            .notification_bindings
            .active_view_model(self.app.notification_center())?;
        model.stacked_actions =
            self.notification_active_modal_component() == Some(ShellComponent::ExitDialog);
        Some(model)
    }

    pub(in crate::session) fn notification_action_index_for_input(
        &self,
        input: &KeyInput,
    ) -> Option<usize> {
        self.ui
            .notification_bindings
            .action_index_for_input(self.app.notification_center(), input)
    }

    pub(in crate::session) fn notification_select_next_action(&mut self) {
        self.dispatch_notification(app::NotificationCommand::SelectNextAction, Instant::now());
    }

    pub(in crate::session) fn notification_select_previous_action(&mut self) {
        self.dispatch_notification(
            app::NotificationCommand::SelectPreviousAction,
            Instant::now(),
        );
    }

    pub(in crate::session) fn notification_select_action(&mut self, index: usize) {
        self.dispatch_notification(
            app::NotificationCommand::SelectAction(index),
            Instant::now(),
        );
    }

    pub(in crate::session) fn notification_cancel_action_index(&self) -> Option<usize> {
        self.app.notification_center().cancel_action_index()
    }

    pub(in crate::session) fn notification_explicit_cancel_action_index(&self) -> Option<usize> {
        self.app
            .notification_center()
            .explicit_cancel_action_index()
    }

    pub(in crate::session) fn notification_dismiss_active_modal_without_response(
        &mut self,
    ) -> bool {
        let had_active = self.notification_has_active_modal();
        self.dispatch_notification(app::NotificationCommand::DismissActiveModal, Instant::now());
        self.ui
            .notification_bindings
            .prune(self.app.notification_center());
        had_active
    }

    pub(in crate::session) fn notification_dismiss_modal_by_key(&mut self, key: &str) {
        self.dispatch_notification(
            app::NotificationCommand::DismissModalByKey(key.to_string()),
            Instant::now(),
        );
        self.ui
            .notification_bindings
            .prune(self.app.notification_center());
    }

    pub(in crate::session) fn resolve_notification_alert_at(&mut self, key: &str, now: Instant) {
        self.dispatch_notification(app::NotificationCommand::ResolveAlert(key.to_string()), now);
    }

    pub(in crate::session) fn notification_alert_message_for_key(
        &self,
        key: &str,
    ) -> Option<&LocalizedText> {
        self.app.notification_center().alert_message_for_key(key)
    }

    pub(in crate::session) fn capture_modal_focus_context(&mut self) {
        if self.modal_focus_context.is_none() && !self.notification_has_active_modal() {
            self.modal_focus_context = Some(ModalFocusContext {
                screen: self.active_screen(),
                component: self.focused_component,
            });
            self.modal_focus_prepared_for_follow_up = false;
        }
    }

    pub(in crate::session) fn notify_modal_with_options(
        &mut self,
        notification: ShellNotification,
    ) -> u64 {
        self.capture_modal_focus_context();
        if !self.notification_has_active_modal() {
            self.modal_focus_prepared_for_follow_up = false;
        }
        let app_notification = notification.to_app_notification();
        let id = self.app.push_notification_modal(app_notification);
        self.ui.notification_bindings.bind(id, &notification);
        self.active_popup = None;
        self.notification_pointer_capture = None;
        if let Some(component) = self.notification_active_modal_component() {
            self.focused_component = component;
        }
        self.refresh_hit_map();
        id
    }

    pub(in crate::session) fn activate_notification_selected(&mut self) -> ShellAction {
        self.notification_pointer_capture = None;
        let response = self.app.activate_selected_notification_action();
        let follow_up = response
            .as_ref()
            .and_then(|response| self.ui.notification_bindings.take_follow_up(response));
        self.apply_notification_follow_up(follow_up)
    }

    pub(in crate::session) fn activate_notification_action(&mut self, index: usize) -> ShellAction {
        self.notification_pointer_capture = None;
        let response = self.app.activate_notification_action(index);
        let follow_up = response
            .as_ref()
            .and_then(|response| self.ui.notification_bindings.take_follow_up(response));
        self.apply_notification_follow_up(follow_up)
    }

    pub(in crate::session) fn apply_notification_follow_up(
        &mut self,
        follow_up: Option<ShellCommand>,
    ) -> ShellAction {
        if let Some(component) = self.notification_active_modal_component() {
            self.focused_component = component;
            self.refresh_hit_map();
        } else {
            self.prepare_modal_focus_for_follow_up();
        }

        if let Some(command) = follow_up {
            self.pending_notification_commands.push_back(command);
        }
        ShellAction::Redraw
    }

    pub(in crate::session) fn prepare_modal_focus_for_follow_up(&mut self) {
        if self.modal_focus_prepared_for_follow_up {
            return;
        }
        let Some(context) = self.modal_focus_context else {
            return;
        };
        if self.active_screen() != context.screen {
            return;
        }

        self.focused_component = context.component;
        if let Some(field) = setup_field_for_component(context.component) {
            self.setup_focused_field = field;
        }
        self.modal_focus_prepared_for_follow_up = true;
    }

    pub(in crate::session) fn finish_modal_focus_transition(&mut self) {
        if let Some(component) = self.notification_active_modal_component() {
            self.focused_component = component;
            self.refresh_hit_map();
            return;
        }

        self.notification_pointer_capture = None;
        let Some(context) = self.modal_focus_context.take() else {
            self.modal_focus_prepared_for_follow_up = false;
            return;
        };
        let focus_was_prepared = self.modal_focus_prepared_for_follow_up;
        self.modal_focus_prepared_for_follow_up = false;
        if self.active_screen() == context.screen && !focus_was_prepared {
            self.focused_component = context.component;
            if let Some(field) = setup_field_for_component(context.component) {
                self.setup_focused_field = field;
            }
        }
        self.refresh_hit_map();
    }
}
