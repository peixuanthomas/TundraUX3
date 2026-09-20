use super::*;

fn snapshots() -> [std::sync::Arc<i18n::LanguageSnapshot>; 2] {
    let root = std::env::temp_dir().join(format!(
        "tux3-shell-message-tests-{}-{}",
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
    let snapshots = ["en-US", "zh-CN"].map(|code| {
        std::sync::Arc::new(
            i18n::LanguageSnapshot::load(&root, code, 1)
                .unwrap()
                .snapshot,
        )
    });
    // Formatting must work entirely from the snapshots, including nested messages.
    std::fs::remove_dir_all(root).unwrap();
    snapshots
}

#[test]
fn retained_shell_notifications_render_with_the_session_snapshot() {
    let [english, chinese] = snapshots();
    let mut session = ShellSession::new(ShellLaunchConfig::default(), (120, 30));
    session.home_mode = ShellHomeMode::User;
    session.language = english.clone();
    let raw_path = "/tmp/{ready}/Saved.txt";
    session.notify_status(i18n::msg!("shell-saving-arg1", arg1 = raw_path));
    session.notify_toast(i18n::msg!(
        "shell-home-arg1",
        arg1 = i18n::msg!("shell-explorer")
    ));
    let retained_status = session.app.notification_center().status().clone();
    let english_model = session.to_shell_chrome_view_model();
    assert_eq!(english_model.status.status, format!("Saving {raw_path}"));
    assert_eq!(
        english_model.status.toast.as_deref(),
        Some("Home: Explorer")
    );

    // The public VM entry must override a different ambient snapshot and restore it on exit.
    let _ambient = i18n::enter_snapshot(english);
    session.language = chinese;
    let chinese_model = session.to_shell_chrome_view_model();
    assert_eq!(chinese_model.status.status, format!("正在保存 {raw_path}"));
    assert_eq!(
        chinese_model.status.toast.as_deref(),
        Some("主页：文件管理器")
    );
    assert_eq!(session.app.notification_center().status(), &retained_status);
    assert_eq!(i18n::tr!("shell-ready"), "Ready");
}

#[test]
fn translated_explorer_sort_choices_keep_stable_command_ids() {
    let [_, chinese] = snapshots();
    let _language = i18n::enter_snapshot(chinese);
    let ui::ExplorerOverlayViewModel::ContextMenu(menu) =
        explorer_sort_menu_view_model((0, 0), ui::ExplorerSortColumn::Name, 0)
    else {
        panic!("sort menu expected");
    };
    assert_eq!(menu.title, "排序方式");
    assert_eq!(
        menu.items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["sort-name", "sort-type", "sort-size", "sort-modified"]
    );
}
