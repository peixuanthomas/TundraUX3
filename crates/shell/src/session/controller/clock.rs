use super::super::*;
impl ShellSession {
    pub(in crate::session) fn open_clock(&mut self) {
        if self.active_screen() != ShellScreen::Clock {
            self.screen_stack.push(ShellScreen::Clock);
        }
        self.active_popup = None;
        self.clock_create_state = None;
        self.focused_component = if self.is_strict_guest() {
            ShellComponent::ClockButton
        } else {
            ShellComponent::ClockNewButton
        };
        self.sync_clock_selection();
        self.notify_status("Clock");
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_clock(&mut self) {
        if self.active_screen() == ShellScreen::Clock {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.clock_create_state = None;
        self.notify_status("Ready");
        self.refresh_hit_map();
    }

    pub(in crate::session) fn load_clock_for_session(&mut self, session: &AuthSession) {
        self.clock_scheduler = None;
        self.clock_selected_entry_id = None;
        self.clock_entry_window_start = 0;
        self.clock_create_state = None;
        self.clock_persist_pending = false;
        self.clock_pending_due_summary = None;
        self.clock_profile_pending_sync = None;

        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let document = match storage.load_clock() {
            Ok(document) => document,
            Err(error) => {
                self.report_clock_storage_error(error.to_string());
                return;
            }
        };
        let profile = document
            .profiles
            .get(&session.user_id)
            .cloned()
            .unwrap_or_default();
        if !self.time_sync_attempted && !profile.entries.is_empty() {
            self.clock_profile_pending_sync = Some(profile);
            self.notify_toast("Waiting for initial time sync to restore reminders");
            return;
        }
        self.restore_clock_profile(profile);
    }

    pub(in crate::session) fn restore_clock_profile(&mut self, profile: ClockProfile) {
        let snapshot = self.app.snapshot().clock;
        let now = Instant::now();
        let (scheduler, due) = ClockScheduler::restore(profile, &snapshot, now);
        self.clock_scheduler = Some(scheduler);
        self.sync_clock_selection_at(now);
        let ordinary_due = self.handle_clock_due_events(due);
        if let Some(summary) = ordinary_due {
            self.remember_clock_due_summary(summary);
        }

        if let Err(error) = self.persist_clock_scheduler_at(&snapshot, now) {
            self.clock_persist_pending = true;
            self.report_clock_storage_error(error);
        } else {
            self.clock_pending_due_summary = None;
            self.resolve_notification_alert(CLOCK_STORAGE_ALERT_KEY);
        }
    }

    pub(in crate::session) fn restore_clock_profile_after_initial_sync(&mut self) {
        if self.app.auth_session().is_none() {
            self.clock_profile_pending_sync = None;
            return;
        }
        if let Some(profile) = self.clock_profile_pending_sync.take() {
            self.restore_clock_profile(profile);
            self.refresh_hit_map();
        }
    }

    pub(in crate::session) fn persist_clock_scheduler_at(
        &self,
        snapshot: &time::ClockSnapshot,
        now: Instant,
    ) -> Result<(), String> {
        let storage = self
            .storage_manager
            .as_ref()
            .ok_or_else(|| "Clock storage is unavailable".to_string())?;
        let user_id = self
            .app
            .auth_session()
            .map(|session| session.user_id.as_str())
            .ok_or_else(|| "Sign in to save alarms and countdowns".to_string())?;
        let scheduler = self
            .clock_scheduler
            .as_ref()
            .ok_or_else(|| "Clock scheduler is unavailable".to_string())?;
        let mut document = storage.load_clock().map_err(|error| error.to_string())?;
        document
            .profiles
            .insert(user_id.to_string(), scheduler.export_profile(snapshot, now));
        storage
            .save_clock(&document)
            .map_err(|error| error.to_string())
    }

    pub(in crate::session) fn report_clock_storage_error(&mut self, message: impl Into<String>) {
        let ordinary_due = self.clock_pending_due_summary.clone();
        self.report_clock_storage_error_with_due(message, ordinary_due.as_deref());
    }

    pub(in crate::session) fn remember_clock_due_summary(&mut self, summary: String) {
        self.clock_pending_due_summary = Some(match self.clock_pending_due_summary.take() {
            None => summary,
            Some(previous) if previous == summary => previous,
            Some(_) => "Multiple reminders are due".to_string(),
        });
    }

    pub(in crate::session) fn report_clock_storage_error_with_due(
        &mut self,
        message: impl Into<String>,
        ordinary_due: Option<&str>,
    ) {
        let storage_error = format!("Clock data could not be saved: {}", message.into());
        let message = ordinary_due
            .map(|due| format!("{due}. {storage_error}"))
            .unwrap_or(storage_error);
        self.notify_alert_with_key(
            CLOCK_STORAGE_ALERT_KEY,
            message,
            ui::NotificationTone::Error,
        );
    }

    pub(in crate::session) fn commit_clock_mutation(
        &mut self,
        previous: ClockScheduler,
        snapshot: &time::ClockSnapshot,
        now: Instant,
    ) -> Result<(), String> {
        match self.persist_clock_scheduler_at(snapshot, now) {
            Ok(()) => {
                self.clock_persist_pending = false;
                self.clock_pending_due_summary = None;
                self.resolve_notification_alert(CLOCK_STORAGE_ALERT_KEY);
                Ok(())
            }
            Err(error) => {
                self.clock_scheduler = Some(previous);
                self.report_clock_storage_error(error.clone());
                Err(error)
            }
        }
    }

    pub(in crate::session) fn advance_clock_background(&mut self) {
        let snapshot = self.app.snapshot().clock;
        self.advance_clock_background_at(&snapshot, Instant::now());
    }

    pub(in crate::session) fn advance_clock_background_at(
        &mut self,
        snapshot: &time::ClockSnapshot,
        now: Instant,
    ) {
        self.notification_expire(now);
        let due = self
            .clock_scheduler
            .as_mut()
            .map(|scheduler| scheduler.advance(snapshot, now))
            .unwrap_or_default();
        let has_due = !due.is_empty();
        let ordinary_due = if has_due {
            self.sync_clock_selection_at(now);
            let ordinary_due = self.handle_clock_due_events(due);
            self.refresh_hit_map();
            ordinary_due
        } else {
            None
        };
        if let Some(summary) = ordinary_due {
            self.remember_clock_due_summary(summary);
        }
        if has_due || self.clock_persist_pending {
            match self.persist_clock_scheduler_at(snapshot, now) {
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
    }

    pub(in crate::session) fn handle_clock_due_events(
        &mut self,
        due: Vec<DueEvent>,
    ) -> Option<String> {
        let mut ordinary = Vec::new();
        for event in due {
            let message = match event.kind {
                ScheduledClockEntryKind::DailyAlarm => {
                    format!("Alarm {} is due", event.display_time)
                }
                ScheduledClockEntryKind::Countdown => "Countdown finished".to_string(),
            };
            if !event.strong {
                ordinary.push(message);
                continue;
            }

            let user_id = self
                .app
                .auth_session()
                .map(|session| session.user_id.as_str())
                .unwrap_or("unknown");
            let key = format!("{CLOCK_DUE_NOTIFICATION_KEY_PREFIX}.{user_id}.{}", event.id);
            let (title, actions) = match event.kind {
                ScheduledClockEntryKind::DailyAlarm => (
                    "Alarm",
                    vec![
                        ShellNotificationAction::new("snooze", "Snooze 5 min")
                            .with_shortcut(InputKey::Char('s'))
                            .with_follow_up(ShellCommand::ClockSnoozeFiveMinutes(event.id)),
                        ShellNotificationAction::new("dismiss", "Dismiss")
                            .with_shortcut(InputKey::Escape)
                            .cancel(),
                    ],
                ),
                ScheduledClockEntryKind::Countdown => (
                    "Countdown",
                    vec![
                        ShellNotificationAction::new("dismiss", "Dismiss")
                            .with_shortcut(InputKey::Escape)
                            .cancel(),
                    ],
                ),
            };
            self.notify_modal_with_options(
                ShellNotification::modal(title, message, ui::NotificationTone::Critical, actions)
                    .with_key(key)
                    .with_component(ShellComponent::NotificationDialog),
            );
        }

        let message = match ordinary.len() {
            0 => None,
            1 => ordinary.pop(),
            count => Some(format!("{count} reminders are due")),
        };
        if let Some(message) = &message {
            self.notify_toast(message.clone());
        }
        message
    }

    pub(in crate::session) fn open_clock_create_dialog(&mut self) {
        if self.is_strict_guest() {
            self.notify_status("Guest clock is read-only");
            return;
        }
        if self.clock_scheduler.is_none() {
            if self.clock_profile_pending_sync.is_some() {
                self.notify_toast("Waiting for initial time sync to restore reminders");
            } else {
                self.notify_toast("Sign in to create alarms and countdowns");
            }
            return;
        }
        self.clock_create_state = Some(ClockCreateState::default());
        self.focused_component = ShellComponent::ClockCreateInput;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_clock_create_dialog(&mut self) {
        self.clock_create_state = None;
        self.focused_component = ShellComponent::ClockNewButton;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn move_clock_create_focus(&mut self, direction: i8) {
        let Some(state) = self.clock_create_state.as_mut() else {
            return;
        };
        let order = [
            ui::ClockCreateDialogFocus::Input,
            ui::ClockCreateDialogFocus::CreateAlarm,
            ui::ClockCreateDialogFocus::CreateCountdown,
        ];
        let current = order
            .iter()
            .position(|focus| *focus == state.focus)
            .unwrap_or(0);
        let next =
            (current as isize + direction as isize).rem_euclid(order.len() as isize) as usize;
        self.set_clock_create_focus(order[next]);
    }

    pub(in crate::session) fn set_clock_create_focus(&mut self, focus: ui::ClockCreateDialogFocus) {
        let Some(state) = self.clock_create_state.as_mut() else {
            return;
        };
        state.focus = focus;
        self.focused_component = match focus {
            ui::ClockCreateDialogFocus::Input => ShellComponent::ClockCreateInput,
            ui::ClockCreateDialogFocus::CreateAlarm => ShellComponent::ClockCreateAlarmButton,
            ui::ClockCreateDialogFocus::CreateCountdown => {
                ShellComponent::ClockCreateCountdownButton
            }
        };
    }

    pub(in crate::session) fn append_clock_create_char(&mut self, character: char) {
        let Some(state) = self.clock_create_state.as_mut() else {
            return;
        };
        if state.focus != ui::ClockCreateDialogFocus::Input
            || state.input.len() >= 8
            || !(character.is_ascii_digit() || character == ' ')
        {
            return;
        }
        state.input.push(character);
        state.error = None;
    }

    pub(in crate::session) fn clock_create_backspace(&mut self) {
        let Some(state) = self.clock_create_state.as_mut() else {
            return;
        };
        if state.focus == ui::ClockCreateDialogFocus::Input {
            state.input.pop();
            state.error = None;
        }
    }

    pub(in crate::session) fn create_clock_entry(&mut self, kind: ScheduledClockEntryKind) {
        let Some(input) = self
            .clock_create_state
            .as_ref()
            .map(|state| state.input.clone())
        else {
            return;
        };
        let snapshot = self.app.snapshot().clock;
        let now = Instant::now();
        let Some(previous) = self.clock_scheduler.clone() else {
            if let Some(state) = self.clock_create_state.as_mut() {
                state.error = Some("Sign in to create clock entries".to_string());
            }
            return;
        };
        let result = match (kind, self.clock_scheduler.as_mut()) {
            (ScheduledClockEntryKind::DailyAlarm, Some(scheduler)) => {
                scheduler.create_daily_alarm(&input, &snapshot)
            }
            (ScheduledClockEntryKind::Countdown, Some(scheduler)) => {
                scheduler.create_countdown(&input, &snapshot, now)
            }
            (_, None) => Err(ClockSchedulerError::EntryNotFound),
        };
        let id = match result {
            Ok(id) => id,
            Err(error) => {
                if let Some(state) = self.clock_create_state.as_mut() {
                    state.error = Some(error.to_string());
                }
                return;
            }
        };
        if let Err(error) = self.commit_clock_mutation(previous, &snapshot, now) {
            if let Some(state) = self.clock_create_state.as_mut() {
                state.error = Some(format!("Could not save: {error}"));
            }
            return;
        }

        self.clock_create_state = None;
        self.clock_selected_entry_id = Some(id);
        self.focused_component = ShellComponent::ClockEntryList;
        self.sync_clock_window_at(now);
        self.notify_toast(match kind {
            ScheduledClockEntryKind::DailyAlarm => "Daily alarm created",
            ScheduledClockEntryKind::Countdown => "Countdown created",
        });
        self.refresh_hit_map();
    }

    pub(in crate::session) fn ordered_clock_entry_ids_at(&self, now: Instant) -> Vec<u64> {
        let Some(scheduler) = &self.clock_scheduler else {
            return Vec::new();
        };
        let entries = scheduler.entries(now);
        entries
            .iter()
            .filter(|entry| entry.kind == ScheduledClockEntryKind::DailyAlarm)
            .chain(
                entries
                    .iter()
                    .filter(|entry| entry.kind == ScheduledClockEntryKind::Countdown),
            )
            .map(|entry| entry.id)
            .collect()
    }

    pub(in crate::session) fn sync_clock_selection(&mut self) {
        self.sync_clock_selection_at(Instant::now());
    }

    pub(in crate::session) fn sync_clock_selection_at(&mut self, now: Instant) {
        let ids = self.ordered_clock_entry_ids_at(now);
        if !self
            .clock_selected_entry_id
            .is_some_and(|selected| ids.contains(&selected))
        {
            self.clock_selected_entry_id = ids.first().copied();
        }
        self.sync_clock_window_at(now);
    }

    pub(in crate::session) fn clock_entry_capacity_at(&self, now: Instant) -> usize {
        let (width, height) = self.terminal_size;
        let area = Rect::new(0, 0, width, height);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return 1;
        };
        let snapshot = self.app.snapshot().clock;
        let model = self.to_clock_view_model_at(&snapshot, now);
        ui::clock_page_layout(main, &model).entry_capacity.max(1)
    }

    pub(in crate::session) fn sync_clock_window_at(&mut self, now: Instant) {
        let ids = self.ordered_clock_entry_ids_at(now);
        let capacity = self.clock_entry_capacity_at(now);
        let max_start = ids.len().saturating_sub(capacity);
        self.clock_entry_window_start = self.clock_entry_window_start.min(max_start);
        let Some(index) = self
            .clock_selected_entry_id
            .and_then(|selected| ids.iter().position(|id| *id == selected))
        else {
            self.clock_entry_window_start = 0;
            return;
        };
        if index < self.clock_entry_window_start {
            self.clock_entry_window_start = index;
        } else if index >= self.clock_entry_window_start.saturating_add(capacity) {
            self.clock_entry_window_start = index.saturating_add(1).saturating_sub(capacity);
        }
    }

    pub(in crate::session) fn select_clock_entry_delta(&mut self, delta: isize) {
        let now = Instant::now();
        let ids = self.ordered_clock_entry_ids_at(now);
        if ids.is_empty() {
            self.clock_selected_entry_id = None;
            self.focused_component = ShellComponent::ClockNewButton;
            return;
        }
        let current = self
            .clock_selected_entry_id
            .and_then(|selected| ids.iter().position(|id| *id == selected))
            .unwrap_or(0);
        let next =
            (current as isize + delta).clamp(0, ids.len().saturating_sub(1) as isize) as usize;
        self.clock_selected_entry_id = Some(ids[next]);
        self.focused_component = ShellComponent::ClockEntryList;
        self.sync_clock_window_at(now);
    }

    pub(in crate::session) fn select_clock_entry_edge(&mut self, last: bool) {
        let now = Instant::now();
        let ids = self.ordered_clock_entry_ids_at(now);
        self.clock_selected_entry_id = if last {
            ids.last().copied()
        } else {
            ids.first().copied()
        };
        if self.clock_selected_entry_id.is_some() {
            self.focused_component = ShellComponent::ClockEntryList;
        }
        self.sync_clock_window_at(now);
    }

    pub(in crate::session) fn select_clock_entry(&mut self, id: u64) {
        if self
            .ordered_clock_entry_ids_at(Instant::now())
            .contains(&id)
        {
            self.clock_selected_entry_id = Some(id);
            self.focused_component = ShellComponent::ClockEntryList;
            self.sync_clock_window_at(Instant::now());
        }
    }

    pub(in crate::session) fn show_clock_manage_dialog(&mut self, id: u64) {
        let Some(entry) = self.clock_scheduler.as_ref().and_then(|scheduler| {
            scheduler
                .entries(Instant::now())
                .into_iter()
                .find(|entry| entry.id == id)
        }) else {
            self.notify_toast("Clock entry no longer exists");
            return;
        };
        self.clock_selected_entry_id = Some(id);
        let (title, kind_label) = match entry.kind {
            ScheduledClockEntryKind::DailyAlarm => ("Manage Alarm", "Daily alarm"),
            ScheduledClockEntryKind::Countdown => ("Manage Countdown", "Countdown"),
        };
        let toggle_label = if entry.strong {
            "Turn Strong Off"
        } else {
            "Turn Strong On"
        };
        let user_id = self
            .app
            .auth_session()
            .map(|session| session.user_id.as_str())
            .unwrap_or("unknown");
        self.notify_modal_with_options(
            ShellNotification::modal(
                title,
                format!("{kind_label} {}", entry.display_time),
                ui::NotificationTone::Info,
                vec![
                    ShellNotificationAction::new("delete", "Delete")
                        .with_shortcut(InputKey::Char('x'))
                        .with_follow_up(ShellCommand::ClockDeleteEntry(id)),
                    ShellNotificationAction::new("toggle-strong", toggle_label)
                        .with_shortcut(InputKey::Char('t'))
                        .with_follow_up(ShellCommand::ClockToggleStrong(id)),
                    ShellNotificationAction::new("cancel", "Cancel")
                        .with_shortcut(InputKey::Escape)
                        .cancel(),
                ],
            )
            .with_key(format!(
                "{CLOCK_MANAGE_NOTIFICATION_KEY_PREFIX}.{user_id}.{id}"
            ))
            .with_component(ShellComponent::NotificationDialog),
        );
    }

    pub(in crate::session) fn delete_clock_entry(&mut self, id: u64) {
        let snapshot = self.app.snapshot().clock;
        let now = Instant::now();
        let Some(previous) = self.clock_scheduler.clone() else {
            return;
        };
        if !self
            .clock_scheduler
            .as_mut()
            .is_some_and(|scheduler| scheduler.delete(id))
        {
            self.notify_toast("Clock entry no longer exists");
            return;
        }
        if self.commit_clock_mutation(previous, &snapshot, now).is_ok() {
            if let Some(user_id) = self
                .app
                .auth_session()
                .map(|session| session.user_id.clone())
            {
                self.notification_dismiss_modal_by_key(&format!(
                    "{CLOCK_DUE_NOTIFICATION_KEY_PREFIX}.{user_id}.{id}"
                ));
            }
            self.sync_clock_selection_at(now);
            self.notify_toast("Clock entry deleted");
            self.refresh_hit_map();
        }
    }

    pub(in crate::session) fn toggle_clock_entry_strong(&mut self, id: u64) {
        let snapshot = self.app.snapshot().clock;
        let now = Instant::now();
        let Some(previous) = self.clock_scheduler.clone() else {
            return;
        };
        let Some(enabled) = self
            .clock_scheduler
            .as_mut()
            .and_then(|scheduler| scheduler.toggle_strong(id))
        else {
            self.notify_toast("Clock entry no longer exists");
            return;
        };
        if self.commit_clock_mutation(previous, &snapshot, now).is_ok() {
            self.notify_toast(if enabled {
                "Strong notification enabled"
            } else {
                "Strong notification disabled"
            });
            self.refresh_hit_map();
        }
    }

    pub(in crate::session) fn snooze_clock_alarm(&mut self, id: u64) {
        let snapshot = self.app.snapshot().clock;
        let now = Instant::now();
        let Some(previous) = self.clock_scheduler.clone() else {
            return;
        };
        let retry_event = previous
            .entries(now)
            .into_iter()
            .find(|entry| {
                entry.id == id && entry.kind == ScheduledClockEntryKind::DailyAlarm && entry.strong
            })
            .map(|entry| DueEvent {
                id: entry.id,
                kind: entry.kind,
                strong: true,
                display_time: entry.display_time,
            });
        let result = self
            .clock_scheduler
            .as_mut()
            .ok_or(ClockSchedulerError::EntryNotFound)
            .and_then(|scheduler| scheduler.snooze_five_minutes(id, &snapshot, now));
        match result {
            Ok(()) => {
                if self.commit_clock_mutation(previous, &snapshot, now).is_ok() {
                    self.notify_toast("Alarm snoozed for 5 minutes");
                    self.refresh_hit_map();
                } else if let Some(event) = retry_event {
                    let _ = self.handle_clock_due_events(vec![event]);
                    self.refresh_hit_map();
                }
            }
            Err(error) => self.notify_toast(error.to_string()),
        }
    }
}
