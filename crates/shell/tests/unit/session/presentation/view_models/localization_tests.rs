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

#[test]
fn selected_file_menu_keeps_the_actions_moved_from_the_toolbar() {
    let ui::ExplorerOverlayViewModel::ContextMenu(menu) =
        explorer_context_menu_view_model(ExplorerContextMenuInput {
            anchor: (0, 0),
            selected_count: 1,
            entry_count: 1,
            clipboard_available: false,
            undo_available: false,
            is_trash: false,
            trash_has_items: false,
            focused_index: 0,
            can_manage_launcher: false,
            can_execute_command_line: false,
            launcher_eligible_count: 0,
        })
    else {
        panic!("file menu");
    };
    for id in ["cut", "copy", "paste", "sort", "options"] {
        assert!(menu.items.iter().any(|item| item.id == id), "{id}");
    }
    assert!(
        !menu
            .items
            .iter()
            .find(|item| item.id == "paste")
            .unwrap()
            .enabled
    );
}

#[test]
fn chrome_paths_localize_page_names_without_changing_navigation_or_page_ids() {
    use ratatui::{Terminal, backend::TestBackend};

    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for (code, home) in [("en-US", "Home"), ("zh-CN", "主页")] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (108, 20),
            ShellHomeMode::User,
        );
        state.language = std::sync::Arc::new(
            i18n::LanguageSnapshot::load(&assets, code, 1)
                .unwrap()
                .snapshot,
        );
        let _language = i18n::enter_snapshot(state.language.clone());
        for (screen, english, chinese) in [
            (
                ShellScreen::FirstRunSetup,
                "First Run Setup",
                "首次运行设置",
            ),
            (ShellScreen::BootstrapAdmin, "Create Admin", "创建管理员"),
            (ShellScreen::CommandLine, "Command Line", "命令行"),
            (ShellScreen::Management, "Linux Management", "Linux 运维"),
            (ShellScreen::SystemStatus, "System Status", "系统状态"),
            (ShellScreen::UserManagement, "User Management", "用户管理"),
        ] {
            state.set_navigation_path(vec![ShellScreen::Home, screen]);
            let ids = state.live_page_ids();
            let chrome = state.to_shell_chrome_view_model();
            let title = if code == "en-US" { english } else { chinese };
            assert_eq!(chrome.screen_stack, [home, title]);
            assert_eq!(state.screen_stack(), [ShellScreen::Home, screen]);
            assert_eq!(state.live_page_ids(), ids);
            let context = ui::RenderContext::default();
            let layout = ui::ShellFrameLayout::new(Rect::new(0, 0, 108, 20), None, &context)
                .with_back_shortcut(chrome.back_shortcut);
            let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
            terminal
                .draw(|frame| ui::render_shell_chrome(frame, &layout, &chrome, &context))
                .unwrap();
            let header: String = (0..108)
                .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
                .collect();
            assert!(
                header.replace(' ', "").contains(&title.replace(' ', "")),
                "{code}: {header}"
            );
            for internal in [
                "FirstRunSetup",
                "BootstrapAdmin",
                "CommandLine",
                "SystemStatus",
                "UserManagement",
            ] {
                assert!(!header.contains(internal), "{code}: {header}");
            }
        }
    }
}

#[test]
fn running_external_terminal_names_stay_in_chrome_and_leave_the_full_pty_visible() {
    use ratatui::{Terminal, backend::TestBackend};

    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for code in ["en-US", "zh-CN"] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (108, 20),
            ShellHomeMode::User,
        );
        state.language = std::sync::Arc::new(
            i18n::LanguageSnapshot::load(&assets, code, 1)
                .unwrap()
                .snapshot,
        );
        let _language = i18n::enter_snapshot(state.language.clone());
        state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::CommandLine]);
        state.pages.command_line.command_line_external = Some(ExternalTerminalLaunch {
            program: PathBuf::from("/opt/tools/previous-app"),
            args: Vec::new(),
            wait_for_enter: None,
        });
        state.enter_new_page(ShellScreen::CommandLine);
        state.pages.command_line.command_line_external = Some(ExternalTerminalLaunch {
            program: PathBuf::from("/opt/tools/external-app"),
            args: Vec::new(),
            wait_for_enter: None,
        });
        let chrome = state.to_shell_chrome_view_model();
        assert_eq!(&chrome.screen_stack[1..], ["previous-app", "external-app"]);
        let context = ui::RenderContext::default();
        let layout = ui::ShellFrameLayout::new(Rect::new(0, 0, 108, 20), None, &context)
            .with_back_shortcut(chrome.back_shortcut);
        assert_eq!(layout.main, Rect::new(0, 1, 108, 18));
        assert_eq!(
            ui::command_line_terminal_area_in(&layout),
            Some(layout.main)
        );
        let mut snapshot = ui::CommandLineTerminalSnapshot::blank(108, 18);
        for (index, character) in "PTY LAST ROW".chars().enumerate() {
            snapshot.cells[17 * 108 + index].symbol = character.to_string();
        }
        let mut command_line = ui::CommandLineViewModel::new(snapshot);
        command_line.title = state
            .pages
            .command_line
            .command_line_external
            .as_ref()
            .map(ExternalTerminalLaunch::title);
        assert_eq!(
            command_line.process_state,
            ui::CommandLineProcessState::Running
        );
        let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
        terminal
            .draw(|frame| {
                ui::ScreenContent::CommandLine(&command_line)
                    .render_content(frame, &layout, &context, None, None);
                ui::render_shell_chrome(frame, &layout, &chrome, &context);
            })
            .unwrap();
        let header: String = (0..108)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect();
        assert!(
            header.contains("previous-app > external-app"),
            "{code}: {header}"
        );
        assert!(!header.contains("CommandLine"));
        let last_row: String = (0..108)
            .map(|x| terminal.backend().buffer()[(x, 18)].symbol())
            .collect();
        assert!(last_row.starts_with("PTY LAST ROW"), "{code}: {last_row}");
        let content: String = layout
            .main
            .positions()
            .map(|point| terminal.backend().buffer()[point].symbol())
            .collect();
        assert!(!content.contains("external-app"));

        let previous = state.live_page_ids()[1];
        state
            .pages
            .command_line
            .get_mut(previous)
            .unwrap()
            .command_line_external
            .as_mut()
            .unwrap()
            .program = PathBuf::from(format!(
            "/opt/tools/{}",
            "之前的外部应用名称很长".repeat(12)
        ));
        let chrome = state.to_shell_chrome_view_model();
        terminal
            .draw(|frame| {
                ui::ScreenContent::CommandLine(&command_line)
                    .render_content(frame, &layout, &context, None, None);
                ui::render_shell_chrome(frame, &layout, &chrome, &context);
            })
            .unwrap();
        let header: String = (0..108)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect();
        assert!(header.contains("external-app"), "{code}: {header}");
        assert_eq!(
            ui::command_line_terminal_area_in(&layout),
            Some(layout.main)
        );
    }
}
