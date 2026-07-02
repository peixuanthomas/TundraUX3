use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use tundra_ui::{
    AuthField, BootstrapAdminViewModel, DebugDiagnosticsViewModel, ExitConfirmViewModel,
    HomeDisplayMode, HomeViewModel, LoginViewModel, ShellChromeViewModel, ShellEntry, ShellLayout,
    StatusViewModel, TundraTheme, UserManagementUserViewModel, UserManagementViewModel,
    compute_shell_layout, render_bootstrap_admin, render_home, render_login,
    render_user_management,
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
    };

    assert_eq!(status.status, "Ready");
    assert_eq!(status.toast.as_deref(), Some("Saved"));
    assert_eq!(status.error.as_deref(), Some("Network unavailable"));
}

#[test]
fn login_renderer_masks_password_length() {
    let chrome = chrome_for("Login");
    let model = LoginViewModel::new(
        "AdminUser",
        "StrongPass123".len(),
        AuthField::Password,
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
    assert!(output.contains("Tab / Down: password"));
    assert!(output.contains("Enter on password: login"));
    assert!(output.contains("Username: AdminUser"));
    assert!(output.contains("Password: *************"));
    assert!(!output.contains("StrongPass123"));
    assert!(output.contains("Invalid username or password"));
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
            role: "User".to_string(),
            enabled: true,
            locked: false,
        }],
        0,
        Some("Created user2".to_string()),
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
    assert!(output.contains("user2 | User | enabled"));
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
