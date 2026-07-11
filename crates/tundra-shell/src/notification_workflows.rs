impl ShellState {
    pub fn status(&self) -> &str {
        self.notifications.status()
    }

    pub fn notify_status(&mut self, message: impl Into<String>) {
        self.notifications.notify_status(message);
    }

    pub fn notify_toast(&mut self, message: impl Into<String>) {
        self.notifications.notify_toast(message);
    }

    pub fn notify_alert(&mut self, message: impl Into<String>) {
        self.notifications
            .notify_alert(message, tundra_ui::NotificationTone::Warning);
    }

    pub fn notify_alert_with_tone(
        &mut self,
        message: impl Into<String>,
        tone: tundra_ui::NotificationTone,
    ) {
        self.notifications.notify_alert(message, tone);
    }

    pub fn notify_alert_with_key(
        &mut self,
        key: impl Into<String>,
        message: impl Into<String>,
        tone: tundra_ui::NotificationTone,
    ) {
        self.notifications.notify_alert_with_key(key, message, tone);
    }

    pub fn resolve_notification_alert(&mut self, key: &str) {
        self.notifications.resolve_alert(key);
    }

    pub fn clear_notification_alert(&mut self) {
        self.notifications.clear_alert();
    }

    pub fn notify_modal(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        tone: tundra_ui::NotificationTone,
        actions: Vec<ShellNotificationAction>,
    ) -> u64 {
        self.notify_modal_with_options(
            ShellNotification::modal(title, message, tone, actions)
                .with_component(ShellComponent::NotificationDialog),
        )
    }

    pub fn take_notification_response(&mut self) -> Option<ShellNotificationResponse> {
        self.notifications.take_response()
    }

    pub fn to_notification_view_model(&self) -> Option<tundra_ui::NotificationViewModel> {
        self.notifications.active_modal_view_model()
    }

    fn capture_modal_focus_context(&mut self) {
        if self.modal_focus_context.is_none() && !self.notifications.has_active_modal() {
            self.modal_focus_context = Some(ModalFocusContext {
                screen: self.active_screen(),
                component: self.focused_component,
            });
            self.modal_focus_prepared_for_follow_up = false;
        }
    }

    fn notify_modal_with_options(&mut self, notification: ShellNotification) -> u64 {
        self.capture_modal_focus_context();
        if !self.notifications.has_active_modal() {
            self.modal_focus_prepared_for_follow_up = false;
        }
        let id = self.notifications.push_modal(notification);
        self.active_popup = None;
        self.notification_pointer_capture = None;
        if let Some(component) = self.notifications.active_modal_component() {
            self.focused_component = component;
        }
        self.refresh_hit_map();
        id
    }

    fn activate_notification_selected(&mut self) -> ShellAction {
        self.notification_pointer_capture = None;
        let follow_up = self.notifications.activate_selected_action();
        self.apply_notification_follow_up(follow_up)
    }

    fn activate_notification_action(&mut self, index: usize) -> ShellAction {
        self.notification_pointer_capture = None;
        let follow_up = self.notifications.activate_action(index);
        self.apply_notification_follow_up(follow_up)
    }

    fn apply_notification_follow_up(&mut self, follow_up: Option<ShellCommand>) -> ShellAction {
        if let Some(component) = self.notifications.active_modal_component() {
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

    fn prepare_modal_focus_for_follow_up(&mut self) {
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

    fn finish_modal_focus_transition(&mut self) {
        if let Some(component) = self.notifications.active_modal_component() {
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
