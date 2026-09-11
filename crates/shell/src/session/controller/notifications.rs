use super::super::*;
use i18n::LocalizedText;
impl ShellSession {
    pub fn status_text(&self) -> &LocalizedText {
        self.app.notification_center().status()
    }

    /// Render on demand using this session's snapshot, including outside a draw scope.
    pub fn status(&self) -> String {
        self.language.render_text(self.status_text())
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
        self.ui.notification_message_scroll = 0;
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
        let previous_modal = self.notification_active_modal_id();
        self.app
            .dispatch_at(app::AppCommand::Notification(command), at);
        if previous_modal != self.notification_active_modal_id() {
            self.ui.notification_message_scroll = 0;
        }
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
        let _language = i18n::enter_snapshot(self.language.clone());
        let mut model = self
            .ui
            .notification_bindings
            .active_view_model(self.app.notification_center())?;
        model.stacked_actions =
            self.notification_active_modal_component() == Some(ShellComponent::ExitDialog);
        model.scroll_offset = self.ui.notification_message_scroll;
        Some(model)
    }

    /// Scroll wrapped message lines without changing the selected notification action.
    pub(in crate::session) fn scroll_notification_message(
        &mut self,
        delta: isize,
        page: bool,
    ) -> ShellAction {
        let _language = i18n::enter_snapshot(self.language.clone());
        self.notification_pointer_capture = None;
        let Some(model) = self.notification_active_modal_view_model() else {
            self.ui.notification_message_scroll = 0;
            return ShellAction::Redraw;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::NotificationLayout::Dialog(layout) = ui::notification_layout(area, &model) else {
            return ShellAction::Redraw;
        };
        let step = if page {
            usize::from(layout.message.height).max(1)
        } else {
            1
        };
        let distance = delta.unsigned_abs().saturating_mul(step);
        self.ui.notification_message_scroll = if delta < 0 {
            layout.scroll_offset.saturating_sub(distance)
        } else {
            layout
                .scroll_offset
                .saturating_add(distance)
                .min(layout.max_scroll_offset)
        };
        ShellAction::Redraw
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
        let previous_modal = self.notification_active_modal_id();
        let id = self.app.push_notification_modal(app_notification);
        if previous_modal != self.notification_active_modal_id()
            || self.notification_active_modal_id() == Some(id)
        {
            self.ui.notification_message_scroll = 0;
        }
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
        let previous_modal = self.notification_active_modal_id();
        let response = self.app.activate_selected_notification_action();
        if previous_modal != self.notification_active_modal_id() {
            self.ui.notification_message_scroll = 0;
        }
        let follow_up = response
            .as_ref()
            .and_then(|response| self.ui.notification_bindings.take_follow_up(response));
        self.apply_notification_follow_up(follow_up)
    }

    pub(in crate::session) fn activate_notification_action(&mut self, index: usize) -> ShellAction {
        self.notification_pointer_capture = None;
        let previous_modal = self.notification_active_modal_id();
        let response = self.app.activate_notification_action(index);
        if previous_modal != self.notification_active_modal_id() {
            self.ui.notification_message_scroll = 0;
        }
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
        self.ui.notification_message_scroll = 0;
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

#[cfg(test)]
mod scrolling_tests {
    use super::*;

    #[test]
    fn notification_public_text_uses_session_snapshot_outside_draw_scope() {
        let root = std::env::temp_dir().join(format!(
            "tux3-notification-snapshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let canonical =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets/locales");
        for code in ["en-US", "zh-CN"] {
            let locale = root.join("locales").join(code);
            std::fs::create_dir_all(locale.join("modules")).unwrap();
            for relative in ["manifest.toml", "modules/shell-messages.ftl"] {
                std::fs::copy(canonical.join(code).join(relative), locale.join(relative)).unwrap();
            }
        }
        let [english, chinese] = ["en-US", "zh-CN"].map(|code| {
            std::sync::Arc::new(
                i18n::LanguageSnapshot::load(&root, code, 1)
                    .unwrap()
                    .snapshot,
            )
        });
        std::fs::remove_dir_all(root).unwrap();
        let mut session = ShellSession::new(ShellLaunchConfig::default(), (50, 12));
        session.language = chinese;
        let _ambient = i18n::enter_snapshot(english);
        session.notify_status(i18n::msg!("shell-ready"));
        session.notify_modal(
            i18n::msg!("shell-explorer"),
            i18n::msg!("shell-saving-arg1", arg1 = "/tmp/{ready}.txt"),
            ui::NotificationTone::Info,
            vec![ShellNotificationAction::new(
                "ready",
                i18n::msg!("shell-ready"),
            )],
        );
        assert_eq!(session.status(), "就绪");
        let model = session.to_notification_view_model().unwrap();
        assert_eq!(model.title, "文件管理器");
        assert_eq!(model.message, "正在保存 /tmp/{ready}.txt");
        assert_eq!(model.actions[0].label, "就绪");
        session.scroll_notification_message(1, true);
        assert_eq!(i18n::tr!("shell-ready"), "Ready");
    }

    fn session_with_long_notification() -> ShellSession {
        let mut session = ShellSession::new(ShellLaunchConfig::default(), (50, 12));
        session.notify_modal(
            "语言资源已修复",
            (0..50)
                .map(|line| format!("已修复文件 {line:02}：中文资源.ftl"))
                .collect::<Vec<_>>()
                .join("\n"),
            ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("continue", "继续启动"),
                ShellNotificationAction::new("exit", "安全退出").cancel(),
            ],
        );
        session
    }

    fn dialog(session: &ShellSession) -> ui::NotificationDialogLayout {
        let model = session.to_notification_view_model().unwrap();
        let area = Rect::new(0, 0, session.terminal_size.0, session.terminal_size.1);
        let ui::NotificationLayout::Dialog(layout) = ui::notification_layout(area, &model) else {
            panic!("notification actions should fit");
        };
        layout
    }

    #[test]
    fn notification_scroll_moves_by_pages_and_lines_without_changing_actions_or_hits() {
        let mut session = session_with_long_notification();
        let first = dialog(&session);
        assert!(first.max_scroll_offset > 30);
        session.scroll_notification_message(1, true);
        assert_eq!(
            dialog(&session).scroll_offset,
            usize::from(first.message.height)
        );
        session.scroll_notification_message(-3, false);
        assert_eq!(
            dialog(&session).scroll_offset,
            usize::from(first.message.height).saturating_sub(3)
        );
        session.scroll_notification_message(isize::MAX, true);
        let last = dialog(&session);
        assert_eq!(last.scroll_offset, last.max_scroll_offset);
        assert_eq!(last.actions, first.actions);
        assert!(session.to_notification_view_model().unwrap().actions[0].selected);
        for action in &last.actions {
            assert_eq!(
                session.notification_action_index_at((action.area.x, action.area.y)),
                Some(action.index)
            );
            assert_eq!(
                session.notification_action_index_at((
                    action.area.right() - 1,
                    action.area.bottom() - 1
                )),
                Some(action.index)
            );
        }
        session.terminal_size = (80, 24);
        session.scroll_notification_message(0, false);
        assert_eq!(
            session.ui.notification_message_scroll,
            dialog(&session).max_scroll_offset
        );
        session.scroll_notification_message(isize::MIN, true);
        assert_eq!(
            session.to_notification_view_model().unwrap().scroll_offset,
            0
        );
    }

    #[test]
    fn notification_scroll_resets_between_modals_but_not_when_a_following_modal_is_queued() {
        let mut session = session_with_long_notification();
        session.scroll_notification_message(2, true);
        let scrolled = session.ui.notification_message_scroll;
        assert!(scrolled > 0);
        let next = session.notify_modal(
            "完成",
            "下一条通知",
            ui::NotificationTone::Info,
            vec![ShellNotificationAction::new("ok", "确认")],
        );
        assert_eq!(session.ui.notification_message_scroll, scrolled);
        session.activate_notification_selected();
        assert_eq!(session.notification_active_modal_id(), Some(next));
        assert_eq!(
            session.to_notification_view_model().unwrap().scroll_offset,
            0
        );
        session.scroll_notification_message(100, true);
        assert_eq!(session.ui.notification_message_scroll, 0);
        session.activate_notification_selected();
        assert!(!session.notification_has_active_modal());
        assert_eq!(session.ui.notification_message_scroll, 0);
    }

    #[test]
    fn critical_notification_preemption_starts_at_the_beginning() {
        let mut session = session_with_long_notification();
        session.scroll_notification_message(2, true);
        session.notify_critical_modal(
            "需要关注",
            "关键通知",
            vec![ShellNotificationAction::new("continue", "继续")],
        );
        assert_eq!(
            session.to_notification_view_model().unwrap().scroll_offset,
            0
        );
        session.activate_notification_selected();
        assert!(session.notification_has_active_modal());
        assert_eq!(
            session.to_notification_view_model().unwrap().scroll_offset,
            0
        );
    }
}
