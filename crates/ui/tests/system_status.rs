use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ui::components::ComponentTone;
use ui::{
    AdminSystemStatusViewModel, BorderShape, HomeDisplayMode, NetworkInterfaceRowViewModel,
    NotificationTone, ShellChromeViewModel, ShellLayout, StatusViewModel,
    StorageVolumeRowViewModel, SystemStatusContentViewModel, SystemStatusHitTarget,
    SystemStatusOverviewViewModel, SystemStatusSectionState, SystemStatusTab,
    SystemStatusViewModel, TundraTheme, UserSystemStatusViewModel, compute_shell_layout,
    render_system_status, system_status_hit_test, system_status_layout,
};

#[test]
fn admin_exposes_all_tabs_and_hit_targets() {
    let model = admin_model(SystemStatusTab::Storage, SystemStatusSectionState::Ready);
    let layout = system_status_layout(full_main(100, 24), &model);
    assert_eq!(layout.tabs.len(), 3);
    for (index, tab) in SystemStatusTab::ALL.into_iter().enumerate() {
        let item = layout.tabs.iter().find(|item| item.tab == tab).unwrap();
        assert_eq!(
            system_status_hit_test(&layout, (item.area.x, item.area.y)),
            Some(SystemStatusHitTarget::Tab(tab))
        );
        assert_eq!(
            system_status_hit_test(&layout, (item.area.right().saturating_sub(1), item.area.y),),
            Some(SystemStatusHitTarget::Tab(tab))
        );
        if let Some(next) = layout.tabs.get(index + 1) {
            assert_eq!(item.area.right(), next.area.x);
            assert_eq!(
                system_status_hit_test(&layout, (next.area.x, next.area.y)),
                Some(SystemStatusHitTarget::Tab(next.tab))
            );
        }
    }
    assert_eq!(
        system_status_hit_test(&layout, (layout.rows[0].area.x, layout.rows[0].area.y)),
        Some(SystemStatusHitTarget::Row(0))
    );
    assert_eq!(
        system_status_hit_test(
            &layout,
            (layout.diagnostics_button.x, layout.diagnostics_button.y)
        ),
        Some(SystemStatusHitTarget::Diagnostics)
    );
    assert_eq!(
        system_status_hit_test(&layout, (layout.refresh_button.x, layout.refresh_button.y)),
        Some(SystemStatusHitTarget::Refresh)
    );
}

#[test]
fn stale_storage_and_network_reserve_notice_without_covering_table_geometry() {
    for tab in [SystemStatusTab::Storage, SystemStatusTab::Network] {
        let mut ready = admin_model(tab, SystemStatusSectionState::Ready);
        let mut stale = admin_model(tab, SystemStatusSectionState::Ready);
        if let SystemStatusContentViewModel::Admin(admin) = &mut ready.content {
            admin.storage_rows = vec![storage_row(0), storage_row(1)];
            admin.network_rows = vec![network_row(0), network_row(1)];
        }
        if let SystemStatusContentViewModel::Admin(admin) = &mut stale.content {
            admin.storage_rows = vec![storage_row(0), storage_row(1)];
            admin.network_rows = vec![network_row(0), network_row(1)];
            match tab {
                SystemStatusTab::Storage => {
                    admin.storage_state = SystemStatusSectionState::Stale {
                        message: "Storage data is old".into(),
                    }
                }
                SystemStatusTab::Network => {
                    admin.network_state = SystemStatusSectionState::Stale {
                        message: "Network data is old".into(),
                    }
                }
                SystemStatusTab::Overview => unreachable!(),
            }
        }
        let ready_layout = system_status_layout(full_main(100, 24), &ready);
        let layout = system_status_layout(full_main(100, 24), &stale);
        let notice = layout.notice_area.expect("stale notice");
        assert_eq!(
            layout.visible_capacity + usize::from(notice.height),
            ready_layout.visible_capacity
        );
        assert!(notice.bottom() <= layout.rows_area.y);
        assert_eq!(system_status_hit_test(&layout, (notice.x, notice.y)), None);
        assert_eq!(
            system_status_hit_test(&layout, (layout.rows[0].area.x, layout.rows[0].area.y)),
            Some(SystemStatusHitTarget::Row(0))
        );

        let output = render(100, 24, &stale, TundraTheme::default_dark());
        assert!(output.contains("Stale data"));
        match tab {
            SystemStatusTab::Storage => {
                assert!(output.contains("Volume"));
                assert!(output.contains("disk0"));
            }
            SystemStatusTab::Network => {
                assert!(output.contains("Display name"));
                assert!(output.contains("en0"));
            }
            SystemStatusTab::Overview => unreachable!(),
        }
    }
}

#[test]
fn user_is_summary_only_and_ignores_admin_tab_and_rows() {
    let model = user_model();
    let layout = system_status_layout(full_main(80, 20), &model);
    assert!(layout.tabs.is_empty());
    assert!(layout.rows.is_empty());
    assert_eq!(model.item_count(), 0);
    let output = render(80, 20, &model, TundraTheme::default_dark());
    assert!(output.contains("Storage status: Healthy"));
    assert!(output.contains("Network status: Connected"));
    assert!(!output.contains("en0"));
    assert!(output.contains("Diagnostics"));
    assert_eq!(
        system_status_hit_test(
            &layout,
            (layout.diagnostics_button.x, layout.diagnostics_button.y)
        ),
        Some(SystemStatusHitTarget::Diagnostics)
    );
}

#[test]
fn loading_unavailable_and_empty_states_render() {
    let loading = admin_model(SystemStatusTab::Storage, SystemStatusSectionState::Loading);
    assert!(render(80, 20, &loading, TundraTheme::default_dark()).contains("Loading..."));
    let mut unavailable = admin_model(SystemStatusTab::Network, SystemStatusSectionState::Ready);
    if let SystemStatusContentViewModel::Admin(admin) = &mut unavailable.content {
        admin.network_state = SystemStatusSectionState::Unavailable {
            message: "Permission denied".into(),
        };
    }
    assert!(
        render(80, 20, &unavailable, TundraTheme::default_dark()).contains("Permission denied")
    );
    let mut empty = admin_model(SystemStatusTab::Storage, SystemStatusSectionState::Ready);
    if let SystemStatusContentViewModel::Admin(admin) = &mut empty.content {
        admin.storage_rows.clear();
    }
    assert!(render(80, 20, &empty, TundraTheme::default_dark()).contains("No storage volumes"));
}

#[test]
fn narrow_terminal_and_two_themes_render_without_panicking() {
    let model = admin_model(SystemStatusTab::Overview, SystemStatusSectionState::Ready);
    let _ = render(20, 7, &model, TundraTheme::default_dark());
    let narrow = system_status_layout(Rect::new(0, 0, 20, 5), &model);
    assert!(narrow.diagnostics_button.right() <= narrow.refresh_button.x);
    assert!(narrow.refresh_button.right() <= narrow.footer.right());
    let _ = render(
        80,
        20,
        &model,
        TundraTheme::default_dark().with_border_shape(BorderShape::Square),
    );
}

#[test]
fn scroll_window_and_scrollbar_hit_are_stable() {
    let mut model = admin_model(SystemStatusTab::Storage, SystemStatusSectionState::Ready);
    if let SystemStatusContentViewModel::Admin(admin) = &mut model.content {
        admin.storage_rows = (0..20).map(storage_row).collect();
    }
    model.selected_row = 18;
    let layout = system_status_layout(full_main(80, 16), &model);
    assert!(layout.visible_start > 0);
    assert!(layout.rows.iter().any(|row| row.index == 18));
    let scrollbar = layout.scrollbar.expect("scrollbar");
    assert_eq!(
        system_status_hit_test(&layout, (scrollbar.x, scrollbar.y)),
        Some(SystemStatusHitTarget::Scrollbar)
    );
}

fn admin_model(
    tab: SystemStatusTab,
    storage_state: SystemStatusSectionState,
) -> SystemStatusViewModel {
    SystemStatusViewModel {
        content: SystemStatusContentViewModel::Admin(AdminSystemStatusViewModel {
            overview: SystemStatusOverviewViewModel {
                storage_status: "Healthy".into(),
                storage_tone: ComponentTone::Success,
                system_volume_usage: "42%".into(),
                active_link_count: "1".into(),
                last_refreshed: "now".into(),
            },
            storage_state,
            storage_rows: vec![storage_row(0)],
            network_state: SystemStatusSectionState::Ready,
            network_rows: vec![network_row(0)],
        }),
        tab,
        selected_row: 0,
        scroll_offset: 0,
        refreshing: false,
        feedback: None,
    }
}

fn user_model() -> SystemStatusViewModel {
    SystemStatusViewModel {
        content: SystemStatusContentViewModel::User(UserSystemStatusViewModel {
            storage_status: "Healthy".into(),
            storage_tone: ComponentTone::Success,
            system_volume_usage: "42%".into(),
            network_status: "Connected".into(),
            network_tone: ComponentTone::Success,
            last_refreshed: "now".into(),
        }),
        tab: SystemStatusTab::Network,
        selected_row: 99,
        scroll_offset: 99,
        refreshing: false,
        feedback: None,
    }
}

fn storage_row(index: usize) -> StorageVolumeRowViewModel {
    StorageVolumeRowViewModel {
        volume: format!("disk{index}"),
        kind: "APFS".into(),
        system_volume: "Yes".into(),
        access: "Read/write".into(),
        usage: "42 GB / 100 GB".into(),
        used_percentage: "42%".into(),
        pressure: "Normal".into(),
        tone: ComponentTone::Success,
    }
}

fn network_row(index: usize) -> NetworkInterfaceRowViewModel {
    NetworkInterfaceRowViewModel {
        name: format!("en{index}"),
        display_name: "Wi-Fi".into(),
        kind: "Wireless".into(),
        link_state: "Up".into(),
        addresses: "192.0.2.1".into(),
        tone: ComponentTone::Success,
    }
}

fn render(width: u16, height: u16, model: &SystemStatusViewModel, theme: TundraTheme) -> String {
    let chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".into(),
        build_mode: "test".into(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (width, height),
        screen_stack: vec!["System Status".into()],
        status: StatusViewModel {
            status: "Ready".into(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    };
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| render_system_status(frame, frame.area(), &chrome, model, &theme))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn full_main(width: u16, height: u16) -> Rect {
    match compute_shell_layout(Rect::new(0, 0, width, height)) {
        ShellLayout::Full { main, .. } => main,
        ShellLayout::Compact(_) => panic!("expected full layout"),
    }
}
