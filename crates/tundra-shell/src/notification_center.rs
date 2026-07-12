use crate::{
    DEFAULT_ALERT_KEY, DEFAULT_TOAST_DURATION, InputKey, InputModifiers, InputPhase, KeyInput,
    MAX_ACTIVE_ALERTS, MAX_NOTIFICATION_RESPONSES, ShellCommand, ShellComponent,
};
use std::collections::VecDeque;
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
                InputKey::Character(character) if character.is_ascii_alphabetic()
            )
        {
            return false;
        }

        match (&self.shortcut, &input.key) {
            (Some(InputKey::Character(expected)), InputKey::Character(actual)) => {
                expected.eq_ignore_ascii_case(actual)
            }
            (Some(expected), actual) => expected == actual,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNotification {
    pub id: u64,
    pub key: Option<String>,
    pub level: tundra_ui::NotificationLevel,
    pub tone: tundra_ui::NotificationTone,
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
        tone: tundra_ui::NotificationTone,
        actions: Vec<ShellNotificationAction>,
    ) -> Self {
        Self {
            id: 0,
            key: None,
            level: tundra_ui::NotificationLevel::Modal,
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

    fn to_view_model(&self) -> tundra_ui::NotificationViewModel {
        tundra_ui::NotificationViewModel::new(
            self.id.to_string(),
            self.level,
            self.tone,
            self.title.clone(),
            self.message.clone(),
            self.actions
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    let mut view =
                        tundra_ui::NotificationActionViewModel::new(&action.id, &action.label);
                    if let Some(shortcut) = action.shortcut_label() {
                        view = view.with_shortcut(shortcut);
                    }
                    view.selected(index == self.selected_action)
                })
                .collect(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNotificationResponse {
    pub notification_id: u64,
    pub action_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AlertState {
    key: String,
    message: String,
    tone: tundra_ui::NotificationTone,
    sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCenter {
    status: String,
    toast: Option<String>,
    toast_expires_at: Option<Instant>,
    alerts: VecDeque<AlertState>,
    active_modal: Option<ShellNotification>,
    modal_queue: VecDeque<ShellNotification>,
    pub(crate) responses: VecDeque<ShellNotificationResponse>,
    next_id: u64,
    next_alert_sequence: u64,
}

impl NotificationCenter {
    pub fn new(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            toast: None,
            toast_expires_at: None,
            alerts: VecDeque::new(),
            active_modal: None,
            modal_queue: VecDeque::new(),
            responses: VecDeque::new(),
            next_id: 1,
            next_alert_sequence: 1,
        }
    }

    pub fn notify_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    pub fn notify_toast(&mut self, message: impl Into<String>) {
        self.notify_toast_at(message, Instant::now());
    }

    pub(crate) fn notify_toast_at(&mut self, message: impl Into<String>, now: Instant) {
        self.toast = Some(message.into());
        self.toast_expires_at = now.checked_add(DEFAULT_TOAST_DURATION).or(Some(now));
    }

    pub fn notify_alert(&mut self, message: impl Into<String>, tone: tundra_ui::NotificationTone) {
        self.notify_alert_with_key(DEFAULT_ALERT_KEY, message, tone);
    }

    pub fn notify_alert_with_key(
        &mut self,
        key: impl Into<String>,
        message: impl Into<String>,
        tone: tundra_ui::NotificationTone,
    ) {
        let key = key.into();
        let sequence = self.next_alert_sequence;
        self.next_alert_sequence = self.next_alert_sequence.saturating_add(1).max(1);
        if let Some(alert) = self.alerts.iter_mut().find(|alert| alert.key == key) {
            alert.message = message.into();
            alert.tone = tone;
            alert.sequence = sequence;
            return;
        }

        if self.alerts.len() >= MAX_ACTIVE_ALERTS
            && let Some(oldest_index) = self
                .alerts
                .iter()
                .enumerate()
                .min_by_key(|(_, alert)| alert.sequence)
                .map(|(index, _)| index)
        {
            self.alerts.remove(oldest_index);
        }
        self.alerts.push_back(AlertState {
            key,
            message: message.into(),
            tone,
            sequence,
        });
    }

    pub fn resolve_alert(&mut self, key: &str) {
        let had_alerts = !self.alerts.is_empty();
        self.alerts.retain(|alert| alert.key != key);
        if had_alerts && self.alerts.is_empty() && self.toast.is_some() {
            let now = Instant::now();
            self.toast_expires_at = now.checked_add(DEFAULT_TOAST_DURATION).or(Some(now));
        }
    }

    pub fn clear_alert(&mut self) {
        let had_alerts = !self.alerts.is_empty();
        self.alerts.clear();
        if had_alerts && self.toast.is_some() {
            let now = Instant::now();
            self.toast_expires_at = now.checked_add(DEFAULT_TOAST_DURATION).or(Some(now));
        }
    }

    pub fn clear_toast(&mut self) {
        self.toast = None;
        self.toast_expires_at = None;
    }

    pub fn tick(&mut self) {
        self.expire(Instant::now());
    }

    pub fn expire(&mut self, now: Instant) {
        if self.alerts.is_empty()
            && self
                .toast_expires_at
                .is_some_and(|expires_at| now >= expires_at)
        {
            self.toast = None;
            self.toast_expires_at = None;
        }
    }

    pub fn poll_timeout(&self, now: Instant, maximum: Duration) -> Duration {
        if !self.alerts.is_empty() {
            return maximum;
        }
        match self.toast_expires_at {
            Some(expires_at) => expires_at
                .checked_duration_since(now)
                .unwrap_or(Duration::ZERO)
                .min(maximum),
            None => maximum,
        }
    }

    pub fn push_modal(&mut self, mut notification: ShellNotification) -> u64 {
        if let Some(key) = notification.key.clone()
            && let Some(existing) = self.modal_with_key_mut(&key)
        {
            notification.id = existing.id;
            *existing = notification;
            return existing.id;
        }

        notification.id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = notification.id;
        if self.active_modal.is_none() {
            self.active_modal = Some(notification);
        } else {
            self.modal_queue.push_back(notification);
        }
        id
    }

    pub fn push_critical_modal(&mut self, mut notification: ShellNotification) -> u64 {
        if let Some(key) = notification.key.clone()
            && let Some(existing) = self.modal_with_key_mut(&key)
        {
            notification.id = existing.id;
            *existing = notification;
            return existing.id;
        }

        notification.id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = notification.id;
        if let Some(previous) = self.active_modal.replace(notification) {
            self.modal_queue.push_front(previous);
        }
        id
    }

    pub fn dismiss_modal_by_key(&mut self, key: &str) {
        let active_matches = self
            .active_modal
            .as_ref()
            .and_then(|modal| modal.key.as_deref())
            == Some(key);
        if active_matches {
            self.active_modal = None;
            self.promote_next_modal();
        }
        self.modal_queue
            .retain(|modal| modal.key.as_deref() != Some(key));
    }

    pub fn has_active_modal(&self) -> bool {
        self.active_modal.is_some()
    }

    pub fn active_modal_component(&self) -> Option<ShellComponent> {
        self.active_modal.as_ref().map(|modal| modal.component)
    }

    pub fn active_modal_view_model(&self) -> Option<tundra_ui::NotificationViewModel> {
        self.active_modal
            .as_ref()
            .map(ShellNotification::to_view_model)
    }

    pub fn active_modal_action_count(&self) -> usize {
        self.active_modal
            .as_ref()
            .map(|modal| modal.actions.len())
            .unwrap_or(0)
    }

    pub fn select_next_action(&mut self) {
        let Some(modal) = self.active_modal.as_mut() else {
            return;
        };
        if modal.actions.is_empty() {
            return;
        }
        modal.selected_action = (modal.selected_action + 1) % modal.actions.len();
    }

    pub fn select_previous_action(&mut self) {
        let Some(modal) = self.active_modal.as_mut() else {
            return;
        };
        if modal.actions.is_empty() {
            return;
        }
        modal.selected_action = if modal.selected_action == 0 {
            modal.actions.len().saturating_sub(1)
        } else {
            modal.selected_action.saturating_sub(1)
        };
    }

    pub fn action_index_for_key(&self, key: &InputKey) -> Option<usize> {
        let input = KeyInput::new(key.clone(), InputModifiers::none(), InputPhase::Press);
        self.action_index_for_input(&input)
    }

    pub fn action_index_for_input(&self, input: &KeyInput) -> Option<usize> {
        self.active_modal.as_ref().and_then(|modal| {
            modal
                .actions
                .iter()
                .position(|action| action.matches_shortcut(input))
        })
    }

    pub fn select_action(&mut self, index: usize) {
        let Some(modal) = self.active_modal.as_mut() else {
            return;
        };
        if index < modal.actions.len() {
            modal.selected_action = index;
        }
    }

    pub fn active_modal_id(&self) -> Option<u64> {
        self.active_modal.as_ref().map(|modal| modal.id)
    }

    pub fn cancel_action_index(&self) -> Option<usize> {
        self.active_modal.as_ref().and_then(|modal| {
            modal
                .actions
                .iter()
                .position(|action| action.cancel)
                .or_else(|| modal.actions.len().checked_sub(1))
        })
    }

    pub fn explicit_cancel_action_index(&self) -> Option<usize> {
        self.active_modal
            .as_ref()
            .and_then(|modal| modal.actions.iter().position(|action| action.cancel))
    }

    pub fn dismiss_active_modal_without_response(&mut self) -> bool {
        if self.active_modal.take().is_none() {
            return false;
        }
        self.promote_next_modal();
        true
    }

    pub fn activate_selected_action(&mut self) -> Option<ShellCommand> {
        let index = self
            .active_modal
            .as_ref()
            .map(|modal| modal.selected_action)?;
        self.activate_action(index)
    }

    pub fn activate_action(&mut self, index: usize) -> Option<ShellCommand> {
        let modal = self.active_modal.take()?;
        if index >= modal.actions.len() {
            self.active_modal = Some(modal);
            return None;
        }

        let action = modal.actions[index].clone();
        if self.responses.len() >= MAX_NOTIFICATION_RESPONSES {
            self.responses.pop_front();
        }
        self.responses.push_back(ShellNotificationResponse {
            notification_id: modal.id,
            action_id: action.id,
        });
        self.promote_next_modal();
        action.follow_up
    }

    pub fn take_response(&mut self) -> Option<ShellNotificationResponse> {
        self.responses.pop_front()
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn toast(&self) -> Option<String> {
        self.toast.clone()
    }

    pub fn alert(&self) -> Option<String> {
        self.active_alert().map(|alert| alert.message.clone())
    }

    pub fn alert_tone(&self) -> Option<tundra_ui::NotificationTone> {
        self.active_alert().map(|alert| alert.tone)
    }

    pub(crate) fn alert_message_for_key(&self, key: &str) -> Option<&str> {
        self.alerts
            .iter()
            .find(|alert| alert.key == key)
            .map(|alert| alert.message.as_str())
    }

    fn modal_with_key_mut(&mut self, key: &str) -> Option<&mut ShellNotification> {
        if self
            .active_modal
            .as_ref()
            .and_then(|modal| modal.key.as_deref())
            == Some(key)
        {
            return self.active_modal.as_mut();
        }

        self.modal_queue
            .iter_mut()
            .find(|modal| modal.key.as_deref() == Some(key))
    }

    fn promote_next_modal(&mut self) {
        if self.active_modal.is_none() {
            self.active_modal = self.modal_queue.pop_front();
        }
    }

    fn active_alert(&self) -> Option<&AlertState> {
        self.alerts
            .iter()
            .max_by_key(|alert| (notification_tone_priority(alert.tone), alert.sequence))
    }
}

const fn notification_tone_priority(tone: tundra_ui::NotificationTone) -> u8 {
    match tone {
        tundra_ui::NotificationTone::Info => 0,
        tundra_ui::NotificationTone::Success => 1,
        tundra_ui::NotificationTone::Warning => 2,
        tundra_ui::NotificationTone::Error => 3,
        tundra_ui::NotificationTone::Critical => 4,
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
