use crate::{InputKey, InputModifiers, InputPhase, KeyInput, ShellCommand, ShellComponent};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNotificationAction {
    pub id: String,
    pub label: String,
    pub shortcut: Option<InputKey>,
    pub cancel: bool,
    pub follow_up: Option<ShellCommand>,
}

impl ShellNotificationAction {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            shortcut: None,
            cancel: false,
            follow_up: None,
        }
    }

    pub fn with_shortcut(mut self, shortcut: InputKey) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn cancel(mut self) -> Self {
        self.cancel = true;
        self
    }

    pub fn with_follow_up(mut self, command: ShellCommand) -> Self {
        self.follow_up = Some(command);
        self
    }

    fn shortcut_label(&self) -> Option<String> {
        self.shortcut.as_ref().map(InputKey::label)
    }

    fn matches_shortcut(&self, input: &KeyInput) -> bool {
        if input.phase != InputPhase::Press || input.has_non_shift_modifier() {
            return false;
        }
        if input.modifiers.shift
            && !matches!(
                input.key,
                InputKey::Char(character) if character.is_ascii_alphabetic()
            )
        {
            return false;
        }
        match (&self.shortcut, &input.key) {
            (Some(InputKey::Char(expected)), InputKey::Char(actual)) => {
                expected.eq_ignore_ascii_case(actual)
            }
            (Some(expected), actual) => expected == actual,
            _ => false,
        }
    }

    fn to_app_action(&self, selected: bool) -> app::NotificationAction {
        app::NotificationAction::new(&self.id, &self.label)
            .selected(selected)
            .with_cancel(self.cancel)
    }
}

trait AppNotificationActionExt {
    fn with_cancel(self, cancel: bool) -> Self;
}

impl AppNotificationActionExt for app::NotificationAction {
    fn with_cancel(self, cancel: bool) -> Self {
        if cancel { self.cancel() } else { self }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNotification {
    pub id: u64,
    pub key: Option<String>,
    pub level: ui::NotificationLevel,
    pub tone: ui::NotificationTone,
    pub component: ShellComponent,
    pub title: String,
    pub message: String,
    pub actions: Vec<ShellNotificationAction>,
    selected_action: usize,
}

impl ShellNotification {
    pub fn modal(
        title: impl Into<String>,
        message: impl Into<String>,
        tone: ui::NotificationTone,
        actions: Vec<ShellNotificationAction>,
    ) -> Self {
        Self {
            id: 0,
            key: None,
            level: ui::NotificationLevel::Modal,
            tone,
            component: ShellComponent::NotificationDialog,
            title: title.into(),
            message: message.into(),
            actions: non_empty_notification_actions(actions),
            selected_action: 0,
        }
    }

    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn with_component(mut self, component: ShellComponent) -> Self {
        self.component = component;
        self
    }

    pub(crate) fn with_selected_action(mut self, index: usize) -> Self {
        self.selected_action = index.min(self.actions.len().saturating_sub(1));
        self
    }

    pub(crate) fn to_app_notification(&self) -> app::Notification {
        let notification = app::Notification::modal(
            &self.title,
            &self.message,
            self.tone,
            self.actions
                .iter()
                .enumerate()
                .map(|(index, action)| action.to_app_action(index == self.selected_action))
                .collect(),
        );
        match &self.key {
            Some(key) => notification.with_key(key),
            None => notification,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNotificationResponse {
    pub notification_id: u64,
    pub action_id: String,
}

impl From<app::NotificationResponse> for ShellNotificationResponse {
    fn from(response: app::NotificationResponse) -> Self {
        Self {
            notification_id: response.notification_id,
            action_id: response.action_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NotificationBinding {
    component: ShellComponent,
    actions: Vec<ShellNotificationAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NotificationBindings {
    modals: HashMap<u64, NotificationBinding>,
}

impl NotificationBindings {
    pub(crate) fn bind(&mut self, id: u64, notification: &ShellNotification) {
        self.modals.insert(
            id,
            NotificationBinding {
                component: notification.component,
                actions: notification.actions.clone(),
            },
        );
    }

    pub(crate) fn prune(&mut self, center: &app::NotificationCenter) {
        self.modals.retain(|id, _| center.contains_modal_id(*id));
    }

    pub(crate) fn active_component(
        &self,
        center: &app::NotificationCenter,
    ) -> Option<ShellComponent> {
        let id = center.active_modal_id()?;
        Some(
            self.modals
                .get(&id)
                .map(|binding| binding.component)
                .unwrap_or(ShellComponent::NotificationDialog),
        )
    }

    pub(crate) fn active_view_model(
        &self,
        center: &app::NotificationCenter,
    ) -> Option<ui::NotificationViewModel> {
        let modal = center.active_modal()?;
        let binding = self.modals.get(&modal.id);
        Some(ui::NotificationViewModel::new(
            modal.id.to_string(),
            modal.level,
            modal.tone,
            modal.title.clone(),
            modal.message.clone(),
            modal
                .actions
                .iter()
                .map(|action| {
                    let mut view = ui::NotificationActionViewModel::new(&action.id, &action.label);
                    if let Some(shortcut) = binding
                        .and_then(|binding| {
                            binding.actions.iter().find(|bound| bound.id == action.id)
                        })
                        .and_then(ShellNotificationAction::shortcut_label)
                    {
                        view = view.with_shortcut(shortcut);
                    }
                    view.selected(action.selected)
                })
                .collect(),
        ))
    }

    pub(crate) fn action_index_for_input(
        &self,
        center: &app::NotificationCenter,
        input: &KeyInput,
    ) -> Option<usize> {
        let modal = center.active_modal()?;
        let binding = self.modals.get(&modal.id)?;
        modal.actions.iter().position(|action| {
            binding
                .actions
                .iter()
                .find(|bound| bound.id == action.id)
                .is_some_and(|bound| bound.matches_shortcut(input))
        })
    }

    pub(crate) fn take_follow_up(
        &mut self,
        response: &app::NotificationResponse,
    ) -> Option<ShellCommand> {
        self.modals
            .remove(&response.notification_id)
            .and_then(|binding| {
                binding
                    .actions
                    .into_iter()
                    .find(|action| action.id == response.action_id)
            })
            .and_then(|action| action.follow_up)
    }
}

/// Standalone compatibility facade. `ShellSession` stores only `NotificationBindings`;
/// the actual notification domain state lives in `app::AppState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCenter {
    core: app::NotificationCenter,
    bindings: NotificationBindings,
}

impl NotificationCenter {
    pub fn new(status: impl Into<String>) -> Self {
        Self {
            core: app::NotificationCenter::new(status),
            bindings: NotificationBindings::default(),
        }
    }

    pub fn notify_status(&mut self, message: impl Into<String>) {
        self.core.notify_status(message);
    }
    pub fn notify_toast(&mut self, message: impl Into<String>) {
        self.core.notify_toast(message);
    }
    #[cfg(test)]
    pub(crate) fn notify_toast_at(&mut self, message: impl Into<String>, now: Instant) {
        self.core.notify_toast_at(message, now);
    }
    pub fn notify_alert(&mut self, message: impl Into<String>, tone: ui::NotificationTone) {
        self.core.notify_alert(message, tone);
    }
    pub fn notify_alert_with_key(
        &mut self,
        key: impl Into<String>,
        message: impl Into<String>,
        tone: ui::NotificationTone,
    ) {
        self.core.notify_alert_with_key(key, message, tone);
    }
    pub fn resolve_alert(&mut self, key: &str) {
        self.core.resolve_alert(key);
    }
    pub fn clear_alert(&mut self) {
        self.core.clear_alerts();
    }
    pub fn clear_toast(&mut self) {
        self.core.clear_toast();
    }
    pub fn tick(&mut self) {
        self.core.tick();
    }
    pub fn expire(&mut self, now: Instant) {
        self.core.expire(now);
    }
    pub fn poll_timeout(&self, now: Instant, maximum: Duration) -> Duration {
        self.core.poll_timeout(now, maximum)
    }
    pub fn push_modal(&mut self, notification: ShellNotification) -> u64 {
        let id = self.core.push_modal(notification.to_app_notification());
        self.bindings.bind(id, &notification);
        id
    }
    pub fn push_critical_modal(&mut self, notification: ShellNotification) -> u64 {
        let id = self
            .core
            .push_critical_modal(notification.to_app_notification());
        self.bindings.bind(id, &notification);
        id
    }
    pub fn dismiss_modal_by_key(&mut self, key: &str) {
        self.core.dismiss_modal_by_key(key);
        self.bindings.prune(&self.core);
    }
    pub fn has_active_modal(&self) -> bool {
        self.core.active_modal().is_some()
    }
    pub fn active_modal_component(&self) -> Option<ShellComponent> {
        self.bindings.active_component(&self.core)
    }
    pub fn active_modal_view_model(&self) -> Option<ui::NotificationViewModel> {
        self.bindings.active_view_model(&self.core)
    }
    pub fn active_modal_action_count(&self) -> usize {
        self.core.active_modal_action_count()
    }
    pub fn select_next_action(&mut self) {
        self.core.select_next_action();
    }
    pub fn select_previous_action(&mut self) {
        self.core.select_previous_action();
    }
    pub fn action_index_for_key(&self, key: &InputKey) -> Option<usize> {
        let input = KeyInput::with_phase(key.clone(), InputModifiers::none(), InputPhase::Press);
        self.action_index_for_input(&input)
    }
    pub fn action_index_for_input(&self, input: &KeyInput) -> Option<usize> {
        self.bindings.action_index_for_input(&self.core, input)
    }
    pub fn select_action(&mut self, index: usize) {
        self.core.select_action(index);
    }
    pub fn active_modal_id(&self) -> Option<u64> {
        self.core.active_modal_id()
    }
    pub fn cancel_action_index(&self) -> Option<usize> {
        self.core.cancel_action_index()
    }
    pub fn explicit_cancel_action_index(&self) -> Option<usize> {
        self.core.explicit_cancel_action_index()
    }
    pub fn dismiss_active_modal_without_response(&mut self) -> bool {
        let dismissed = self.core.dismiss_active_modal_without_response();
        self.bindings.prune(&self.core);
        dismissed
    }
    pub fn activate_selected_action(&mut self) -> Option<ShellCommand> {
        let response = self.core.activate_selected_action()?;
        self.bindings.take_follow_up(&response)
    }
    pub fn activate_action(&mut self, index: usize) -> Option<ShellCommand> {
        let response = self.core.activate_action(index)?;
        self.bindings.take_follow_up(&response)
    }
    pub fn take_response(&mut self) -> Option<ShellNotificationResponse> {
        self.core.take_response().map(Into::into)
    }
    pub fn response_count(&self) -> usize {
        self.core.response_count()
    }
    pub fn status(&self) -> &str {
        self.core.status()
    }
    pub fn toast(&self) -> Option<String> {
        self.core.toast().map(str::to_owned)
    }
    pub fn alert(&self) -> Option<String> {
        self.core.alert().map(str::to_owned)
    }
    pub fn alert_tone(&self) -> Option<ui::NotificationTone> {
        self.core.alert_tone()
    }
}

fn non_empty_notification_actions(
    actions: Vec<ShellNotificationAction>,
) -> Vec<ShellNotificationAction> {
    if actions.is_empty() {
        vec![ShellNotificationAction::new("ok", "OK").cancel()]
    } else {
        actions
    }
}
