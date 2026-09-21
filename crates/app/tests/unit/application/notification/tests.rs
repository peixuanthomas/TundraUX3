use super::*;

fn language_snapshots() -> Vec<(std::sync::Arc<i18n::LanguageSnapshot>, &'static str)> {
    let root = std::env::temp_dir().join(format!(
        "tux3-app-notifications-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let chinese = root.join("locales/zh-CN");
    std::fs::create_dir_all(chinese.join("common")).unwrap();
    std::fs::write(
        chinese.join("manifest.toml"),
        include_str!("../../../../../ascii-assets/assets/locales/zh-CN/manifest.toml"),
    )
    .unwrap();
    std::fs::write(
        chinese.join("common/notifications.ftl"),
        include_str!("../../../../../ascii-assets/assets/locales/zh-CN/common/notifications.ftl"),
    )
    .unwrap();
    let snapshots = [("en-US", "OK"), ("zh-CN", "确定")]
        .into_iter()
        .map(|(code, label)| {
            let loaded = i18n::LanguageSnapshot::load(&root, code, 1).unwrap();
            (std::sync::Arc::new(loaded.snapshot), label)
        })
        .collect();
    std::fs::remove_dir_all(root).unwrap();
    snapshots
}

#[test]
fn locale_snapshots_rerender_retained_notifications_without_changing_lifecycle() {
    let started_at = Instant::now();
    let mut center = NotificationCenter::new(i18n::msg!("notifications-action-ok"));
    center.notify_toast_at(i18n::msg!("notifications-action-ok"), started_at);
    center.notify_alert_with_key(
        "stable-alert",
        i18n::msg!("notifications-action-ok"),
        NotificationTone::Warning,
    );
    let active_id = center.push_modal(
        Notification::modal(
            i18n::msg!("notifications-action-ok"),
            i18n::msg!("notifications-action-ok"),
            NotificationTone::Info,
            vec![
                NotificationAction::new("accept", i18n::msg!("notifications-action-ok")),
                NotificationAction::new("cancel", "Raw cancel").cancel(),
            ],
        )
        .with_key("stable-modal")
        .with_selected_action(1),
    );
    let queued_id = center.push_modal(Notification::modal(
        i18n::msg!("notifications-action-ok"),
        "Raw queued body",
        NotificationTone::Info,
        Vec::new(),
    ));
    let retained = center.clone();

    for (snapshot, expected) in language_snapshots() {
        let _language = i18n::enter_snapshot(snapshot);
        assert_eq!(center.status().render_current(), expected);
        assert_eq!(center.toast().unwrap().render_current(), expected);
        assert_eq!(center.alert().unwrap().render_current(), expected);
        let modal = center.active_modal().unwrap();
        assert_eq!(modal.title.render_current(), expected);
        assert_eq!(modal.message.render_current(), expected);
        assert_eq!(modal.actions[0].label.render_current(), expected);
        assert_eq!(modal.actions[1].label.render_current(), "Raw cancel");
        assert_eq!(center.modal_queue[0].title.render_current(), expected);
        assert_eq!(
            center.modal_queue[0].actions[0].label.render_current(),
            expected
        );
        // Equality includes deadlines, IDs, selection, alert sequences and both queues.
        assert_eq!(center, retained);
    }

    assert_eq!(center.active_modal_id(), Some(active_id));
    assert_eq!(center.queued_modal_count(), 1);
    let response = center.activate_selected_action().unwrap();
    assert_eq!(response.notification_id, active_id);
    assert_eq!(response.action_id, "cancel");
    assert_eq!(center.active_modal_id(), Some(queued_id));
    assert_eq!(center.take_response(), Some(response));
    let resolved_at = started_at + Duration::from_secs(20);
    center.expire(resolved_at);
    assert!(center.toast().is_some());
    center.resolve_alert_at("stable-alert", resolved_at);
    assert_eq!(
        center.poll_deadline(),
        Some(resolved_at + DEFAULT_TOAST_DURATION)
    );
    center.expire(resolved_at + DEFAULT_TOAST_DURATION);
    assert!(center.toast().is_none());
}

#[test]
fn raw_callers_preserve_literal_text_without_message_lookup() {
    let literal = "notifications-action-ok";
    let mut center = NotificationCenter::new(literal.to_string());
    center.notify_toast(literal);
    center.notify_alert(literal.to_string(), NotificationTone::Warning);
    center.push_modal(Notification::modal(
        literal,
        literal.to_string(),
        NotificationTone::Info,
        vec![NotificationAction::new("literal", literal)],
    ));

    let raw = LocalizedText::Raw(literal.to_string());
    assert_eq!(center.status(), &raw);
    assert_eq!(center.toast(), Some(&raw));
    assert_eq!(center.alert(), Some(&raw));
    let modal = center.active_modal().unwrap();
    assert_eq!(modal.title, raw);
    assert_eq!(modal.message, raw);
    assert_eq!(modal.actions[0].label, raw);
}

fn modal(title: &str) -> Notification {
    Notification::modal(
        title,
        "Continue?",
        NotificationTone::Info,
        vec![NotificationAction::new("ok", "OK")],
    )
}

#[test]
fn toast_expires_at_deadline_and_reports_poll_deadline() {
    let started_at = Instant::now();
    let mut center = NotificationCenter::new("Ready");
    center.notify_toast_at("Saved", started_at);

    assert_eq!(
        center.poll_deadline(),
        started_at.checked_add(DEFAULT_TOAST_DURATION)
    );
    assert_eq!(
        center.poll_timeout(
            started_at + DEFAULT_TOAST_DURATION - Duration::from_millis(100),
            Duration::from_millis(250),
        ),
        Duration::from_millis(100)
    );
    center.expire(started_at + DEFAULT_TOAST_DURATION - Duration::from_millis(1));
    assert_eq!(
        center.toast().map(LocalizedText::render_current).as_deref(),
        Some("Saved")
    );
    center.expire(started_at + DEFAULT_TOAST_DURATION);
    assert_eq!(center.toast(), None);
}

#[test]
fn toast_pauses_behind_alert_and_restarts_after_last_alert_clears() {
    let started_at = Instant::now();
    let resolved_at = started_at + Duration::from_secs(20);
    let mut center = NotificationCenter::new("Ready");
    center.notify_alert_with_key("storage", "Unavailable", NotificationTone::Error);
    center.notify_alert_with_key("settings", "Invalid", NotificationTone::Warning);
    center.notify_toast_at("Saved", started_at);

    center.expire(started_at + DEFAULT_TOAST_DURATION + Duration::from_secs(1));
    assert_eq!(
        center.toast().map(LocalizedText::render_current).as_deref(),
        Some("Saved")
    );
    assert_eq!(center.poll_deadline(), None);

    center.resolve_alert_at("storage", resolved_at);
    assert_eq!(center.poll_deadline(), None);
    center.resolve_alert_at("settings", resolved_at);
    assert_eq!(
        center.poll_deadline(),
        resolved_at.checked_add(DEFAULT_TOAST_DURATION)
    );
    center.expire(resolved_at + DEFAULT_TOAST_DURATION - Duration::from_millis(1));
    assert_eq!(
        center.toast().map(LocalizedText::render_current).as_deref(),
        Some("Saved")
    );
    center.expire(resolved_at + DEFAULT_TOAST_DURATION);
    assert_eq!(center.toast(), None);
}

#[test]
fn keyed_alert_updates_in_place_and_becomes_latest_within_same_tone() {
    let mut center = NotificationCenter::new("Ready");
    center.notify_alert_with_key("first", "First", NotificationTone::Warning);
    center.notify_alert_with_key("second", "Second", NotificationTone::Warning);
    assert_eq!(center.alert_key(), Some("second"));

    center.notify_alert_with_key("first", "First updated", NotificationTone::Warning);

    assert_eq!(center.alert_count(), 2);
    assert_eq!(center.alert_key(), Some("first"));
    assert_eq!(
        center.alert().map(LocalizedText::render_current).as_deref(),
        Some("First updated")
    );
}

#[test]
fn alerts_choose_tone_priority_before_recency() {
    let mut center = NotificationCenter::new("Ready");
    center.notify_alert_with_key("error", "Error", NotificationTone::Error);
    center.notify_alert_with_key("new-warning", "Warning", NotificationTone::Warning);

    assert_eq!(center.alert_key(), Some("error"));
    assert_eq!(center.alert_tone(), Some(NotificationTone::Error));
}

#[test]
fn alert_capacity_evicts_the_oldest_sequence() {
    let mut center = NotificationCenter::new("Ready");
    for index in 0..MAX_ACTIVE_ALERTS {
        center.notify_alert_with_key(
            format!("key-{index}"),
            format!("message-{index}"),
            NotificationTone::Info,
        );
    }
    center.notify_alert_with_key("key-0", "refreshed", NotificationTone::Info);
    center.notify_alert_with_key("overflow", "latest", NotificationTone::Info);

    assert_eq!(center.alert_count(), MAX_ACTIVE_ALERTS);
    assert_eq!(
        center
            .alert_message_for_key("key-0")
            .map(LocalizedText::render_current)
            .as_deref(),
        Some("refreshed")
    );
    assert_eq!(center.alert_message_for_key("key-1"), None);
    assert_eq!(
        center
            .alert_message_for_key("overflow")
            .map(LocalizedText::render_current)
            .as_deref(),
        Some("latest")
    );
}

#[test]
fn normal_modals_are_fifo() {
    let mut center = NotificationCenter::new("Ready");
    center.push_modal(modal("First"));
    center.push_modal(modal("Second"));
    center.push_modal(modal("Third"));

    assert_eq!(
        center
            .active_modal()
            .map(|item| item.title.render_current())
            .as_deref(),
        Some("First")
    );
    center.activate_selected_action();
    assert_eq!(
        center
            .active_modal()
            .map(|item| item.title.render_current())
            .as_deref(),
        Some("Second")
    );
    center.activate_selected_action();
    assert_eq!(
        center
            .active_modal()
            .map(|item| item.title.render_current())
            .as_deref(),
        Some("Third")
    );
}

#[test]
fn critical_modal_preempts_then_restores_previous_active_first() {
    let mut center = NotificationCenter::new("Ready");
    center.push_modal(modal("Active"));
    center.push_modal(modal("Queued"));
    center.push_critical_modal(Notification::modal(
        "Critical",
        "Recovered",
        NotificationTone::Critical,
        vec![NotificationAction::new("continue", "Continue")],
    ));

    assert_eq!(
        center
            .active_modal()
            .map(|item| item.title.render_current())
            .as_deref(),
        Some("Critical")
    );
    center.activate_selected_action();
    assert_eq!(
        center
            .active_modal()
            .map(|item| item.title.render_current())
            .as_deref(),
        Some("Active")
    );
    center.activate_selected_action();
    assert_eq!(
        center
            .active_modal()
            .map(|item| item.title.render_current())
            .as_deref(),
        Some("Queued")
    );
}

#[test]
fn keyed_modal_update_preserves_id_and_queue_position() {
    let mut center = NotificationCenter::new("Ready");
    let active_id = center.push_modal(modal("Active").with_key("active"));
    let queued_id = center.push_modal(modal("Queued").with_key("queued"));

    assert_eq!(
        center.push_modal(modal("Active updated").with_key("active")),
        active_id
    );
    assert_eq!(
        center.push_critical_modal(modal("Queued updated").with_key("queued")),
        queued_id
    );
    assert_eq!(
        center
            .active_modal()
            .map(|item| item.title.render_current())
            .as_deref(),
        Some("Active updated")
    );
    center.activate_selected_action();
    assert_eq!(
        center
            .active_modal()
            .map(|item| item.title.render_current())
            .as_deref(),
        Some("Queued updated")
    );
}

#[test]
fn action_selection_cancel_and_response_preserve_domain_ids() {
    let mut center = NotificationCenter::new("Ready");
    let notification_id = center.push_modal(Notification::modal(
        "Confirm",
        "Choose",
        NotificationTone::Warning,
        vec![
            NotificationAction::new("save", "Save"),
            NotificationAction::new("discard", "Discard"),
            NotificationAction::new("cancel", "Cancel").cancel(),
        ],
    ));

    assert_eq!(center.cancel_action_index(), Some(2));
    assert_eq!(center.explicit_cancel_action_index(), Some(2));
    center.select_previous_action();
    assert_eq!(
        center
            .active_modal()
            .and_then(Notification::selected_action_index),
        Some(2)
    );
    center.select_next_action();
    center.select_next_action();
    let response = center.activate_selected_action().unwrap();

    assert_eq!(
        response,
        NotificationResponse {
            notification_id,
            action_id: "discard".to_string(),
        }
    );
    assert_eq!(center.take_response(), Some(response));
}

#[test]
fn response_queue_is_bounded_and_evicts_oldest() {
    let mut center = NotificationCenter::new("Ready");
    for index in 0..(MAX_NOTIFICATION_RESPONSES + 5) {
        center.push_modal(Notification::modal(
            "Notice",
            "Continue?",
            NotificationTone::Info,
            vec![NotificationAction::new(format!("ok-{index}"), "OK")],
        ));
        center.activate_selected_action();
    }

    assert_eq!(center.response_count(), MAX_NOTIFICATION_RESPONSES);
    assert_eq!(
        center
            .take_response()
            .map(|response| response.notification_id),
        Some(6)
    );
}

#[test]
fn empty_actions_receive_selected_cancel_fallback() {
    let notification = Notification::modal(
        "Notice",
        "No explicit action",
        NotificationTone::Info,
        Vec::new(),
    );

    assert_eq!(notification.actions.len(), 1);
    assert_eq!(notification.actions[0].id, "ok");
    assert!(notification.actions[0].cancel);
    assert!(notification.actions[0].selected);
}

#[test]
fn invalid_action_keeps_active_modal_and_dismiss_without_response_promotes_next() {
    let mut center = NotificationCenter::new("Ready");
    let first_id = center.push_modal(modal("First"));
    center.push_modal(modal("Second"));

    assert_eq!(center.activate_action(99), None);
    assert_eq!(center.active_modal_id(), Some(first_id));
    assert!(center.dismiss_active_modal_without_response());
    assert_eq!(
        center
            .active_modal()
            .map(|item| item.title.render_current())
            .as_deref(),
        Some("Second")
    );
    assert_eq!(center.response_count(), 0);
}
