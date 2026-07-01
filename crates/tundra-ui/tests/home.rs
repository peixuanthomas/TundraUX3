use ratatui::layout::Rect;
use ratatui::style::Color;
use tundra_ui::{
    DebugDiagnosticsViewModel, ExitConfirmViewModel, HomeDisplayMode, HomeViewModel, ShellEntry,
    ShellLayout, StatusViewModel, TundraTheme, compute_shell_layout,
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
        terminal_flags: vec!["alternate-screen".to_string(), "mouse-capture".to_string()],
    };

    let home = HomeViewModel::debug(diagnostics.clone());

    assert_eq!(home.display_mode(), HomeDisplayMode::Debug);
    assert_eq!(home.diagnostics(), Some(&diagnostics));
    assert!(home.entries().is_empty());
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
