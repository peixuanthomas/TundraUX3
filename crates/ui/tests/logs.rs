#[path = "support/composition.rs"]
mod composition;
use composition as ui;
mod support;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use support::terminal_output;
use ui::{
    HomeDisplayMode, LogsCategory, LogsEventViewModel, LogsHitTarget, LogsViewModel,
    NotificationTone, RenderContext, ShellChromeViewModel, ShellLayout, StatusViewModel,
    TundraTheme, compute_shell_layout, logs_hit_test, logs_layout, render_logs_with_context,
};

fn model() -> LogsViewModel {
    let mut model = LogsViewModel::default();
    model.diagnostics.can_view_details = true;
    model.can_view_system = true;
    model.linux_available = true;
    model.level_filter = "All".into();
    model.events = (0..30)
        .map(|index| LogsEventViewModel {
            id: format!("event-{index}"),
            timestamp: "2026-09-11T00:00:00Z".into(),
            time_label: "00:00:00".into(),
            level: "Error".into(),
            status: ui::DiagnosticsStatus::Fail,
            module: "ux.explorer".into(),
            operation: "Copy".into(),
            summary: "Permission denied".into(),
            detail: "Run: run-1\nTask: task-1\nCode: EACCES".into(),
            incident_id: Some("incident-1".into()),
        })
        .collect();
    model
}

#[test]
fn more_menu_reuses_button_geometry_and_covers_background_controls() {
    let mut model = model();
    model.more_selected = Some(0);
    let main = main_area();
    let layout = logs_layout(main, &model);
    for control in &layout.menu_controls {
        assert!(control.area.right() <= main.right() && control.area.bottom() <= main.bottom());
        assert_eq!(
            logs_hit_test(main, &model, (control.area.x, control.area.y)),
            ui::logs_control_enabled(&model, control.target).then_some(control.target)
        );
    }
    let toolbar = layout.controls[0].area;
    if !layout.menu.contains((toolbar.x, toolbar.y).into()) {
        assert_eq!(logs_hit_test(main, &model, (toolbar.x, toolbar.y)), None);
    }
    let theme = TundraTheme::default_dark();
    let output = render(120, 32, &model, &theme);
    assert!(terminal_output(&output).contains("Service, boot or file"));
}

#[test]
fn compact_more_dialog_scrolls_options_and_keeps_close_visible() {
    let mut model = model();
    let main = Rect::new(0, 1, 49, 10);
    for selected in [0, 3, 5, 6] {
        model.more_selected = Some(selected);
        let layout = logs_layout(main, &model);
        let selected_target = ui::logs_more_controls()[selected].0;
        assert!(
            layout
                .menu_controls
                .iter()
                .any(|control| control.target == selected_target)
        );
        let scrollbar = layout.menu_scrollbar.expect("short dialog needs scrolling");
        assert_eq!(
            logs_hit_test(main, &model, (scrollbar.track.x, scrollbar.track.y)),
            Some(LogsHitTarget::MoreScrollbar)
        );
        let close = layout
            .menu_controls
            .iter()
            .find(|control| control.target == LogsHitTarget::CloseMore)
            .unwrap();
        assert_eq!(
            logs_hit_test(main, &model, (close.area.x, close.area.y)),
            Some(LogsHitTarget::CloseMore)
        );
    }
}

#[test]
fn more_dialog_leaves_global_chrome_untouched_at_all_sizes() {
    let mut model = model();
    model.more_selected = Some(7);
    for (width, height) in [(0, 0), (1, 1), (40, 10), (49, 11), (120, 32)] {
        let area = Rect::new(0, 0, width, height);
        let context = RenderContext::from_theme(
            &TundraTheme::default_dark(),
            Default::default(),
            Default::default(),
        );
        let main = ui::ShellFrameLayout::new(area, None, &context).main;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                for y in 0..height {
                    for x in 0..width {
                        frame.buffer_mut()[(x, y)].set_symbol("x");
                    }
                }
                ui::render_logs_content(frame, main, &model, &context);
                ui::render_logs_overlay(frame, main, &model, &context);
            })
            .unwrap();
        for y in 0..height {
            for x in 0..width {
                if !main.contains((x, y).into()) {
                    assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), "x");
                }
            }
        }
    }
}
fn main_area() -> Rect {
    match compute_shell_layout(Rect::new(0, 0, 120, 32)) {
        ShellLayout::Full { main, .. } => main,
        _ => panic!("full shell expected"),
    }
}
fn render(
    width: u16,
    height: u16,
    model: &LogsViewModel,
    theme: &TundraTheme,
) -> Terminal<TestBackend> {
    let chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".into(),
        build_mode: "debug".into(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (width, height),
        back_button_hovered: false,
        help_available: false,
        help_button_hovered: false,
        help_button_focused: false,
        back_shortcut: "Esc",
        screen_stack: vec!["Logs".into()],
        status: StatusViewModel {
            status: "Ready".into(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    };
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| render_logs_with_context(frame, frame.area(), &chrome, model, &context))
        .unwrap();
    terminal
}
#[test]
fn event_list_uses_shared_scrolling_geometry_and_exposes_correlation() {
    let mut model = model();
    model.selected_event = 29;
    let main = main_area();
    let layout = logs_layout(main, &model);
    assert_eq!(layout.content.rows.last().unwrap().index, 29);
    assert!(layout.visible_start > 0);
    let row = layout.content.rows.last().unwrap();
    assert_eq!(
        logs_hit_test(main, &model, (row.area.x, row.area.y)),
        Some(LogsHitTarget::Event(29))
    );
    let bar = layout.content.list_scrollbar.unwrap();
    assert_eq!(
        logs_hit_test(main, &model, (bar.track.x, bar.track.y)),
        Some(LogsHitTarget::Scrollbar)
    );
    let output = terminal_output(&render(120, 32, &model, &TundraTheme::default_dark()));
    for expected in [
        "UX log",
        "Linux log",
        "Open folder",
        "L Level: All",
        "EACCES",
        "event-29",
        "incident-1",
        "run-1",
        "task-1",
    ] {
        assert!(output.contains(expected), "missing {expected}:\n{output}");
    }
}
#[test]
fn category_and_control_geometry_match_rendered_components() {
    let model = model();
    let main = main_area();
    let layout = logs_layout(main, &model);
    for tab in &layout.category_tabs {
        assert_eq!(
            logs_hit_test(main, &model, (tab.area.x, tab.area.y)),
            Some(LogsHitTarget::Category(tab.category))
        );
    }
    for control in &layout.controls {
        assert_eq!(
            logs_hit_test(main, &model, (control.area.x, control.area.y)),
            Some(control.target)
        );
    }
}
#[test]
fn restricted_and_unsupported_sources_hide_rows_and_disable_actions() {
    let mut model = model();
    model.category = LogsCategory::Linux;
    model.linux_available = false;
    let layout = logs_layout(main_area(), &model);
    assert!(layout.content.rows.is_empty());
    assert!(
        terminal_output(&render(120, 32, &model, &TundraTheme::default_dark()))
            .contains("Windows and macOS")
    );
    model.linux_available = true;
    model.can_view_system = false;
    let output = terminal_output(&render(120, 32, &model, &TundraTheme::default_dark()));
    assert!(output.contains("administrator access"));
    assert!(!output.contains("EACCES"));
    model.category = LogsCategory::Ux;
    model.diagnostics.can_view_details = false;
    let output = terminal_output(&render(120, 32, &model, &TundraTheme::default_dark()));
    assert!(output.contains("Guest access is disabled"));
    for control in logs_layout(main_area(), &model).controls {
        assert_eq!(
            logs_hit_test(main_area(), &model, (control.area.x, control.area.y)),
            None
        );
    }
}
#[test]
fn explicit_pointer_scroll_does_not_snap_back_to_selection() {
    let mut model = model();
    model.selected_event = 0;
    model.scroll_offset = 8;
    model.diagnostics.list_window_is_explicit = true;
    assert_eq!(logs_layout(main_area(), &model).visible_start, 8);
}
#[test]
fn themes_and_narrow_or_empty_terminal_sizes_render_without_panicking() {
    let mut model = model();
    for theme in [
        TundraTheme::default_dark(),
        TundraTheme {
            background: ratatui::style::Color::White,
            foreground: ratatui::style::Color::Black,
            accent_color: ratatui::style::Color::Blue,
            ..TundraTheme::default_dark()
        },
    ] {
        for (width, height) in [(0, 0), (1, 1), (40, 10), (108, 20), (120, 32)] {
            render(width, height, &model, &theme);
        }
        model.loading = true;
        assert!(terminal_output(&render(120, 32, &model, &theme)).contains("Loading logs"));
    }
}

#[test]
fn long_log_details_expose_a_draggable_scrollbar_and_the_last_line() {
    let mut model = model();
    model.events[0].detail = (0..80).map(|index| format!("detail-{index}\n")).collect();
    let layout = logs_layout(main_area(), &model);
    let bar = layout.detail_scrollbar.unwrap();
    assert_eq!(
        logs_hit_test(main_area(), &model, (bar.track.x, bar.track.y)),
        Some(LogsHitTarget::DetailScrollbar)
    );
    let end = bar.offset_at((bar.track.x, bar.track.bottom()), 0);
    assert_eq!(end, bar.content_len - bar.viewport_len);
    model.detail_scroll = end;
    let output = terminal_output(&render(120, 32, &model, &TundraTheme::default_dark()));
    assert!(output.contains("detail-79"), "{output}");
}
