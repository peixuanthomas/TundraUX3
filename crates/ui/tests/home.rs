#[path = "support/composition.rs"]
mod composition;
use composition as ui;
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use std::cell::RefCell;
use support::terminal_output;
use ui::{
    AuthField, BootstrapAdminViewModel, ClockViewModel, DebugDiagnosticsViewModel,
    ExitConfirmViewModel, HomeDisplayMode, HomeIconRenderer, HomeViewModel, LoginField,
    LoginUserOptionViewModel, LoginViewModel, NotificationActionViewModel, NotificationLayout,
    NotificationLevel, NotificationTone, NotificationViewModel, ShellChromeViewModel, ShellEntry,
    ShellLayout, StatusViewModel, TundraTheme, UserManagementViewModel, compute_shell_layout,
    home_entry_icon_area, home_logout_area, login_password_area, login_password_visibility_area,
    login_user_list_area, login_user_list_visible_rows, notification_layout,
    notification_too_small_message, render_bootstrap_admin, render_clock, render_exit_confirmation,
    render_home, render_home_with_icons, render_login, render_notification_overlay,
    render_user_management, status_time_button_area,
};

#[derive(Default)]
struct RecordingHomeIconRenderer {
    calls: RefCell<Vec<(String, Rect)>>,
}

impl HomeIconRenderer for RecordingHomeIconRenderer {
    fn render_icon(&self, entry_label: &str, _frame: &mut ratatui::Frame<'_>, area: Rect) -> bool {
        self.calls
            .borrow_mut()
            .push((entry_label.to_string(), area));
        true
    }
}

struct UnavailableHomeIconRenderer;

impl HomeIconRenderer for UnavailableHomeIconRenderer {
    fn render_icon(
        &self,
        _entry_label: &str,
        _frame: &mut ratatui::Frame<'_>,
        _area: Rect,
    ) -> bool {
        false
    }
}

#[test]
fn storage_free_debug_home_does_not_expose_logout_hit_area() {
    let home = HomeViewModel::debug(DebugDiagnosticsViewModel {
        tick_count: 0,
        last_key_event: None,
        last_mouse_event: None,
        last_resize_event: None,
        mouse_coordinates: None,
        scroll_direction: None,
        drag_direction: None,
        terminal_flags: Vec::new(),
        platform_capability_summary: "supported".to_string(),
    });

    assert_eq!(home_logout_area(main_rect(80, 24), &home).width, 0);
}

#[test]
fn contextual_home_uses_stable_image_keys_and_preserves_ascii_fallback() {
    let context = ui::RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    for label in ["Explorer", "文件管理器"] {
        let home = HomeViewModel::user(
            "Strix",
            "2026-07-01 09:30",
            vec![ShellEntry::new(label, "Browse files").with_icon_key("explorer")],
        );
        let chrome = chrome_for("Home");
        let tile = ui::home_entry_tile_areas(main_rect(100, 30), 1)[0];
        let icon_area = home_entry_icon_area(tile);
        let icons = RecordingHomeIconRenderer::default();
        let unavailable = UnavailableHomeIconRenderer;
        for (renderer, image_available) in [
            (Some(&icons as &dyn HomeIconRenderer), true),
            (Some(&unavailable as &dyn HomeIconRenderer), false),
            (None, false),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");
            terminal
                .draw(|frame| {
                    ui::render_home_with_context(
                        frame,
                        frame.area(),
                        &chrome,
                        &home,
                        &context,
                        renderer,
                    );
                })
                .expect("render contextual Home");
            let output = terminal_output(&terminal);
            // Wide characters leave a trailing blank cell in TestBackend's buffer.
            assert!(output.replace(' ', "").contains(label));
            assert!(output.contains("Browse files"));
            if image_available {
                assert_eq!(
                    icons.calls.borrow().as_slice(),
                    &[("explorer".to_string(), icon_area)]
                );
                for y in icon_area.y..icon_area.bottom() {
                    assert!(
                        buffer_row_text(&terminal, icon_area.x, y, icon_area.width)
                            .trim()
                            .is_empty()
                    );
                }
            } else {
                assert_centered_icon_matches_asset(
                    &terminal,
                    tile,
                    home.home_icon_for_label("explorer")
                        .expect("ASCII fallback"),
                );
            }
        }
    }
}

#[test]
fn user_home_falls_back_to_ascii_when_graphical_icon_loading_is_unavailable() {
    let entries = vec![ShellEntry::new("Explorer", "Browse files")];
    let home = HomeViewModel::user("Strix", "2026-07-01 09:30", entries);
    let chrome = chrome_for("Home");
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_home_with_icons(
                frame,
                frame.area(),
                &chrome,
                &home,
                &TundraTheme::default_dark(),
                Some(&UnavailableHomeIconRenderer),
            );
        })
        .expect("render Home with unavailable graphical icon");

    let main = main_rect(100, 30);
    let tile = ui::home_entry_tile_areas(main, home.entries().len())[0];
    let icon = home
        .home_icon_for_label("Explorer")
        .expect("Explorer ASCII fallback icon");
    assert_centered_icon_matches_asset(&terminal, tile, icon);
}

#[test]
fn home_entry_index_at_maps_coordinates_to_entry_tiles() {
    let main = main_rect(100, 30);
    let tile_areas = ui::home_entry_tile_areas(main, 5);
    let first_tile = tile_areas[0];
    let second_tile = tile_areas[1];

    assert_eq!(
        ui::home_entry_index_at(
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
        ui::home_entry_index_at(
            main,
            5,
            (
                second_tile.x.saturating_add(1),
                second_tile.y.saturating_add(1),
            ),
        ),
        Some(1)
    );
    assert_eq!(ui::home_entry_index_at(main, 5, (main.x, main.y)), None);
}

#[test]
fn debug_status_returns_after_a_notification_clears() {
    let home = HomeViewModel::debug(DebugDiagnosticsViewModel {
        tick_count: 0,
        last_key_event: Some("x".to_string()),
        last_mouse_event: None,
        last_resize_event: None,
        mouse_coordinates: Some((12, 7)),
        scroll_direction: Some("Down".to_string()),
        drag_direction: None,
        terminal_flags: Vec::new(),
        platform_capability_summary: "Windows: ready".to_string(),
    });
    let mut chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Debug,
        terminal_size: (120, 30),
        back_button_hovered: false,
        screen_stack: vec!["Home".to_string()],
        status: StatusViewModel {
            status: "Last Key: x | Mouse position: 12,7 | Size: 120x30 | Scroll: Down | Drag: none"
                .to_string(),
            toast: Some("Saved".to_string()),
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    };
    let theme = TundraTheme::default_dark();
    let mut notification_terminal =
        Terminal::new(TestBackend::new(120, 30)).expect("notification terminal");

    notification_terminal
        .draw(|frame| render_home(frame, frame.area(), &chrome, &home, &theme))
        .expect("render notification");
    let notification_output = terminal_output(&notification_terminal);
    assert!(notification_output.contains("Saved"));
    assert!(!notification_output.contains("Last Key: x"));

    chrome.status.toast = None;
    let mut restored_terminal =
        Terminal::new(TestBackend::new(120, 30)).expect("restored terminal");
    restored_terminal
        .draw(|frame| render_home(frame, frame.area(), &chrome, &home, &theme))
        .expect("render restored status");
    let restored_output = terminal_output(&restored_terminal);
    assert!(restored_output.contains("Last Key: x"));
    assert!(restored_output.contains("Mouse position: 12,7"));
    assert!(!restored_output.contains("Saved"));
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
fn narrow_full_status_prioritizes_alert_and_uses_ascii_ellipsis_without_wrapping() {
    let home = HomeViewModel::user("Strix", "2026-07-10 09:30", Vec::new());
    let mut chrome = chrome_for("Home");
    chrome.terminal_size = (50, 12);
    chrome.status = StatusViewModel {
        status: "Low priority status".to_string(),
        toast: Some("Lower priority toast".to_string()),
        error: Some(
            "This alert is intentionally long and must not wrap into hidden rows".to_string(),
        ),
        alert_tone: NotificationTone::Error,
        time_button_label: None,
        time_button_selected: false,
    };
    let mut terminal = Terminal::new(TestBackend::new(50, 12)).expect("test terminal");

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
        .expect("render narrow full status");

    let status = status_rect(50, 12);
    let rendered = buffer_row_text(
        &terminal,
        status.x.saturating_add(1),
        status.y.saturating_add(1),
        status.width.saturating_sub(2),
    );
    assert!(rendered.starts_with("[ERROR] This alert"));
    assert!(rendered.ends_with("..."));
    assert!(!rendered.contains("Low priority status"));
    assert!(!rendered.contains("Lower priority toast"));
    assert!(!terminal_output(&terminal).contains("hidden rows"));
}

#[test]
fn compact_home_clock_login_bootstrap_and_user_management_show_highest_priority_notification() {
    let theme = TundraTheme::default_dark();
    let mut outputs = Vec::new();

    let home = HomeViewModel::user("Strix", "now", Vec::new());
    let chrome = compact_alert_chrome("Home");
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).expect("test terminal");
    terminal
        .draw(|frame| render_home(frame, frame.area(), &chrome, &home, &theme))
        .expect("render compact home");
    outputs.push(terminal_output(&terminal));

    let clock = ClockViewModel::new("now");
    let chrome = compact_alert_chrome("Clock");
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).expect("test terminal");
    terminal
        .draw(|frame| render_clock(frame, frame.area(), &chrome, &clock, &theme))
        .expect("render compact clock");
    outputs.push(terminal_output(&terminal));

    let login = LoginViewModel::new(Vec::new(), 0, 0, 0, LoginField::Password, None);
    let chrome = compact_alert_chrome("Login");
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).expect("test terminal");
    terminal
        .draw(|frame| render_login(frame, frame.area(), &chrome, &login, &theme))
        .expect("render compact login");
    outputs.push(terminal_output(&terminal));

    let bootstrap = BootstrapAdminViewModel::new("", 0, AuthField::Username, None);
    let chrome = compact_alert_chrome("BootstrapAdmin");
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_bootstrap_admin(frame, frame.area(), &chrome, &bootstrap, &theme);
        })
        .expect("render compact bootstrap admin");
    outputs.push(terminal_output(&terminal));

    let management = UserManagementViewModel::new("AdminUser", Vec::new(), 0, None, true, None);
    let chrome = compact_alert_chrome("UserManagement");
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_user_management(frame, frame.area(), &chrome, &management, &theme);
        })
        .expect("render compact user management");
    outputs.push(terminal_output(&terminal));

    for output in outputs {
        assert!(output.contains("[ERROR] Compact alert"));
        assert!(!output.contains("Compact toast"));
        assert!(!output.contains("Compact status"));
    }
}

#[test]
fn extremely_small_compact_layout_uses_borderless_notification_fallback() {
    let home = HomeViewModel::user("User", "Now", Vec::new());
    let chrome = compact_alert_chrome("Home");
    let mut terminal = Terminal::new(TestBackend::new(2, 2)).expect("test terminal");

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
        .expect("render tiny compact notification");

    assert!(terminal_output(&terminal).contains("[E"));
}

#[test]
fn exit_menu_actions_remain_separate_and_visible_after_resize() {
    let labels = [
        "Exit TundraUX",
        "Restart TundraUX",
        "Restart computer",
        "Shut down computer",
        "Cancel",
    ];
    let mut model = NotificationViewModel::new(
        "exit",
        NotificationLevel::Modal,
        NotificationTone::Warning,
        "Exit & power",
        "Choose an action. Esc returns to TundraUX.",
        labels
            .iter()
            .zip(["Y", "R", "B", "P", "N"])
            .map(|(label, key)| NotificationActionViewModel::new(*label, *label).with_shortcut(key))
            .collect(),
    );
    model.stacked_actions = true;
    for (width, height) in [(120, 40), (80, 24), (50, 12)] {
        let area = Rect::new(0, 0, width, height);
        let NotificationLayout::Dialog(layout) = notification_layout(area, &model) else {
            panic!("exit actions must fit {width}x{height}");
        };
        assert_eq!(layout.actions.len(), labels.len());
        for pair in layout.actions.windows(2) {
            assert_eq!(pair[0].area.x, pair[1].area.x);
            assert_eq!(pair[0].area.width, pair[1].area.width);
            assert!(pair[0].area.bottom() <= pair[1].area.y);
            if height >= 24 {
                assert!(pair[0].area.bottom() < pair[1].area.y);
            }
        }
        assert!(
            layout
                .actions
                .iter()
                .all(|action| action.area.bottom() < layout.dialog.bottom())
        );
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render_notification_overlay(frame, area, &model, &TundraTheme::default_dark())
            })
            .unwrap();
        let output = terminal_output(&terminal);
        for label in labels {
            assert!(output.contains(label), "{width}x{height}: {label}");
        }
    }
}

#[test]
fn exit_confirmation_keeps_all_componentized_actions_visible() {
    let model = ExitConfirmViewModel::new();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_exit_confirmation(frame, frame.area(), &model, &TundraTheme::default_dark());
        })
        .expect("render exit confirmation");

    let output = terminal_output(&terminal);
    for text in [
        &model.title,
        &model.message,
        &model.confirm_label,
        &model.restart_label,
        &model.cancel_label,
    ] {
        assert!(output.contains(text));
    }
}

#[test]
fn notification_overlay_renders_modal_actions_and_replaces_too_small_terminal_content() {
    let model = NotificationViewModel::new(
        "42",
        NotificationLevel::Modal,
        NotificationTone::Warning,
        "Delete File",
        "Move README.md to TundraUX trash?",
        vec![
            NotificationActionViewModel::new("confirm", "Move")
                .with_shortcut("Y")
                .selected(true),
            NotificationActionViewModel::new("cancel", "Cancel").with_shortcut("N"),
        ],
    );
    let theme = TundraTheme::default_dark();

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal
        .draw(|frame| render_notification_overlay(frame, frame.area(), &model, &theme))
        .expect("render notification");

    let output = terminal_output(&terminal);
    assert!(output.contains("[WARN] Delete File"));
    assert!(output.contains("Delete File"));
    assert!(output.contains("Move README.md to TundraUX trash?"));
    assert!(output.contains("Y: Move"));
    assert!(output.contains("N: Cancel"));

    let mut narrow = Terminal::new(TestBackend::new(18, 10)).expect("test terminal");
    narrow
        .draw(|frame| {
            frame.render_widget(
                ratatui::widgets::Paragraph::new("BACKGROUND CONTENT"),
                frame.area(),
            );
            render_notification_overlay(frame, frame.area(), &model, &theme);
        })
        .expect("render narrow notification");

    let narrow_output = terminal_output(&narrow);
    assert!(
        visible_text_without_spaces(&narrow_output).contains(&visible_text_without_spaces(
            &notification_too_small_message()
        ))
    );
    assert!(!narrow_output.contains("BACKGROUND CONTENT"));
    assert!(!narrow_output.contains("Delete File"));
    assert!(!narrow_output.contains("README.md"));
    assert!(!narrow_output.contains("Y: Move"));
    assert!(!narrow_output.contains("N: Cancel"));
}

#[test]
fn notification_layout_uses_nominal_size_and_adapts_to_full_shell_widths() {
    let model = NotificationViewModel::new(
        "42",
        NotificationLevel::Modal,
        NotificationTone::Warning,
        "Delete File",
        "Move README.md to TundraUX trash?",
        vec![
            NotificationActionViewModel::new("confirm", "Move")
                .with_shortcut("Y")
                .selected(true),
            NotificationActionViewModel::new("cancel", "Cancel").with_shortcut("N"),
        ],
    );

    let NotificationLayout::Dialog(layout) = notification_layout(Rect::new(5, 7, 64, 9), &model)
    else {
        panic!("the exact nominal notification size must render");
    };
    assert_eq!(layout.dialog, Rect::new(5, 7, 64, 9));
    assert_eq!(layout.message, Rect::new(6, 8, 62, 1));
    assert_eq!(layout.actions.len(), 2);
    assert_eq!(layout.actions[0].index, 0);
    assert_eq!(layout.actions[0].area, Rect::new(25, 10, 9, 1));
    assert_eq!(layout.actions[1].index, 1);
    assert_eq!(layout.actions[1].area, Rect::new(38, 10, 11, 1));

    let NotificationLayout::Dialog(narrow) = notification_layout(Rect::new(0, 0, 50, 12), &model)
    else {
        panic!("the minimum full-shell width must keep modal actions operable");
    };
    assert_eq!(narrow.dialog.width, 50);
    assert_eq!(narrow.actions.len(), 2);
    assert!(narrow.actions.iter().all(|action| action.area.width > 0));
    for area in [Rect::new(0, 0, 39, 9), Rect::new(0, 0, 64, 8)] {
        assert!(
            matches!(
                notification_layout(area, &model),
                NotificationLayout::Dialog(_)
            ),
            "nominal padding must not prevent a usable notification"
        );
    }
    assert!(matches!(
        notification_layout(Rect::new(0, 0, 64, 4), &model),
        NotificationLayout::TooSmall { .. }
    ));
}

#[test]
fn notification_wrapping_uses_terminal_columns_for_wide_text() {
    let model = NotificationViewModel::new(
        "wide",
        NotificationLevel::Modal,
        NotificationTone::Info,
        "Wide text",
        "界".repeat(20),
        Vec::new(),
    );

    let NotificationLayout::Dialog(layout) = notification_layout(Rect::new(0, 0, 40, 12), &model)
    else {
        panic!("wide notification should fit");
    };

    assert_eq!(layout.message.width, 38);
    assert_eq!(layout.message.height, 2);
}

#[test]
fn notification_layout_and_renderer_share_wrapped_message_and_stacked_action_rects() {
    let model = NotificationViewModel::new(
        "long",
        NotificationLevel::Modal,
        NotificationTone::Info,
        "Long Notification",
        "M".repeat(190),
        vec![
            NotificationActionViewModel::new("first", "A".repeat(70)).selected(true),
            NotificationActionViewModel::new("second", "B".repeat(40)),
        ],
    );
    let area = Rect::new(3, 4, 80, 20);
    let NotificationLayout::Dialog(layout) = notification_layout(area, &model) else {
        panic!("long notification should fit the supplied terminal");
    };

    assert_eq!(layout.dialog, Rect::new(11, 9, 64, 10));
    assert_eq!(layout.message, Rect::new(12, 10, 62, 4));
    assert_eq!(layout.actions.len(), 2);
    assert_eq!(layout.actions[0].area, Rect::new(12, 15, 62, 2));
    assert_eq!(layout.actions[1].area, Rect::new(22, 17, 42, 1));
    let NotificationLayout::Dialog(scrolled) = notification_layout(Rect::new(0, 0, 64, 9), &model)
    else {
        panic!("long messages should scroll while their actions remain visible");
    };
    assert_eq!(scrolled.message.height, 3);
    assert_eq!(scrolled.max_scroll_offset, 1);
    assert!(
        scrolled
            .actions
            .iter()
            .all(|action| action.area.bottom() < scrolled.dialog.bottom())
    );

    let mut terminal = Terminal::new(TestBackend::new(86, 28)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_notification_overlay(frame, area, &model, &TundraTheme::default_dark())
        })
        .expect("render long notification");
    assert!(region_has_bg(
        &terminal,
        layout.actions[0].area,
        TundraTheme::default_dark().background,
    ));
    assert!(region_has_fg(
        &terminal,
        layout.actions[0].area,
        TundraTheme::default_dark().accent_color,
    ));
    assert!(region_has_fg(
        &terminal,
        layout.actions[1].area,
        TundraTheme::default_dark().foreground,
    ));
}

#[test]
fn login_renderer_masks_password_length() {
    let chrome = chrome_for("Login");
    let model = LoginViewModel::new(
        vec![
            login_user("AdminUser", "Admin User", "Admin"),
            login_user("Strix", "Local User", "User"),
            login_user("Operator", "Backup User", "User"),
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
    assert!(output.contains("[Show]"));
    assert!(!output.to_ascii_lowercase().contains("guest"));
    assert!(!output.contains("F3"));
    assert!(output.contains("Invalid username or password"));

    let main = main_rect(80, 24);
    let list_area = login_user_list_area(main);
    let password_area = login_password_area(main);
    let visibility_area = login_password_visibility_area(main);
    assert!(list_area.x < password_area.x);
    assert!(password_area.right() <= visibility_area.x);
    assert!(visibility_area.right() <= main.right());
    assert_eq!(
        login_user_list_visible_rows(main),
        usize::from(list_area.height.saturating_sub(2))
    );
    assert!(
        region_has_fg(
            &terminal,
            password_area,
            TundraTheme::default_dark().accent_color
        ),
        "focused password field should use the accent style"
    );
}

#[test]
fn login_renderer_reveals_only_explicit_plaintext_and_focuses_visibility_control() {
    let chrome = chrome_for("Login");
    let visible = "密碼🙂";
    let model = LoginViewModel::new(
        vec![login_user("Strix", "Local User", "User")],
        0,
        0,
        visible.chars().count(),
        LoginField::PasswordVisibility,
        None,
    )
    .with_visible_password(visible);
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
        .expect("render revealed login password");

    let output = terminal_output(&terminal);
    let visibility = login_password_visibility_area(main_rect(80, 24));
    assert_eq!(model.visible_password(), Some(visible));
    assert!(model.password_is_visible());
    assert!(visible.chars().all(|character| output.contains(character)));
    assert!(output.contains("[Hide]"));
    assert!(!output.contains("***"));
    assert!(region_has_fg(
        &terminal,
        visibility,
        TundraTheme::default_dark().accent_color,
    ));
}

fn chrome_for(screen: &str) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (80, 24),
        back_button_hovered: false,
        screen_stack: vec![screen.to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn compact_alert_chrome(screen: &str) -> ShellChromeViewModel {
    let mut chrome = chrome_for(screen);
    chrome.terminal_size = (49, 11);
    chrome.status = StatusViewModel {
        status: "Compact status".to_string(),
        toast: Some("Compact toast".to_string()),
        error: Some("Compact alert".to_string()),
        alert_tone: NotificationTone::Error,
        time_button_label: None,
        time_button_selected: false,
    };
    chrome
}

fn visible_text_without_spaces(output: &str) -> String {
    output
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn buffer_row_text(terminal: &Terminal<TestBackend>, x: u16, y: u16, width: u16) -> String {
    let buffer = terminal.backend().buffer();
    (x..x.saturating_add(width))
        .filter_map(|column| buffer.cell((column, y)))
        .map(|cell| cell.symbol())
        .collect()
}

fn assert_centered_icon_matches_asset(
    terminal: &Terminal<TestBackend>,
    tile: Rect,
    icon: &ui::HomeIcon,
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

fn region_has_bg(terminal: &Terminal<TestBackend>, area: Rect, bg: Color) -> bool {
    let buffer = terminal.backend().buffer();
    (area.y..area.y.saturating_add(area.height)).any(|y| {
        (area.x..area.x.saturating_add(area.width)).any(|x| {
            buffer
                .cell((x, y))
                .is_some_and(|cell| cell.bg == bg && cell.symbol() != " ")
        })
    })
}

#[test]
fn linux_login_labels_system_identity_and_keeps_errors_visible() {
    let chrome = chrome_for("Login");
    let mut model = LoginViewModel::new(
        vec![login_user("alice", "Alice", "Admin")],
        0,
        0,
        6,
        LoginField::Password,
        Some("Invalid username or password".into()),
    );
    model.system_users = true;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            render_login(
                frame,
                frame.area(),
                &chrome,
                &model,
                &TundraTheme::default_dark(),
            )
        })
        .unwrap();
    let output = terminal_output(&terminal);
    assert!(output.contains("Linux Login"));
    assert!(output.contains("Linux password"));
    assert!(output.contains("Invalid username or password"));
    assert!(output.contains("******"));
}
