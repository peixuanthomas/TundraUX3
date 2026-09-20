use i18n::LocalizedText;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const DEFAULT_TOAST_DURATION: Duration = Duration::from_secs(4);
pub const MAX_ACTIVE_ALERTS: usize = 64;
pub const MAX_NOTIFICATION_RESPONSES: usize = 128;
pub const DEFAULT_ALERT_KEY: &str = "shell.default";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Status,
    Toast,
    Alert,
    Modal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationTone {
    Info,
    Success,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAction {
    pub id: String,
    pub label: LocalizedText,
    pub cancel: bool,
    pub selected: bool,
}

impl NotificationAction {
    pub fn new(id: impl Into<String>, label: impl Into<LocalizedText>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            cancel: false,
            selected: false,
        }
    }

    pub fn cancel(mut self) -> Self {
        self.cancel = true;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: u64,
    pub key: Option<String>,
    pub level: NotificationLevel,
    pub tone: NotificationTone,
    pub title: LocalizedText,
    pub message: LocalizedText,
    pub actions: Vec<NotificationAction>,
}

impl Notification {
    pub fn modal(
        title: impl Into<LocalizedText>,
        message: impl Into<LocalizedText>,
        tone: NotificationTone,
        actions: Vec<NotificationAction>,
    ) -> Self {
        Self {
            id: 0,
            key: None,
            level: NotificationLevel::Modal,
            tone,
            title: title.into(),
            message: message.into(),
            actions: normalized_actions(actions),
        }
    }

    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn with_selected_action(mut self, index: usize) -> Self {
        if !self.actions.is_empty() {
            let index = index.min(self.actions.len().saturating_sub(1));
            select_action_in(&mut self.actions, index);
        }
        self
    }

    pub fn selected_action_index(&self) -> Option<usize> {
        self.actions.iter().position(|action| action.selected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationCommand {
    Reset(LocalizedText),
    SetStatus(LocalizedText),
    ShowToast(LocalizedText),
    ClearToast,
    ShowAlert {
        key: String,
        message: LocalizedText,
        tone: NotificationTone,
    },
    ResolveAlert(String),
    ClearAlerts,
    SelectNextAction,
    SelectPreviousAction,
    SelectAction(usize),
    DismissActiveModal,
    DismissModalByKey(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationResponse {
    pub notification_id: u64,
    pub action_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AlertState {
    key: String,
    message: LocalizedText,
    tone: NotificationTone,
    sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCenter {
    status: LocalizedText,
    toast: Option<LocalizedText>,
    toast_expires_at: Option<Instant>,
    alerts: VecDeque<AlertState>,
    active_modal: Option<Notification>,
    modal_queue: VecDeque<Notification>,
    responses: VecDeque<NotificationResponse>,
    next_id: u64,
    next_alert_sequence: u64,
}

impl NotificationCenter {
    pub fn new(status: impl Into<LocalizedText>) -> Self {
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

    pub fn notify_status(&mut self, message: impl Into<LocalizedText>) {
        self.status = message.into();
    }

    pub fn notify_toast(&mut self, message: impl Into<LocalizedText>) {
        self.notify_toast_at(message, Instant::now());
    }

    pub fn notify_toast_at(&mut self, message: impl Into<LocalizedText>, now: Instant) {
        self.toast = Some(message.into());
        self.toast_expires_at = toast_deadline(now);
    }

    pub fn clear_toast(&mut self) {
        self.toast = None;
        self.toast_expires_at = None;
    }

    pub fn notify_alert(&mut self, message: impl Into<LocalizedText>, tone: NotificationTone) {
        self.notify_alert_with_key(DEFAULT_ALERT_KEY, message, tone);
    }

    pub fn notify_alert_with_key(
        &mut self,
        key: impl Into<String>,
        message: impl Into<LocalizedText>,
        tone: NotificationTone,
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
        self.resolve_alert_at(key, Instant::now());
    }

    pub fn resolve_alert_at(&mut self, key: &str, now: Instant) {
        let had_alerts = !self.alerts.is_empty();
        self.alerts.retain(|alert| alert.key != key);
        self.resume_toast_if_last_alert_cleared(had_alerts, now);
    }

    pub fn clear_alerts(&mut self) {
        self.clear_alerts_at(Instant::now());
    }

    pub fn clear_alerts_at(&mut self, now: Instant) {
        let had_alerts = !self.alerts.is_empty();
        self.alerts.clear();
        self.resume_toast_if_last_alert_cleared(had_alerts, now);
    }

    pub fn expire(&mut self, now: Instant) {
        if self.alerts.is_empty()
            && self
                .toast_expires_at
                .is_some_and(|expires_at| now >= expires_at)
        {
            self.clear_toast();
        }
    }

    pub fn tick(&mut self) {
        self.expire(Instant::now());
    }

    pub fn poll_deadline(&self) -> Option<Instant> {
        self.alerts
            .is_empty()
            .then_some(self.toast_expires_at)
            .flatten()
    }

    pub fn poll_timeout(&self, now: Instant, maximum: Duration) -> Duration {
        self.poll_deadline()
            .map(|deadline| {
                deadline
                    .checked_duration_since(now)
                    .unwrap_or(Duration::ZERO)
                    .min(maximum)
            })
            .unwrap_or(maximum)
    }

    pub fn push_modal(&mut self, mut notification: Notification) -> u64 {
        notification.actions = normalized_actions(std::mem::take(&mut notification.actions));
        if let Some(key) = notification.key.clone()
            && let Some(existing) = self.modal_with_key_mut(&key)
        {
            notification.id = existing.id;
            let id = existing.id;
            *existing = notification;
            return id;
        }

        notification.id = self.allocate_id();
        let id = notification.id;
        if self.active_modal.is_none() {
            self.active_modal = Some(notification);
        } else {
            self.modal_queue.push_back(notification);
        }
        id
    }

    pub fn push_critical_modal(&mut self, mut notification: Notification) -> u64 {
        notification.actions = normalized_actions(std::mem::take(&mut notification.actions));
        if let Some(key) = notification.key.clone()
            && let Some(existing) = self.modal_with_key_mut(&key)
        {
            notification.id = existing.id;
            let id = existing.id;
            *existing = notification;
            return id;
        }

        notification.id = self.allocate_id();
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

    pub fn dismiss_active_modal_without_response(&mut self) -> bool {
        if self.active_modal.take().is_none() {
            return false;
        }
        self.promote_next_modal();
        true
    }

    pub fn select_next_action(&mut self) {
        let Some(modal) = self.active_modal.as_mut() else {
            return;
        };
        let Some(selected) = modal.selected_action_index() else {
            return;
        };
        let next = (selected + 1) % modal.actions.len();
        select_action_in(&mut modal.actions, next);
    }

    pub fn select_previous_action(&mut self) {
        let Some(modal) = self.active_modal.as_mut() else {
            return;
        };
        let Some(selected) = modal.selected_action_index() else {
            return;
        };
        let previous = if selected == 0 {
            modal.actions.len().saturating_sub(1)
        } else {
            selected.saturating_sub(1)
        };
        select_action_in(&mut modal.actions, previous);
    }

    pub fn select_action(&mut self, index: usize) {
        let Some(modal) = self.active_modal.as_mut() else {
            return;
        };
        if index < modal.actions.len() {
            select_action_in(&mut modal.actions, index);
        }
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

    pub fn activate_selected_action(&mut self) -> Option<NotificationResponse> {
        let index = self.active_modal.as_ref()?.selected_action_index()?;
        self.activate_action(index)
    }

    pub fn activate_action(&mut self, index: usize) -> Option<NotificationResponse> {
        let modal = self.active_modal.take()?;
        if index >= modal.actions.len() {
            self.active_modal = Some(modal);
            return None;
        }

        let response = NotificationResponse {
            notification_id: modal.id,
            action_id: modal.actions[index].id.clone(),
        };
        if self.responses.len() >= MAX_NOTIFICATION_RESPONSES {
            self.responses.pop_front();
        }
        self.responses.push_back(response.clone());
        self.promote_next_modal();
        Some(response)
    }

    pub fn take_response(&mut self) -> Option<NotificationResponse> {
        self.responses.pop_front()
    }

    /// Retained text; render against the presentation's locale snapshot when displayed.
    pub fn status(&self) -> &LocalizedText {
        &self.status
    }

    pub fn toast(&self) -> Option<&LocalizedText> {
        self.toast.as_ref()
    }

    pub fn toast_expires_at(&self) -> Option<Instant> {
        self.toast_expires_at
    }

    pub fn alert(&self) -> Option<&LocalizedText> {
        self.active_alert().map(|alert| &alert.message)
    }

    pub fn alert_key(&self) -> Option<&str> {
        self.active_alert().map(|alert| alert.key.as_str())
    }

    pub fn alert_tone(&self) -> Option<NotificationTone> {
        self.active_alert().map(|alert| alert.tone)
    }

    pub fn alert_message_for_key(&self, key: &str) -> Option<&LocalizedText> {
        self.alerts
            .iter()
            .find(|alert| alert.key == key)
            .map(|alert| &alert.message)
    }

    pub fn alert_count(&self) -> usize {
        self.alerts.len()
    }

    pub fn active_modal(&self) -> Option<&Notification> {
        self.active_modal.as_ref()
    }

    pub fn active_modal_id(&self) -> Option<u64> {
        self.active_modal.as_ref().map(|modal| modal.id)
    }

    pub fn contains_modal_id(&self, id: u64) -> bool {
        self.active_modal_id() == Some(id) || self.modal_queue.iter().any(|modal| modal.id == id)
    }

    pub fn active_modal_action_count(&self) -> usize {
        self.active_modal
            .as_ref()
            .map(|modal| modal.actions.len())
            .unwrap_or(0)
    }

    pub fn queued_modal_count(&self) -> usize {
        self.modal_queue.len()
    }

    pub fn response_count(&self) -> usize {
        self.responses.len()
    }

    fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(1);
        id
    }

    fn resume_toast_if_last_alert_cleared(&mut self, had_alerts: bool, now: Instant) {
        if had_alerts && self.alerts.is_empty() && self.toast.is_some() {
            self.toast_expires_at = toast_deadline(now);
        }
    }

    fn modal_with_key_mut(&mut self, key: &str) -> Option<&mut Notification> {
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

const fn notification_tone_priority(tone: NotificationTone) -> u8 {
    match tone {
        NotificationTone::Info => 0,
        NotificationTone::Success => 1,
        NotificationTone::Warning => 2,
        NotificationTone::Error => 3,
        NotificationTone::Critical => 4,
    }
}

fn toast_deadline(now: Instant) -> Option<Instant> {
    now.checked_add(DEFAULT_TOAST_DURATION).or(Some(now))
}

fn normalized_actions(mut actions: Vec<NotificationAction>) -> Vec<NotificationAction> {
    if actions.is_empty() {
        actions.push(NotificationAction::new("ok", i18n::msg!("notifications-action-ok")).cancel());
    }
    let selected = actions
        .iter()
        .position(|action| action.selected)
        .unwrap_or(0);
    select_action_in(&mut actions, selected);
    actions
}

fn select_action_in(actions: &mut [NotificationAction], selected: usize) {
    for (index, action) in actions.iter_mut().enumerate() {
        action.selected = index == selected;
    }
}
#[cfg(test)]
#[path = "../../tests/unit/application/notification/tests.rs"]
mod tests;
