use ratatui::{Terminal, backend::TestBackend, layout::Rect, widgets::Paragraph};
use ui::*;

fn chrome() -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".into(),
        build_mode: "test".into(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (49, 11),
        back_button_hovered: false,
        screen_stack: vec!["Compact".into()],
        status: StatusViewModel {
            status: "Compact status".into(),
            toast: Some("Compact toast".into()),
            error: Some("Compact alert".into()),
            alert_tone: NotificationTone::Error,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn first_line(terminal: &Terminal<TestBackend>, width: u16) -> String {
    (0..width)
        .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
        .collect()
}

#[test]
fn compact_pages_keep_the_highest_priority_notification_above_their_content() {
    let clock = ClockViewModel::new("now");
    let login = LoginViewModel::new(Vec::new(), 0, 0, 0, LoginField::Password, None);
    let bootstrap = BootstrapAdminViewModel::new("", 0, AuthField::Username, None);
    let users = UserManagementViewModel::new("Admin", Vec::new(), 0, None, true, None);
    let management = ManagementViewModel::default();
    let logs = LogsViewModel::default();
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let chrome = chrome();
    for page in [
        ScreenContent::Clock(&clock),
        ScreenContent::Login(&login),
        ScreenContent::BootstrapAdmin(&bootstrap),
        ScreenContent::UserManagement(&users),
        ScreenContent::Management(&management),
        ScreenContent::Logs(&logs),
    ] {
        let mut terminal = Terminal::new(TestBackend::new(49, 11)).unwrap();
        terminal
            .draw(|frame| {
                let layout = ShellFrameLayout::new(frame.area(), None, &context);
                page.render_content(frame, &layout, &context, None, None);
                page.render_overlay(frame, &layout, &context);
                render_shell_chrome(frame, &layout, &chrome, &context);
            })
            .unwrap();
        let line = first_line(&terminal, 49);
        assert!(line.starts_with("[ERROR] Compact alert"), "{line}");
        assert!(!line.contains("Compact toast"));
        assert!(!line.contains("Compact status"));
    }
}

#[test]
fn compact_chrome_shows_toasts_and_preserves_page_header_without_notifications() {
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let mut chrome = chrome();
    chrome.status.error = None;
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).unwrap();
    for has_toast in [true, false] {
        if !has_toast {
            chrome.status.toast = None;
        }
        terminal
            .draw(|frame| {
                let area = frame.area();
                let layout = ShellFrameLayout::new(area, None, &context);
                frame.render_widget(
                    Paragraph::new("Page header"),
                    Rect::new(layout.main.x, layout.main.y, layout.main.width, 1),
                );
                render_shell_chrome(frame, &layout, &chrome, &context);
            })
            .unwrap();
        let line = first_line(&terminal, 49);
        assert!(
            line.starts_with(if has_toast {
                "Compact toast"
            } else {
                "TundraUX 3"
            }),
            "{line}"
        );
        assert!(line.contains("[◀]"));
        let content: String = (0..49)
            .map(|x| terminal.backend().buffer()[(x, 1)].symbol())
            .collect();
        assert!(content.starts_with("Page header"));
    }
}

#[test]
fn compact_notification_respects_empty_and_tiny_terminal_bounds() {
    let context = RenderContext::default();
    let chrome = chrome();
    for (width, height) in [(0, 0), (1, 1), (2, 2), (49, 1)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let layout = ShellFrameLayout::new(frame.area(), None, &context);
                render_shell_chrome(frame, &layout, &chrome, &context);
            })
            .unwrap();
        if width > 0 && height > 0 {
            let line = first_line(&terminal, width);
            if width == 1 {
                // A single cell can only show the opening button bracket.
                assert_eq!(line, "[");
            } else {
                assert!(line.contains('◀'), "{width}x{height}: {line}");
            }
        }
    }
}
