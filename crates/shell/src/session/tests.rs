use super::queries::{ResolvedExplorerOverlay, ShellOverlayCategory};
use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

struct SystemStatusTestWeatherProvider;

impl system_services::WeatherProvider for SystemStatusTestWeatherProvider {
    fn current_weather<'life0, 'async_trait>(
        &'life0 self,
        _location: system_services::WeatherLocation,
        _units: system_services::WeatherUnits,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<system_services::WeatherData, String>>
                + Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Err("weather disabled in system status test".into()) })
    }
}

struct SystemStatusServiceGuard(Option<system_services::SystemServicesHandle>);

impl Drop for SystemStatusServiceGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = handle.shutdown();
        }
    }
}

struct SystemStatusTempGuard(PathBuf);

impl Drop for SystemStatusTempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn set_test_auth_role(state: &mut ShellSession, role: UserRole) {
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            session_id: format!("{}-session", role.as_str()),
            user_id: format!("{}-id", role.as_str()),
            username: role.as_str().to_ascii_lowercase(),
            role,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
}

fn system_status_test_snapshot(
    revision: u64,
    pressure: system_services::StoragePressure,
    link: bool,
    source: system_services::SystemVolumeSource,
) -> app::AppSystemStatusSnapshot {
    let sampled_at = Utc::now();
    app::AppSystemStatusSnapshot {
        revision,
        observed_at: sampled_at,
        metrics: system_services::SystemMetricsSnapshot::loading(),
        storage: system_services::StorageState::Ready(system_services::StorageSnapshot {
            volumes: vec![system_services::StorageVolumeSnapshot {
                identifier: "/sensitive/mount".into(),
                label: Some("secret-label".into()),
                kind: system_services::StorageVolumeKind::Fixed,
                is_system: true,
                access: system_services::StorageVolumeAccess::ReadWrite,
                total_bytes: Some(1000),
                available_bytes: Some(100),
                pressure,
            }],
            overall_pressure: pressure,
            system_volume_index: Some(0),
            system_volume_source: source,
            sampled_at,
        }),
        network: system_services::NetworkState::Ready(system_services::NetworkSnapshot {
            interfaces: vec![system_services::NetworkInterfaceSnapshot {
                name: "secret-iface".into(),
                display_name: Some("secret-display".into()),
                kind: system_services::NetworkInterfaceKind::Virtual,
                link_state: if link {
                    system_services::NetworkLinkState::Up
                } else {
                    system_services::NetworkLinkState::Down
                },
                addresses: vec!["192.0.2.99".into(), "2001:db8::99".into()],
            }],
            active_link_count: usize::from(link),
            has_active_link: link,
            sampled_at,
        }),
    }
}

fn system_status_metric_snapshot(uptime: u64) -> app::AppSystemStatusSnapshot {
    let mut snapshot = system_status_test_snapshot(
        uptime,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    );
    snapshot.metrics.uptime =
        system_services::MetricState::Ready(system_services::UptimeSnapshot { seconds: uptime });
    snapshot.metrics.cpu = system_services::MetricState::Ready(system_services::CpuSnapshot {
        usage_percent: 25.0,
        per_core_percent: vec![25.0],
        logical_core_count: 1,
        physical_core_count: Some(1),
    });
    snapshot.metrics.memory =
        system_services::MetricState::Ready(system_services::MemorySnapshot {
            total_bytes: 100,
            used_bytes: 40,
            available_bytes: 60,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
        });
    snapshot.metrics.network_io =
        system_services::MetricState::Ready(system_services::NetworkIoSnapshot {
            interfaces: vec![],
            total_received_bytes: 1,
            total_transmitted_bytes: 2,
            total_received_bytes_per_second: 3.0,
            total_transmitted_bytes_per_second: 4.0,
        });
    snapshot.metrics.thermal =
        system_services::MetricState::Ready(vec![system_services::ThermalSensorSnapshot {
            label: "CPU".into(),
            temperature_celsius: 50.0,
            critical_celsius: Some(90.0),
        }]);
    snapshot
}

#[test]
fn system_status_history_deduplicates_uptime_rejects_stale_and_caps_queues() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let first = system_status_metric_snapshot(10);
    state.apply_system_status_snapshot(first.clone());
    state.apply_system_status_snapshot(first);
    assert_eq!(state.system_status_history.cpu.len(), 1);
    state.apply_system_status_snapshot(system_status_metric_snapshot(11));
    assert_eq!(state.system_status_history.cpu.len(), 2);
    let mut stale = system_status_metric_snapshot(12);
    stale.metrics.uptime = system_services::MetricState::Stale {
        last_good: system_services::UptimeSnapshot { seconds: 12 },
        error: "old".into(),
    };
    state.apply_system_status_snapshot(stale);
    assert_eq!(
        state.system_status_history.cpu.len(),
        2,
        "stale uptime inserts no fake points"
    );
    for uptime in 20..90 {
        state.apply_system_status_snapshot(system_status_metric_snapshot(uptime));
    }
    for len in [
        state.system_status_history.cpu.len(),
        state.system_status_history.memory.len(),
        state.system_status_history.network_received.len(),
        state.system_status_history.network_transmitted.len(),
        state.system_status_history.temperature.len(),
    ] {
        assert!(len <= 60);
    }
}

#[test]
fn system_status_metric_vm_exposes_progress_bars_status_and_booted_rows() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let mut snapshot = system_status_metric_snapshot(3_600);
    if let system_services::MetricState::Ready(cpu) = &mut snapshot.metrics.cpu {
        cpu.usage_percent = 140.0;
        cpu.per_core_percent = vec![-5.0, 150.0];
    }
    state.apply_system_status_snapshot(snapshot);
    let model = state.to_system_status_view_model().unwrap();
    let widgets = &model.dashboard.wide_widgets;
    let overview = widgets
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::SystemOverview)
        .unwrap();
    assert_eq!(overview.primary, "Healthy");
    assert_eq!(overview.tone, ui::components::ComponentTone::Success);
    assert!(overview.secondary.len() <= 2);
    assert!(
        overview
            .compact_rows
            .iter()
            .any(|row| row.first().is_some_and(|label| label == "Booted"))
    );
    let cpu = widgets
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::Cpu)
        .unwrap();
    assert_eq!(cpu.progress_percent, Some(100));
    assert_eq!(
        cpu.bars.iter().map(|bar| bar.value).collect::<Vec<_>>(),
        vec![0, 100]
    );
    let memory = widgets
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::Memory)
        .unwrap();
    assert_eq!(memory.progress_percent, Some(40));
    assert_eq!(
        memory.bars.first().map(|bar| bar.label.as_str()),
        Some("RAM")
    );
    let storage = widgets
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::Storage)
        .unwrap();
    assert_eq!(storage.progress_percent, Some(90));
    let uptime = widgets
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::UptimeLoad)
        .unwrap();
    assert!(
        uptime
            .compact_rows
            .iter()
            .any(|row| row.first().is_some_and(|label| label == "Booted"))
    );

    state.apply_system_status_snapshot(system_status_test_snapshot(
        4_000,
        system_services::StoragePressure::Critical,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    let model = state.to_system_status_view_model().unwrap();
    let overview = model
        .dashboard
        .wide_widgets
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::SystemOverview)
        .unwrap();
    assert_eq!(overview.primary, "Needs attention");
    assert_eq!(overview.tone, ui::components::ComponentTone::Danger);

    let mut degraded = system_status_metric_snapshot(4_100);
    degraded.metrics.thermal = system_services::MetricState::Unavailable {
        reason: "No sensors".into(),
    };
    state.apply_system_status_snapshot(degraded);
    let model = state.to_system_status_view_model().unwrap();
    let overview = model
        .dashboard
        .wide_widgets
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::SystemOverview)
        .unwrap();
    assert_eq!(overview.primary, "Degraded");
    assert_eq!(overview.tone, ui::components::ComponentTone::Warning);
}

#[test]
fn system_status_role_gate_and_missing_service_reject_open() {
    for role in [UserRole::Admin, UserRole::User, UserRole::Guest] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        set_test_auth_role(&mut state, role);
        let labels = state
            .user_home_entries()
            .into_iter()
            .map(|entry| entry.label)
            .collect::<Vec<_>>();
        assert_eq!(
            labels.contains(&"System Status".to_string()),
            role != UserRole::Guest
        );
        assert!(!labels.contains(&"Diagnostics".to_string()));
        let original_focus = state.focused_component;
        state.open_system_status();
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(state.focused_component, original_focus);
        if role != UserRole::Guest {
            assert_eq!(state.status(), "System Status service unavailable");
        }
    }
}

#[test]
fn diagnostics_is_integrated_into_system_status_tabs() {
    for role in [UserRole::Admin, UserRole::User] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        set_test_auth_role(&mut state, role);

        state.apply_input(InputEvent::from_key_label("D"));
        assert_eq!(state.active_screen(), ShellScreen::Home);

        state.apply_routed_event_once(
            RoutedEvent {
                input: InputEvent::from_key_label("D"),
                target: RoutedTarget::Global,
                command: ShellCommand::OpenDiagnostics,
            },
            &platform::mock::UnsupportedPlatform,
            Instant::now(),
        );
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(state.status(), "Open Diagnostics from System Status");

        state.screen_stack.push(ShellScreen::SystemStatus);
        state.focused_component = ShellComponent::SystemStatus;
        state.open_diagnostics();
        assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
        assert_eq!(state.system_status_tab, ui::SystemStatusTab::Health);
        assert_eq!(
            state.system_status_route,
            ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Diagnostics)
        );
        assert_eq!(state.focused_component(), ShellComponent::SystemStatus);

        state.apply_input(InputEvent::from_key_label("Esc"));
        assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
        assert_eq!(state.system_status_route, ui::SystemStatusRoute::Dashboard);
        state.apply_input(InputEvent::from_key_label("Esc"));
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(state.focused_component(), ShellComponent::Home);
    }
}

#[test]
fn system_status_widget_double_click_opens_detail_without_leaving_page() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    let model = state.to_system_status_view_model().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!("system status mouse test requires a full layout");
    };
    let widget = ui::system_status_layout(main, &model)
        .widgets
        .into_iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::Diagnostics)
        .expect("diagnostics widget");

    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        widget.area.x.saturating_add(1),
        widget.area.y.saturating_add(1),
        ui::MouseEventKind::DoubleClick(PointerButton::Left),
    )));

    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(state.system_status_tab, ui::SystemStatusTab::Health);
    assert_eq!(
        state.system_status_route,
        ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Diagnostics)
    );
    assert_eq!(state.focused_component(), ShellComponent::SystemStatus);
}

#[test]
fn system_status_keyboard_navigates_dashboard_and_routes_detail_actions() {
    let mut user = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut user, UserRole::User);
    user.screen_stack.push(ShellScreen::SystemStatus);
    user.focused_component = ShellComponent::SystemStatus;
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("Tab")).1,
        ShellCommand::SystemStatusFocusNext
    );
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::SystemStatusActivateFocus
    );
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("e")).1,
        ShellCommand::SystemStatusBeginEdit
    );
    user.set_system_status_tab(ui::SystemStatusTab::Health);
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("r")).1,
        ShellCommand::DiagnosticsRescan
    );
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("Down")).1,
        ShellCommand::DiagnosticsNext
    );

    let mut admin = user;
    set_test_auth_role(&mut admin, UserRole::Admin);
    admin.set_system_status_tab(ui::SystemStatusTab::Overview);
    assert_eq!(
        admin.route_key_input(&KeyInput::from_label("Shift+Tab")).1,
        ShellCommand::SystemStatusFocusPrevious
    );
}

#[test]
fn system_status_activity_local_tabs_route_mouse_without_leaving_page() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.set_system_status_tab(ui::SystemStatusTab::Logs);
    let model = state.to_system_status_view_model().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!()
    };
    let layout = ui::system_status_layout(main, &model);
    let incidents = layout
        .activity_tabs
        .iter()
        .find(|tab| tab.tab == ui::DiagnosticsTab::Incidents)
        .unwrap()
        .area;
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        incidents.x,
        incidents.y,
        ui::MouseEventKind::Down(PointerButton::Left),
    )));
    assert_eq!(state.diagnostics_tab, ui::DiagnosticsTab::Incidents);
    assert_eq!(
        state.system_status_route,
        ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Activity)
    );

    let model = state.to_system_status_view_model().unwrap();
    let layout = ui::system_status_layout(main, &model);
    let logs = layout
        .activity_tabs
        .iter()
        .find(|tab| tab.tab == ui::DiagnosticsTab::Logs)
        .unwrap()
        .area;
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        logs.x,
        logs.y,
        ui::MouseEventKind::Down(PointerButton::Left),
    )));
    assert_eq!(state.diagnostics_tab, ui::DiagnosticsTab::Logs);
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
}

#[test]
fn system_status_dashboard_focus_wraps_skips_disabled_and_activates() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.ensure_system_status_widget_selection();
    state.restore_system_status_widget_focus();
    let first = state.system_status_dashboard_focus;
    state.system_status_dashboard_focus = ui::SystemStatusDashboardFocus::Refresh;
    state.move_system_status_dashboard_focus(1);
    assert_eq!(state.system_status_dashboard_focus, first, "forward wraps");
    state.move_system_status_dashboard_focus(-1);
    assert_eq!(
        state.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Refresh,
        "reverse wraps"
    );

    state.begin_system_status_dashboard_edit();
    state.system_status_selected_widget = None;
    state.system_status_dashboard_focus = ui::SystemStatusDashboardFocus::Add;
    state.move_system_status_dashboard_focus(1);
    assert_eq!(
        state.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Cancel,
        "disabled Size, Remove, and clean Save are skipped"
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(
        state.system_status_dashboard_draft.is_none(),
        "focused Cancel activates"
    );
}

#[test]
fn system_status_dashboard_focus_restores_and_tabs_offscreen() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (100, 20),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.ensure_system_status_widget_selection();
    let kind = state.system_status_selected_widget.expect("default widget");
    state.system_status_dashboard_focus = ui::SystemStatusDashboardFocus::Widget(
        super::controller::system_status::ui_widget_kind(kind),
    );
    state.open_system_status_detail(kind);
    state.back_from_system_status_detail();
    assert_eq!(
        state.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Widget(super::controller::system_status::ui_widget_kind(
            kind
        ))
    );

    state.begin_system_status_dashboard_edit();
    let draft = state.system_status_dashboard_draft.as_mut().unwrap();
    let last = draft.wide.placements.last_mut().unwrap();
    last.row = 20;
    let last_kind = super::controller::system_status::ui_widget_kind(last.kind);
    let order = state.system_status_dashboard_focus_order();
    let position = order
        .iter()
        .position(|focus| *focus == ui::SystemStatusDashboardFocus::Widget(last_kind))
        .unwrap();
    state.system_status_dashboard_focus = order[position.saturating_sub(1)];
    state.move_system_status_dashboard_focus(1);
    assert_eq!(
        state.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Widget(last_kind)
    );
    assert!(
        state.system_status_dashboard_scroll_row > 0,
        "Tab scrolls the offscreen widget into view"
    );
}

#[test]
fn system_status_size_shortcut_cycles_and_picker_applies_active_profile_only() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    state.system_status_selected_widget = Some(storage::SystemStatusWidgetKind::Cpu);
    state.system_status_dashboard_focus =
        ui::SystemStatusDashboardFocus::Widget(ui::SystemStatusWidgetKind::Cpu);
    let size = |state: &ShellSession, profile| {
        state
            .system_status_dashboard_draft
            .as_ref()
            .unwrap()
            .layout(profile)
            .placements
            .iter()
            .find(|p| p.kind == storage::SystemStatusWidgetKind::Cpu)
            .unwrap()
            .size
    };
    let before = size(&state, storage::DashboardProfile::Wide);
    state.apply_input(InputEvent::from_key_label("s"));
    assert_eq!(
        size(&state, storage::DashboardProfile::Wide),
        before.cycle(),
        "S remains immediate cycle"
    );

    state.system_status_dashboard_focus = ui::SystemStatusDashboardFocus::Size;
    state.apply_input(InputEvent::from_key_label("Enter"));
    let expected = match size(&state, storage::DashboardProfile::Wide) {
        storage::SystemStatusWidgetSize::Small => 0,
        storage::SystemStatusWidgetSize::Wide => 1,
        storage::SystemStatusWidgetSize::Large => 2,
    };
    assert_eq!(state.system_status_size_picker.unwrap().selected, expected);
    let unchanged = state.system_status_dashboard_draft.clone().unwrap();
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(
        state.system_status_dashboard_draft.as_ref().unwrap(),
        &unchanged,
        "Esc does not resize"
    );

    let narrow_before = size(&state, storage::DashboardProfile::Narrow);
    state.open_system_status_size_picker();
    state.select_system_status_size_picker_item(2);
    state.apply_system_status_size_picker();
    assert_eq!(
        size(&state, storage::DashboardProfile::Wide),
        storage::SystemStatusWidgetSize::Large
    );
    assert_eq!(
        size(&state, storage::DashboardProfile::Narrow),
        narrow_before,
        "other profile is unchanged"
    );
    assert_eq!(
        state.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Size
    );
}

#[test]
fn system_status_size_picker_double_clicks_exact_size_and_transitions_clear_it() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    state.system_status_selected_widget = Some(storage::SystemStatusWidgetKind::Cpu);
    state.open_system_status_size_picker();
    let model = state.to_system_status_view_model().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!()
    };
    let row = ui::system_status_layout(main, &model).size_picker_items[2].area;
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        row.x,
        row.y,
        ui::MouseEventKind::DoubleClick(PointerButton::Left),
    )));
    let wide = state
        .system_status_dashboard_draft
        .as_ref()
        .unwrap()
        .wide
        .placements
        .iter()
        .find(|p| p.kind == storage::SystemStatusWidgetKind::Cpu)
        .unwrap();
    assert_eq!(wide.size, storage::SystemStatusWidgetSize::Large);
    assert!(state.system_status_size_picker.is_none());

    state.open_system_status_size_picker();
    state.open_system_status_add_picker();
    assert!(state.system_status_size_picker.is_none());
    state.close_system_status_add_picker();
    state.open_system_status_size_picker();
    state.finish_cancel_system_status_dashboard_edit();
    assert!(state.system_status_size_picker.is_none());
}

#[test]
fn system_status_draft_profiles_catalog_cancel_and_save_failure_are_isolated() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (80, 24),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    let baseline = state.system_status_dashboard_config();
    state.begin_system_status_dashboard_edit();
    state.system_status_selected_widget = Some(storage::SystemStatusWidgetKind::Cpu);
    state.move_selected_system_status_widget(1, 2);
    state.cycle_selected_system_status_widget_size();
    let changed = state.system_status_dashboard_draft.as_ref().unwrap();
    assert_eq!(
        changed.wide, baseline.wide,
        "narrow edits leave wide semantics identical"
    );
    assert_ne!(changed.narrow, baseline.narrow);
    state.finish_cancel_system_status_dashboard_edit();
    assert_eq!(
        state.system_status_dashboard_config(),
        baseline,
        "Cancel restores baseline"
    );

    state.begin_system_status_dashboard_edit();
    let add_kind = storage::SystemStatusWidgetKind::Battery;
    let picker_index = super::controller::system_status::SYSTEM_STATUS_WIDGET_KINDS
        .iter()
        .position(|kind| *kind == add_kind)
        .unwrap();
    state.open_system_status_add_picker();
    state.select_system_status_picker_item(picker_index);
    state.apply_input(InputEvent::from_key_label("Enter"));
    let added = state
        .system_status_dashboard_draft
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(
        added
            .widgets
            .iter()
            .filter(|kind| **kind == add_kind)
            .count(),
        1
    );
    for layout in [&added.wide, &added.narrow] {
        assert_eq!(
            layout
                .placements
                .iter()
                .filter(|p| p.kind == add_kind)
                .count(),
            1
        );
    }
    state.remove_selected_system_status_widget();
    let removed = state.system_status_dashboard_draft.as_ref().unwrap();
    assert!(!removed.widgets.contains(&add_kind));
    assert_eq!(
        removed.wide, baseline.wide,
        "removal does not compact unrelated wide gaps"
    );
    assert_eq!(
        removed.narrow, baseline.narrow,
        "removal does not compact unrelated narrow gaps"
    );

    state.system_status_selected_widget = Some(storage::SystemStatusWidgetKind::Cpu);
    state.cycle_selected_system_status_widget_size();
    let dirty = state.system_status_dashboard_draft.clone().unwrap();
    state.save_system_status_dashboard();
    assert_eq!(state.system_status_dashboard_draft.as_ref(), Some(&dirty));
    assert_eq!(state.app.active_system_status_dashboard(), None);
    assert!(
        state
            .system_status_dashboard_feedback
            .as_deref()
            .unwrap_or_default()
            .contains("Storage unavailable")
    );
}

#[test]
fn system_status_permissions_filter_without_mutating_persisted_catalog() {
    let persisted = storage::SystemStatusDashboardConfig::for_role("Admin");
    let mut user = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut user, UserRole::User);
    user.app.dispatch_at(
        app::AppCommand::SetActiveSystemStatusDashboard(Some(persisted.clone())),
        Instant::now(),
    );
    assert!(!user.system_status_picker_kind_enabled(storage::SystemStatusWidgetKind::TopProcesses));
    for kind in [
        storage::SystemStatusWidgetKind::Storage,
        storage::SystemStatusWidgetKind::Network,
        storage::SystemStatusWidgetKind::TopProcesses,
    ] {
        user.open_system_status_detail(kind);
        assert_eq!(user.system_status_route, ui::SystemStatusRoute::Dashboard);
        assert!(
            user.system_status_dashboard_feedback
                .as_deref()
                .unwrap_or_default()
                .contains("Administrator")
        );
    }
    assert!(
        user.app
            .active_system_status_dashboard()
            .unwrap()
            .widgets
            .contains(&storage::SystemStatusWidgetKind::TopProcesses)
    );

    set_test_auth_role(&mut user, UserRole::Admin);
    for (kind, detail) in [
        (
            storage::SystemStatusWidgetKind::Storage,
            ui::SystemStatusDetail::Storage,
        ),
        (
            storage::SystemStatusWidgetKind::Network,
            ui::SystemStatusDetail::Network,
        ),
        (
            storage::SystemStatusWidgetKind::TopProcesses,
            ui::SystemStatusDetail::Processes,
        ),
    ] {
        user.back_from_system_status_detail();
        user.open_system_status_detail(kind);
        assert_eq!(
            user.system_status_route,
            ui::SystemStatusRoute::Detail(detail)
        );
    }
}

#[test]
fn system_status_drag_changes_only_active_profile_and_clears_capture() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    state.system_status_selected_widget = Some(storage::SystemStatusWidgetKind::Cpu);
    let inactive = state
        .system_status_dashboard_draft
        .as_ref()
        .unwrap()
        .narrow
        .clone();
    let before = state
        .system_status_dashboard_draft
        .as_ref()
        .unwrap()
        .wide
        .clone();
    let model = state.to_system_status_view_model().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!()
    };
    let layout = ui::system_status_layout(main, &model);
    let card = layout
        .widgets
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::Cpu)
        .unwrap();
    let down = (card.area.x.saturating_add(1), card.area.y.saturating_add(1));
    let target = (
        down.0,
        down.1
            .saturating_add(6)
            .min(layout.canvas.bottom().saturating_sub(1)),
    );
    state.begin_system_status_widget_drag(ui::SystemStatusWidgetKind::Cpu, down);
    assert!(state.system_status_widget_drag.is_some());
    state.update_system_status_widget_drag(target);
    state.finish_system_status_widget_drag(target);
    assert!(state.system_status_widget_drag.is_none());
    let draft = state.system_status_dashboard_draft.as_ref().unwrap();
    assert_ne!(draft.wide, before);
    assert_eq!(draft.narrow, inactive);
    let rendered = state.to_system_status_view_model().unwrap();
    let layout = ui::system_status_layout(main, &rendered);
    for (index, left) in layout.widgets.iter().filter(|w| !w.preview).enumerate() {
        for right in layout.widgets.iter().filter(|w| !w.preview).skip(index + 1) {
            assert!(
                left.area.intersection(right.area).is_empty(),
                "resolved placements do not overlap"
            );
        }
    }
}

#[test]
fn system_status_save_persists_dashboard_to_user_record() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tundra-shell-dashboard-save-{}-{unique}",
        std::process::id()
    ));
    let _guard = SystemStatusTempGuard(root.clone());
    let paths = platform::build_linux_app_paths(
        root.join("Config"),
        root.join("Data"),
        root.join("Cache"),
        root.join("State"),
        root.join("Temp"),
    )
    .unwrap();
    let manager = StorageManager::open(paths).unwrap().manager;
    UserService::new(manager.clone())
        .bootstrap_admin("DashboardAdmin", "StrongPass123")
        .unwrap();
    let login = SessionService::new(manager.clone())
        .login("DashboardAdmin", "StrongPass123")
        .unwrap();
    let mut startup = ShellStartupState::clean(
        PlatformKind::Linux,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(manager.clone());
    let mut state =
        ShellSession::new_with_startup(ShellLaunchConfig::default(), (120, 40), startup);
    state.complete_login(login);
    let loaded = state
        .app
        .active_system_status_dashboard()
        .cloned()
        .expect("login loads dashboard");
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    let index = super::controller::system_status::SYSTEM_STATUS_WIDGET_KINDS
        .iter()
        .position(|kind| *kind == storage::SystemStatusWidgetKind::Battery)
        .unwrap();
    state.open_system_status_add_picker();
    state.select_system_status_picker_item(index);
    state.apply_input(InputEvent::from_key_label("Enter"));
    let expected = state.system_status_dashboard_draft.clone().unwrap();
    assert_ne!(expected, loaded);
    state.save_system_status_dashboard();
    assert!(state.system_status_dashboard_draft.is_none());
    assert_eq!(state.app.active_system_status_dashboard(), Some(&expected));
    let persisted = manager
        .load_users()
        .unwrap()
        .users
        .into_iter()
        .find(|user| user.username == "DashboardAdmin")
        .unwrap()
        .system_status_dashboard;
    assert_eq!(persisted, expected);
}

#[test]
fn diagnostics_rejects_guest_even_with_system_status_parent() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Guest);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;

    state.open_diagnostics();

    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(state.status(), "Open Diagnostics from System Status");
}

#[test]
fn system_status_admin_fallback_and_user_stale_unavailable_are_desensitized() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let snapshot = system_status_test_snapshot(
        1,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::FixedVolumeFallback,
    );
    state.apply_system_status_snapshot(snapshot.clone());
    let admin = state.to_system_status_view_model().unwrap();
    let ui::SystemStatusContentViewModel::Admin(admin) = admin.content else {
        panic!()
    };
    assert!(
        admin
            .overview
            .system_volume_usage
            .contains("fixed-volume fallback; source unknown")
    );
    assert_eq!(admin.overview.system_volume_used_percentage, Some(90));
    assert_eq!(admin.overview.network_status, "Connected");
    assert_eq!(
        admin.storage_rows[0].system_volume,
        "Fallback (source unknown)"
    );

    set_test_auth_role(&mut state, UserRole::User);
    let mut stale = snapshot;
    let storage = match stale.storage {
        system_services::StorageState::Ready(value) => value,
        _ => unreachable!(),
    };
    let network = match stale.network {
        system_services::NetworkState::Ready(value) => value,
        _ => unreachable!(),
    };
    stale.storage = system_services::StorageState::Stale {
        last_good: storage,
        error: "/sensitive/mount secret-label".into(),
    };
    stale.network = system_services::NetworkState::Stale {
        last_good: network,
        error: "secret-iface secret-display 192.0.2.99 2001:db8::99".into(),
    };
    state.apply_system_status_snapshot(stale);
    let model = state.to_system_status_view_model().unwrap();
    let debug = format!("{model:?}");
    assert!(debug.contains("(stale)"));
    for secret in [
        "/sensitive/mount",
        "secret-label",
        "secret-iface",
        "secret-display",
        "192.0.2.99",
        "2001:db8::99",
    ] {
        assert!(!debug.contains(secret), "leaked {secret}");
    }
    let mut unavailable = system_status_test_snapshot(
        2,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    );
    unavailable.storage = system_services::StorageState::Unavailable {
        reason: "/sensitive/mount".into(),
    };
    unavailable.network = system_services::NetworkState::Unavailable {
        reason: "secret-iface".into(),
    };
    state.apply_system_status_snapshot(unavailable);
    let debug = format!("{:?}", state.to_system_status_view_model().unwrap());
    assert!(debug.contains("Unavailable"));
    assert!(!debug.contains("sensitive"));
    assert!(!debug.contains("secret-iface"));
}

#[test]
fn system_status_alert_dedupe_upgrade_recovery_and_network_baseline() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.apply_system_status_snapshot(system_status_test_snapshot(
        1,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(
        state.system_status_storage_alerts.get("/sensitive/mount"),
        Some(&SystemStatusAlertLevel::Low)
    );
    assert!(
        state
            .app
            .notification_center()
            .alert()
            .unwrap()
            .contains("/sensitive/mount")
    );
    assert!(
        state
            .app
            .notification_center()
            .alert()
            .unwrap()
            .contains("100 B")
    );
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    assert_eq!(state.app.notification_center().alert_count(), 1);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    assert!(
        state
            .app
            .notification_center()
            .alert_message_for_key("system-status.storage:/sensitive/mount")
            .is_some()
    );
    let repeated = state.app.notification_center().alert().map(str::to_string);
    state.apply_system_status_snapshot(system_status_test_snapshot(
        2,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(state.app.notification_center().alert(), repeated.as_deref());
    assert_eq!(state.app.notification_center().alert_count(), 1);
    assert_eq!(
        state.system_status_storage_alerts.get("/sensitive/mount"),
        Some(&SystemStatusAlertLevel::Low)
    );
    state.apply_system_status_snapshot(system_status_test_snapshot(
        3,
        system_services::StoragePressure::Critical,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(
        state.system_status_storage_alerts.get("/sensitive/mount"),
        Some(&SystemStatusAlertLevel::Critical)
    );
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Critical)
    );
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert!(state.system_status_disconnected_notified);
    state.apply_system_status_snapshot(system_status_test_snapshot(
        4,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    assert!(state.system_status_storage_alerts.is_empty());
    assert!(!state.system_status_disconnected_notified);
    assert!(state.app.notification_center().alert().is_none());
    assert_eq!(state.app.notification_center().alert_count(), 0);
    state.apply_system_status_snapshot(system_status_test_snapshot(
        5,
        system_services::StoragePressure::Low,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(
        state.system_status_storage_alerts.get("/sensitive/mount"),
        Some(&SystemStatusAlertLevel::Low)
    );
    assert!(state.system_status_disconnected_notified);
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    state.app.dispatch_at(
        app::AppCommand::SetSystemStatusSnapshot(None),
        Instant::now(),
    );
    state.complete_login(AuthSession {
        session_id: "next-admin-session".into(),
        user_id: "next-admin".into(),
        username: "next-admin".into(),
        role: UserRole::Admin,
        started_at_epoch_ms: 2,
    });
    assert!(state.system_status_network_baseline.is_none());
    assert!(state.system_status_storage_alerts.is_empty());
    assert_eq!(state.app.notification_center().alert_count(), 0);

    state.complete_login(AuthSession {
        session_id: "user-session".into(),
        user_id: "user-id".into(),
        username: "user".into(),
        role: UserRole::User,
        started_at_epoch_ms: 3,
    });
    state.apply_system_status_snapshot(system_status_test_snapshot(
        10,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    let storage_key = "system-status.storage:/sensitive/mount";
    let storage_message = state
        .app
        .notification_center()
        .alert_message_for_key(storage_key)
        .unwrap();
    assert!(storage_message.contains("Device storage"));
    for secret in [
        "/sensitive/mount",
        "secret-label",
        "secret-iface",
        "secret-display",
        "192.0.2.99",
        "2001:db8::99",
    ] {
        assert!(!storage_message.contains(secret));
    }
    assert_eq!(state.app.notification_center().alert_count(), 1);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    state.apply_system_status_snapshot(system_status_test_snapshot(
        11,
        system_services::StoragePressure::Low,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    let network_message = state
        .app
        .notification_center()
        .alert_message_for_key("system-status.network")
        .unwrap();
    assert_eq!(network_message, "Network connection was lost");
    for secret in [
        "/sensitive/mount",
        "secret-label",
        "secret-iface",
        "secret-display",
        "192.0.2.99",
        "2001:db8::99",
    ] {
        assert!(!network_message.contains(secret));
    }
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    state.apply_system_status_snapshot(system_status_test_snapshot(
        12,
        system_services::StoragePressure::Low,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    state.apply_system_status_snapshot(system_status_test_snapshot(
        13,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    assert!(
        state
            .app
            .notification_center()
            .alert_message_for_key("system-status.network")
            .is_none()
    );
    assert_eq!(state.app.notification_center().alert_count(), 1);
    state.apply_system_status_snapshot(system_status_test_snapshot(
        14,
        system_services::StoragePressure::Low,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    assert!(
        state
            .app
            .notification_center()
            .alert_message_for_key("system-status.network")
            .is_some()
    );
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
}

#[test]
fn system_status_focus_clock_roundtrip_and_close_restore_home() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.refresh_hit_map();
    assert_eq!(state.focus_order(), vec![ShellComponent::SystemStatus]);
    state.open_clock();
    state.close_clock();
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(state.focused_component(), ShellComponent::SystemStatus);
    state.close_system_status();
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.focused_component(), ShellComponent::Home);
}

#[test]
fn system_status_mouse_wheel_and_scrollbar_drag_update_explicit_viewport() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (80, 16),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let mut snapshot = system_status_test_snapshot(
        1,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    );
    let system_services::StorageState::Ready(ref mut storage) = snapshot.storage else {
        unreachable!()
    };
    let template = storage.volumes[0].clone();
    storage.volumes = (0..30)
        .map(|index| system_services::StorageVolumeSnapshot {
            identifier: format!("volume-{index}"),
            is_system: index == 0,
            ..template.clone()
        })
        .collect();
    state.apply_system_status_snapshot(snapshot);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.set_system_status_tab(ui::SystemStatusTab::Storage);
    state.refresh_hit_map();
    state.apply_input(InputEvent::from_key_label("End"));
    let model = state.to_system_status_view_model().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 80, 16))
    else {
        panic!()
    };
    let keyboard_layout = ui::system_status_layout(main, &model);
    assert!(keyboard_layout.visible_start > 1);
    assert_eq!(state.system_status_scroll_offset, 0);
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        10,
        8,
        ui::MouseEventKind::Scroll(ScrollDirection::Up),
    )));
    let after_wheel_model = state.to_system_status_view_model().unwrap();
    let after_wheel_layout = ui::system_status_layout(main, &after_wheel_model);
    assert_eq!(
        after_wheel_layout.visible_start,
        keyboard_layout.visible_start.saturating_sub(1)
    );
    assert!(state.system_status_selected_row >= after_wheel_layout.visible_start);
    assert!(
        state.system_status_selected_row
            < after_wheel_layout.visible_start + after_wheel_layout.visible_capacity
    );
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        10,
        8,
        ui::MouseEventKind::Scroll(ScrollDirection::Down),
    )));
    assert!(state.system_status_scroll_offset > 0);
    let model = state.to_system_status_view_model().unwrap();
    let layout = ui::system_status_layout(main, &model);
    let track = layout.scrollbar.expect("scrollbar");
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        track.x,
        track.bottom().saturating_sub(1),
        ui::MouseEventKind::Down(PointerButton::Left),
    )));
    assert!(matches!(
        state.scrollbar_drag,
        Some(ScrollbarDragState::SystemStatus { .. })
    ));
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        track.x,
        track.bottom().saturating_sub(1),
        ui::MouseEventKind::Drag(PointerButton::Left),
    )));
    assert!(state.system_status_scroll_offset > 1);
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        track.x,
        track.bottom().saturating_sub(1),
        ui::MouseEventKind::Up(PointerButton::Left),
    )));
    assert!(state.scrollbar_drag.is_none());
}

#[test]
fn system_status_modal_focus_traps_and_restores_page_focus() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack.push(ShellScreen::SystemStatus);
    state.focused_component = ShellComponent::SystemStatus;
    state.notify_modal(
        "Confirm",
        "Modal over status",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );
    state.refresh_hit_map();
    assert_eq!(
        state.focus_order(),
        vec![ShellComponent::NotificationDialog]
    );
    assert_eq!(state.focused_component, ShellComponent::NotificationDialog);
    state.move_focus(ui::FocusDirection::Next);
    assert_eq!(state.focused_component, ShellComponent::NotificationDialog);
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.refresh_hit_map();
    assert_eq!(state.focus_order(), vec![ShellComponent::SystemStatus]);
    assert_eq!(state.focused_component, ShellComponent::SystemStatus);

    state.apply_time_sync_failure_for_test("offline");
    state.refresh_hit_map();
    assert_eq!(state.focus_order(), vec![ShellComponent::TimeSyncDialog]);
    state.move_focus(ui::FocusDirection::Previous);
    assert_eq!(state.focused_component, ShellComponent::TimeSyncDialog);
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.refresh_hit_map();
    assert_eq!(state.focused_component, ShellComponent::SystemStatus);
}

#[test]
fn system_status_runtime_watch_drain_applies_revision_and_completes_refresh() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let initial = system_status_test_snapshot(
        4,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    );
    state.apply_system_status_snapshot(initial.clone());
    state.system_status_refresh_requested_revision = Some(4);
    let system = |snapshot: app::AppSystemStatusSnapshot| system_services::SystemSnapshot {
        revision: snapshot.revision,
        observed_at: snapshot.observed_at,
        weather: system_services::WeatherState::Loading,
        time: system_services::TimeState::Local {
            local_time: snapshot.observed_at.fixed_offset(),
        },
        storage: snapshot.storage,
        network: snapshot.network,
        metrics: snapshot.metrics,
    };
    let (sender, mut receiver) = tokio::sync::watch::channel(system(initial));
    assert!(!drain_system_status_snapshot(&mut receiver, &mut state));
    let updated = system_status_test_snapshot(
        5,
        system_services::StoragePressure::Critical,
        false,
        system_services::SystemVolumeSource::Detected,
    );
    sender.send(system(updated)).unwrap();
    assert!(drain_system_status_snapshot(&mut receiver, &mut state));
    assert_eq!(state.app.system_status_snapshot().unwrap().revision, 5);
    assert!(state.system_status_refresh_requested_revision.is_none());
}

#[test]
fn system_status_live_service_home_open_refresh_and_background_close() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tundra-shell-system-status-live-{}-{unique}",
        std::process::id(),
    ));
    let _temp_guard = SystemStatusTempGuard(root.clone());
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("UserData"),
    )
    .unwrap();
    let app_paths = platform::build_linux_app_paths(
        root.join("Config"),
        root.join("Data"),
        root.join("Cache"),
        root.join("State"),
        root.join("Temp"),
    )
    .unwrap();
    let opened_storage = StorageManager::open(app_paths.clone()).unwrap();
    let manager = opened_storage.manager.clone();
    UserService::new(manager.clone())
        .bootstrap_admin("StatusAdmin", "StrongPass123")
        .unwrap();
    let admin_session = SessionService::new(manager.clone())
        .login("StatusAdmin", "StrongPass123")
        .unwrap();
    let platform = Arc::new(platform::mock::MockPlatform::new(user_dirs, app_paths));
    platform.set_local_volumes_result(Ok(vec![platform::LocalVolume {
        root: PathBuf::from("/live-system"),
        label: Some("Live".into()),
        kind: platform::VolumeKind::Fixed,
        total_bytes: Some(10 * 1024_u64.pow(3)),
        available_bytes: Some(6 * 1024_u64.pow(3)),
        is_system: true,
        access: platform::VolumeAccess::ReadWrite,
    }]));
    let watchdog = default_editor_watchdog()
        .expect("watchdog")
        .child_component(watchdog::ComponentId::new("system-status-live-test").unwrap());
    let config = system_services::SystemServicesConfig::default();
    let (handle, mut receiver) =
        system_services::SystemServicesRuntime::start_with_platform_and_provider(
            config.clone(),
            watchdog.clone(),
            platform.clone(),
            Arc::new(SystemStatusTestWeatherProvider),
        );
    let _service_guard = SystemStatusServiceGuard(Some(handle.clone()));
    let mut startup = ShellStartupState::clean(
        PlatformKind::Linux,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(manager.clone());
    let mut state = ShellSession::new_with_runtime_services(
        ShellLaunchConfig::default(),
        (120, 40),
        startup,
        ui::RuntimeAsciiAssets::load_default().unwrap(),
        ShellRuntimeServices {
            explorer: None,
            diagnostics: None,
            editor: ShellEditorTaskRuntime::unavailable(),
            settings: ShellSettingsTaskRuntime::new_managed_with_system_services(
                watchdog,
                Some(handle),
                config,
            ),
        },
    );
    state.complete_login(admin_session);
    state.screen_stack = vec![ShellScreen::Home];
    state.focused_component = ShellComponent::Home;
    let index = state
        .user_home_entries()
        .iter()
        .position(|entry| entry.label == "System Status")
        .unwrap();
    state.select_home_entry(index);
    let baseline_revision = receiver.borrow_and_update().revision;
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                receiver.changed().await.unwrap();
                apply_current_system_status_snapshot(&mut receiver, &mut state);
                if state
                    .app
                    .system_status_snapshot()
                    .is_some_and(|snapshot| snapshot.revision > baseline_revision)
                {
                    break;
                }
            }
        })
        .await
        .expect("active status sample timed out");
    });
    let opened_revision = state
        .app
        .system_status_snapshot()
        .expect("active sample")
        .revision;
    state.apply_input(InputEvent::from_key_label("R"));
    assert_eq!(
        state.system_status_refresh_requested_revision,
        Some(opened_revision)
    );
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            while state.system_status_refresh_requested_revision.is_some() {
                receiver.changed().await.unwrap();
                apply_current_system_status_snapshot(&mut receiver, &mut state);
            }
        })
        .await
        .expect("refreshed status sample timed out");
    });
    assert!(state.system_status_refresh_requested_revision.is_none());
    assert!(state.app.system_status_snapshot().unwrap().revision > opened_revision);
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(state.active_screen(), ShellScreen::Home);

    state.open_settings();
    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_category,
        ui::SettingsCategory::System
    );
    assert!(
        matches!(&state.app.system_status_snapshot().unwrap().storage, system_services::StorageState::Ready(storage) if storage.overall_pressure == system_services::StoragePressure::Normal)
    );
    state.apply_input(InputEvent::from_key_label("Right"));
    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .system_status
            .low_available_gib,
        6
    );
    let changed_from = state.app.system_status_snapshot().unwrap().revision;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop { receiver.changed().await.unwrap(); apply_current_system_status_snapshot(&mut receiver, &mut state); if state.app.system_status_snapshot().is_some_and(|snapshot| snapshot.revision > changed_from && matches!(&snapshot.storage, system_services::StorageState::Ready(storage) if storage.overall_pressure == system_services::StoragePressure::Low)) { break; } }
        }).await.expect("threshold reconfigure sample timed out");
    });
    state.request_settings_restore_defaults();
    state.apply_input(InputEvent::from_key_label("R"));
    assert_eq!(
        manager.load_config().unwrap().system_status,
        storage::SystemStatusConfig::default()
    );
    let restored_from = state.app.system_status_snapshot().unwrap().revision;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop { receiver.changed().await.unwrap(); apply_current_system_status_snapshot(&mut receiver, &mut state); if state.app.system_status_snapshot().is_some_and(|snapshot| snapshot.revision > restored_from && matches!(&snapshot.storage, system_services::StorageState::Ready(storage) if storage.overall_pressure == system_services::StoragePressure::Normal)) { break; } }
        }).await.expect("restored threshold sample timed out");
    });
    drop(state);
}

#[test]
fn shell_and_lockscreen_share_the_same_session_recovery_budget() {
    let now = Instant::now();
    let mut recoveries = VecDeque::new();

    assert!(reserve_session_recovery(&mut recoveries, now));
    assert!(reserve_session_recovery(&mut recoveries, now));
    assert!(!reserve_session_recovery(&mut recoveries, now));
    assert_eq!(recoveries.len(), MAX_SESSION_RECOVERIES);
}

#[test]
fn session_recovery_budget_resets_after_the_crash_loop_window() {
    let now = Instant::now();
    let mut recoveries = VecDeque::from([now, now]);
    let after_window = now + SESSION_RECOVERY_WINDOW + Duration::from_millis(1);

    assert!(reserve_session_recovery(&mut recoveries, after_window));
    assert_eq!(recoveries, VecDeque::from([after_window]));
}

#[test]
fn critical_modal_preempts_and_then_restores_the_previous_modal() {
    let mut center = NotificationCenter::new("Ready");
    center.push_modal(ShellNotification::modal(
        "Normal confirmation",
        "normal",
        ui::NotificationTone::Warning,
        vec![ShellNotificationAction::new("ok", "OK")],
    ));
    center.push_critical_modal(ShellNotification::modal(
        "Recovered from panic",
        "critical",
        ui::NotificationTone::Critical,
        vec![ShellNotificationAction::new("continue", "Continue")],
    ));

    assert_eq!(
        center.active_modal_view_model().unwrap().title,
        "Recovered from panic"
    );
    center.activate_selected_action();
    assert_eq!(
        center.active_modal_view_model().unwrap().title,
        "Normal confirmation"
    );
}

#[test]
fn exit_confirmation_keeps_login_as_the_content_screen() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    state.screen_stack = vec![ShellScreen::Login];
    state.focused_component = ShellComponent::LoginUserList;
    state.refresh_hit_map();

    let action = state.apply_input(InputEvent::from_key_label("Esc"));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert_eq!(state.content_screen(), ShellScreen::Login);
    assert_eq!(
        state.to_shell_chrome_view_model().display_mode,
        ui::HomeDisplayMode::Auth
    );
    assert!(
        state
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::LoginUserList)
    );
    assert!(
        state
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::ExitDialog)
    );

    state.apply_input(InputEvent::from_key_label("Esc"));

    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert_eq!(state.content_screen(), ShellScreen::Login);
    assert_eq!(state.focused_component(), ShellComponent::LoginUserList);
}

#[test]
fn motion_progress_drives_page_and_overlay_hit_regions() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let transition = |kind, progress| ui::MotionTransition {
        kind,
        direction: ui::MotionDirection::Entering,
        progress,
        phase_progress: progress,
        active: progress < 1_000,
        next_redraw_in: Duration::from_millis(16),
    };

    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        screen: Some(transition(ui::MotionTransitionKind::Page, 0)),
        ..ui::MotionTransitions::default()
    });
    let shifted_top = state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == ShellComponent::TopBar)
        .expect("shifted top bar");
    assert_eq!(shifted_top.area.y, 1);

    state.apply_input(InputEvent::from_key_label("Esc"));
    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(transition(ui::MotionTransitionKind::Dialog, 0)),
        ..ui::MotionTransitions::default()
    });
    assert!(
        state
            .hit_map()
            .regions()
            .iter()
            .all(|region| region.component != ShellComponent::ExitDialog),
        "a partially revealed dialog must not expose stale mouse targets"
    );

    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(transition(ui::MotionTransitionKind::Dialog, 500)),
        ..ui::MotionTransitions::default()
    });
    assert!(
        state
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::ExitDialog)
    );
}

#[test]
fn entering_overlays_gate_keyboard_focus_and_popup_activation_until_ready() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(
        state
            .active_overlay_descriptor()
            .map(|overlay| overlay.category),
        Some(ShellOverlayCategory::ShellModal)
    );
    let transition = |progress| ui::MotionTransition {
        kind: ui::MotionTransitionKind::Dialog,
        direction: ui::MotionDirection::Entering,
        progress,
        phase_progress: progress,
        active: progress < 1_000,
        next_redraw_in: Duration::from_millis(16),
    };
    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(transition(499)),
        ..ui::MotionTransitions::default()
    });
    assert_ne!(state.focus_order(), vec![ShellComponent::ExitDialog]);
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::Noop
    );
    assert_ne!(
        state.route_key_input(&KeyInput::from_label("Esc")).1,
        ShellCommand::Noop
    );

    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(transition(500)),
        ..ui::MotionTransitions::default()
    });
    assert_eq!(state.focus_order(), vec![ShellComponent::ExitDialog]);
    assert_ne!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::Noop
    );

    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Home),
        anchor: (10, 10),
    });
    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(ui::MotionTransition {
            kind: ui::MotionTransitionKind::Popover,
            ..transition(499)
        }),
        ..ui::MotionTransitions::default()
    });
    assert_ne!(state.focus_order(), vec![ShellComponent::ContextMenu]);
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::Noop
    );
    assert_eq!(
        state
            .route_input_at(
                InputEvent::mouse_down(PointerButton::Left, (0, 0)),
                Instant::now(),
            )
            .command,
        ShellCommand::CaptureOverlayInput
    );
    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(ui::MotionTransition {
            kind: ui::MotionTransitionKind::Popover,
            ..transition(500)
        }),
        ..ui::MotionTransitions::default()
    });
    assert_eq!(state.focus_order(), vec![ShellComponent::ContextMenu]);
    assert_ne!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::Noop
    );
}

#[test]
fn overlay_resolver_categories_share_readiness_input_focus_and_hit_regions() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.screen_stack = vec![ShellScreen::Clock];
    state.clock_create_state = Some(ClockCreateState::default());
    assert_eq!(
        state
            .active_overlay_descriptor()
            .map(|overlay| overlay.category),
        Some(ShellOverlayCategory::PageDialog)
    );
    let entering = ui::MotionTransition {
        kind: ui::MotionTransitionKind::Dialog,
        direction: ui::MotionDirection::Entering,
        progress: 499,
        phase_progress: 499,
        active: true,
        next_redraw_in: Duration::from_millis(16),
    };
    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(entering),
        ..ui::MotionTransitions::default()
    });
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::Noop
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Esc")).1,
        ShellCommand::ClockCloseCreate
    );
    assert!(
        !state
            .focus_order()
            .contains(&ShellComponent::ClockCreateInput)
    );

    let mut state = explorer_routing_test_state();
    state.explorer_overlay_mode = Some(ExplorerOverlayMode::Options);
    assert_eq!(
        state
            .active_overlay_descriptor()
            .map(|overlay| overlay.category),
        Some(ShellOverlayCategory::PagePopover)
    );
    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (10, 10),
    });
    assert_eq!(
        state
            .active_overlay_descriptor()
            .map(|overlay| overlay.category),
        Some(ShellOverlayCategory::PagePopover)
    );

    state.active_popup = None;
    state.explorer_overlay_mode = None;
    state.screen_stack = vec![ShellScreen::Home];
    state.notify_toast("Saved");
    let toast = state.active_overlay_descriptor().expect("toast descriptor");
    assert_eq!(toast.category, ShellOverlayCategory::Toast);
    assert!(toast.target.is_none());
    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(ui::MotionTransition {
            kind: ui::MotionTransitionKind::Toast,
            ..entering
        }),
        ..ui::MotionTransitions::default()
    });
    assert!(state.overlay_interaction_ready);
    assert_ne!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::Noop
    );
}

#[test]
fn setup_clock_and_diagnostics_publish_exact_overlay_surfaces_while_gated() {
    let gated = ui::MotionTransitions {
        overlay: Some(ui::MotionTransition {
            kind: ui::MotionTransitionKind::Dialog,
            direction: ui::MotionDirection::Entering,
            progress: 100,
            phase_progress: 100,
            active: true,
            next_redraw_in: Duration::from_millis(16),
        }),
        ..ui::MotionTransitions::default()
    };
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let terminal = Rect::new(0, 0, 120, 40);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(terminal) else {
        panic!("expected full layout");
    };

    state.screen_stack = vec![ShellScreen::FirstRunSetup];
    state.setup_custom_color_target = Some(ui::SetupCustomColorTarget::Theme);
    let setup_area = ui::setup_custom_color_dialog_area(main);
    for motion in [ui::MotionTransitions::default(), gated] {
        state.refresh_hit_map_with_motion(motion);
        assert_eq!(
            overlay_areas_for(&state, ShellComponent::SetupCustomColorDialog),
            vec![setup_area]
        );
    }

    state.setup_custom_color_target = None;
    state.screen_stack = vec![ShellScreen::Clock];
    state.clock_create_state = Some(ClockCreateState::default());
    let clock_model = state.to_clock_view_model();
    let clock_layout = ui::clock_page_layout(main, &clock_model)
        .create_dialog
        .expect("create dialog");
    state.refresh_hit_map();
    assert_eq!(
        overlay_areas_for(&state, ShellComponent::ClockCreateDialog),
        vec![clock_layout.dialog]
    );
    assert!(
        state
            .focus_order()
            .contains(&ShellComponent::ClockCreateInput)
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Tab")).1,
        ShellCommand::ClockCreateFocusNext
    );
    state.refresh_hit_map_with_motion(gated);
    assert_eq!(
        overlay_areas_for(&state, ShellComponent::ClockCreateDialog),
        vec![clock_layout.dialog]
    );
    assert!(
        !state
            .focus_order()
            .contains(&ShellComponent::ClockCreateInput)
    );

    state.clock_create_state = None;
    state.screen_stack = vec![ShellScreen::Diagnostics];
    state.diagnostics_repair_preview =
        vec![app::diagnostics::DiagnosticsRepairAction::CreateDirectory {
            label: "Data".into(),
            path: std::path::PathBuf::from("/private/example/data"),
        }];
    let diagnostics_model = state.to_diagnostics_view_model();
    let diagnostics_area = ui::diagnostics_layout(main, &diagnostics_model)
        .repair_dialog
        .expect("repair dialog")
        .dialog;
    for motion in [ui::MotionTransitions::default(), gated] {
        state.refresh_hit_map_with_motion(motion);
        assert_eq!(
            overlay_areas_for(&state, ShellComponent::DiagnosticsRepairDialog),
            vec![diagnostics_area]
        );
    }

    state.screen_stack = vec![ShellScreen::SystemStatus];
    let system_dialog = state
        .to_diagnostics_view_model()
        .repair_dialog
        .expect("system-status repair dialog");
    let system_area = ui::diagnostics_repair_dialog_layout(main, &system_dialog).dialog;
    for motion in [ui::MotionTransitions::default(), gated] {
        state.refresh_hit_map_with_motion(motion);
        assert_eq!(
            overlay_areas_for(&state, ShellComponent::DiagnosticsRepairDialog),
            vec![system_area]
        );
    }
}

#[test]
fn rendered_overlay_resolver_ids_are_variant_stable_and_precedence_is_preserved() {
    let mut launcher = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    launcher.screen_stack = vec![ShellScreen::Launcher];
    launcher.launcher_pending_confirmation = Some(LauncherPendingConfirmation::Launch {
        id: "app-1".into(),
        path: "/first/display/path".into(),
        kind: LauncherExecutableKind::NativeBinary,
    });
    let launch = launcher
        .active_overlay_descriptor()
        .expect("launch confirm");
    assert_eq!(launch.kind, ui::MotionOverlayKind::Dialog);
    assert_eq!(launch.category, ShellOverlayCategory::PageDialog);
    assert_eq!(
        launch.target,
        Some(RoutedTarget::Component(ShellComponent::Launcher))
    );
    launcher.launcher_pending_confirmation = Some(LauncherPendingConfirmation::Launch {
        id: "app-1".into(),
        path: "/changed/display/path".into(),
        kind: LauncherExecutableKind::NativeBinary,
    });
    assert_eq!(
        launcher
            .active_overlay_descriptor()
            .expect("stable launch")
            .id,
        launch.id
    );

    launcher.launcher_pending_confirmation = Some(LauncherPendingConfirmation::Remove {
        ids: vec!["app-1".into()],
        label: "First label".into(),
    });
    let remove = launcher
        .active_overlay_descriptor()
        .expect("remove confirm");
    launcher.launcher_pending_confirmation = Some(LauncherPendingConfirmation::Remove {
        ids: vec!["app-1".into()],
        label: "Changed label".into(),
    });
    assert_eq!(
        launcher
            .active_overlay_descriptor()
            .expect("stable remove")
            .id,
        remove.id
    );
    assert_ne!(launch.id, remove.id);

    launcher.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Launcher),
        anchor: (2, 2),
    });
    assert_eq!(
        launcher
            .active_overlay_descriptor()
            .expect("popup precedence")
            .category,
        ShellOverlayCategory::ContextPopup
    );
    launcher.notify_modal(
        "Priority",
        "Shell modal wins",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );
    assert_eq!(
        launcher
            .active_overlay_descriptor()
            .expect("modal precedence")
            .category,
        ShellOverlayCategory::ShellModal
    );

    let mut editor = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    editor.screen_stack = vec![ShellScreen::Editor];
    let mut menu_ids = Vec::new();
    for menu in [
        ui::EditorMenu::File,
        ui::EditorMenu::Edit,
        ui::EditorMenu::Insert,
        ui::EditorMenu::Format,
        ui::EditorMenu::View,
        ui::EditorMenu::Settings,
    ] {
        editor.editor_open_menu = Some(menu);
        let overlay = editor.active_overlay_descriptor().expect("editor menu");
        assert_eq!(overlay.kind, ui::MotionOverlayKind::Popover);
        assert_eq!(overlay.category, ShellOverlayCategory::PagePopover);
        assert_eq!(
            overlay.target,
            Some(RoutedTarget::Component(ShellComponent::Editor))
        );
        menu_ids.push(overlay.id);
    }
    menu_ids.sort();
    menu_ids.dedup();
    assert_eq!(menu_ids.len(), 6);
    editor.editor_open_menu = None;
    editor.editor_quick_menu_anchor = Some((4, 5));
    let quick = editor.active_overlay_descriptor().expect("quick menu");
    editor.editor_quick_menu_anchor = Some((40, 15));
    assert_eq!(
        editor
            .active_overlay_descriptor()
            .expect("moved quick menu")
            .id,
        quick.id
    );

    let mut users = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    users.screen_stack = vec![ShellScreen::UserManagement];
    users.user_management_mode = UserManagementMode::Create(UserManagementCreateForm {
        username: "first".into(),
        display_name: "First".into(),
        password: "secret".into(),
        role: UserRole::User,
        focused_field: UserManagementFormField::Username,
    });
    let create = users.active_overlay_descriptor().expect("create user");
    if let UserManagementMode::Create(form) = &mut users.user_management_mode {
        form.username = "typing".into();
    }
    assert_eq!(
        users.active_overlay_descriptor().expect("stable create").id,
        create.id
    );
    users.user_management_mode = UserManagementMode::EditInfo(UserManagementInfoForm {
        username: "user".into(),
        display_name: "Display".into(),
        focused_field: UserManagementFormField::DisplayName,
    });
    let edit = users.active_overlay_descriptor().expect("edit user");
    users.user_management_mode = UserManagementMode::Password(UserManagementPasswordForm {
        username: "user".into(),
        password: "typed".into(),
        focused_field: UserManagementFormField::Password,
    });
    let password = users.active_overlay_descriptor().expect("password user");
    for overlay in [&create, &edit, &password] {
        assert_eq!(overlay.kind, ui::MotionOverlayKind::Dialog);
        assert_eq!(overlay.category, ShellOverlayCategory::PageDialog);
        assert_eq!(
            overlay.target,
            Some(RoutedTarget::Component(ShellComponent::UserManagement))
        );
    }
    assert_ne!(create.id, edit.id);
    assert_ne!(edit.id, password.id);
}

#[test]
fn newly_tracked_overlay_groups_share_keyboard_mouse_focus_and_readiness_gating() {
    let mut launcher = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    launcher.screen_stack = vec![ShellScreen::Launcher];
    launcher.launcher_pending_confirmation = Some(LauncherPendingConfirmation::Launch {
        id: "app-1".into(),
        path: "/app".into(),
        kind: LauncherExecutableKind::NativeBinary,
    });

    let mut editor = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    editor.screen_stack = vec![ShellScreen::Editor];
    editor.editor_open_menu = Some(ui::EditorMenu::File);

    let mut users = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    users.screen_stack = vec![ShellScreen::UserManagement];
    users.user_management_mode = UserManagementMode::Password(UserManagementPasswordForm {
        username: "user".into(),
        password: String::new(),
        focused_field: UserManagementFormField::Password,
    });

    for (state, kind, owner) in [
        (
            &mut launcher,
            ui::MotionTransitionKind::Dialog,
            ShellComponent::Launcher,
        ),
        (
            &mut editor,
            ui::MotionTransitionKind::Popover,
            ShellComponent::Editor,
        ),
        (
            &mut users,
            ui::MotionTransitionKind::Dialog,
            ShellComponent::UserManagement,
        ),
    ] {
        let entering = ui::MotionTransition {
            kind,
            direction: ui::MotionDirection::Entering,
            progress: 0,
            phase_progress: 0,
            active: true,
            next_redraw_in: Duration::from_millis(16),
        };
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(entering),
            ..ui::MotionTransitions::default()
        });
        assert!(!state.overlay_interaction_ready);
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::Noop
        );
        assert_ne!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::Noop
        );
        assert_eq!(state.focus_order(), vec![owner]);
        let has_surface =
            state.hit_map().regions().iter().any(|region| {
                region.layer == ShellHitLayer::AppOverlay && region.component == owner
            });
        if owner == ShellComponent::Editor {
            assert!(!has_surface, "unrendered Editor menu has no layout surface");
        } else {
            assert!(has_surface, "missing overlay surface for {owner:?}");
        }
        assert_eq!(
            state
                .route_input_at(
                    InputEvent::mouse_down(PointerButton::Left, (1, 1)),
                    Instant::now(),
                )
                .command,
            ShellCommand::CaptureOverlayInput
        );

        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(ui::MotionTransition {
                progress: 1_000,
                phase_progress: 1_000,
                active: false,
                next_redraw_in: Duration::ZERO,
                ..entering
            }),
            ..ui::MotionTransitions::default()
        });
        assert!(state.overlay_interaction_ready);
        assert_ne!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::Noop
        );

        state.refresh_hit_map_with_motion(ui::MotionTransitions::default());
        assert!(
            state.overlay_interaction_ready,
            "Reduced/inactive motion settles ready"
        );
    }
}

#[test]
fn rendered_editor_menu_hit_surface_matches_editor_layout() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(EditorState::untitled(
            app::editor::DocumentKind::PlainText,
        ))),
        Instant::now(),
    );
    state.screen_stack = vec![ShellScreen::Editor];
    state.editor_open_menu = Some(ui::EditorMenu::File);
    state.refresh_hit_map();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!("full layout");
    };
    let expected = ui::editor_layout(main, &state.to_editor_view_model())
        .menu_popup
        .expect("rendered menu popup");
    let surfaces = state
        .hit_map()
        .regions()
        .iter()
        .filter(|region| {
            region.layer == ShellHitLayer::AppOverlay && region.component == ShellComponent::Editor
        })
        .map(|region| region.area)
        .collect::<Vec<_>>();
    assert_eq!(surfaces, vec![expected]);
}

#[test]
fn explorer_dialog_and_transient_overlay_identities_are_semantic_and_stable() {
    let cases = [
        (
            ResolvedExplorerOverlay::RestoreConflict,
            "explorer-restore-conflict",
        ),
        (
            ResolvedExplorerOverlay::OperationConflict,
            "explorer-operation-conflict",
        ),
        (
            ResolvedExplorerOverlay::Input(ExplorerInputMode::NewFolder),
            "explorer-input:new-folder",
        ),
        (
            ResolvedExplorerOverlay::Input(ExplorerInputMode::NewTextFile),
            "explorer-input:new-text-file",
        ),
        (
            ResolvedExplorerOverlay::Input(ExplorerInputMode::Rename),
            "explorer-input:rename",
        ),
        (
            ResolvedExplorerOverlay::Input(ExplorerInputMode::RestoreDestination),
            "explorer-input:restore-destination",
        ),
    ];
    for (resolved, expected) in cases {
        let descriptor = resolved.descriptor();
        assert_eq!(descriptor.id, expected);
        assert_eq!(descriptor.kind, ui::MotionOverlayKind::Dialog);
    }

    let mut state = explorer_routing_test_state();
    state.explorer_input_mode = ExplorerInputMode::NewFolder;
    state.explorer_input = "first typed value".into();
    let input = state
        .active_overlay_descriptor()
        .expect("new folder dialog");
    assert_eq!(input.kind, ui::MotionOverlayKind::Dialog);
    assert_eq!(input.category, ShellOverlayCategory::PageDialog);
    assert_eq!(
        input.target,
        Some(RoutedTarget::Component(ShellComponent::Explorer))
    );
    state.explorer_input = "changed typed value".into();
    assert_eq!(
        state.active_overlay_descriptor().expect("stable input").id,
        input.id
    );
    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (1, 1),
    });
    assert!(matches!(
        state.resolved_explorer_overlay(),
        Some(ResolvedExplorerOverlay::Input(ExplorerInputMode::NewFolder))
    ));
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::SubmitExplorerInput
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Esc")).1,
        ShellCommand::CancelExplorerInput
    );

    state.explorer_input_mode = ExplorerInputMode::Browse;
    state.active_popup = None;
    let mut explorer = state.app.explorer_state().expect("explorer state").clone();
    explorer.pending_conflict = Some(app::explorer::ExplorerConflict {
        source: "/source-a".into(),
        target: "/target-a".into(),
        remaining: 3,
    });
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer.clone())),
        Instant::now(),
    );
    let conflict = state
        .active_overlay_descriptor()
        .expect("operation conflict");
    explorer.pending_conflict = Some(app::explorer::ExplorerConflict {
        source: "/source-b".into(),
        target: "/target-b".into(),
        remaining: 1,
    });
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer)),
        Instant::now(),
    );
    assert_eq!(
        state
            .active_overlay_descriptor()
            .expect("stable conflict")
            .id,
        conflict.id
    );
    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (1, 1),
    });
    assert_eq!(
        state.resolved_explorer_overlay(),
        Some(ResolvedExplorerOverlay::OperationConflict)
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::ExplorerConflictKeepBoth
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("r")).1,
        ShellCommand::ExplorerConflictReplace
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("s")).1,
        ShellCommand::ExplorerConflictSkip
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("a")).1,
        ShellCommand::ExplorerConflictToggleApplyToRemaining
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Esc")).1,
        ShellCommand::ExplorerConflictCancel
    );

    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (1, 1),
    });
    let popup = state.active_overlay_descriptor().expect("popup");
    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (80, 20),
    });
    assert_eq!(
        state.active_overlay_descriptor().expect("moved popup").id,
        popup.id
    );

    state.active_popup = None;
    let mut explorer = state.app.explorer_state().expect("explorer state").clone();
    explorer.pending_conflict = None;
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer)),
        Instant::now(),
    );
    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (5, 5),
    });
    for (mode, expected) in [
        (
            ExplorerOverlayMode::ContextMenu { anchor: (1, 1) },
            "explorer-popover:context-menu",
        ),
        (
            ExplorerOverlayMode::Sort { anchor: (2, 2) },
            "explorer-popover:sort",
        ),
        (ExplorerOverlayMode::Options, "explorer-popover:options"),
        (
            ExplorerOverlayMode::Properties,
            "explorer-popover:properties",
        ),
    ] {
        state.explorer_overlay_mode = Some(mode);
        assert_eq!(
            state
                .active_overlay_descriptor()
                .expect("explorer popover")
                .id,
            expected
        );
        assert_eq!(
            state
                .active_overlay_descriptor()
                .expect("semantic popover")
                .category,
            ShellOverlayCategory::PagePopover
        );
    }
    state.explorer_overlay_mode = Some(ExplorerOverlayMode::ContextMenu { anchor: (90, 30) });
    assert_eq!(
        state
            .active_overlay_descriptor()
            .expect("moved context menu")
            .id,
        "explorer-popover:context-menu"
    );

    let mut explorer = state.app.explorer_state().expect("explorer state").clone();
    explorer.pending_dialog = Some(app::explorer::ExplorerDialog::delete(std::path::Path::new(
        "/first",
    )));
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer.clone())),
        Instant::now(),
    );
    assert_eq!(
        state
            .active_overlay_descriptor()
            .expect("popover hides dialog")
            .id,
        "explorer-popover:context-menu"
    );
    state.explorer_overlay_mode = None;
    let delete = state.active_overlay_descriptor().expect("delete dialog");
    assert_eq!(delete.id, "explorer-dialog:delete-to-trash");
    assert_eq!(delete.kind, ui::MotionOverlayKind::Dialog);
    explorer.pending_dialog = Some(app::explorer::ExplorerDialog::dump_trash(4));
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer)),
        Instant::now(),
    );
    assert_eq!(
        state.active_overlay_descriptor().expect("dump dialog").id,
        "explorer-dialog:dump-trash"
    );

    let mut toast = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let instance = Instant::now();
    toast.app.dispatch_at(
        app::AppCommand::Notification(app::NotificationCommand::ShowToast("First".into())),
        instance,
    );
    let first = toast.active_overlay_descriptor().expect("toast instance");
    toast.app.dispatch_at(
        app::AppCommand::Notification(app::NotificationCommand::ShowToast("Changed".into())),
        instance,
    );
    assert_eq!(
        toast.active_overlay_descriptor().expect("same instance").id,
        first.id
    );
    toast.app.dispatch_at(
        app::AppCommand::Notification(app::NotificationCommand::ShowToast("Changed".into())),
        instance + Duration::from_secs(1),
    );
    assert_ne!(
        toast.active_overlay_descriptor().expect("renewed toast").id,
        first.id
    );
    assert!(
        toast
            .active_overlay_descriptor()
            .expect("toast target")
            .target
            .is_none()
    );
}

#[test]
fn explorer_address_and_search_keep_non_overlay_text_input_routing() {
    for mode in [ExplorerInputMode::Address, ExplorerInputMode::Search] {
        let mut state = explorer_routing_test_state();
        state.explorer_input_mode = mode;

        assert_eq!(state.resolved_explorer_overlay(), None);
        assert_eq!(state.active_overlay_descriptor(), None);
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::SubmitExplorerInput
        );
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::CancelExplorerInput
        );
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Backspace")).1,
            ShellCommand::ExplorerBackspace
        );
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("x")).1,
            ShellCommand::AppendExplorerChar('x')
        );
    }
}

#[test]
fn settings_editor_identities_distinguish_variants_without_mutable_content() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.settings_state = Some(SettingsState {
        category: ui::SettingsCategory::Appearance,
        selected_field: ui::SettingsField::Theme,
        status: String::new(),
        scroll_offset: 0,
        picker: None,
        color_editor: None,
        weather_location_editor: None,
        file_extensions_editor: None,
        time_sync_server_editor: None,
        time_sync_validation_request_id: None,
    });

    let mut ids = Vec::new();
    for kind in [
        ui::SettingsPickerKind::BorderColor,
        ui::SettingsPickerKind::AccentColor,
    ] {
        let settings = state.settings_state.as_mut().expect("settings");
        settings.color_editor = Some(SettingsColorEditorState {
            kind,
            value: "#112233".into(),
            error: Some("first error".into()),
        });
        let first = state.active_overlay_descriptor().expect("color editor");
        let settings = state.settings_state.as_mut().expect("settings");
        let editor = settings.color_editor.as_mut().expect("color editor");
        editor.value = "#abcdef".into();
        editor.error = Some("changed error".into());
        assert_eq!(
            state.active_overlay_descriptor().expect("stable color").id,
            first.id
        );
        ids.push(first.id);
        state
            .settings_state
            .as_mut()
            .expect("settings")
            .color_editor = None;
    }

    state
        .settings_state
        .as_mut()
        .expect("settings")
        .weather_location_editor = Some(SettingsWeatherLocationEditorState {
        value: "first".into(),
        error: None,
    });
    ids.push(
        state
            .active_overlay_descriptor()
            .expect("weather editor")
            .id,
    );
    state
        .settings_state
        .as_mut()
        .expect("settings")
        .weather_location_editor = None;
    state
        .settings_state
        .as_mut()
        .expect("settings")
        .file_extensions_editor = Some(SettingsFileExtensionsEditorState {
        value: "md".into(),
        error: None,
    });
    ids.push(
        state
            .active_overlay_descriptor()
            .expect("extensions editor")
            .id,
    );
    state
        .settings_state
        .as_mut()
        .expect("settings")
        .file_extensions_editor = None;
    state
        .settings_state
        .as_mut()
        .expect("settings")
        .time_sync_server_editor = Some(SettingsTimeSyncServerEditorState {
        value: "time.example".into(),
        error: Some("error".into()),
        validating: true,
    });
    let time_sync = state.active_overlay_descriptor().expect("time sync editor");
    let editor = state
        .settings_state
        .as_mut()
        .expect("settings")
        .time_sync_server_editor
        .as_mut()
        .expect("time sync editor");
    editor.value = "changed.example".into();
    editor.error = None;
    editor.validating = false;
    assert_eq!(
        state
            .active_overlay_descriptor()
            .expect("stable time sync")
            .id,
        time_sync.id
    );
    ids.push(time_sync.id);
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 5);
}

#[test]
fn explorer_dialogs_share_central_keyboard_mouse_focus_and_hit_map_gating() {
    let entering = ui::MotionTransition {
        kind: ui::MotionTransitionKind::Dialog,
        direction: ui::MotionDirection::Entering,
        progress: 0,
        phase_progress: 0,
        active: true,
        next_redraw_in: Duration::from_millis(16),
    };
    for mode in [
        ExplorerInputMode::NewFolder,
        ExplorerInputMode::NewTextFile,
        ExplorerInputMode::Rename,
        ExplorerInputMode::RestoreDestination,
    ] {
        let mut state = explorer_routing_test_state();
        state.explorer_input_mode = mode;
        state.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Explorer),
            anchor: (1, 1),
        });
        assert_eq!(
            state.resolved_explorer_overlay(),
            Some(ResolvedExplorerOverlay::Input(mode))
        );
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::SubmitExplorerInput
        );
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::CancelExplorerInput
        );
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(entering),
            ..ui::MotionTransitions::default()
        });
        assert!(!state.overlay_interaction_ready);
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::Noop
        );
        assert_ne!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::Noop
        );
        assert_eq!(state.focus_order(), vec![ShellComponent::Explorer]);
        assert_eq!(
            state
                .hit_map()
                .regions()
                .iter()
                .filter(|region| region.layer == ShellHitLayer::AppOverlay)
                .map(|region| region.area)
                .collect::<Vec<_>>(),
            vec![explorer_overlay_surface(&state)]
        );
        assert_eq!(
            state
                .route_input_at(
                    InputEvent::mouse_down(PointerButton::Left, (1, 1)),
                    Instant::now(),
                )
                .command,
            ShellCommand::CaptureOverlayInput
        );
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(ui::MotionTransition {
                progress: 1_000,
                phase_progress: 1_000,
                active: false,
                next_redraw_in: Duration::ZERO,
                ..entering
            }),
            ..ui::MotionTransitions::default()
        });
        assert!(state.overlay_interaction_ready);
        state.refresh_hit_map_with_motion(ui::MotionTransitions::default());
        assert!(state.overlay_interaction_ready);
    }

    let mut conflict = explorer_routing_test_state();
    let mut explorer = conflict
        .app
        .explorer_state()
        .expect("explorer state")
        .clone();
    explorer.pending_conflict = Some(app::explorer::ExplorerConflict {
        source: "/source".into(),
        target: "/target".into(),
        remaining: 1,
    });
    conflict.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer)),
        Instant::now(),
    );
    conflict.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(entering),
        ..ui::MotionTransitions::default()
    });
    assert_eq!(
        conflict.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::Noop
    );
    assert_ne!(
        conflict.route_key_input(&KeyInput::from_label("Esc")).1,
        ShellCommand::Noop
    );
}

#[test]
fn explorer_semantic_replacements_gate_input_while_active_popup_is_present() {
    let replacing = |kind| ui::MotionTransition {
        kind,
        direction: ui::MotionDirection::Replacing,
        progress: 0,
        phase_progress: 0,
        active: true,
        next_redraw_in: Duration::from_millis(16),
    };
    for mode in [
        ExplorerOverlayMode::Sort { anchor: (3, 3) },
        ExplorerOverlayMode::Options,
        ExplorerOverlayMode::Properties,
    ] {
        let mut state = explorer_routing_test_state();
        state.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Explorer),
            anchor: (1, 1),
        });
        state.explorer_overlay_mode = Some(mode);
        assert!(matches!(
            state.resolved_explorer_overlay(),
            Some(ResolvedExplorerOverlay::Semantic(resolved)) if resolved == mode
        ));
        let descriptor = state
            .active_overlay_descriptor()
            .expect("semantic replacement");
        assert_eq!(descriptor.category, ShellOverlayCategory::PagePopover);
        state.refresh_hit_map();
        let enter = state.route_key_input(&KeyInput::from_label("Enter"));
        assert_eq!(enter.0, RoutedTarget::Component(ShellComponent::Explorer));
        assert_eq!(enter.1, ShellCommand::ExplorerOverlayActivate);
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::ClosePopup
        );
        assert_eq!(state.focus_order(), vec![ShellComponent::Explorer]);
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .all(|region| region.component != ShellComponent::ContextMenu)
        );
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == ShellComponent::Explorer)
        );
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(replacing(ui::MotionTransitionKind::Popover)),
            ..ui::MotionTransitions::default()
        });
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::Noop
        );
        assert_eq!(state.focus_order(), vec![ShellComponent::Explorer]);
        assert_eq!(
            state
                .hit_map()
                .regions()
                .iter()
                .filter(|region| region.layer == ShellHitLayer::AppOverlay)
                .map(|region| region.area)
                .collect::<Vec<_>>(),
            vec![explorer_overlay_surface(&state)]
        );
        assert_eq!(
            state
                .route_input_at(
                    InputEvent::mouse_down(PointerButton::Left, (1, 1)),
                    Instant::now(),
                )
                .command,
            ShellCommand::CaptureOverlayInput
        );
    }

    for dialog in [
        app::explorer::ExplorerDialog::delete(std::path::Path::new("/delete")),
        app::explorer::ExplorerDialog::dump_trash(2),
    ] {
        let mut state = explorer_routing_test_state();
        state.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Explorer),
            anchor: (1, 1),
        });
        let mut explorer = state.app.explorer_state().expect("explorer state").clone();
        explorer.pending_dialog = Some(dialog);
        state.app.dispatch_at(
            app::AppCommand::SetExplorerState(Some(explorer)),
            Instant::now(),
        );
        assert!(matches!(
            state.resolved_explorer_overlay(),
            Some(ResolvedExplorerOverlay::PendingDialog(_))
        ));
        let descriptor = state.active_overlay_descriptor().expect("pending dialog");
        assert_eq!(descriptor.kind, ui::MotionOverlayKind::Dialog);
        assert_eq!(descriptor.category, ShellOverlayCategory::PageDialog);
        state.refresh_hit_map();
        let enter = state.route_key_input(&KeyInput::from_label("Enter"));
        assert_eq!(enter.0, RoutedTarget::Component(ShellComponent::Explorer));
        assert!(matches!(
            enter.1,
            ShellCommand::ExplorerConfirmDelete | ShellCommand::ExplorerConfirmDumpTrash
        ));
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::CancelExplorerInput
        );
        assert_eq!(state.focus_order(), vec![ShellComponent::Explorer]);
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .all(|region| region.component != ShellComponent::ContextMenu)
        );
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == ShellComponent::Explorer)
        );
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(replacing(ui::MotionTransitionKind::Dialog)),
            ..ui::MotionTransitions::default()
        });
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::Noop
        );
        assert_ne!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::Noop
        );
    }

    let mut generic = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    generic.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Home),
        anchor: (10, 10),
    });
    generic.refresh_hit_map();
    assert_eq!(
        generic.resolved_overlay_owner(),
        Some(ShellComponent::ContextMenu)
    );
    assert_eq!(generic.focus_order(), vec![ShellComponent::ContextMenu]);
    assert!(
        generic
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::ContextMenu)
    );
    assert_eq!(
        generic.route_key_input(&KeyInput::from_label("Esc")).0,
        RoutedTarget::Popup(ShellComponent::ContextMenu)
    );
}

#[test]
fn exit_confirmation_names_the_physical_power_action_poweroff() {
    let root = std::env::temp_dir().join(format!(
        "tundra-shell-poweroff-label-{}",
        std::process::id()
    ));
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("AppData"),
    )
    .expect("absolute mock user directories");
    let app_paths = platform::build_windows_app_paths(
        root.join("Roaming"),
        root.join("Local"),
        root.join("Temp"),
    )
    .expect("absolute mock app paths");
    let platform = platform::mock::MockPlatform::new(user_dirs, app_paths);
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));

    state.apply_input_with_platform(InputEvent::from_key_label("q"), &platform);

    let modal = state
        .to_notification_view_model()
        .expect("exit confirmation modal");
    let poweroff = modal
        .actions
        .iter()
        .find(|action| action.id == "poweroff")
        .expect("poweroff action");
    assert_eq!(poweroff.label, "Poweroff");
}

#[test]
fn command_line_is_a_fixed_admin_only_launcher_item() {
    let mut admin = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    set_test_auth_role(&mut admin, UserRole::Admin);

    let launcher = admin.to_launcher_view_model();
    assert_eq!(launcher.items.len(), 1);
    let item = &launcher.items[0];
    assert_eq!(item.id, app::COMMAND_LINE_APPLICATION.id);
    assert!(item.is_builtin());
    assert!(!item.capabilities.removable);
    assert!(!item.capabilities.reapprovable);
    assert!(!item.capabilities.reorderable);

    let mut user = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    set_test_auth_role(&mut user, UserRole::User);
    assert!(user.to_launcher_view_model().items.is_empty());
}

#[test]
fn launcher_uses_cached_ascii_assets_after_the_runtime_directory_is_deleted() {
    fn copy_directory(source: &std::path::Path, destination: &std::path::Path) {
        std::fs::create_dir_all(destination).expect("create temporary asset directory");
        for entry in std::fs::read_dir(source).expect("read source asset directory") {
            let entry = entry.expect("read source asset entry");
            let target = destination.join(entry.file_name());
            if entry.file_type().expect("read asset entry type").is_dir() {
                copy_directory(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).expect("copy source asset file");
            }
        }
    }

    let root = std::env::temp_dir().join(format!(
        "tundra-shell-deleted-assets-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let canonical = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    copy_directory(&canonical, &root);
    let launcher_icons = root.join("themes/default/launcher_icons.toml");
    let source = std::fs::read_to_string(&launcher_icons).expect("read Launcher icons");
    let customized = source.replacen("' ______ '", "'CACHE!!!'", 1);
    assert_ne!(customized, source, "Launcher icon fixture should change");
    std::fs::write(&launcher_icons, customized).expect("write cached Launcher icon fixture");
    let assets = ui::RuntimeAsciiAssets::from_store(
        ui::AsciiAssetStore::load_with_root(&root, ui::DEFAULT_THEME_ID)
            .expect("temporary ASCII assets should load"),
    );
    std::fs::remove_dir_all(&root).expect("simulate deleting the runtime asset directory");

    let startup = ShellStartupState::clean(
        PlatformKind::Windows,
        PlatformCapabilities::native_supported(),
    );
    let mut state = ShellSession::new_with_startup_and_assets(
        ShellLaunchConfig::default(),
        (120, 40),
        startup,
        assets,
    );
    set_test_auth_role(&mut state, UserRole::Admin);

    let launcher = state.to_launcher_view_model();
    let icon = launcher
        .item_icon(&launcher.items[0])
        .expect("cached Command Line ASCII icon");

    assert!(icon.lines().iter().any(|line| line.contains("CACHE!!!")));
    assert!(
        launcher
            .item_graphic_bytes(&launcher.items[0])
            .is_some_and(|bytes| bytes.starts_with(b"\x89PNG\r\n\x1a\n")),
        "the deleted runtime directory must not invalidate the cached PNG"
    );
}

#[test]
fn explicit_theme_refresh_reloads_assets_from_disk() {
    let root = std::env::temp_dir().join(format!(
        "tundra-shell-asset-refresh-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    ui::restore_default_theme(&root).expect("create complete default theme fixture");
    let launcher_icons = root.join("themes/default/launcher_icons.toml");
    let source = std::fs::read_to_string(&launcher_icons).expect("read Launcher icons");
    let first = source.replacen("' ______ '", "'CACHE001'", 1);
    assert_ne!(first, source);
    std::fs::write(&launcher_icons, &first).expect("write initial cached Launcher icon");

    let assets = ui::RuntimeAsciiAssets::load_with_root(&root, ui::DEFAULT_THEME_ID)
        .expect("load initial asset cache");
    let startup = ShellStartupState::clean(
        PlatformKind::Windows,
        PlatformCapabilities::native_supported(),
    );
    let mut state = ShellSession::new_with_startup_and_assets(
        ShellLaunchConfig::default(),
        (120, 40),
        startup,
        assets,
    );
    set_test_auth_role(&mut state, UserRole::Admin);

    let second = first.replacen("CACHE001", "CACHE002", 1);
    std::fs::write(&launcher_icons, second).expect("change Launcher icon after startup");

    let cached = state.to_launcher_view_model();
    assert!(
        cached
            .item_icon(&cached.items[0])
            .expect("cached Launcher icon")
            .lines()
            .iter()
            .any(|line| line.contains("CACHE001"))
    );

    state
        .refresh_asset_cache_for_theme(ui::DEFAULT_THEME_ID)
        .expect("explicit cache refresh");
    let refreshed = state.to_launcher_view_model();
    assert!(
        refreshed
            .item_icon(&refreshed.items[0])
            .expect("refreshed Launcher icon")
            .lines()
            .iter()
            .any(|line| line.contains("CACHE002"))
    );

    std::fs::remove_dir_all(root).expect("clean asset refresh fixture");
}

#[test]
fn command_line_open_requires_size_and_routes_ctrl_c_to_the_child() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack = vec![ShellScreen::Home, ShellScreen::Launcher];
    state.open_command_line();

    assert_eq!(state.active_screen(), ShellScreen::CommandLine);
    assert_eq!(state.focused_component(), ShellComponent::CommandLine);
    let ctrl_c = ui::KeyEvent::with_modifiers(ui::Key::Char('c'), ui::KeyModifiers::CTRL);
    let (_, command) = state.route_key_input(&ctrl_c);
    assert_eq!(command, ShellCommand::CommandLineKey(ctrl_c));
    assert!(!state.shutdown_requested());

    state.close_command_line();
    assert_eq!(state.active_screen(), ShellScreen::Launcher);

    state.terminal_size = (107, 22);
    state.open_command_line();
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
}

#[test]
fn command_line_keeps_the_shell_clock_button_visible_and_clickable() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    set_test_auth_role(&mut state, UserRole::Admin);
    state.screen_stack = vec![ShellScreen::Home, ShellScreen::Launcher];
    state.open_command_line();

    assert!(
        state
            .to_shell_chrome_view_model()
            .status
            .time_button_label
            .is_some()
    );
    let clock = hit_region_center(&state, ShellComponent::ClockButton);
    assert_eq!(
        state.hit_map.layer_at(clock),
        Some(ShellHitLayer::ShellChrome)
    );

    let routed = state.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, clock),
        Instant::now(),
    );
    assert_eq!(
        routed.target,
        RoutedTarget::Component(ShellComponent::ClockButton)
    );
    assert_eq!(routed.command, ShellCommand::OpenClock);
}

#[test]
fn key_event_to_label_maps_requested_keys() {
    let cases = [
        (
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            "Ctrl+C",
        ),
        (KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), "x"),
        (KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), "Enter"),
        (KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), "Esc"),
        (
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            "Backspace",
        ),
        (KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), "Tab"),
        (
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
            "Shift+Tab",
        ),
        (KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), "Left"),
        (KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), "Right"),
        (KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), "Up"),
        (KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), "Down"),
        (KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE), "F(5)"),
    ];

    for (event, expected) in cases {
        assert_eq!(key_event_to_label(event), expected);
    }
}

#[test]
fn mouse_event_to_input_maps_button_motion_and_scroll_events() {
    let down = mouse_event_to_input(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 12,
        row: 7,
        modifiers: KeyModifiers::NONE,
    });
    let drag = mouse_event_to_input(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Right),
        column: 13,
        row: 8,
        modifiers: KeyModifiers::NONE,
    });
    let moved = mouse_event_to_input(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 14,
        row: 9,
        modifiers: KeyModifiers::NONE,
    });
    let scroll_up = mouse_event_to_input(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 15,
        row: 10,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(
        down,
        InputEvent::Mouse(ui::MouseEvent {
            position: ui::Point::new(12, 7),
            kind: ui::MouseEventKind::Down(PointerButton::Left),
            modifiers: InputModifiers::none(),
        })
    );
    assert_eq!(
        drag,
        InputEvent::Mouse(ui::MouseEvent {
            position: ui::Point::new(13, 8),
            kind: ui::MouseEventKind::Drag(PointerButton::Right),
            modifiers: InputModifiers::none(),
        })
    );
    assert_eq!(
        moved,
        InputEvent::Mouse(ui::MouseEvent {
            position: ui::Point::new(14, 9),
            kind: ui::MouseEventKind::Moved,
            modifiers: InputModifiers::none(),
        })
    );
    assert_eq!(
        scroll_up,
        InputEvent::Mouse(ui::MouseEvent {
            position: ui::Point::new(15, 10),
            kind: ui::MouseEventKind::Scroll(ScrollDirection::Up),
            modifiers: InputModifiers::none(),
        })
    );
}

#[test]
fn mouse_event_to_input_uses_required_scroll_direction_labels() {
    let cases = [
        (MouseEventKind::ScrollDown, "Down"),
        (MouseEventKind::ScrollUp, "Up"),
        (MouseEventKind::ScrollLeft, "Left"),
        (MouseEventKind::ScrollRight, "Right"),
    ];

    for (kind, expected_direction) in cases {
        let input = mouse_event_to_input(MouseEvent {
            kind,
            column: 1,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(
            input,
            InputEvent::Mouse(ui::MouseEvent {
                position: ui::Point::new(1, 2),
                kind: ui::MouseEventKind::Scroll(match expected_direction {
                    "Down" => ScrollDirection::Down,
                    "Up" => ScrollDirection::Up,
                    "Left" => ScrollDirection::Left,
                    "Right" => ScrollDirection::Right,
                    _ => unreachable!("test direction"),
                }),
                modifiers: InputModifiers::none(),
            })
        );
    }
}

#[test]
fn platform_capability_summary_counts_native_supported_capabilities() {
    let summary = platform_capability_summary(
        PlatformKind::Windows,
        &PlatformCapabilities::native_supported(),
    );

    assert_eq!(
        summary,
        "Windows: 16 supported, 0 best-effort, 0 unsupported"
    );
}

#[test]
fn notification_toast_expires_at_wall_clock_deadline() {
    let started_at = Instant::now();
    let mut notifications = NotificationCenter::new("Ready");

    notifications.notify_toast_at("Saved", started_at);
    assert_eq!(
        notifications.poll_timeout(started_at, Duration::from_millis(250)),
        Duration::from_millis(250)
    );
    assert_eq!(
        notifications.poll_timeout(
            started_at + DEFAULT_TOAST_DURATION - Duration::from_millis(100),
            Duration::from_millis(250),
        ),
        Duration::from_millis(100)
    );
    assert_eq!(
        notifications.poll_timeout(
            started_at + DEFAULT_TOAST_DURATION,
            Duration::from_millis(250),
        ),
        Duration::ZERO
    );
    notifications.expire(started_at + DEFAULT_TOAST_DURATION - Duration::from_millis(1));
    assert_eq!(notifications.toast().as_deref(), Some("Saved"));

    notifications.expire(started_at + DEFAULT_TOAST_DURATION);
    assert_eq!(notifications.toast(), None);

    let replacement_at = started_at + Duration::from_secs(10);
    notifications.notify_toast_at("First", replacement_at);
    notifications.notify_toast_at("Saved again", replacement_at + Duration::from_secs(3));
    notifications.expire(replacement_at + DEFAULT_TOAST_DURATION);
    assert_eq!(notifications.toast().as_deref(), Some("Saved again"));

    notifications.expire(replacement_at + Duration::from_secs(3) + DEFAULT_TOAST_DURATION);
    assert_eq!(notifications.toast(), None);
}

#[test]
fn notification_toast_waits_behind_an_active_alert() {
    let started_at = Instant::now();
    let mut notifications = NotificationCenter::new("Ready");
    notifications.notify_alert_with_key(
        "storage",
        "Storage unavailable",
        ui::NotificationTone::Error,
    );
    notifications.notify_toast_at("Countdown finished", started_at);

    notifications.expire(started_at + DEFAULT_TOAST_DURATION + Duration::from_secs(1));

    assert_eq!(notifications.toast().as_deref(), Some("Countdown finished"));
    assert_eq!(
        notifications.poll_timeout(started_at, Duration::from_millis(250)),
        Duration::from_millis(250)
    );
    notifications.resolve_alert("storage");
    assert_eq!(notifications.toast().as_deref(), Some("Countdown finished"));
}

#[test]
fn clock_storage_retry_keeps_the_due_summary_visible() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    state.remember_clock_due_summary("Countdown finished".to_string());

    state.report_clock_storage_error("first failure");
    state.report_clock_storage_error("retry failure");

    assert!(
        state
            .to_shell_chrome_view_model()
            .status
            .error
            .as_deref()
            .is_some_and(|message| {
                message.contains("Countdown finished") && message.contains("retry failure")
            })
    );
}

#[test]
fn compact_clock_routes_only_escape_and_does_not_open_hidden_controls() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (49, 11));
    state.screen_stack = vec![ShellScreen::Clock];

    assert_eq!(
        state.route_clock_key(&KeyInput::from_label("n")).1,
        ShellCommand::CaptureOverlayInput
    );
    assert_eq!(state.focus_order(), vec![ShellComponent::CompactHome]);

    state.clock_create_state = Some(ClockCreateState::default());
    assert_eq!(
        state.route_clock_key(&KeyInput::from_label("Esc")).1,
        ShellCommand::ClockCloseCreate
    );
}

#[test]
fn focus_navigation_cycles_the_dynamic_home_order_in_both_directions() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.screen_stack = vec![ShellScreen::Home];
    state.focused_component = ShellComponent::Home;
    state.refresh_hit_map();

    assert_eq!(
        state.focus_order(),
        vec![
            ShellComponent::Home,
            ShellComponent::ClockButton,
            ShellComponent::StatusBar,
            ShellComponent::TopBar,
        ]
    );

    state.move_focus(ui::FocusDirection::Previous);
    assert_eq!(state.focused_component, ShellComponent::TopBar);
    state.move_focus(ui::FocusDirection::Next);
    assert_eq!(state.focused_component, ShellComponent::Home);
    state.move_focus(ui::FocusDirection::Next);
    assert_eq!(state.focused_component, ShellComponent::ClockButton);
}

#[test]
fn refresh_hit_map_normalizes_an_illegal_focus_to_the_order_start() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.screen_stack = vec![ShellScreen::Login];
    state.focused_component = ShellComponent::TopBar;

    state.refresh_hit_map();

    assert_eq!(state.focused_component, ShellComponent::LoginUserList);
}

#[test]
fn focus_navigation_does_not_leave_a_single_component_modal_scope() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.notify_modal(
        "Confirm",
        "Stay focused",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );

    assert_eq!(
        state.focus_order(),
        vec![ShellComponent::NotificationDialog]
    );
    state.move_focus(ui::FocusDirection::Previous);
    assert_eq!(state.focused_component, ShellComponent::NotificationDialog);
    state.move_focus(ui::FocusDirection::Next);
    assert_eq!(state.focused_component, ShellComponent::NotificationDialog);
}
#[test]
fn notification_alerts_resolve_by_key_and_preserve_other_sources() {
    let mut notifications = NotificationCenter::new("Ready");
    notifications.notify_alert_with_key(
        "settings",
        "Settings warning",
        ui::NotificationTone::Warning,
    );
    notifications.notify_alert_with_key(
        "explorer.operation",
        "Explorer failed",
        ui::NotificationTone::Error,
    );

    assert_eq!(notifications.alert().as_deref(), Some("Explorer failed"));
    assert_eq!(
        notifications.alert_tone(),
        Some(ui::NotificationTone::Error)
    );

    notifications.resolve_alert("explorer.operation");
    assert_eq!(notifications.alert().as_deref(), Some("Settings warning"));
    assert_eq!(
        notifications.alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
}

#[test]
fn notification_response_queue_is_bounded() {
    let mut notifications = NotificationCenter::new("Ready");
    let total = MAX_NOTIFICATION_RESPONSES + 5;

    for index in 0..total {
        notifications.push_modal(ShellNotification::modal(
            "Notice",
            "Continue?",
            ui::NotificationTone::Info,
            vec![ShellNotificationAction::new(format!("ok-{index}"), "OK")],
        ));
        let _follow_up = notifications.activate_selected_action();
    }

    assert_eq!(notifications.response_count(), MAX_NOTIFICATION_RESPONSES);
    assert_eq!(
        notifications
            .take_response()
            .map(|response| response.notification_id),
        Some(6)
    );
}

#[test]
fn keyed_modal_update_replaces_presentation_binding_atomically() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    let first_id = state.notify_modal_with_options(
        ShellNotification::modal(
            "First",
            "Original",
            ui::NotificationTone::Info,
            vec![
                ShellNotificationAction::new("continue", "Continue")
                    .with_follow_up(ShellCommand::FocusNext),
            ],
        )
        .with_key("same"),
    );
    let updated_id = state.notify_modal_with_options(
        ShellNotification::modal(
            "Updated",
            "Replacement",
            ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("continue", "Continue")
                    .with_follow_up(ShellCommand::FocusPrevious),
            ],
        )
        .with_key("same"),
    );

    assert_eq!(updated_id, first_id);
    assert_eq!(
        state
            .to_notification_view_model()
            .map(|model| (model.title, model.message)),
        Some(("Updated".to_string(), "Replacement".to_string()))
    );
    state.activate_notification_selected();
    assert_eq!(
        state.pending_notification_commands.pop_front(),
        Some(ShellCommand::FocusPrevious)
    );
}
#[test]
fn notification_follow_up_activation_is_iterative_and_bounded() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    for index in 0..(MAX_NOTIFICATION_FOLLOW_UP_STEPS + 3) {
        state.notify_modal(
            format!("Notice {index}"),
            "Continue?",
            ui::NotificationTone::Info,
            vec![
                ShellNotificationAction::new(format!("ok-{index}"), "OK")
                    .with_follow_up(ShellCommand::NotificationActivateSelected),
            ],
        );
    }

    let action = state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(action, ShellAction::Redraw);
    assert!(state.to_notification_view_model().is_some());
    assert_eq!(
        state.to_shell_chrome_view_model().status.error.as_deref(),
        Some("Notification follow-up limit reached")
    );
    assert_eq!(
        state.to_shell_chrome_view_model().status.alert_tone,
        ui::NotificationTone::Critical
    );
}

#[test]
fn cached_time_sync_replays_into_recreated_shell_state() {
    let received_at = Instant::now();
    let consumed_at = received_at + Duration::from_secs(3);
    let replayed_at = received_at + Duration::from_secs(5);
    let utc = Utc::now();
    let mut cached = None;
    let mut original_state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));

    apply_timed_time_sync_result_at(
        &mut original_state,
        &mut cached,
        TimedTimeSyncResult {
            result: Ok(utc),
            received_at,
        },
        consumed_at,
    );

    assert_eq!(
        original_state.last_time_sync_utc,
        Some(utc + Duration::from_secs(3))
    );

    let mut recreated_state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    cached
        .as_ref()
        .expect("successful sync should be cached")
        .apply_to_state_at(&mut recreated_state, replayed_at);

    assert!(recreated_state.time_sync_attempted);
    assert_eq!(
        recreated_state.last_time_sync_utc,
        Some(utc + Duration::from_secs(5))
    );

    let mut failed_state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    CachedTimeSyncResult::Failure.apply_to_state_at(&mut failed_state, replayed_at);
    assert!(failed_state.time_sync_attempted);
    assert!(failed_state.time_sync_failure_dialog_visible());
}

#[test]
fn auth_poll_timeout_wakes_at_password_reveal_deadline() {
    let now = Instant::now();
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    state.screen_stack = vec![ShellScreen::Login];
    state.login_idle_deadline = now + LOGIN_IDLE_TIMEOUT;
    state.login_password_visible_until = Some(now + Duration::from_millis(10));

    assert_eq!(
        state.auth_poll_timeout(now, Duration::from_millis(250)),
        Duration::from_millis(10)
    );
}

#[test]
fn system_services_startup_config_uses_storage_timezone_and_location() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "tundra-shell-lockscreen-options-{}-{nanos}",
        std::process::id()
    ));
    let app_paths = platform::build_windows_app_paths(
        base.join("Roaming"),
        base.join("Local"),
        base.join("Temp"),
    )
    .expect("app paths");
    let opened = StorageManager::open(app_paths).expect("storage opens");
    let mut config = opened.manager.load_config().expect("config loads");
    config.timezone = "Asia/Shanghai".to_string();
    config.weather_location = Some("Pudong, Shanghai, China".to_string());
    opened.manager.save_config(&config).expect("config saves");

    let mut startup = ShellStartupState::clean(
        PlatformKind::Windows,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(opened.manager.clone());

    let services = system_services_config_for_startup(&startup);

    assert_eq!(services.timezone_id, "Asia/Shanghai");
    assert_eq!(
        services.weather_location.as_deref(),
        Some("Pudong, Shanghai, China")
    );
    let location = services.timezone_location.expect("mapped location");
    assert_eq!(location.city.as_deref(), Some("Shanghai"));
    assert!((location.latitude - 31.2304).abs() < 0.001);
    assert!((location.longitude - 121.4737).abs() < 0.001);

    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn shell_construction_injects_the_loaded_config_into_app_state() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "tundra-shell-app-config-{}-{nanos}",
        std::process::id()
    ));
    let app_paths = platform::build_windows_app_paths(
        base.join("Roaming"),
        base.join("Local"),
        base.join("Temp"),
    )
    .expect("app paths");
    let opened = StorageManager::open(app_paths).expect("storage opens");
    let mut config = opened.manager.load_config().expect("config loads");
    config.timezone = "Asia/Tokyo".to_string();
    config.editor.cursor_acceleration_delay_ms = 750;
    config.explorer.show_hidden = true;
    opened.manager.save_config(&config).expect("config saves");

    let mut startup = ShellStartupState::clean(
        PlatformKind::Windows,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(opened.manager);
    let state = ShellSession::new_with_startup(ShellLaunchConfig::default(), (120, 40), startup);

    assert_eq!(state.app.storage_config(), &config);
    assert_eq!(state.current_editor_config(), config.editor);
    assert_eq!(state.app.snapshot().clock_timezone_id, Some("Asia/Tokyo"));
    let _ = std::fs::remove_dir_all(base);
}
#[test]
fn hit_map_uses_explicit_layer_priority_instead_of_insertion_order() {
    let area = Rect::new(0, 0, 10, 5);
    let map = ShellHitMap::new(
        (10, 5),
        1,
        vec![
            ShellHitRegion {
                component: ShellComponent::ExitDialog,
                area,
                layer: ShellHitLayer::ShellModal,
            },
            ShellHitRegion {
                component: ShellComponent::ClockButton,
                area,
                layer: ShellHitLayer::ShellChrome,
            },
            ShellHitRegion {
                component: ShellComponent::ContextMenu,
                area,
                layer: ShellHitLayer::AppOverlay,
            },
            ShellHitRegion {
                component: ShellComponent::Explorer,
                area,
                layer: ShellHitLayer::AppContent,
            },
        ],
    );

    assert_eq!(map.target_at((2, 2)), Some(ShellComponent::ExitDialog));
    assert_eq!(map.layer_at((2, 2)), Some(ShellHitLayer::ShellModal));

    let without_modal = ShellHitMap::new((10, 5), 2, map.regions()[1..].to_vec());
    assert_eq!(
        without_modal.target_at((2, 2)),
        Some(ShellComponent::ClockButton)
    );

    let app_only = ShellHitMap::new((10, 5), 3, map.regions()[2..].to_vec());
    assert_eq!(
        app_only.target_at((2, 2)),
        Some(ShellComponent::ContextMenu)
    );
}

#[test]
fn hit_map_keeps_duplicate_component_regions_distinct() {
    let map = ShellHitMap::new(
        (10, 5),
        1,
        vec![
            ShellHitRegion::new(
                ShellComponent::ClockButton,
                Rect::new(0, 0, 5, 5),
                ShellHitLayer::AppContent,
            ),
            ShellHitRegion::new(
                ShellComponent::ClockButton,
                Rect::new(1, 1, 2, 2),
                ShellHitLayer::ShellChrome,
            ),
        ],
    );

    assert_eq!(map.region_at((0, 0)), Some(&map.regions()[0]));
    assert_eq!(map.region_at((1, 1)), Some(&map.regions()[1]));
}

#[test]
fn clock_button_routes_before_explorer_popup_and_app_forms() {
    let mut explorer = explorer_routing_test_state();
    let clock = hit_region_center(&explorer, ShellComponent::ClockButton);
    explorer.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (10, 10),
    });
    explorer.explorer_overlay_mode = Some(ExplorerOverlayMode::ContextMenu { anchor: (10, 10) });
    explorer.refresh_hit_map();

    let routed = explorer.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, clock),
        Instant::now(),
    );
    assert_eq!(
        routed.target,
        RoutedTarget::Component(ShellComponent::ClockButton)
    );
    assert_eq!(routed.command, ShellCommand::OpenClock);

    explorer.active_popup = None;
    explorer.explorer_overlay_mode = None;
    explorer.explorer_input_mode = ExplorerInputMode::NewFolder;
    explorer.refresh_hit_map();
    let routed = explorer.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, clock),
        Instant::now(),
    );
    assert_eq!(routed.command, ShellCommand::OpenClock);

    let mut user_management = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    user_management.screen_stack = vec![ShellScreen::UserManagement];
    user_management.user_management_mode = UserManagementMode::Create(UserManagementCreateForm {
        username: String::new(),
        display_name: String::new(),
        password: String::new(),
        role: UserRole::User,
        focused_field: UserManagementFormField::Username,
    });
    user_management.refresh_hit_map();
    let clock = hit_region_center(&user_management, ShellComponent::ClockButton);
    let routed = user_management.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, clock),
        Instant::now(),
    );
    assert_eq!(routed.command, ShellCommand::OpenClock);
}

#[test]
fn clock_button_routes_outside_shell_modal_while_modal_region_stays_highest() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.notify_modal(
        "Confirm",
        "Keep the clock available",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );
    let clock = hit_region_center(&state, ShellComponent::ClockButton);
    let dialog = hit_region_center(&state, ShellComponent::NotificationDialog);

    assert_eq!(
        state.hit_map.layer_at(clock),
        Some(ShellHitLayer::ShellChrome)
    );
    assert_eq!(
        state.hit_map.layer_at(dialog),
        Some(ShellHitLayer::ShellModal)
    );

    let routed = state.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, clock),
        Instant::now(),
    );
    assert_eq!(routed.command, ShellCommand::OpenClock);
}

#[test]
fn editor_load_blocks_clock_navigation_and_restores_its_origin() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.screen_stack = vec![
        ShellScreen::Home,
        ShellScreen::Explorer,
        ShellScreen::Editor,
    ];
    state.focused_component = ShellComponent::Editor;
    let operation = EditorLoadOperation::Open {
        navigation: EditorLoadNavigation::Explorer,
        reload: None,
        replacing_dirty: false,
    };
    state.editor_load_state = Some(EditorLoadState {
        id: 17,
        path: PathBuf::from("large.log"),
        stage: EditorTaskStage::Reading,
        completed_bytes: 1,
        total_bytes: Some(2),
        operation: operation.clone(),
    });
    state.refresh_hit_map();
    let clock = hit_region_center(&state, ShellComponent::ClockButton);

    state.apply_input(InputEvent::mouse_down(PointerButton::Left, clock));

    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(
        state.screen_stack,
        vec![
            ShellScreen::Home,
            ShellScreen::Explorer,
            ShellScreen::Editor
        ]
    );
    assert!(
        state
            .editor_message
            .as_deref()
            .is_some_and(|message| message.contains("Press Esc"))
    );

    state.editor_load_state = None;
    state.restore_editor_load_navigation(&operation);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(state.focused_component, ShellComponent::Explorer);
}

#[test]
fn cancelled_editor_loads_release_the_concurrency_permit() {
    let runtime = ShellEditorTaskRuntime::new();
    let path = std::env::temp_dir().join(format!(
        "tundra-editor-cancel-permit-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&path, b"cancel me").expect("seed editor load fixture");
    let first = next_editor_task_id();
    let second = next_editor_task_id();
    for id in [first, second] {
        runtime
            .submit_load(id, path.clone(), EditorTaskAccess::Editable)
            .expect("submit load");
        runtime.cancel(id);
    }

    let mut terminal_events = 0;
    for _ in 0..400 {
        terminal_events += runtime
            .drain_events()
            .into_iter()
            .filter(|event| matches!(event, EditorTaskEvent::LoadFinished { .. }))
            .count();
        if terminal_events == 2 {
            break;
        }
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(terminal_events, 2);
    assert_eq!(runtime.shared.active_loads.load(Ordering::Acquire), 0);
    assert_eq!(runtime.shared.active_load_bytes.load(Ordering::Acquire), 0);

    let third = next_editor_task_id();
    runtime
        .submit_load(third, path.clone(), EditorTaskAccess::Editable)
        .expect("a later load must not be blocked by leaked permits");
    runtime.cancel(third);
    let _ = std::fs::remove_file(path);
}

#[test]
fn editor_load_byte_budget_is_shared_and_released() {
    let active = AtomicU64::new(0);
    let first = reserve_editor_load_bytes(&active, 700 * 1024 * 1024)
        .expect("first document fits the shared budget");
    assert!(reserve_editor_load_bytes(&active, 400 * 1024 * 1024).is_err());
    drop(first);
    let maximum = reserve_editor_load_bytes(&active, platform::MAX_DOCUMENT_BYTES)
        .expect("one maximum-sized document fits");
    assert_eq!(active.load(Ordering::Acquire), platform::MAX_DOCUMENT_BYTES);
    drop(maximum);
    assert_eq!(active.load(Ordering::Acquire), 0);
}

#[test]
fn stale_save_completion_does_not_mark_a_replacement_document_saved() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let mut replacement = EditorState::untitled(app::editor::DocumentKind::PlainText);
    replacement.apply(app::editor::EditorCommand::InsertText(
        "replacement".to_string(),
    ));
    let expected = replacement.clone();
    state.editor_document_generation = 9;
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(replacement)),
        Instant::now(),
    );
    state.editor_close_after_save = true;
    state.editor_open_after_save = true;

    state.finish_editor_save(
        EditorSaveState {
            id: 1,
            path: PathBuf::from("old.txt"),
            document_generation: 8,
            revision: 1,
            stage: EditorTaskStage::Writing,
        },
        Ok(DocumentFingerprint {
            len: 3,
            modified: None,
            content_hash: 7,
        }),
        &platform::mock::UnsupportedPlatform,
    );

    assert_eq!(state.app.editor_state(), Some(&expected));
    assert_eq!(state.editor_fingerprint, None);
    assert!(!state.editor_close_after_save);
    assert!(!state.editor_open_after_save);
}

#[test]
fn shutdown_waits_for_an_active_editor_save() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.editor_save_state = Some(EditorSaveState {
        id: 5,
        path: PathBuf::from("large.txt"),
        document_generation: state.editor_document_generation,
        revision: 1,
        stage: EditorTaskStage::Writing,
    });

    assert_eq!(state.apply_input(InputEvent::Shutdown), ShellAction::Redraw);
    assert!(!state.shutdown_requested);
    assert!(state.editor_save_state.is_some());

    state.editor_save_state = None;
    assert_eq!(state.apply_input(InputEvent::Shutdown), ShellAction::Exit);
    assert!(state.shutdown_requested);
}

#[test]
fn explorer_never_receives_shell_chrome_pointer_commands_and_clears_drag() {
    let mut state = explorer_routing_test_state();
    let _ = state.update_explorer_state(|explorer| {
        explorer.drag = Some(app::explorer::ExplorerDragState {
            sources: vec![std::path::PathBuf::from("source")],
            target: None,
            mode: app::explorer::ExplorerTransferMode::Copy,
            active: true,
        });
    });
    let top = hit_region_center(&state, ShellComponent::TopBar);

    let routed = state.route_input_at(
        InputEvent::mouse_drag(PointerButton::Left, top),
        Instant::now(),
    );
    assert_eq!(
        routed.target,
        RoutedTarget::Component(ShellComponent::TopBar)
    );
    assert_eq!(routed.command, ShellCommand::CaptureOverlayInput);
    assert!(
        state
            .app
            .explorer_state()
            .expect("Explorer state")
            .drag
            .is_none()
    );

    let status = hit_region_center(&state, ShellComponent::StatusBar);
    for input in [
        InputEvent::mouse_down(PointerButton::Left, status),
        InputEvent::mouse_down(PointerButton::Right, status),
        InputEvent::Mouse(ui::MouseEvent {
            position: ui::Point::new(status.0, status.1),
            kind: ui::MouseEventKind::Scroll(ScrollDirection::Down),
            modifiers: InputModifiers::none(),
        }),
    ] {
        let routed = state.route_input_at(input, Instant::now());
        assert_eq!(
            routed.target,
            RoutedTarget::Component(ShellComponent::StatusBar)
        );
        assert_eq!(routed.command, ShellCommand::CaptureOverlayInput);
    }

    let _ = state.update_explorer_state(|explorer| {
        explorer.drag = Some(app::explorer::ExplorerDragState {
            sources: vec![std::path::PathBuf::from("source")],
            target: None,
            mode: app::explorer::ExplorerTransferMode::Move,
            active: true,
        });
    });
    let (target, command) = state.route_explorer_mouse(
        ui::MouseEvent {
            position: ui::Point::new(top.0, top.1),
            kind: ui::MouseEventKind::Up(PointerButton::Left),
            modifiers: InputModifiers::none(),
        },
        Some(ShellComponent::TopBar),
        Instant::now(),
    );
    assert_eq!(target, RoutedTarget::Component(ShellComponent::TopBar));
    assert_eq!(command, ShellCommand::CaptureOverlayInput);
    assert!(
        state
            .app
            .explorer_state()
            .expect("Explorer state")
            .drag
            .is_none()
    );
}

#[test]
fn watchdog_incident_redacts_details_and_actions_for_standard_users() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            session_id: "user-session".to_string(),
            user_id: "user-id".to_string(),
            username: "user".to_string(),
            role: UserRole::User,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    show_watchdog_incident(
        &mut state,
        IncidentReceipt {
            incident_id: "SECRET-INCIDENT-ID".to_string(),
            kind: watchdog::IncidentKind::Error,
            severity: watchdog::IncidentSeverity::Critical,
            app_id: None,
            component: Some("private-component".to_string()),
            task_id: None,
            task_group: None,
            boundary: "private-boundary".to_string(),
            panic_action: None,
            operation_kind: None,
            operation_id: None,
            recovery_handler_version: None,
            restart_attempt: 0,
            summary: "SECRET watchdog summary".to_string(),
            recovery: RecoveryOutcome::Recovered("SECRET recovery detail".to_string()),
            json_report_path: Some(std::path::PathBuf::from(
                "/private/reports/SECRET-INCIDENT-ID.json",
            )),
            text_report_path: None,
        },
    );

    let modal = state.to_notification_view_model().expect("watchdog modal");
    assert!(modal.message.contains("restricted to administrators"));
    assert!(!modal.message.contains("SECRET"));
    assert!(!modal.message.contains("/private"));
    assert!(
        modal
            .actions
            .iter()
            .all(|action| { action.id != "open-report" && action.id != "copy-summary" })
    );
}

#[test]
fn previous_unclean_exit_does_not_interrupt_the_login_screen() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.screen_stack = vec![ShellScreen::Login];
    state.focused_component = ShellComponent::LoginUserList;
    let report_path = std::path::PathBuf::from("/reports/previous-run.txt");

    show_watchdog_incident(
        &mut state,
        IncidentReceipt {
            incident_id: "unclean-previous-run".to_string(),
            kind: IncidentKind::UncleanExit,
            severity: watchdog::IncidentSeverity::Critical,
            app_id: None,
            component: None,
            task_id: None,
            task_group: None,
            boundary: "process.unhandled".to_string(),
            panic_action: None,
            operation_kind: None,
            operation_id: None,
            recovery_handler_version: None,
            restart_attempt: 0,
            summary: "previous run ended without a clean shutdown".to_string(),
            recovery: RecoveryOutcome::Unrecoverable(
                "the previous process had already terminated".to_string(),
            ),
            json_report_path: None,
            text_report_path: Some(report_path.clone()),
        },
    );

    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert_eq!(state.focused_component(), ShellComponent::LoginUserList);
    assert!(state.to_notification_view_model().is_none());
    assert_eq!(state.latest_watchdog_report.as_ref(), Some(&report_path));
    assert!(
        state
            .latest_watchdog_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("previous run ended"))
    );
}

fn explorer_routing_test_state() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.screen_stack = vec![ShellScreen::Explorer];
    state.focused_component = ShellComponent::Explorer;
    state.replace_explorer_state(Some(ExplorerState::new(".", false)));
    state.refresh_hit_map();
    state
}

fn explorer_overlay_surface(state: &ShellSession) -> Rect {
    let terminal = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(terminal) else {
        panic!("expected full shell layout");
    };
    ui::explorer_layout(main, &state.to_explorer_view_model())
        .overlay
        .expect("active Explorer overlay layout")
        .area
}

fn overlay_areas_for(state: &ShellSession, component: ShellComponent) -> Vec<Rect> {
    state
        .hit_map()
        .regions()
        .iter()
        .filter(|region| {
            region.component == component
                && matches!(
                    region.layer,
                    ShellHitLayer::AppOverlay | ShellHitLayer::ShellModal
                )
        })
        .map(|region| region.area)
        .collect()
}

fn hit_region_center(state: &ShellSession, component: ShellComponent) -> CellPosition {
    let area = state
        .hit_map
        .regions()
        .iter()
        .find(|region| region.component == component)
        .unwrap_or_else(|| panic!("missing {component:?} hit region"))
        .area;
    (
        area.x.saturating_add(area.width / 2),
        area.y.saturating_add(area.height / 2),
    )
}
