use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use tundra_ui::{
    AuthField, BootstrapAdminViewModel, ClockViewModel, DebugDiagnosticsViewModel,
    ExitConfirmViewModel, HomeDisplayMode, HomeViewModel, LoginField, LoginUserOptionViewModel,
    LoginViewModel, RuntimeAsciiAssets, ShellChromeViewModel, ShellEntry, ShellLayout,
    StatusViewModel, TimeSyncDialogViewModel, TundraTheme, UserManagementUserViewModel,
    UserManagementViewModel, compute_shell_layout, login_password_area, login_user_list_area,
    login_user_list_visible_rows, render_bootstrap_admin, render_clock_placeholder, render_home,
    render_login, render_time_sync_failure_dialog, render_user_management, status_time_button_area,
};

#[test]
fn debug_home_exposes_diagnostics_and_no_entries() {
    let diagnostics = DebugDiagnosticsViewModel {
        tick_count: 42,
        last_key_event: Some("Ctrl-C".to_string()),
        last_mouse_event: Some("Down".to_string()),
        last_resize_event: Some("100x30".to_string()),
        mouse_coordinates: Some((12, 7)),
        scroll_direction: Some("up".to_string()),
        drag_direction: Some("Right".to_string()),
        terminal_flags: vec!["alternate-screen".to_string(), "mouse-capture".to_string()],
        platform_capability_summary: "Windows: 10 supported, 0 best-effort, 3 unsupported"
            .to_string(),
    };

    let home = HomeViewModel::debug(diagnostics.clone());

    assert_eq!(home.display_mode(), HomeDisplayMode::Debug);
    assert_eq!(home.diagnostics(), Some(&diagnostics));
    assert!(home.entries().is_empty());
}

#[test]
fn debug_home_renders_platform_capability_summary() {
    let diagnostics = DebugDiagnosticsViewModel {
        tick_count: 0,
        last_key_event: None,
        last_mouse_event: None,
        last_resize_event: None,
        mouse_coordinates: None,
        scroll_direction: None,
        drag_direction: None,
        terminal_flags: Vec::new(),
        platform_capability_summary: "Windows: 10 supported, 0 best-effort, 3 unsupported"
            .to_string(),
    };
    let home = HomeViewModel::debug(diagnostics);
    let chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Debug,
        terminal_size: (100, 30),
        screen_stack: vec!["Home".to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            time_button_label: None,
            time_button_selected: false,
        },
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_home(
                frame,
                frame.area(),
                &chrome,
                &home,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render home");

    let output: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(output.contains("Platform capabilities: Windows: 10 supported"));
}

#[test]
fn user_home_hides_diagnostics_and_lists_five_entries_including_explorer() {
    let entries = vec![
        ShellEntry::new("Explorer", "Browse files and pinned places"),
        ShellEntry::new("Terminal", "Open a shell session"),
        ShellEntry::new("Settings", "Configure TundraUX 3"),
        ShellEntry::new("Sessions", "Resume recent work"),
        ShellEntry::new("Help", "Show keyboard shortcuts"),
    ];

    let home = HomeViewModel::user("Strix", "2026-07-01 09:30", entries);

    assert_eq!(home.display_mode(), HomeDisplayMode::User);
    assert_eq!(home.diagnostics(), None);
    assert_eq!(home.entries().len(), 5);
    assert!(home.entries().iter().any(|entry| entry.label == "Explorer"));
}

#[test]
fn home_icon_asset_exposes_known_ascii_icon_metadata() {
    let assets = RuntimeAsciiAssets::load_default().expect("home icon assets should load");
    let catalog = assets.home_icon_catalog();
    let icon = catalog
        .icon_for_label("Explorer")
        .expect("catalog should expose Explorer by label");
    let key: &str = icon.key.as_ref();
    let icon_by_key = catalog
        .icon_for_key(key)
        .expect("catalog should expose the same icon by key");

    assert!(icon.width > 0);
    assert!(icon.height > 0);
    assert_eq!(icon.lines.len(), icon.height);
    assert!(icon.lines.iter().all(|line| line.is_ascii()));
    assert_eq!(icon_by_key.width, icon.width);
    assert_eq!(icon_by_key.height, icon.height);
    assert_eq!(icon_by_key.lines.len(), icon.lines.len());
    assert_eq!(
        first_non_blank_icon_line(icon_by_key),
        first_non_blank_icon_line(icon)
    );
}

#[test]
fn user_home_renders_ascii_entry_tiles_with_selected_accent() {
    let entries = vec![
        ShellEntry::new("Explorer", "Browse files"),
        ShellEntry::new("Launcher", "Open apps and commands"),
        ShellEntry::new("Editor", "Edit text files"),
    ];
    let home = HomeViewModel::user_with_selection("Strix", "2026-07-01 09:30", entries, 1);
    let chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::User,
        terminal_size: (100, 30),
        screen_stack: vec!["Home".to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            time_button_label: None,
            time_button_selected: false,
        },
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_home(
                frame,
                frame.area(),
                &chrome,
                &home,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render home");

    let output = terminal_output(&terminal);
    let icon_line = first_non_blank_icon_line(
        home.home_icon_for_label("Launcher")
            .expect("home view model should carry loaded icon assets"),
    );
    assert!(output.contains("User: Strix"));
    assert!(output.contains("Time: 2026-07-01 09:30"));
    assert!(output.contains(icon_line));
    assert!(output.contains("Launcher"));
    assert!(output.contains("Open apps and commands"));
    assert!(output.contains("Arrows: select"));
    assert!(output.contains("Enter: open"));

    let main = main_rect(100, 30);
    let selected_tile = tundra_ui::home_entry_tile_areas(main, home.entries().len())[1];
    assert!(
        region_has_fg(&terminal, selected_tile, TundraTheme::default_dark().accent),
        "selected home tile should use the accent style"
    );
}

#[test]
fn user_home_preserves_ascii_icon_spacing_when_centered() {
    let entries = vec![ShellEntry::new("Settings", "Adjust TundraUX")];
    let home = HomeViewModel::user_with_selection("Strix", "2026-07-01 09:30", entries, 0);
    let chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::User,
        terminal_size: (100, 30),
        screen_stack: vec!["Home".to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            time_button_label: None,
            time_button_selected: false,
        },
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_home(
                frame,
                frame.area(),
                &chrome,
                &home,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render home");

    let main = main_rect(100, 30);
    let tile = tundra_ui::home_entry_tile_areas(main, home.entries().len())[0];
    let icon = home
        .home_icon_for_label("Settings")
        .expect("home view model should carry loaded icon assets");

    assert_centered_icon_matches_asset(&terminal, tile, icon);
}

#[test]
fn home_entry_index_at_maps_coordinates_to_entry_tiles() {
    let main = main_rect(100, 30);
    let tile_areas = tundra_ui::home_entry_tile_areas(main, 5);
    let first_tile = tile_areas[0];
    let second_tile = tile_areas[1];

    assert_eq!(
        tundra_ui::home_entry_index_at(
            main,
            5,
            (
                first_tile.x.saturating_add(1),
                first_tile.y.saturating_add(1),
            ),
        ),
        Some(0)
    );
    assert_eq!(
        tundra_ui::home_entry_index_at(
            main,
            5,
            (
                second_tile.x.saturating_add(1),
                second_tile.y.saturating_add(1),
            ),
        ),
        Some(1)
    );
    assert_eq!(
        tundra_ui::home_entry_index_at(main, 5, (main.x, main.y)),
        None
    );
}

#[test]
fn small_terminal_returns_compact_layout() {
    assert_eq!(
        compute_shell_layout(Rect::new(0, 0, 49, 30)),
        ShellLayout::Compact(Rect::new(0, 0, 49, 30))
    );
    assert_eq!(
        compute_shell_layout(Rect::new(0, 0, 100, 11)),
        ShellLayout::Compact(Rect::new(0, 0, 100, 11))
    );
}

#[test]
fn normal_terminal_splits_top_main_status() {
    assert_eq!(
        compute_shell_layout(Rect::new(0, 0, 100, 30)),
        ShellLayout::Full {
            top: Rect::new(0, 0, 100, 3),
            main: Rect::new(0, 3, 100, 24),
            status: Rect::new(0, 27, 100, 3),
        }
    );
}

#[test]
fn default_dark_theme_exposes_expected_colors_and_styles() {
    let theme = TundraTheme::default_dark();

    assert_eq!(theme.background, Color::Black);
    assert_eq!(theme.foreground, Color::Gray);
    assert_eq!(theme.accent, Color::Cyan);
    assert_eq!(theme.muted, Color::DarkGray);
    assert_eq!(theme.error, Color::Red);
    assert_eq!(theme.title_style().fg, Some(Color::Cyan));
    assert_eq!(theme.body_style().fg, Some(Color::Gray));
    assert_eq!(theme.muted_style().fg, Some(Color::DarkGray));
    assert_eq!(theme.error_style().fg, Some(Color::Red));
}

#[test]
fn exit_confirmation_defaults_match_shell_copy() {
    let expected = ExitConfirmViewModel::new();
    let defaulted = ExitConfirmViewModel::default();

    assert_eq!(expected.title, "Exit TundraUX 3");
    assert_eq!(
        expected.message,
        "Leave the shell and restore the terminal?"
    );
    assert_eq!(expected.confirm_label, "Y / Enter: exit");
    assert_eq!(expected.cancel_label, "N / Esc: cancel");
    assert_eq!(defaulted, expected);
}

#[test]
fn status_view_model_exposes_status_toast_and_error() {
    let status = StatusViewModel {
        status: "Ready".to_string(),
        toast: Some("Saved".to_string()),
        error: Some("Network unavailable".to_string()),
        time_button_label: Some("2026-07-10 09:30".to_string()),
        time_button_selected: true,
    };

    assert_eq!(status.status, "Ready");
    assert_eq!(status.toast.as_deref(), Some("Saved"));
    assert_eq!(status.error.as_deref(), Some("Network unavailable"));
    assert_eq!(
        status.time_button_label.as_deref(),
        Some("2026-07-10 09:30")
    );
    assert!(status.time_button_selected);
}

#[test]
fn status_bar_renders_selectable_time_button_on_the_right() {
    let label = "2026-07-10 09:30";
    let diagnostics = DebugDiagnosticsViewModel {
        tick_count: 0,
        last_key_event: None,
        last_mouse_event: None,
        last_resize_event: None,
        mouse_coordinates: None,
        scroll_direction: None,
        drag_direction: None,
        terminal_flags: Vec::new(),
        platform_capability_summary: "Windows: ready".to_string(),
    };
    let home = HomeViewModel::debug(diagnostics);
    let chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Debug,
        terminal_size: (120, 30),
        screen_stack: vec!["Home".to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: Some("Saved".to_string()),
            error: Some("Network unavailable".to_string()),
            time_button_label: Some(label.to_string()),
            time_button_selected: true,
        },
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_home(
                frame,
                frame.area(),
                &chrome,
                &home,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render home");

    let output = terminal_output(&terminal);
    assert!(output.contains("Ready"));
    assert!(output.contains("Saved"));
    assert!(output.contains("Network unavailable"));
    assert!(output.contains(label));

    let status = status_rect(120, 30);
    let button = status_time_button_area(status, label);
    assert_eq!(
        button.x.saturating_add(button.width),
        status.x + status.width
    );
    assert_eq!(
        button.width,
        u16::try_from(label.chars().count()).unwrap() + 4
    );
    assert!(
        region_has_fg(&terminal, button, TundraTheme::default_dark().accent),
        "selected time button should use the accent style"
    );
}

#[test]
fn status_time_button_area_clamps_long_labels_and_preserves_left_space() {
    let status = Rect::new(0, 20, 50, 3);
    let button = status_time_button_area(status, "2026-07-10 09:30:45 Asia/Shanghai UTC+08");

    assert_eq!(
        button.x.saturating_add(button.width),
        status.x + status.width
    );
    assert_eq!(button.width, 38);
    assert_eq!(button.x, 12);
}

#[test]
fn clock_placeholder_and_time_sync_failure_dialog_render_expected_content() {
    let mut clock_terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    let clock = ClockViewModel::new("2026-07-10 09:30 Asia/Shanghai");

    clock_terminal
        .draw(|frame| {
            render_clock_placeholder(
                frame,
                frame.area(),
                &chrome_for("Clock"),
                &clock,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render clock placeholder");

    let output = terminal_output(&clock_terminal);
    assert!(output.contains("Clock"));
    assert!(output.contains("Current time: 2026-07-10 09:30 Asia/Shanghai"));

    let mut dialog_terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    dialog_terminal
        .draw(|frame| {
            render_time_sync_failure_dialog(
                frame,
                frame.area(),
                &TimeSyncDialogViewModel::new(),
                &TundraTheme::default_dark(),
            );
        })
        .expect("render time sync dialog");

    let output = terminal_output(&dialog_terminal);
    assert!(output.contains("Time Sync"));
    assert!(visible_text_without_spaces(&output).contains("联网校准时间失败"));
}

#[test]
fn login_renderer_masks_password_length() {
    let chrome = chrome_for("Login");
    let model = LoginViewModel::new(
        vec![
            login_user("AdminUser", "Admin User", "Admin"),
            login_user("Strix", "Local User", "User"),
            login_user("debug", "Debug User", "Debug"),
        ],
        1,
        0,
        "StrongPass123".len(),
        LoginField::Password,
        Some("Invalid username or password".to_string()),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_login(
                frame,
                frame.area(),
                &chrome,
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render login");

    let output = terminal_output(&terminal);
    assert!(output.contains("Users"));
    assert!(output.contains("AdminUser (Admin)"));
    assert!(output.contains("Strix"));
    assert!(output.contains("Local User"));
    assert!(output.contains("Password"));
    assert!(output.contains("*************"));
    assert!(!output.contains("StrongPass123"));
    assert!(output.contains("Invalid username or password"));

    let main = main_rect(80, 24);
    let list_area = login_user_list_area(main);
    let password_area = login_password_area(main);
    assert!(list_area.x < password_area.x);
    assert_eq!(
        login_user_list_visible_rows(main),
        usize::from(list_area.height.saturating_sub(2))
    );
    assert!(
        region_has_fg(&terminal, password_area, TundraTheme::default_dark().accent),
        "focused password field should use the accent style"
    );
}

#[test]
fn bootstrap_and_user_management_render_expected_content() {
    let chrome = chrome_for("BootstrapAdmin");
    let bootstrap = BootstrapAdminViewModel::new("AdminUser", 10, AuthField::Username, None);
    let mut terminal = Terminal::new(TestBackend::new(90, 24)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_bootstrap_admin(
                frame,
                frame.area(),
                &chrome,
                &bootstrap,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render bootstrap");
    let output = terminal_output(&terminal);
    assert!(output.contains("Tab / Down: password"));
    assert!(output.contains("Enter on password: create admin"));
    assert!(output.contains("Admin username: AdminUser"));

    let management = UserManagementViewModel::new(
        "AdminUser",
        vec![UserManagementUserViewModel {
            username: "user2".to_string(),
            display_name: "User Two".to_string(),
            role: "User".to_string(),
            enabled: true,
            locked: false,
        }],
        0,
        Some("Created user2".to_string()),
        true,
        None,
    );
    terminal
        .draw(|frame| {
            render_user_management(
                frame,
                frame.area(),
                &chrome,
                &management,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render user management");
    let output = terminal_output(&terminal);
    assert!(output.contains("Current user: AdminUser"));
    assert!(output.contains("user2 (User Two) | User | enabled"));
    assert!(output.contains("Created user2"));
}

fn chrome_for(screen: &str) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (80, 24),
        screen_stack: vec![screen.to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn terminal_output(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn visible_text_without_spaces(output: &str) -> String {
    output
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn first_non_blank_icon_line(icon: &tundra_ui::HomeIcon) -> &str {
    icon.lines
        .iter()
        .find_map(|line| {
            let line: &str = line.as_ref();
            (!line.trim().is_empty()).then_some(line)
        })
        .expect("icon asset should contain visible content")
}

fn assert_centered_icon_matches_asset(
    terminal: &Terminal<TestBackend>,
    tile: Rect,
    icon: &tundra_ui::HomeIcon,
) {
    let buffer = terminal.backend().buffer();
    let content_x = tile.x.saturating_add(1);
    let content_y = tile.y.saturating_add(1);
    let content_width = tile.width.saturating_sub(2);
    let icon_width = u16::try_from(icon.width).expect("icon width should fit terminal");
    let start_x = content_x + content_width.saturating_sub(icon_width) / 2;

    for (row, line) in icon.lines.iter().enumerate() {
        let y = content_y + u16::try_from(row).expect("icon row should fit terminal");
        for (column, character) in line.chars().enumerate() {
            let x = start_x + u16::try_from(column).expect("icon column should fit terminal");
            let actual = buffer
                .cell((x, y))
                .expect("expected rendered icon cell")
                .symbol();
            let expected = character.to_string();
            assert_eq!(
                actual,
                expected.as_str(),
                "icon line {row}, column {column} should preserve asset spacing"
            );
        }
    }
}

fn login_user(username: &str, display_name: &str, role: &str) -> LoginUserOptionViewModel {
    LoginUserOptionViewModel {
        username: username.to_string(),
        display_name: display_name.to_string(),
        role: role.to_string(),
        enabled: true,
        locked: false,
    }
}

fn main_rect(width: u16, height: u16) -> Rect {
    match compute_shell_layout(Rect::new(0, 0, width, height)) {
        ShellLayout::Full { main, .. } => main,
        ShellLayout::Compact(_) => panic!("home render tests expect a full shell layout"),
    }
}

fn status_rect(width: u16, height: u16) -> Rect {
    match compute_shell_layout(Rect::new(0, 0, width, height)) {
        ShellLayout::Full { status, .. } => status,
        ShellLayout::Compact(_) => panic!("home render tests expect a full shell layout"),
    }
}

fn region_has_fg(terminal: &Terminal<TestBackend>, area: Rect, fg: Color) -> bool {
    let buffer = terminal.backend().buffer();
    (area.y..area.y.saturating_add(area.height)).any(|y| {
        (area.x..area.x.saturating_add(area.width)).any(|x| {
            buffer
                .cell((x, y))
                .is_some_and(|cell| cell.fg == fg && cell.symbol() != " ")
        })
    })
}
