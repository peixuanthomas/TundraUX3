use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use tundra_ui::{
    DiagnosticsCheckViewModel, DiagnosticsHitTarget, DiagnosticsIncidentViewModel,
    DiagnosticsLogViewModel, DiagnosticsRepairDialogViewModel, DiagnosticsRepairItemViewModel,
    DiagnosticsStatus, DiagnosticsTab, DiagnosticsViewModel, HomeDisplayMode, NotificationTone,
    ShellChromeViewModel, ShellLayout, StatusViewModel, TundraTheme, compute_shell_layout,
    diagnostics_hit_test, diagnostics_layout, render_diagnostics,
};

#[test]
fn minimum_full_layout_keeps_selected_health_check_visible_and_exposes_hit_targets() {
    let mut model = health_model();
    model.checks = (0..12)
        .map(|index| check(index, DiagnosticsStatus::Pass))
        .collect();
    model.selected_check = 10;
    model.list_window_start = 0;
    let main = full_main(108, 20);

    let layout = diagnostics_layout(main, &model);

    assert!(layout.list_panel.width >= 28);
    assert!(layout.detail_panel.width > layout.list_panel.width);
    assert_eq!(layout.visible_capacity, 7);
    assert_eq!(layout.visible_start, 4);
    assert_eq!(layout.rows.last().map(|row| row.index), Some(10));
    let health_tab = layout
        .tabs
        .iter()
        .find(|tab| tab.tab == DiagnosticsTab::Health)
        .expect("health tab");
    assert_eq!(
        diagnostics_hit_test(&layout, (health_tab.area.x, health_tab.area.y)),
        Some(DiagnosticsHitTarget::Tab(DiagnosticsTab::Health))
    );
    let selected = layout.rows.last().expect("selected row");
    assert_eq!(
        diagnostics_hit_test(&layout, (selected.area.x, selected.area.y)),
        Some(DiagnosticsHitTarget::Check(10))
    );
}

#[test]
fn overflowing_checks_show_a_proportional_scrollbar_at_the_current_window() {
    let mut model = health_model();
    model.checks = (0..12)
        .map(|index| check(index, DiagnosticsStatus::Pass))
        .collect();
    model.selected_check = 10;
    let main = full_main(108, 20);

    let layout = diagnostics_layout(main, &model);
    let scrollbar = layout
        .list_scrollbar
        .expect("an overflowing checks list should expose a scrollbar");

    assert_eq!(layout.visible_capacity, 7);
    assert_eq!(layout.visible_start, 4);
    assert_eq!(scrollbar.track.height, 7);
    assert_eq!(scrollbar.thumb.height, 4);
    assert_eq!(scrollbar.thumb.y, scrollbar.track.y.saturating_add(2));
    assert_eq!(
        diagnostics_hit_test(&layout, (scrollbar.thumb.x, scrollbar.thumb.y)),
        Some(DiagnosticsHitTarget::Scrollbar)
    );
    assert_eq!(
        layout.list_rows_area.right().saturating_add(1),
        scrollbar.track.x
    );

    let terminal = render(108, 20, &model);
    let buffer = terminal.backend().buffer();
    for y in scrollbar.track.y..scrollbar.track.bottom() {
        let expected = if y >= scrollbar.thumb.y && y < scrollbar.thumb.bottom() {
            "#"
        } else {
            "|"
        };
        assert_eq!(
            buffer
                .cell((scrollbar.track.x, y))
                .expect("scrollbar cell")
                .symbol(),
            expected
        );
    }
}

#[test]
fn checks_that_fit_do_not_reserve_a_scrollbar_column() {
    let mut model = health_model();
    model.checks = (0..3)
        .map(|index| check(index, DiagnosticsStatus::Pass))
        .collect();

    let layout = diagnostics_layout(full_main(108, 20), &model);

    assert!(layout.list_scrollbar.is_none());
    assert_eq!(layout.list_rows_area, Rect::new(2, 7, 40, 7));
}

#[test]
fn health_renderer_draws_two_columns_statuses_and_admin_details() {
    let mut model = health_model();
    model.checks = vec![
        check(0, DiagnosticsStatus::Pass),
        check(1, DiagnosticsStatus::Warning),
        check(2, DiagnosticsStatus::Fail),
    ];
    model.selected_check = 1;
    model.can_view_details = true;
    model.can_repair = true;
    let terminal = render(140, 30, &model);
    let output = terminal_output(&terminal);
    let layout = diagnostics_layout(full_main(140, 30), &model);

    assert!(output.contains("Diagnostics"));
    assert!(output.contains("Health"));
    assert!(output.contains("Incidents"));
    assert!(output.contains("Checks"));
    assert!(output.contains("Details"));
    assert!(output.contains("System needs attention"));
    assert!(output.contains("Detail: private detail 1"));
    assert!(output.contains("Recommended: repair guidance 1"));
    assert!(output.contains("Repair available"));
    assert!(output.contains("F Repair"));
    assert!(output.contains("A Repair all"));
    assert!(output.contains("O Open logs"));
    assert!(output.contains("E Log folder"));
    assert!(region_has_fg(
        &terminal,
        layout.rows[0].area,
        TundraTheme::default_dark().accent_color
    ));
    assert!(region_has_fg(&terminal, layout.rows[1].area, Color::Yellow));
    assert!(region_has_fg(
        &terminal,
        layout.rows[2].area,
        TundraTheme::default_dark().error
    ));
}

#[test]
fn incident_renderer_redacts_sensitive_fields_for_non_admins_at_108x20() {
    let mut model = health_model();
    model.tab = DiagnosticsTab::Incidents;
    model.can_view_details = false;
    model.incidents = vec![incident(false)];
    let terminal = render(108, 20, &model);
    let output = terminal_output(&terminal);

    assert!(output.contains("2026-07-13 14:32"));
    assert!(output.contains("Explorer"));
    assert!(output.contains("Recovered automatically"));
    assert!(output.contains("restricted to administrators"));
    assert!(!output.contains("SECRET incident summary"));
    assert!(!output.contains("SECRET stack trace"));
    assert!(!output.contains("/private/reports/incident-7.json"));
    assert!(!output.contains("incident-7"));
}

#[test]
fn logs_tab_lists_metadata_scrolls_and_exposes_log_hit_targets() {
    let mut model = health_model();
    model.tab = DiagnosticsTab::Logs;
    model.can_view_details = true;
    model.can_repair = true;
    model.logs = (0..12).map(log).collect();
    model.selected_log = 10;
    let layout = diagnostics_layout(full_main(108, 20), &model);

    assert_eq!(layout.visible_start, 4);
    assert_eq!(layout.rows.last().map(|row| row.index), Some(10));
    let logs_tab = layout
        .tabs
        .iter()
        .find(|tab| tab.tab == DiagnosticsTab::Logs)
        .expect("logs tab");
    assert_eq!(
        diagnostics_hit_test(&layout, (logs_tab.area.x, logs_tab.area.y)),
        Some(DiagnosticsHitTarget::Tab(DiagnosticsTab::Logs))
    );
    let selected = layout.rows.last().expect("selected log row");
    assert_eq!(
        diagnostics_hit_test(&layout, (selected.area.x, selected.area.y)),
        Some(DiagnosticsHitTarget::Log(10))
    );

    let output = terminal_output(&render(140, 32, &model));
    assert!(output.contains("Logs"));
    assert!(output.contains("service-10.log.1"));
    assert!(output.contains("Modified: 2026-07-17 12:10"));
    assert!(output.contains("Size: 1034 bytes"));
    assert!(output.contains("Press O to open read-only or E to explore the log folder"));
    assert!(output.contains("O Open log"));
    assert!(output.contains("E Log folder"));
    assert!(!output.contains("F Repair"));
    assert!(!output.contains("A Repair all"));
}

#[test]
fn logs_tab_redacts_availability_for_non_admins() {
    let mut model = health_model();
    model.tab = DiagnosticsTab::Logs;
    model.logs = vec![log(0)];
    let output = terminal_output(&render(108, 20, &model));

    assert!(output.contains("Logs are restricted to administrators"));
    assert!(!output.contains("E Log folder"));
    assert!(!output.contains("service-0.log.1"));
    assert!(!output.contains("/private/logs"));
}

#[test]
fn admin_incident_renderer_shows_full_report_on_larger_terminals() {
    let mut model = health_model();
    model.tab = DiagnosticsTab::Incidents;
    model.can_view_details = true;
    model.incidents = vec![incident(false)];
    let terminal = render(140, 32, &model);
    let output = terminal_output(&terminal);

    assert!(output.contains("SECRET incident summary"));
    assert!(output.contains("SECRET stack trace"));
    assert!(output.contains("/private/reports/incident-7.json"));
}

#[test]
fn restricted_incident_stays_redacted_even_when_detail_permission_is_present() {
    let mut model = health_model();
    model.tab = DiagnosticsTab::Incidents;
    model.can_view_details = true;
    model.incidents = vec![incident(true)];
    let output = terminal_output(&render(140, 32, &model));

    assert!(output.contains("restricted to administrators"));
    assert!(!output.contains("SECRET incident summary"));
    assert!(!output.contains("/private/reports/incident-7.json"));
}

#[test]
fn repair_preview_renders_items_and_modal_hit_geometry() {
    let mut model = health_model();
    model.repair_dialog = Some(DiagnosticsRepairDialogViewModel {
        items: vec![
            DiagnosticsRepairItemViewModel {
                id: "paths.apps".to_string(),
                label: "Create missing applications directory".to_string(),
            },
            DiagnosticsRepairItemViewModel {
                id: "storage.config".to_string(),
                label: "Back up and rebuild config document".to_string(),
            },
        ],
        selected: 1,
        confirm_selected: true,
        scroll_offset: 0,
    });
    let main = full_main(120, 28);
    let layout = diagnostics_layout(main, &model);
    let dialog = layout.repair_dialog.as_ref().expect("repair dialog");

    assert_eq!(dialog.rows.len(), 2);
    assert_eq!(
        diagnostics_hit_test(&layout, (dialog.rows[1].area.x, dialog.rows[1].area.y)),
        Some(DiagnosticsHitTarget::RepairItem(1))
    );
    assert_eq!(
        diagnostics_hit_test(&layout, (dialog.confirm.x, dialog.confirm.y)),
        Some(DiagnosticsHitTarget::RepairConfirm)
    );
    assert_eq!(
        diagnostics_hit_test(&layout, (dialog.cancel.x, dialog.cancel.y)),
        Some(DiagnosticsHitTarget::RepairCancel)
    );
    let output = terminal_output(&render(120, 28, &model));
    assert!(output.contains("Repair preview"));
    assert!(output.contains("Create missing applications directory"));
    assert!(output.contains("Back up and rebuild config document"));
    assert!(output.contains("Confirm repair"));
    assert!(output.contains("Cancel"));
}

#[test]
fn compact_terminal_renders_shared_compact_home_instead_of_diagnostics_content() {
    let mut model = health_model();
    model.checks = vec![check(0, DiagnosticsStatus::Fail)];
    let output = terminal_output(&render(49, 30, &model));

    assert!(output.contains("TundraUX 3"));
    assert!(output.contains("needs at least 50x12"));
    assert!(!output.contains("private detail"));
}

fn health_model() -> DiagnosticsViewModel {
    DiagnosticsViewModel {
        tab: DiagnosticsTab::Health,
        checks: vec![check(0, DiagnosticsStatus::Warning)],
        incidents: Vec::new(),
        logs: Vec::new(),
        selected_check: 0,
        selected_incident: 0,
        selected_log: 0,
        list_window_start: 0,
        list_window_is_explicit: false,
        scanning: false,
        can_view_details: false,
        can_repair: false,
        restart_required: false,
        repair_dialog: None,
        feedback: None,
        scanned_at: Some("2026-07-13 14:33".to_string()),
    }
}

fn log(index: usize) -> DiagnosticsLogViewModel {
    DiagnosticsLogViewModel {
        relative_path: format!("nested/service-{index}.log.1"),
        path: format!("/private/logs/nested/service-{index}.log.1"),
        modified_at: format!("2026-07-17 12:{index:02}"),
        size_bytes: 1024 + index as u64,
    }
}

fn check(index: usize, status: DiagnosticsStatus) -> DiagnosticsCheckViewModel {
    DiagnosticsCheckViewModel {
        id: format!("check-{index}"),
        label: format!("Check {index}"),
        category: if index.is_multiple_of(2) {
            "Paths"
        } else {
            "Storage"
        }
        .to_string(),
        status,
        summary: format!("public summary {index}"),
        detail: format!("private detail {index}"),
        remediation: format!("repair guidance {index}"),
        repairable: index == 1,
    }
}

fn incident(restricted: bool) -> DiagnosticsIncidentViewModel {
    DiagnosticsIncidentViewModel {
        id: "incident-7".to_string(),
        occurred_at: "2026-07-13 14:32".to_string(),
        app: "Explorer".to_string(),
        severity: DiagnosticsStatus::Fail,
        recovery: "Recovered automatically".to_string(),
        summary: "SECRET incident summary".to_string(),
        detail: "SECRET stack trace".to_string(),
        report_path: "/private/reports/incident-7.json".to_string(),
        restricted,
    }
}

fn render(width: u16, height: u16, model: &DiagnosticsViewModel) -> Terminal<TestBackend> {
    let chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (width, height),
        screen_stack: vec!["Diagnostics".to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    };
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_diagnostics(
                frame,
                frame.area(),
                &chrome,
                model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render diagnostics");
    terminal
}

fn full_main(width: u16, height: u16) -> Rect {
    match compute_shell_layout(Rect::new(0, 0, width, height)) {
        ShellLayout::Full { main, .. } => main,
        ShellLayout::Compact(_) => panic!("test dimensions should produce a full shell"),
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

fn region_has_fg(terminal: &Terminal<TestBackend>, area: Rect, fg: Color) -> bool {
    let buffer = terminal.backend().buffer();
    (area.y..area.bottom()).any(|y| {
        (area.x..area.right()).any(|x| {
            let cell = &buffer[(x, y)];
            cell.fg == fg && !cell.symbol().trim().is_empty()
        })
    })
}
