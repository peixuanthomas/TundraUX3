use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::components::ComponentTone;
use ui::*;

#[test]
fn breakpoint_is_exact() {
    let m = model();
    assert_eq!(
        system_status_layout(Rect::new(0, 0, 88, 20), &m).column_count,
        4
    );
    assert_eq!(
        system_status_layout(Rect::new(0, 0, 89, 20), &m).column_count,
        8
    )
}
#[test]
fn three_sizes_have_expected_geometry_and_no_overlap() {
    let m = model();
    let l = system_status_layout(full_main(100, 24), &m);
    let small = l
        .widgets
        .iter()
        .find(|w| w.kind == SystemStatusWidgetKind::Cpu)
        .unwrap();
    let wide = l
        .widgets
        .iter()
        .find(|w| w.kind == SystemStatusWidgetKind::Memory)
        .unwrap();
    let large = l
        .widgets
        .iter()
        .find(|w| w.kind == SystemStatusWidgetKind::SystemOverview)
        .unwrap();
    assert_eq!(small.area.height, 5);
    assert_eq!(wide.area.height, 5);
    assert_eq!(large.area.height, 11);
    for (i, a) in l.widgets.iter().enumerate() {
        for b in l.widgets.iter().skip(i + 1) {
            assert!(a.area.intersection(b.area).is_empty())
        }
    }
}
#[test]
fn logical_scroll_scrollbar_and_widget_hits() {
    let mut m = model();
    m.dashboard.wide_widgets.push(widget(
        SystemStatusWidgetKind::Activity,
        SystemStatusWidgetSize::Wide,
        0,
        9,
    ));
    m.dashboard.scroll_row = 2;
    let l = system_status_layout(full_main(100, 20), &m);
    assert_eq!(l.visible_row_start, 2);
    let bar = l.scrollbar.unwrap();
    assert_eq!(
        system_status_hit_test(&l, (bar.x, bar.y)),
        Some(SystemStatusHitTarget::Scrollbar)
    );
    let w = l.widgets.iter().find(|w| !w.preview).unwrap();
    assert_eq!(
        system_status_hit_test(&l, (w.area.x, w.area.y)),
        Some(SystemStatusHitTarget::Widget(w.kind))
    )
}
#[test]
fn dashboard_scroll_is_clamped_after_content_shrinks() {
    let mut m = model();
    m.dashboard.scroll_row = u16::MAX;
    let l = system_status_layout(full_main(100, 20), &m);
    let extent = m
        .dashboard
        .widgets(l.profile)
        .iter()
        .map(|w| w.row + w.size.rows())
        .max()
        .unwrap();
    let capacity = l.visible_row_end - l.visible_row_start;
    assert_eq!(l.visible_row_start, extent.saturating_sub(capacity));
    assert!(!l.widgets.is_empty());
}
#[test]
fn dashboard_partial_scroll_clips_cards_without_overlap() {
    let mut m = model();
    m.dashboard.wide_widgets = vec![
        widget(
            SystemStatusWidgetKind::Cpu,
            SystemStatusWidgetSize::Wide,
            0,
            0,
        ),
        widget(
            SystemStatusWidgetKind::Memory,
            SystemStatusWidgetSize::Wide,
            0,
            2,
        ),
    ];
    m.dashboard.scroll_row = 1;
    let l = system_status_layout(Rect::new(0, 0, 120, 12), &m);
    assert_eq!(l.visible_row_start, 1);
    let upper = l
        .widgets
        .iter()
        .find(|w| w.kind == SystemStatusWidgetKind::Cpu)
        .unwrap();
    let lower = l
        .widgets
        .iter()
        .find(|w| w.kind == SystemStatusWidgetKind::Memory)
        .unwrap();
    assert_eq!(upper.area.y, l.canvas.y);
    assert_eq!(upper.area.height, LOGICAL_ROW_HEIGHT);
    assert_eq!(
        lower.area.y,
        l.canvas.y + LOGICAL_ROW_HEIGHT + LOGICAL_ROW_GAP
    );
    assert!(upper.area.intersection(lower.area).is_empty());
    assert_eq!(
        system_status_hit_test(&l, (upper.area.x, upper.area.y)),
        Some(SystemStatusHitTarget::Widget(SystemStatusWidgetKind::Cpu))
    );
    assert_eq!(
        system_status_hit_test(&l, (lower.area.x, lower.area.y)),
        Some(SystemStatusHitTarget::Widget(
            SystemStatusWidgetKind::Memory
        ))
    );
}
#[test]
fn dashboard_has_no_global_tabs_and_footer_switches_modes() {
    let mut m = model();
    let out = render(100, 24, &m);
    assert!(out.contains("Dashboard"));
    assert!(!out.contains("Overview  Storage  Network"));
    assert!(out.contains("Edit"));
    assert!(out.contains("Refresh"));
    let l = system_status_layout(full_main(100, 24), &m);
    assert_eq!(
        system_status_hit_test(&l, (l.edit_button.x, l.edit_button.y)),
        Some(SystemStatusHitTarget::Edit)
    );
    assert!(l.save_button.is_empty());
    assert!(l.cancel_button.is_empty());
    m.dashboard.editing = true;
    let editing = system_status_layout(full_main(100, 24), &m);
    assert!(editing.edit_button.is_empty());
    assert!(editing.refresh_button.is_empty());
    assert_eq!(
        system_status_hit_test(&editing, (editing.save_button.x, editing.save_button.y)),
        Some(SystemStatusHitTarget::Save)
    );
    assert_eq!(
        system_status_hit_test(&editing, (editing.cancel_button.x, editing.cancel_button.y)),
        Some(SystemStatusHitTarget::Cancel)
    );
    let out = render(100, 24, &m);
    for s in ["Add", "Size", "Remove", "Save", "Cancel"] {
        assert!(out.contains(s))
    }
}
#[test]
fn too_short_uses_empty_state_but_keeps_footer() {
    let m = model();
    let l = system_status_layout(Rect::new(0, 0, 80, 8), &m);
    assert!(l.empty_canvas);
    assert!(l.refresh_button.width > 0);
}
#[test]
fn all_kinds_and_sizes_and_states_render() {
    for kind in SystemStatusWidgetKind::ALL {
        for size in [
            SystemStatusWidgetSize::Small,
            SystemStatusWidgetSize::Wide,
            SystemStatusWidgetSize::Large,
        ] {
            let mut m = model();
            m.dashboard.wide_widgets = vec![widget(kind, size, 0, 0)];
            let out = render(100, 24, &m);
            assert!(out.contains(kind.label()));
            assert!(out.contains("42%"));
            assert!(out.contains("secondary"));
            if size == SystemStatusWidgetSize::Large {
                assert!(out.contains("A"));
            }
        }
    }
    for state in [
        SystemStatusWidgetState::Loading,
        SystemStatusWidgetState::Stale {
            message: "old".into(),
        },
        SystemStatusWidgetState::Unavailable {
            message: "denied".into(),
        },
    ] {
        let mut m = model();
        m.dashboard.wide_widgets[0].state = state;
        let out = render(100, 24, &m);
        assert!(out.contains("Loading") || out.contains("Stale") || out.contains("Unavailable"))
    }
}
#[test]
fn storage_network_and_diagnostics_details_remain_integrated() {
    let mut m = model();
    m.route = SystemStatusRoute::Detail(SystemStatusDetail::Storage);
    assert!(render(100, 24, &m).contains("disk0"));
    m.route = SystemStatusRoute::Detail(SystemStatusDetail::Network);
    assert!(render(100, 24, &m).contains("en0"));
    m.route = SystemStatusRoute::Detail(SystemStatusDetail::Diagnostics);
    m.diagnostics.checks.push(DiagnosticsCheckViewModel {
        id: "check".into(),
        label: "Data path".into(),
        category: "Paths".into(),
        status: DiagnosticsStatus::Warning,
        summary: "missing".into(),
        detail: "missing".into(),
        remediation: "repair".into(),
        repairable: true,
    });
    let l = system_status_layout(full_main(120, 28), &m);
    let row = l.diagnostics_content.as_ref().unwrap().rows[0].area;
    assert_eq!(
        system_status_hit_test(&l, (row.x, row.y)),
        Some(SystemStatusHitTarget::Diagnostics(
            DiagnosticsHitTarget::Check(0)
        ))
    );
    assert!(render(120, 28, &m).contains("Data path"))
}
#[test]
fn activity_has_only_local_logs_and_incidents_tabs() {
    let mut m = model();
    m.route = SystemStatusRoute::Detail(SystemStatusDetail::Activity);
    m.diagnostics.tab = DiagnosticsTab::Health;
    let l = system_status_layout(full_main(120, 28), &m);
    assert_eq!(
        l.activity_tabs
            .iter()
            .map(|tab| tab.tab)
            .collect::<Vec<_>>(),
        vec![DiagnosticsTab::Logs, DiagnosticsTab::Incidents]
    );
    assert_eq!(
        l.diagnostics_content.as_ref().unwrap().active_tab,
        DiagnosticsTab::Logs
    );
    let tabs_area = l.activity_tabs_area.expect("activity tab bar");
    assert!(l.diagnostics_content.as_ref().unwrap().list_panel.y >= tabs_area.bottom());
    for tab in &l.activity_tabs {
        assert_eq!(
            system_status_hit_test(&l, (tab.area.x, tab.area.y)),
            Some(SystemStatusHitTarget::Diagnostics(
                DiagnosticsHitTarget::Tab(tab.tab)
            ))
        );
    }
    let out = render(120, 28, &m);
    assert!(out.contains("Logs"));
    assert!(out.contains("Incidents"));

    m.route = SystemStatusRoute::Detail(SystemStatusDetail::Diagnostics);
    let diagnostics = system_status_layout(full_main(120, 28), &m);
    assert!(diagnostics.activity_tabs_area.is_none());
    assert!(diagnostics.activity_tabs.is_empty());
    assert_eq!(
        diagnostics
            .diagnostics_content
            .as_ref()
            .unwrap()
            .list_panel
            .y,
        diagnostics.canvas.y
    );
}
#[test]
fn size_picker_renders_exact_rows_and_captures_dashboard_hits() {
    let mut m = model();
    m.dashboard.editing = true;
    m.dashboard.size_picker = Some(SystemStatusSizePickerViewModel { selected: 1 });
    let l = system_status_layout(full_main(100, 24), &m);
    assert_eq!(l.size_picker_items.len(), 3);
    for (index, row) in l.size_picker_items.iter().enumerate() {
        assert_eq!(
            system_status_hit_test(&l, (row.area.x, row.area.y)),
            Some(SystemStatusHitTarget::SizePickerItem(index))
        );
    }
    let widget = l.widgets.first().unwrap().area;
    assert_eq!(
        system_status_hit_test(&l, (widget.x, widget.y)),
        None,
        "modal captures underlying widget hits"
    );
    let out = render(100, 24, &m);
    for label in ["2x2", "2x4", "4x4"] {
        assert!(out.contains(label));
    }
}
#[test]
fn add_picker_viewport_hit_rows_match_absolute_rendered_items() {
    let mut m = model();
    m.dashboard.editing = true;
    m.dashboard.picker = Some(SystemStatusPickerViewModel {
        title: "Add widget".into(),
        items: SystemStatusWidgetKind::ALL
            .into_iter()
            .map(|kind| SystemStatusPickerItemViewModel {
                kind,
                label: kind.label().into(),
                detail: String::new(),
                enabled: true,
            })
            .collect(),
        selected: 5,
    });
    let main = full_main(100, 12);
    let l = system_status_layout(main, &m);
    assert_eq!(l.picker_viewport_start, 2);
    assert_eq!(
        l.picker_items
            .iter()
            .map(|row| row.index)
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
    assert!(!l.picker_items.iter().any(|row| row.index < 2));
    let temperature = l.picker_items.iter().find(|row| row.index == 5).unwrap();
    assert_eq!(
        system_status_hit_test(&l, (temperature.area.x, temperature.area.y)),
        Some(SystemStatusHitTarget::PickerItem(5))
    );
    assert!(render(100, 12, &m).contains("Temperature"));
}
#[test]
fn theme_and_size_smoke() {
    for theme in [
        TundraTheme::default_dark().with_border_shape(BorderShape::Rounded),
        TundraTheme::default().with_border_shape(BorderShape::Square),
    ] {
        for (w, h) in [(50, 12), (80, 20), (100, 24)] {
            let _ = render_theme(w, h, &model(), theme.clone());
        }
    }
}

fn widget(
    kind: SystemStatusWidgetKind,
    size: SystemStatusWidgetSize,
    column: u16,
    row: u16,
) -> SystemStatusWidgetViewModel {
    SystemStatusWidgetViewModel {
        kind,
        size,
        column,
        row,
        state: SystemStatusWidgetState::Ready,
        tone: ComponentTone::Accent,
        primary: "42%".into(),
        secondary: vec!["secondary".into()],
        trend: Some(vec![1, 3, 2, 5]),
        progress_percent: Some(42),
        bars: vec![SystemStatusBarItem {
            label: "A".into(),
            value: 42,
        }],
        compact_rows: vec![vec!["process".into(), "12%".into()]],
        openable: true,
    }
}
fn model() -> SystemStatusViewModel {
    SystemStatusViewModel {
        content: SystemStatusContentViewModel::Admin(AdminSystemStatusViewModel {
            overview: SystemStatusOverviewViewModel {
                storage_status: "Healthy".into(),
                storage_tone: ComponentTone::Success,
                system_volume_usage: "42 GB / 100 GB".into(),
                system_volume_used_percentage: Some(42),
                network_status: "Connected".into(),
                network_tone: ComponentTone::Success,
                active_link_count: "1".into(),
                last_refreshed: "now".into(),
            },
            storage_state: SystemStatusSectionState::Ready,
            storage_rows: vec![StorageVolumeRowViewModel {
                volume: "disk0".into(),
                kind: "APFS".into(),
                system_volume: "Yes".into(),
                access: "Read/write".into(),
                usage: "42 GB".into(),
                used_percentage: "42%".into(),
                pressure: "Normal".into(),
                tone: ComponentTone::Success,
            }],
            network_state: SystemStatusSectionState::Ready,
            network_rows: vec![NetworkInterfaceRowViewModel {
                name: "en0".into(),
                display_name: "Wi-Fi".into(),
                kind: "Wireless".into(),
                link_state: "Up".into(),
                received_rate: "2.4 MiB/s".into(),
                transmitted_rate: "0.6 MiB/s".into(),
                addresses: "192.0.2.1".into(),
                tone: ComponentTone::Success,
            }],
        }),
        diagnostics: diagnostics(),
        route: SystemStatusRoute::Dashboard,
        dashboard: SystemStatusDashboardViewModel {
            wide_widgets: vec![
                widget(
                    SystemStatusWidgetKind::SystemOverview,
                    SystemStatusWidgetSize::Large,
                    0,
                    0,
                ),
                widget(
                    SystemStatusWidgetKind::Cpu,
                    SystemStatusWidgetSize::Small,
                    4,
                    0,
                ),
                widget(
                    SystemStatusWidgetKind::Memory,
                    SystemStatusWidgetSize::Wide,
                    4,
                    2,
                ),
            ],
            narrow_widgets: vec![
                widget(
                    SystemStatusWidgetKind::Cpu,
                    SystemStatusWidgetSize::Small,
                    0,
                    0,
                ),
                widget(
                    SystemStatusWidgetKind::Memory,
                    SystemStatusWidgetSize::Small,
                    2,
                    0,
                ),
            ],
            selected: Some(SystemStatusWidgetKind::Cpu),
            updated: "now".into(),
            ..Default::default()
        },
        selected_row: 0,
        scroll_offset: 0,
        refreshing: false,
        feedback: None,
    }
}
fn diagnostics() -> DiagnosticsViewModel {
    DiagnosticsViewModel {
        tab: DiagnosticsTab::Health,
        checks: vec![],
        incidents: vec![],
        logs: vec![],
        selected_check: 0,
        selected_incident: 0,
        selected_log: 0,
        list_window_start: 0,
        list_window_is_explicit: false,
        scanning: false,
        can_view_details: true,
        can_repair: true,
        restart_required: false,
        repair_dialog: None,
        feedback: None,
        scanned_at: Some("now".into()),
    }
}
fn chrome(w: u16, h: u16) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".into(),
        build_mode: "test".into(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (w, h),
        screen_stack: vec!["System Status".into()],
        status: StatusViewModel {
            status: "Ready".into(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}
fn render(w: u16, h: u16, m: &SystemStatusViewModel) -> String {
    render_theme(w, h, m, TundraTheme::default_dark())
}
fn render_theme(w: u16, h: u16, m: &SystemStatusViewModel, t: TundraTheme) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let c = chrome(w, h);
    terminal
        .draw(|f| render_system_status(f, f.area(), &c, m, &t))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn full_main(w: u16, h: u16) -> Rect {
    match compute_shell_layout(Rect::new(0, 0, w, h)) {
        ShellLayout::Full { main, .. } => main,
        _ => panic!(),
    }
}
