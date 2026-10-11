use super::*;

#[test]
fn page_error_alerts_only_clear_their_owner_and_leave_session_alerts_alone() {
    for (screen, key) in [
        (ShellScreen::Editor, EDITOR_ALERT_KEY),
        (ShellScreen::Explorer, EXPLORER_ALERT_KEY),
        (
            ShellScreen::UserManagement,
            USER_MANAGEMENT_REFRESH_ALERT_KEY,
        ),
    ] {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (100, 35));
        let report = |session: &mut ShellSession, message: &str| match screen {
            ShellScreen::Editor => session.report_editor_error(message),
            ShellScreen::Explorer => session.report_explorer_task_error(message),
            ShellScreen::UserManagement => session.report_user_management_refresh_error(message),
            _ => unreachable!(),
        };
        state.notify_alert_with_key(key, "session", ui::NotificationTone::Warning);
        state.enter_new_page(screen);
        let first = state.active_page_id();
        report(&mut state, "first");
        state.enter_new_page(screen);
        let second = state.active_page_id();
        report(&mut state, "second");
        state.with_page_context(first, |session| match screen {
            ShellScreen::Editor => session.resolve_page_alert(key),
            ShellScreen::Explorer => session.resolve_explorer_alert(),
            ShellScreen::UserManagement => session.resolve_user_management_refresh_alert(),
            _ => unreachable!(),
        });
        assert_eq!(state.active_page_id(), second);
        assert_eq!(
            state.page_alert_message_for_key(key),
            Some(&"second".into())
        );
        assert_eq!(
            state.notification_alert_message_for_key(key),
            Some(&"session".into())
        );
        assert_eq!(state.pages.common.error_message, Some("second".into()));
        state.close_page_instance(second);
        assert!(
            state
                .app
                .notification_center()
                .page_alert_message_for_key(second, key)
                .is_none()
        );
        assert_eq!(state.app.notification_center().alert_count(), 1);
    }
}

#[test]
fn notification_scroll_returns_with_its_page_after_another_page_is_closed() {
    let mut session = session_with_long_notification();
    let first = session.active_page_id();
    let original = session.notification_active_modal_id();
    session.scroll_notification_message(2, true);
    let scrolled = session.pages.common.notification_message_scroll;
    assert!(scrolled > 0);
    session.enter_new_page(session.active_screen());
    let second = session.active_page_id();
    assert_eq!(session.pages.common.notification_message_scroll, 0);
    session.notify_modal(
        "Second",
        "Short message",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );
    assert_eq!(session.pages.common.notification_message_scroll, 0);
    session.close_page_instance(second);
    assert_eq!(session.active_page_id(), first);
    assert_eq!(session.notification_active_modal_id(), original);
    assert_eq!(session.pages.common.notification_message_scroll, scrolled);
    assert_eq!(
        session.to_notification_view_model().unwrap().scroll_offset,
        scrolled
    );
}

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
            session
                .notification_action_index_at((action.area.right() - 1, action.area.bottom() - 1)),
            Some(action.index)
        );
    }
    session.terminal_size = (80, 24);
    session.scroll_notification_message(0, false);
    assert_eq!(
        session.pages.common.notification_message_scroll,
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
    let scrolled = session.pages.common.notification_message_scroll;
    assert!(scrolled > 0);
    let next = session.notify_modal(
        "完成",
        "下一条通知",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "确认")],
    );
    assert_eq!(session.pages.common.notification_message_scroll, scrolled);
    session.activate_notification_selected();
    assert_eq!(session.notification_active_modal_id(), Some(next));
    assert_eq!(
        session.to_notification_view_model().unwrap().scroll_offset,
        0
    );
    session.scroll_notification_message(100, true);
    assert_eq!(session.pages.common.notification_message_scroll, 0);
    session.activate_notification_selected();
    assert!(!session.notification_has_active_modal());
    assert_eq!(session.pages.common.notification_message_scroll, 0);
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
