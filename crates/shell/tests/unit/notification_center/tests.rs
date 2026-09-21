use super::*;

#[test]
fn presentation_rerenders_scoped_messages_without_mutating_notifications_or_bindings() {
    let root = std::env::temp_dir().join(format!(
        "tux3-shell-notifications-{}-{}",
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
        include_str!("../../../../ascii-assets/assets/locales/zh-CN/manifest.toml"),
    )
    .unwrap();
    std::fs::write(
        chinese.join("common/notifications.ftl"),
        include_str!("../../../../ascii-assets/assets/locales/zh-CN/common/notifications.ftl"),
    )
    .unwrap();
    for (code, source) in [
        ("en-US", "notification-test-detail = Saved { $name }\n"),
        ("zh-CN", "notification-test-detail = 已保存 { $name }\n"),
    ] {
        let common = root.join("locales").join(code).join("common");
        std::fs::create_dir_all(&common).unwrap();
        std::fs::write(common.join("notification-test.ftl"), source).unwrap();
    }
    let english = i18n::LanguageSnapshot::load(&root, "en-US", 1).unwrap();
    let chinese = i18n::LanguageSnapshot::load(&root, "zh-CN", 2).unwrap();
    std::fs::remove_dir_all(root).unwrap();

    let started_at = Instant::now();
    let mut center = NotificationCenter::new(i18n::msg!("notifications-status-ready"));
    let detail =
        i18n::LocalizedMessage::new("notification-test-detail").with_arg("name", "report.txt");
    center.notify_toast_at(detail.clone(), started_at);
    center.notify_alert_with_key(
        "stable-alert",
        i18n::msg!("notifications-action-ok"),
        ui::NotificationTone::Warning,
    );
    let id = center.push_modal(
        ShellNotification::modal(
            i18n::msg!("notifications-action-ok"),
            detail,
            ui::NotificationTone::Info,
            vec![
                ShellNotificationAction::new("accept", i18n::msg!("notifications-action-ok"))
                    .with_shortcut(InputKey::Char('o')),
                ShellNotificationAction::new("cancel", "Raw cancel").cancel(),
            ],
        )
        .with_selected_action(1)
        .with_component(ShellComponent::ExitDialog)
        .with_key("stable-modal"),
    );
    center.push_modal(ShellNotification::modal(
        "Raw queued title",
        "Raw queued body",
        ui::NotificationTone::Info,
        Vec::new(),
    ));
    let retained = center.clone();

    for (snapshot, ready, label, saved) in [
        (english.snapshot, "Ready", "OK", "Saved"),
        (chinese.snapshot, "就绪", "确定", "已保存"),
    ] {
        let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
        assert_eq!(center.status(), ready);
        let toast = center.toast().unwrap();
        assert!(toast.starts_with(saved));
        assert!(toast.contains("report.txt"));
        assert_eq!(center.alert().as_deref(), Some(label));
        let model = center.active_modal_view_model().unwrap();
        assert_eq!(model.id, id.to_string());
        assert_eq!(model.title, label);
        assert_eq!(model.message, toast);
        assert_eq!(model.actions[0].id, "accept");
        assert_eq!(model.actions[0].label, label);
        assert_eq!(model.actions[0].shortcut.as_deref(), Some("o"));
        assert_eq!(model.actions[1].label, "Raw cancel");
        assert!(model.actions[1].selected);
        assert_eq!(center.action_index_for_key(&InputKey::Char('O')), Some(0));
        assert_eq!(
            center.active_modal_component(),
            Some(ShellComponent::ExitDialog)
        );
        assert_eq!(center, retained);
    }
    center.activate_selected_action();
    assert_eq!(
        center.take_response(),
        Some(ShellNotificationResponse {
            notification_id: id,
            action_id: "cancel".to_string(),
        })
    );
    assert_eq!(
        center.active_modal_view_model().unwrap().title,
        "Raw queued title"
    );
}
