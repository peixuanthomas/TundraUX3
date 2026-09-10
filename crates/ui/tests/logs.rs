mod support;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use support::terminal_output;
use ui::{
    DiagnosticsIncidentViewModel, DiagnosticsLogViewModel, DiagnosticsStatus, DiagnosticsTab,
    HomeDisplayMode, LogsCategory, LogsEventViewModel, LogsHitTarget, LogsSection, LogsViewModel,
    NotificationTone, RenderContext, ShellChromeViewModel, ShellLayout, StatusViewModel,
    TundraTheme, compute_shell_layout, diagnostics_content_layout, logs_hit_test, logs_layout,
    render_logs_with_context,
};

fn model() -> LogsViewModel {
    let mut model = LogsViewModel::default();
    model.diagnostics.can_view_details = true;
    model.can_view_system = true;
    model.linux_available = true;
    model.events = (0..30)
        .map(|index| LogsEventViewModel {
            id: format!("event-{index}"),
            timestamp: "2026-09-11T00:00:00Z".into(),
            level: "Error".into(),
            module: "ux.explorer".into(),
            operation: "Copy".into(),
            summary: "Permission denied".into(),
            detail: "Run: run-1\nTask: task-1\nCode: EACCES".into(),
            incident_id: Some("incident-1".into()),
        })
        .collect();
    model
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
        "Events",
        "Files",
        "Incidents",
        "UX events",
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
fn category_section_and_control_geometry_match_rendered_components() {
    let model = model();
    let main = main_area();
    let layout = logs_layout(main, &model);
    for tab in &layout.category_tabs {
        assert_eq!(
            logs_hit_test(main, &model, (tab.area.x, tab.area.y)),
            Some(LogsHitTarget::Category(tab.category))
        );
    }
    for tab in &layout.section_tabs {
        assert_eq!(
            logs_hit_test(main, &model, (tab.area.x, tab.area.y)),
            Some(LogsHitTarget::Section(tab.section))
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
fn files_and_incidents_reuse_diagnostics_content_without_health_navigation() {
    let mut model = model();
    model.section = LogsSection::Files;
    model.diagnostics.logs = vec![DiagnosticsLogViewModel {
        relative_path: "my-run.jsonl".into(),
        path: "/logs/my-run.jsonl".into(),
        modified_at: "today".into(),
        size_bytes: 12,
    }];
    let main = main_area();
    let layout = logs_layout(main, &model);
    let mut diagnostics = model.diagnostics.clone();
    diagnostics.tab = DiagnosticsTab::Logs;
    let area = Rect::new(
        layout.content.list_panel.x,
        layout.content.list_panel.y,
        layout.content.detail_panel.right() - layout.content.list_panel.x,
        layout.content.list_panel.height,
    );
    assert_eq!(
        layout.content,
        diagnostics_content_layout(area, &diagnostics)
    );
    let row = layout.content.rows[0].area;
    assert_eq!(
        logs_hit_test(main, &model, (row.x, row.y)),
        Some(LogsHitTarget::File(0))
    );
    let output = terminal_output(&render(120, 32, &model, &TundraTheme::default_dark()));
    assert!(output.contains("my-run.jsonl"));
    assert!(!output.contains("Health"));
    model.section = LogsSection::Incidents;
    model.diagnostics.incidents = vec![DiagnosticsIncidentViewModel {
        id: "incident-1".into(),
        occurred_at: "today".into(),
        app: "Explorer".into(),
        severity: DiagnosticsStatus::Fail,
        recovery: "Recovered".into(),
        summary: "Failed operation".into(),
        detail: "Details".into(),
        report_path: "/logs/incident-1.json".into(),
        restricted: false,
    }];
    let row = logs_layout(main, &model).content.rows[0].area;
    assert_eq!(
        logs_hit_test(main, &model, (row.x, row.y)),
        Some(LogsHitTarget::Incident(0))
    );
}
#[test]
fn restricted_and_unsupported_sources_hide_rows_and_disable_actions() {
    let mut model = model();
    model.category = LogsCategory::Linux;
    model.linux_available = false;
    let layout = logs_layout(main_area(), &model);
    assert!(layout.section_tabs.is_empty());
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
fn empty_events_use_log_language_and_disable_open() {
    let mut model = model();
    model.events.clear();
    let output = terminal_output(&render(120, 32, &model, &TundraTheme::default_dark()));
    assert!(output.contains("No events match the current query"));
    assert!(!output.contains("No check"));
    let open = logs_layout(main_area(), &model)
        .controls
        .into_iter()
        .find(|control| control.target == LogsHitTarget::Open)
        .unwrap();
    assert_eq!(
        logs_hit_test(main_area(), &model, (open.area.x, open.area.y)),
        None
    );
}
