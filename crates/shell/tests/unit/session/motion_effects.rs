use super::*;
use crate::session::runtime::dispatch_motion_aware_input;

const POPOVER_MS: u32 = ui::MotionTimings::POPOVER.as_millis() as u32;

impl ShellMotionEffects {
    // Legacy fixtures provide a completed synthetic frame. Capture its base only
    // when no overlay is painted; phase-specific regressions call captures directly.
    fn test_frame(&mut self, delta: Duration, buffer: &mut Buffer, state: &ShellSession) {
        if current_overlay(state).is_none_or(|overlay| overlay.kind == ui::MotionOverlayKind::Toast)
        {
            self.capture_base(buffer, state);
        }
        self.capture_overlay(buffer, state);
        self.process(delta, buffer, state);
    }
}

#[test]
fn snapshots_exclude_image_protocol_skip_cells() {
    let area = Rect::new(0, 0, 2, 1);
    let mut buffer = Buffer::empty(area);
    buffer[(0, 0)].set_symbol("A");
    buffer[(1, 0)].diff_option = CellDiffOption::Skip;
    let snapshot = snapshot_normal_cells(&buffer, area).unwrap();
    assert_eq!(snapshot.cells.len(), 1);
    assert_eq!(snapshot.cells[0].0, Position::new(0, 0));
}

#[test]
fn clock_dialog_motion_surface_preserves_input_and_button_hit_targets() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.set_navigation_path(vec![ShellScreen::Clock]);
    state.pages.clock.clock_create_state = Some(ClockCreateState::default());
    state.refresh_hit_map();
    let full = Rect::new(0, 0, 120, 40);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(full) else {
        panic!("expected full layout");
    };
    let layout = ui::clock_page_layout(main, &state.to_clock_view_model())
        .create_dialog
        .expect("create dialog");
    assert_eq!(overlay_area(&state), Some(layout.dialog));
    assert_eq!(
        focused_area(&state, ShellComponent::ClockCreateInput),
        Some(layout.input)
    );
    for area in [layout.create_alarm, layout.create_countdown, layout.cancel] {
        let center = (
            area.x.saturating_add(area.width / 2),
            area.y.saturating_add(area.height / 2),
        );
        assert_eq!(
            state
                .route_input_at(
                    InputEvent::mouse_down(PointerButton::Left, center),
                    Instant::now(),
                )
                .command,
            ShellCommand::ClockCreateFormInput(InputEvent::mouse_down(PointerButton::Left, center))
        );
    }
}

#[test]
fn motion_dispatch_consumes_weather_wakeup_before_deferral_and_pre_route_blocking() {
    fn login_exit_fixture(now: Instant) -> (ShellSession, ShellMotionEffects) {
        let full = Rect::new(0, 0, 120, 40);
        let theme = ui::ThemeTokens::glacier_night();
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        while state.notification_dismiss_active_modal_without_response() {}
        state.set_navigation_path(vec![ShellScreen::Login]);
        state.refresh_hit_map();
        let mut motion = ShellMotionEffects::default();
        motion.update(&state, full, full, None, theme, false);
        let mut buffer = Buffer::filled(full, Cell::new("N"));
        motion.test_frame(Duration::ZERO, &mut buffer, &state);
        state.enter_screen(ShellScreen::ExitConfirm);
        state.refresh_hit_map();
        motion.update(&state, full, full, None, theme, false);
        buffer = Buffer::filled(full, Cell::new("A"));
        motion.test_frame(Duration::ZERO, &mut buffer, &state);
        state.screen_saver.last_activity = now;
        (state, motion)
    }

    let platform = platform::mock::UnsupportedPlatform;
    let now = Instant::now();
    let (mut expired, mut expired_motion) = login_exit_fixture(now);
    expired.screen_saver.last_activity =
        now - Duration::from_secs(storage::DEFAULT_SCREEN_TIMEOUT_SECONDS);
    expired.advance_screen_saver_at(now);
    let (_, motion_blocked) = dispatch_motion_aware_input(
        &mut expired,
        &mut expired_motion,
        InputEvent::from_key_label("Esc"),
        &platform,
        now,
    );
    assert!(!motion_blocked);
    assert!(!expired.return_to_lockscreen_requested());
    assert!(!expired.screen_saver_active());
    assert_eq!(expired.active_screen(), ShellScreen::ExitConfirm);
    assert!(expired_motion.deferred_close.is_none());

    let (mut active, mut active_motion) = login_exit_fixture(now);
    let (_, motion_blocked) = dispatch_motion_aware_input(
        &mut active,
        &mut active_motion,
        InputEvent::from_key_label("Esc"),
        &platform,
        now,
    );
    assert!(!motion_blocked);
    assert_eq!(active.screen_saver.last_activity, now);
    assert!(active_motion.deferred_close.is_none());
    assert_ne!(active.active_screen(), ShellScreen::ExitConfirm);

    let (mut clicked, mut clicked_motion) = login_exit_fixture(now);
    let back = clicked
        .hit_map
        .regions()
        .iter()
        .find(|region| region.component == ShellComponent::BackButton)
        .unwrap()
        .area;
    let (_, motion_blocked) = dispatch_motion_aware_input(
        &mut clicked,
        &mut clicked_motion,
        InputEvent::mouse_down(PointerButton::Left, (back.x, back.y)),
        &platform,
        now,
    );
    assert!(motion_blocked);
    assert_eq!(clicked.screen_saver.last_activity, now);
    assert!(clicked_motion.deferred_close.is_some());

    active_motion.deferred_close = None;
    active_motion.exiting = false;
    active_motion.outgoing_block_remaining = Duration::from_millis(50);
    let previous_click = active.last_click;
    let (_, motion_blocked) = dispatch_motion_aware_input(
        &mut active,
        &mut active_motion,
        InputEvent::mouse_down(PointerButton::Left, (4, 4)),
        &platform,
        now + Duration::from_millis(1),
    );
    assert!(motion_blocked);
    assert_eq!(active.last_click, previous_click);
}

#[test]
fn exit_never_overwrites_dynamic_skip_destinations_or_neighbors() {
    let area = Rect::new(0, 0, 4, 1);
    let old = CellSnapshot {
        area,
        cells: area
            .positions()
            .map(|position| (position, Cell::new("O")))
            .collect(),
    };
    for kind in [
        ui::MotionOverlayKind::Dialog,
        ui::MotionOverlayKind::Popover,
    ] {
        for elapsed in [Duration::ZERO, overlay_duration(kind) / 2] {
            let mut natural = Buffer::filled(area, Cell::new("N"));
            natural[(1, 0)].set_symbol("IMG");
            natural[(1, 0)].diff_option = CellDiffOption::Skip;
            natural[(2, 0)].set_symbol("");
            natural[(2, 0)].diff_option = CellDiffOption::Skip;
            let protected_one = natural[(1, 0)].clone();
            let protected_two = natural[(2, 0)].clone();
            outgoing_snapshot_effect(old.clone(), None, kind).process(elapsed, &mut natural, area);
            assert_eq!(natural[(1, 0)], protected_one);
            assert_eq!(natural[(2, 0)], protected_two);
            assert_eq!(natural[(1, 0)].diff_option, CellDiffOption::Skip);
            assert_eq!(natural[(2, 0)].diff_option, CellDiffOption::Skip);
        }
    }
}

#[test]
fn resumed_overlay_exit_keeps_the_original_speed_curve() {
    let area = Rect::new(0, 0, 120, 40);
    let old = Buffer::filled(area, Cell::new("O"));
    let underlay = Buffer::filled(area, Cell::new("N"));
    let snapshot = snapshot_normal_cells(&old, area).unwrap();
    let base = snapshot_normal_cells(&underlay, area).unwrap();
    let mut uninterrupted = outgoing_snapshot_effect(
        snapshot.clone(),
        Some(base.clone()),
        ui::MotionOverlayKind::Dialog,
    );
    let mut resumed = outgoing_snapshot_effect_from(
        snapshot,
        Some(base),
        ui::MotionOverlayKind::Dialog,
        1.0 / 3.0,
        Duration::from_millis(120),
        None,
        ui::RevealOrigin::Center,
        ui::MotionPhases::for_duration(ui::MotionTimings::DIALOG),
    );
    for (full_delta, resumed_delta) in [(60, 0), (30, 30), (45, 45), (45, 45)] {
        let mut expected = underlay.clone();
        let mut actual = underlay.clone();
        uninterrupted.process(Duration::from_millis(full_delta), &mut expected, area);
        resumed.process(Duration::from_millis(resumed_delta), &mut actual, area);
        assert_eq!(actual, expected);
    }
}

#[test]
fn page_remembers_the_opening_button_after_other_clicks_and_keyboard_actions() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    let button = Rect::new(10, 8, 20, 7);
    state.button_regions.push(ui::components::ButtonRegion {
        id: "home.test".into(),
        area: button,
        disabled: false,
    });
    let now = Instant::now();
    state.prepare_input(
        InputEvent::mouse_down(PointerButton::Left, (15, 10)),
        &platform::mock::UnsupportedPlatform,
        now,
    );
    assert_eq!(state.interaction.motion_origin, Some(button));
    state.enter_screen(ShellScreen::Clock);
    assert_eq!(state.content_opening_button(), Some(button));
    state.prepare_input(
        InputEvent::from_key_label("Tab"),
        &platform::mock::UnsupportedPlatform,
        now,
    );
    assert_eq!(state.interaction.motion_origin, None);
    assert_eq!(state.content_opening_button(), Some(button));
    state.terminal_size = (140, 40);
    assert_eq!(state.content_opening_button(), None);
}

#[test]
fn exit_confirmation_preempts_underlying_page_motion() {
    let full = Rect::new(0, 0, 120, 40);
    let main = shell_main_area(full);
    let theme = ui::ThemeTokens::glacier_night();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.refresh_hit_map();

    let mut motion = ShellMotionEffects::default();
    motion.update(&state, full, full, None, theme, false);
    let mut initial = Buffer::empty(full);
    motion.test_frame(Duration::ZERO, &mut initial, &state);
    motion.schedule(
        MotionEffectId::Page,
        page_effect(
            main,
            None,
            false,
            ui::MotionPhases::for_duration(ui::MotionTimings::PAGE),
            ui::RevealOrigin::Right,
        ),
    );
    motion.test_frame(Duration::ZERO, &mut initial, &state);

    state.apply_input_with_platform(
        InputEvent::from_key_label("q"),
        &platform::mock::UnsupportedPlatform,
    );
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    motion.update(&state, full, full, None, theme, false);
    assert_eq!(motion.overlay_gate, Duration::from_millis(180));

    let corner = Position::new(main.x, main.y);
    let exit_area = overlay_area(&state).expect("exit confirmation area");
    assert!(!exit_area.contains(corner), "{exit_area:?}");
    let mut exit_frame = Buffer::empty(full);
    exit_frame[corner].set_symbol("┌").set_fg(theme.border);
    motion.test_frame(Duration::ZERO, &mut exit_frame, &state);
    motion.test_frame(Duration::from_millis(60), &mut exit_frame, &state);

    assert_eq!(exit_frame[corner].symbol(), "┌");
    assert_eq!(exit_frame[corner].fg, theme.border);
    assert!(motion.exit_confirmation);
}

#[test]
fn semantic_cancel_is_deferred_and_replayed_once() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.apply_input(InputEvent::from_key_label("q"));
    let overlay = current_overlay(&state).unwrap();
    let mut motion = ShellMotionEffects {
        overlay: Some(overlay),
        overlay_snapshot: Some(CellSnapshot {
            area: Rect::new(1, 1, 2, 1),
            cells: Vec::new(),
        }),
        overlay_underlay_snapshot: Some(FrozenUnderlaySnapshot {
            screen: state.content_screen(),
            bounds: Rect::new(0, 0, 120, 40),
            snapshot: CellSnapshot {
                area: Rect::new(1, 1, 2, 1),
                cells: Vec::new(),
            },
        }),
        screen: Some(state.content_screen()),
        bounds: Some(Rect::new(0, 0, 120, 40)),
        ..ShellMotionEffects::default()
    };
    let routed = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    assert_eq!(
        motion.intercept_input(&routed),
        MotionInputDisposition::Defer
    );
    assert!(motion.take_deferred_close(&state).is_none());
    motion.overlay_gate = Duration::ZERO;
    assert_eq!(motion.take_deferred_close(&state), Some(routed));
    assert!(motion.take_deferred_close(&state).is_none());
}

#[test]
fn reduced_motion_cancels_effects_and_flushes_pending_close() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    state.apply_input(InputEvent::from_key_label("q"));
    let overlay = current_overlay(&state).unwrap();
    let routed = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    let mut motion = ShellMotionEffects {
        overlay: Some(overlay.clone()),
        deferred_close: Some(DeferredClose { routed, overlay }),
        exiting: true,
        overlay_gate: Duration::from_millis(160),
        ..ShellMotionEffects::default()
    };
    motion.clear();
    assert_eq!(motion.overlay_gate, Duration::ZERO);
    assert!(motion.take_deferred_close(&state).is_some());
    assert!(motion.take_deferred_close(&state).is_none());
}

#[test]
fn reduced_cleanup_invalidates_completed_exit_before_same_id_reopens() {
    fn theme_picker() -> SettingsPickerState {
        SettingsPickerState {
            editing: Default::default(),
            options: Vec::new(),
            kind: ui::SettingsPickerKind::Theme,
            query: String::new(),
            selected_index: 0,
            window_start: 0,
            image_icons_supported: false,
        }
    }

    let full = Rect::new(0, 0, 120, 40);
    let theme = ui::ThemeTokens::glacier_night();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.set_navigation_path(vec![ShellScreen::Settings]);
    state.pages.settings.settings_state = Some(SettingsState {
        form_selected: 0,
        form_interaction: Default::default(),
        application: Default::default(),
        system_time: Default::default(),
        animation_editor: None,
        number_editor: None,
        category: ui::SettingsCategory::Appearance,
        selected_field: ui::SettingsField::Theme,
        status: String::new().into(),
        scroll_offset: 0,
        picker: None,
        color_editor: None,
        weather_location_editor: None,
        file_extensions_editor: None,
        time_sync_server_editor: None,
        time_sync_validation_request_id: None,
    });
    state.refresh_hit_map();
    let mut motion = ShellMotionEffects::default();
    motion.update(&state, full, full, None, theme, false);
    let mut buffer = Buffer::filled(full, Cell::new("N"));
    motion.test_frame(Duration::ZERO, &mut buffer, &state);

    state.pages.settings.settings_state.as_mut().unwrap().picker = Some(theme_picker());
    state.refresh_hit_map();
    motion.update(&state, full, full, None, theme, false);
    let snapshot_area = Rect::new(20, 8, 30, 10);
    motion.overlay_snapshot = Some(CellSnapshot {
        area: snapshot_area,
        cells: snapshot_area
            .positions()
            .map(|position| (position, Cell::new("P")))
            .collect(),
    });
    motion.overlay_underlay_snapshot = Some(FrozenUnderlaySnapshot {
        screen: state.content_screen(),
        bounds: full,
        snapshot: CellSnapshot {
            area: snapshot_area,
            cells: snapshot_area
                .positions()
                .map(|position| (position, Cell::new("N")))
                .collect(),
        },
    });
    assert!(motion.overlay_snapshot.is_some());
    assert!(motion.overlay_underlay_snapshot.is_some());
    let original = current_overlay(&state).expect("theme picker");
    let routed = state.route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    assert_eq!(
        motion.intercept_input(&routed),
        MotionInputDisposition::Defer
    );

    motion.update(&state, full, full, None, theme, true);
    let deferred = motion
        .take_deferred_close(&state)
        .expect("Reduced makes close flushable");
    state.apply_routed_event(
        deferred,
        &platform::mock::UnsupportedPlatform,
        Instant::now(),
    );
    assert!(motion.take_deferred_close(&state).is_none());
    assert_eq!(motion.completed_exit.as_ref(), Some(&original));
    motion.update(&state, full, full, None, theme, true);
    assert!(motion.completed_exit.is_none());
    assert!(!motion.manager.is_running());
    assert_eq!(motion.overlay_gate, Duration::ZERO);

    motion.update(&state, full, full, None, theme, false);
    buffer = Buffer::filled(full, Cell::new("N"));
    motion.test_frame(Duration::ZERO, &mut buffer, &state);
    state.pages.settings.settings_state.as_mut().unwrap().picker = Some(theme_picker());
    state.refresh_hit_map();
    motion.update(&state, full, full, None, theme, false);
    assert_eq!(current_overlay(&state).as_ref(), Some(&original));
    motion.overlay_snapshot = Some(CellSnapshot {
        area: snapshot_area,
        cells: snapshot_area
            .positions()
            .map(|position| (position, Cell::new("P")))
            .collect(),
    });
    motion.overlay_underlay_snapshot = Some(FrozenUnderlaySnapshot {
        screen: state.content_screen(),
        bounds: full,
        snapshot: CellSnapshot {
            area: snapshot_area,
            cells: snapshot_area
                .positions()
                .map(|position| (position, Cell::new("N")))
                .collect(),
        },
    });

    state.pages.settings.settings_state.as_mut().unwrap().picker = None;
    state.refresh_hit_map();
    motion.update(&state, full, full, None, theme, false);
    assert!(motion.completed_exit.is_none());
    assert!(motion.manager.is_running());
    assert_eq!(
        motion.outgoing_block_remaining,
        Duration::from_millis(POPOVER_MS.into())
    );
    assert_eq!(
        motion.overlay_gate,
        Duration::from_millis(POPOVER_MS.into())
    );
}

#[test]
fn missing_overlay_snapshot_closes_immediately_without_invisible_delay() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    state.apply_input(InputEvent::from_key_label("q"));
    let routed = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    let mut motion = ShellMotionEffects {
        overlay: current_overlay(&state),
        ..ShellMotionEffects::default()
    };
    assert_eq!(
        motion.intercept_input(&routed),
        MotionInputDisposition::Apply
    );
    assert_eq!(motion.overlay_gate, Duration::ZERO);
    assert!(!motion.manager.is_running());
}

#[test]
fn resize_bypasses_exit_gate_and_flushes_original_route_once() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    state.apply_input(InputEvent::from_key_label("q"));
    let overlay = current_overlay(&state).unwrap();
    let close = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    let resize = state.clone().route_input_at(
        InputEvent::Resize {
            width: 100,
            height: 30,
        },
        Instant::now(),
    );
    let mut motion = ShellMotionEffects {
        overlay: Some(overlay.clone()),
        deferred_close: Some(DeferredClose {
            routed: close.clone(),
            overlay,
        }),
        exiting: true,
        overlay_gate: Duration::from_millis(180),
        ..ShellMotionEffects::default()
    };
    assert_eq!(
        motion.intercept_input(&resize),
        MotionInputDisposition::Apply
    );
    let generation = state.hit_map_generation();
    state.apply_input(resize.input.clone());
    assert_eq!(state.terminal_size(), (100, 30));
    assert!(state.hit_map_generation() > generation);
    assert_eq!(state.hit_map().terminal_size(), (100, 30));
    motion.cancel_for_bounds_change();
    assert_eq!(motion.take_deferred_close(&state), Some(close));
    assert!(motion.take_deferred_close(&state).is_none());
    assert!(!motion.manager.is_running());
    assert!(motion.overlay_snapshot.is_none());
}

#[test]
fn suspend_flushes_close_before_resume_resize_rebuilds_geometry() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.apply_input(InputEvent::from_key_label("q"));
    let original = current_overlay(&state).unwrap();
    let routed = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    let mut motion = ShellMotionEffects {
        overlay: Some(original),
        overlay_snapshot: Some(CellSnapshot {
            area: Rect::new(20, 10, 40, 10),
            cells: Vec::new(),
        }),
        overlay_underlay_snapshot: Some(FrozenUnderlaySnapshot {
            screen: state.content_screen(),
            bounds: Rect::new(0, 0, 120, 40),
            snapshot: CellSnapshot {
                area: Rect::new(20, 10, 40, 10),
                cells: Vec::new(),
            },
        }),
        screen: Some(state.content_screen()),
        bounds: Some(Rect::new(0, 0, 120, 40)),
        ..ShellMotionEffects::default()
    };
    assert_eq!(
        motion.intercept_input(&routed),
        MotionInputDisposition::Defer
    );
    let routed = motion.cancel_for_suspend(&state).expect("close flush");
    let platform = platform::native_platform();
    state.apply_routed_event(routed, platform.as_ref(), Instant::now());
    assert!(current_overlay(&state).is_none());
    assert!(motion.cancel_for_suspend(&state).is_none());

    let generation = state.hit_map_generation();
    state.apply_input(InputEvent::Resize {
        width: 100,
        height: 30,
    });
    assert_eq!(state.terminal_size(), (100, 30));
    assert!(state.hit_map_generation() > generation);
    assert_eq!(state.hit_map().terminal_size(), (100, 30));
    assert!(current_overlay(&state).is_none());
}

#[test]
fn preempted_overlay_never_retargets_deferred_cancel() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    state.apply_input(InputEvent::from_key_label("q"));
    let overlay = OverlayIdentity {
        category: ShellOverlayCategory::ShellModal,
        kind: ui::MotionOverlayKind::Dialog,
        id: "preempted:A".into(),
        immediate: false,
    };
    let routed = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    let mut motion = ShellMotionEffects {
        deferred_close: Some(DeferredClose {
            routed,
            overlay: overlay.clone(),
        }),
        exiting: true,
        ..ShellMotionEffects::default()
    };
    assert_eq!(
        current_overlay(&state).unwrap().kind,
        ui::MotionOverlayKind::Dialog
    );
    assert_ne!(current_overlay(&state), Some(overlay));
    assert!(motion.take_deferred_close(&state).is_none());
    assert!(motion.deferred_close.is_none());
}

#[test]
fn critical_modal_preempts_then_restores_original_deferred_route() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    state.apply_input(InputEvent::from_key_label("q"));
    let original = current_overlay(&state).unwrap();
    assert!(!original.immediate);
    let routed = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    let mut motion = ShellMotionEffects {
        overlay: Some(original.clone()),
        overlay_snapshot: Some(CellSnapshot {
            area: Rect::new(20, 10, 40, 10),
            cells: Vec::new(),
        }),
        overlay_underlay_snapshot: Some(FrozenUnderlaySnapshot {
            screen: state.content_screen(),
            bounds: Rect::new(0, 0, 120, 40),
            snapshot: CellSnapshot {
                area: Rect::new(20, 10, 40, 10),
                cells: Vec::new(),
            },
        }),
        screen: Some(state.content_screen()),
        bounds: Some(Rect::new(0, 0, 120, 40)),
        ..ShellMotionEffects::default()
    };
    assert_eq!(
        motion.intercept_input(&routed),
        MotionInputDisposition::Defer
    );

    state.notify_critical_modal("Critical", "Interrupt", Vec::new());
    motion.update(
        &state,
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 120, 40),
        Some(Rect::new(0, 37, 120, 3)),
        ui::ThemeTokens::glacier_night(),
        false,
    );
    assert!(current_overlay(&state).unwrap().immediate);
    assert_eq!(motion.overlay_gate, Duration::ZERO);
    assert!(!motion.exiting);
    assert!(motion.overlay_snapshot.is_none());
    assert!(motion.take_deferred_close(&state).is_none());
    let critical_input = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Tab"), Instant::now());
    assert_eq!(
        motion.intercept_input(&critical_input),
        MotionInputDisposition::Apply
    );

    assert!(state.notification_dismiss_active_modal_without_response());
    assert_eq!(current_overlay(&state), Some(original.clone()));
    motion.update(
        &state,
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 120, 40),
        Some(Rect::new(0, 37, 120, 3)),
        ui::ThemeTokens::glacier_night(),
        false,
    );
    assert_eq!(motion.take_deferred_close(&state), Some(routed));
    assert!(motion.take_deferred_close(&state).is_none());
}

#[test]
fn critical_modal_without_prior_overlay_is_immediate_and_ungated() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    let mut motion = ShellMotionEffects::default();
    motion.update(
        &state,
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 120, 40),
        None,
        ui::ThemeTokens::glacier_night(),
        false,
    );
    state.notify_critical_modal("Critical", "Natural", Vec::new());
    motion.update(
        &state,
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 120, 40),
        None,
        ui::ThemeTokens::glacier_night(),
        false,
    );
    assert!(motion.overlay.as_ref().unwrap().immediate);
    assert_eq!(motion.overlay_gate, Duration::ZERO);
    assert!(motion.overlay_snapshot.is_none());
    let pointer = state.clone().route_input_at(
        InputEvent::mouse_down(PointerButton::Left, (1, 1)),
        Instant::now(),
    );
    assert_eq!(
        motion.intercept_input(&pointer),
        MotionInputDisposition::Apply
    );
}

#[test]
fn completed_notification_exit_schedules_promoted_notification_enter() {
    let full = Rect::new(0, 0, 120, 40);
    let theme = ui::ThemeTokens::glacier_night();
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    while state.notification_dismiss_active_modal_without_response() {}
    state.refresh_hit_map();
    let mut motion = ShellMotionEffects::default();
    motion.update(&state, full, full, None, theme, false);
    let mut natural = Buffer::filled(full, Cell::new("N"));
    motion.test_frame(Duration::ZERO, &mut natural, &state);

    state.notify_modal("A", "First", ui::NotificationTone::Info, Vec::new());
    state.notify_modal("B", "Second", ui::NotificationTone::Info, Vec::new());
    state.refresh_hit_map();
    motion.update(&state, full, full, None, theme, false);
    let first = current_overlay(&state).expect("first notification");
    let mut first_frame = Buffer::filled(full, Cell::new("A"));
    motion.test_frame(Duration::ZERO, &mut first_frame, &state);
    let cancel = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    assert_eq!(
        motion.intercept_input(&cancel),
        MotionInputDisposition::Defer
    );
    motion.test_frame(Duration::from_millis(180), &mut first_frame, &state);
    motion.test_frame(Duration::from_millis(180), &mut first_frame, &state);
    let routed = motion
        .take_deferred_close(&state)
        .expect("completed cancel");
    state.apply_routed_event(routed, platform::native_platform().as_ref(), Instant::now());
    state.refresh_hit_map();
    let second = current_overlay(&state).expect("promoted notification");
    assert_ne!(second, first);

    motion.update(&state, full, full, None, theme, false);
    assert!(motion.manager.is_running());
    assert_eq!(motion.overlay_gate, Duration::from_millis(180));
    assert!(motion.completed_exit.is_none());
    let activate = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Enter"), Instant::now());
    assert_eq!(
        motion.intercept_input(&activate),
        MotionInputDisposition::Block
    );
    let mut second_frame = Buffer::filled(full, Cell::new("B"));
    motion.test_frame(Duration::from_secs(1), &mut second_frame, &state);
    assert!(motion.manager.is_running());
    assert_eq!(motion.overlay_gate, Duration::from_millis(180));
    assert!(motion.take_deferred_close(&state).is_none());
}

#[test]
fn asynchronous_overlay_preserves_remaining_visual_outgoing() {
    let full = Rect::new(0, 0, 120, 40);
    let theme = ui::ThemeTokens::glacier_night();
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    while state.notification_dismiss_active_modal_without_response() {}
    state.refresh_hit_map();
    let mut motion = ShellMotionEffects::default();
    motion.update(&state, full, full, None, theme, false);
    let mut buffer = Buffer::filled(full, Cell::new("N"));
    motion.test_frame(Duration::ZERO, &mut buffer, &state);

    state.notify_modal("A", "Outgoing", ui::NotificationTone::Info, Vec::new());
    state.refresh_hit_map();
    motion.update(&state, full, full, None, theme, false);
    buffer = Buffer::filled(full, Cell::new("A"));
    motion.test_frame(Duration::ZERO, &mut buffer, &state);
    assert!(state.notification_dismiss_active_modal_without_response());
    state.refresh_hit_map();
    motion.update(&state, full, full, None, theme, false);
    assert_eq!(
        motion.active_visual_outgoing.as_ref().unwrap().remaining,
        Duration::from_millis(180)
    );
    motion.test_frame(Duration::from_secs(1), &mut buffer, &state);
    motion.test_frame(Duration::from_millis(60), &mut buffer, &state);
    assert_eq!(
        motion.active_visual_outgoing.as_ref().unwrap().remaining,
        Duration::from_millis(120)
    );

    state.notify_modal("B", "Incoming", ui::NotificationTone::Info, Vec::new());
    state.refresh_hit_map();
    motion.update(&state, full, full, None, theme, false);
    assert_eq!(
        motion.active_visual_outgoing.as_ref().unwrap().remaining,
        Duration::from_millis(120)
    );
    assert_eq!(motion.outgoing_block_remaining, Duration::from_millis(120));
    assert_eq!(motion.overlay_gate, Duration::from_millis(300));
    motion.test_frame(Duration::from_secs(1), &mut buffer, &state);
    assert_eq!(motion.outgoing_block_remaining, Duration::from_millis(120));
    motion.test_frame(Duration::from_millis(120), &mut buffer, &state);
    assert!(motion.active_visual_outgoing.is_none());
    assert_eq!(motion.outgoing_block_remaining, Duration::ZERO);
    assert!(motion.manager.is_running());
    assert_eq!(motion.overlay_gate, Duration::from_millis(180));
}

#[test]
fn generic_and_explorer_menus_both_animate_and_gate_pointer_input() {
    let full = Rect::new(0, 0, 120, 40);
    let theme = ui::ThemeTokens::glacier_night();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    let mut motion = ShellMotionEffects::default();
    motion.update(&state, full, full, None, theme, false);
    let mut buffer = Buffer::filled(full, Cell::new("N"));
    let natural = buffer.clone();
    motion.capture_base(&buffer, &state);
    state.interaction.motion_origin = Some(Rect::new(20, 10, 1, 1));
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Home),
        anchor: (20, 10),
    });
    state.pages.common.focused_component = ShellComponent::ContextMenu;
    state.refresh_hit_map();
    motion.update(&state, full, full, None, theme, false);
    let generic_input = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Enter"), Instant::now());
    assert_eq!(
        motion.intercept_input(&generic_input),
        MotionInputDisposition::Block
    );
    motion.test_frame(Duration::from_millis(16), &mut buffer, &state);
    assert_eq!(buffer, natural);
    assert!(motion.manager.is_running());
    assert_eq!(motion.overlay_gate, Duration::from_millis(160));
    motion.test_frame(Duration::from_millis(160), &mut buffer, &state);
    state.pages.common.active_popup = None;
    state.pages.common.focused_component = ShellComponent::Home;
    state.refresh_hit_map();
    motion.update(&state, full, full, None, theme, false);
    assert!(motion.manager.is_running());
    motion.test_frame(Duration::ZERO, &mut buffer, &state);
    motion.test_frame(Duration::from_millis(160), &mut buffer, &state);

    state.set_navigation_path(vec![ShellScreen::Explorer]);
    state.replace_explorer_state(Some(ExplorerState::new(".", false)));
    state.pages.explorer.explorer_overlay_mode = Some(ExplorerOverlayMode::Options);
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (20, 10),
    });
    state.pages.common.focused_component = ShellComponent::Explorer;
    state.refresh_hit_map();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(full) else {
        panic!("expected full layout");
    };
    let expected = ui::explorer_layout(main, &state.to_explorer_view_model())
        .overlay
        .expect("Explorer semantic overlay")
        .area;
    assert_eq!(overlay_area(&state), Some(expected));
    motion.update(&state, full, full, None, theme, false);
    assert!(motion.manager.is_running());
    assert_eq!(motion.overlay_gate, Duration::from_millis(160));
}

#[test]
fn identity_sync_gates_confirm_before_first_render() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    assert!(current_overlay(&state).is_none());
    let mut motion = ShellMotionEffects::default();
    motion.update(
        &state,
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 120, 40),
        Some(Rect::new(0, 37, 120, 3)),
        ui::ThemeTokens::glacier_night(),
        false,
    );
    state.apply_input(InputEvent::from_key_label("q"));
    assert!(!current_overlay(&state).unwrap().immediate);
    assert!(overlay_area(&state).is_some());
    motion.update(
        &state,
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 120, 40),
        Some(Rect::new(0, 37, 120, 3)),
        ui::ThemeTokens::glacier_night(),
        false,
    );
    assert!(motion.overlay_gate > Duration::ZERO);
    let confirm = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Enter"), Instant::now());
    assert_eq!(
        motion.intercept_input(&confirm),
        MotionInputDisposition::Block
    );
    let outside = state.clone().route_input_at(
        InputEvent::mouse_down(PointerButton::Left, (0, 0)),
        Instant::now(),
    );
    assert_eq!(
        motion.intercept_input(&outside),
        MotionInputDisposition::Block
    );
}

#[test]
fn replacement_missing_required_old_geometry_falls_back_without_gate() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    let mut motion = ShellMotionEffects {
        overlay: Some(OverlayIdentity {
            category: ShellOverlayCategory::ShellModal,
            kind: ui::MotionOverlayKind::Popover,
            id: "preempted".into(),
            immediate: false,
        }),
        ..ShellMotionEffects::default()
    };
    state.apply_input(InputEvent::from_key_label("q"));
    motion.update(
        &state,
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 120, 40),
        Some(Rect::new(0, 37, 120, 3)),
        ui::ThemeTokens::glacier_night(),
        false,
    );
    assert_eq!(motion.overlay_gate, Duration::ZERO);
    assert_eq!(motion.outgoing_block_remaining, Duration::ZERO);
    let confirm = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Enter"), Instant::now());
    assert_eq!(
        motion.intercept_input(&confirm),
        MotionInputDisposition::Apply
    );
}

fn exit_buffer(kind: ui::MotionOverlayKind, elapsed: Duration) -> Buffer {
    let full = Rect::new(0, 0, 11, 7);
    let area = Rect::new(2, 1, 7, 5);
    let mut old = Buffer::filled(full, Cell::new("O"));
    for position in full
        .positions()
        .filter(|position| !area.contains(*position))
    {
        old[position].set_symbol("Z");
    }
    let snapshot = snapshot_normal_cells(&old, area).unwrap();
    let mut underlay_buffer = Buffer::filled(full, Cell::new("N"));
    for position in full
        .positions()
        .filter(|position| !area.contains(*position))
    {
        underlay_buffer[position].set_symbol("Z");
    }
    let underlay = snapshot_normal_cells(&underlay_buffer, area).unwrap();
    let mut current = old;
    let mut effect = outgoing_snapshot_effect(snapshot, Some(underlay), kind);
    effect.process(elapsed, &mut current, full);
    current
}

#[test]
fn dialog_and_popover_snapshot_exits_are_spatial_single_stage_and_end_natural() {
    let full = Rect::new(0, 0, 11, 7);
    let area = Rect::new(2, 1, 7, 5);
    for kind in [
        ui::MotionOverlayKind::Dialog,
        ui::MotionOverlayKind::Popover,
    ] {
        let start = exit_buffer(kind, Duration::ZERO);
        assert!(
            area.positions()
                .all(|position| start[position].symbol() == "O")
        );
        assert!(
            full.positions()
                .filter(|position| !area.contains(*position))
                .all(|position| start[position].symbol() == "Z")
        );
        let duration = overlay_duration(kind);
        let final_frame = exit_buffer(kind, duration);
        assert!(
            area.positions()
                .all(|position| final_frame[position].symbol() == "N")
        );

        let mut old = Buffer::filled(area, Cell::new("O"));
        let snapshot = snapshot_normal_cells(&old, area).unwrap();
        let mut effect = outgoing_snapshot_effect(snapshot, None, kind);
        effect.process(
            duration.saturating_sub(Duration::from_millis(1)),
            &mut old,
            area,
        );
        assert!(effect.running());
        effect.process(Duration::from_millis(1), &mut old, area);
        assert!(!effect.running());
    }
    let dialog_mid = exit_buffer(
        ui::MotionOverlayKind::Dialog,
        overlay_duration(ui::MotionOverlayKind::Dialog) / 2,
    );
    let popover_mid = exit_buffer(
        ui::MotionOverlayKind::Popover,
        overlay_duration(ui::MotionOverlayKind::Popover) / 2,
    );
    assert_ne!(dialog_mid, popover_mid);
}

#[test]
fn post_mutation_outgoing_blocks_second_escape_only_for_old_phase() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.set_navigation_path(vec![ShellScreen::Settings]);
    state.pages.settings.settings_state = Some(SettingsState {
        form_selected: 0,
        form_interaction: Default::default(),
        application: Default::default(),
        system_time: Default::default(),
        animation_editor: None,
        number_editor: None,
        category: ui::SettingsCategory::Appearance,
        selected_field: ui::SettingsField::Theme,
        status: String::new().into(),
        scroll_offset: 0,
        picker: Some(SettingsPickerState {
            editing: Default::default(),
            options: Vec::new(),
            kind: ui::SettingsPickerKind::Theme,
            query: String::new(),
            selected_index: 0,
            window_start: 0,
            image_icons_supported: false,
        }),
        color_editor: None,
        weather_location_editor: None,
        file_extensions_editor: None,
        time_sync_server_editor: None,
        time_sync_validation_request_id: None,
    });
    state.refresh_hit_map();
    let old = current_overlay(&state).expect("settings picker");
    let area = Rect::new(20, 8, 30, 10);
    let mut old_buffer = Buffer::filled(area, Cell::new("P"));
    old_buffer[(20, 8)].set_symbol("P");
    let mut motion = ShellMotionEffects {
        overlay: Some(old),
        overlay_snapshot: snapshot_normal_cells(&old_buffer, area),
        overlay_underlay_snapshot: Some(FrozenUnderlaySnapshot {
            screen: state.content_screen(),
            bounds: Rect::new(0, 0, 120, 40),
            snapshot: CellSnapshot {
                area,
                cells: area
                    .positions()
                    .map(|position| (position, Cell::new("N")))
                    .collect(),
            },
        }),
        bounds: Some(Rect::new(0, 0, 120, 40)),
        screen: Some(state.content_screen()),
        ..ShellMotionEffects::default()
    };
    state.pages.settings.settings_state.as_mut().unwrap().picker = None;
    state.refresh_hit_map();
    motion.update(
        &state,
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 120, 40),
        None,
        ui::ThemeTokens::glacier_night(),
        false,
    );
    assert_eq!(
        motion.outgoing_block_remaining,
        Duration::from_millis(POPOVER_MS.into())
    );
    let second_escape = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    assert_eq!(
        motion.intercept_input(&second_escape),
        MotionInputDisposition::Block
    );
    let screen = state.content_screen();
    let mut natural = Buffer::filled(Rect::new(0, 0, 120, 40), Cell::new("N"));
    motion.test_frame(Duration::from_secs(1), &mut natural, &state);
    assert_eq!(state.content_screen(), screen);
    assert_eq!(
        motion.outgoing_block_remaining,
        Duration::from_millis(POPOVER_MS.into())
    );
    motion.test_frame(
        Duration::from_millis(POPOVER_MS.into()),
        &mut natural,
        &state,
    );
    assert_eq!(motion.outgoing_block_remaining, Duration::ZERO);
    assert_eq!(
        motion.intercept_input(&second_escape),
        MotionInputDisposition::Apply
    );
}

#[test]
fn stale_base_screen_or_bounds_falls_back_to_immediate_close() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    let mut motion = ShellMotionEffects {
        base_snapshot: Some(BaseFrameSnapshot {
            screen: ShellScreen::Settings,
            bounds: Rect::new(0, 0, 80, 24),
            cells: vec![(Position::new(1, 1), Cell::new("N"))].into(),
        }),
        bounds: Some(Rect::new(0, 0, 120, 40)),
        screen: Some(state.content_screen()),
        ..ShellMotionEffects::default()
    };
    state.apply_input(InputEvent::from_key_label("q"));
    motion.update(
        &state,
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 120, 40),
        Some(Rect::new(0, 37, 120, 3)),
        ui::ThemeTokens::glacier_night(),
        false,
    );
    assert!(motion.overlay_underlay_snapshot.is_none());
    motion.overlay_snapshot = Some(CellSnapshot {
        area: Rect::new(20, 10, 40, 10),
        cells: vec![(Position::new(20, 10), Cell::new("A"))],
    });
    let close = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    assert_eq!(
        motion.intercept_input(&close),
        MotionInputDisposition::Apply
    );
    assert!(motion.deferred_close.is_none());
}

#[test]
fn phased_capture_reveals_current_underlay_without_rewinding_chrome() {
    let full = Rect::new(0, 0, 120, 40);
    let theme = ui::ThemeTokens::glacier_night();
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    while state.notification_dismiss_active_modal_without_response() {}
    state.notify_modal("Live", "Dismiss me", ui::NotificationTone::Info, Vec::new());
    state.refresh_hit_map();
    let area = overlay_area(&state).unwrap();
    let mut motion = ShellMotionEffects::default();
    let layout = ui::ShellFrameLayout::new(full, None, &ui::RenderContext::default());
    motion.update_layout(&state, &layout, theme, false);
    let mut frame = Buffer::filled(full, Cell::new("B"));
    motion.capture_base(&frame, &state);
    for position in area.positions() {
        frame[position].set_symbol("D");
    }
    motion.capture_overlay(&frame, &state);
    motion.process(Duration::ZERO, &mut frame, &state);
    let cancel = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    assert_eq!(
        motion.intercept_input(&cancel),
        MotionInputDisposition::Defer
    );

    // The modal remains in state during deferred close, but its base is redrawn.
    frame = Buffer::filled(full, Cell::new("N"));
    let clock = Position::new(full.right() - 2, full.bottom() - 2);
    assert!(!area.contains(clock));
    frame[clock].set_symbol("9");
    motion.capture_base(&frame, &state);
    for position in area.positions() {
        frame[position].set_symbol("D");
    }
    motion.capture_overlay(&frame, &state);
    motion.process(Duration::ZERO, &mut frame, &state);
    motion.process(Duration::from_millis(180), &mut frame, &state);
    assert!(
        area.positions()
            .all(|position| frame[position].symbol() == "N")
    );
    assert_eq!(frame[clock].symbol(), "9");
    assert!(motion.take_deferred_close(&state).is_some());
    assert!(motion.take_deferred_close(&state).is_none());
}

#[test]
fn snapshot_exit_protects_wide_glyphs_overlapping_image_cells() {
    let area = Rect::new(0, 0, 3, 1);
    let mut old = Buffer::filled(area, Cell::new("O"));
    old[(0, 0)].set_symbol("界");
    let snapshot = snapshot_normal_cells(&old, area).unwrap();
    let mut current = Buffer::filled(area, Cell::new("N"));
    current[(1, 0)].diff_option = CellDiffOption::Skip;
    outgoing_snapshot_effect(snapshot, None, ui::MotionOverlayKind::Dialog).process(
        Duration::ZERO,
        &mut current,
        area,
    );
    assert_eq!(current[(0, 0)].symbol(), "N");
    assert_eq!(current[(1, 0)].diff_option, CellDiffOption::Skip);
    old[(1, 0)].diff_option = CellDiffOption::Skip;
    let captured = snapshot_normal_cells(&old, area).unwrap();
    assert!(captured.cells.iter().all(|(position, _)| position.x == 2));
}

#[test]
fn shell_modal_outgoing_keeps_its_capture_phase_until_finished() {
    let full = Rect::new(0, 0, 120, 40);
    let theme = ui::ThemeTokens::glacier_night();
    let layout = ui::ShellFrameLayout::new(full, None, &ui::RenderContext::default());
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    while state.notification_dismiss_active_modal_without_response() {}
    state.notify_modal("Modal", "Outgoing", ui::NotificationTone::Info, Vec::new());
    state.refresh_hit_map();
    let mut motion = ShellMotionEffects::default();
    motion.update_layout(&state, &layout, theme, false);
    let base = Buffer::filled(full, Cell::new("B"));
    motion.capture_base(&base, &state);
    let mut frame = Buffer::filled(full, Cell::new("D"));
    motion.capture_overlay(&frame, &state);
    assert!(motion.needs_shell_modal_base());
    state.notification_dismiss_active_modal_without_response();
    state.refresh_hit_map();
    motion.update_layout(&state, &layout, theme, false);
    assert!(current_overlay(&state).is_none());
    assert!(motion.needs_shell_modal_base());
    motion.process(Duration::ZERO, &mut frame, &state);
    motion.process(Duration::from_millis(180), &mut frame, &state);
    assert!(!motion.needs_shell_modal_base());
}

fn clock_overlay_covered_by_shell_modal() -> (ShellSession, ShellMotionEffects, ui::ShellFrameLayout)
{
    page_overlay_covered_by_shell_modal(ShellScreen::Clock)
}

fn page_overlay_covered_by_shell_modal(
    screen: ShellScreen,
) -> (ShellSession, ShellMotionEffects, ui::ShellFrameLayout) {
    let full = Rect::new(0, 0, 120, 40);
    let layout = ui::ShellFrameLayout::new(full, None, &ui::RenderContext::default());
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.set_navigation_path(vec![screen]);
    if screen == ShellScreen::Clock {
        state.pages.clock.clock_create_state = Some(ClockCreateState::default());
    } else {
        state.app.dispatch_at(
            app::AppCommand::SetEditorState(Some(EditorState::untitled(
                app::editor::DocumentKind::PlainText,
            ))),
            Instant::now(),
        );
        state.pages.editor.editor_open_menu = Some(ui::EditorMenu::File);
    }
    state.refresh_hit_map();
    let mut motion = ShellMotionEffects::default();
    let theme = ui::ThemeTokens::glacier_night();
    motion.update_layout(&state, &layout, theme, false);
    let mut frame = Buffer::filled(full, Cell::new("B"));
    motion.capture_base(&frame, &state);
    for position in overlay_area(&state).unwrap().positions() {
        frame[position].set_symbol("P");
    }
    motion.capture_overlay(&frame, &state);
    motion.process(Duration::ZERO, &mut frame, &state);
    motion.process(Duration::from_millis(180), &mut frame, &state);
    state.notify_modal(
        "Cover",
        "Shell overlay",
        ui::NotificationTone::Info,
        Vec::new(),
    );
    state.refresh_hit_map();
    motion.update_layout(&state, &layout, theme, false);
    assert_eq!(motion.overlay_gate, Duration::from_millis(180));
    assert!(
        motion.active_visual_outgoing.is_none(),
        "covering does not close the page overlay"
    );
    motion.capture_base(&frame, &state);
    for position in overlay_area(&state).unwrap().positions() {
        frame[position].set_symbol("S");
    }
    motion.capture_overlay(&frame, &state);
    motion.process(Duration::ZERO, &mut frame, &state);
    motion.process(Duration::from_millis(180), &mut frame, &state);
    (state, motion, layout)
}

#[test]
fn closing_shell_modal_only_reveals_its_existing_page_overlay() {
    let (mut state, mut motion, layout) = clock_overlay_covered_by_shell_modal();
    state.notification_dismiss_active_modal_without_response();
    state.refresh_hit_map();
    motion.update_layout(&state, &layout, ui::ThemeTokens::glacier_night(), false);
    assert_eq!(motion.overlay_gate, Duration::from_millis(180));
    assert!(motion.needs_shell_modal_base());
    let mut frame = Buffer::filled(layout.bounds, Cell::new("P"));
    motion.capture_base(&frame, &state);
    motion.capture_overlay(&frame, &state);
    motion.process(Duration::ZERO, &mut frame, &state);
    motion.process(Duration::from_millis(180), &mut frame, &state);
    assert!(!motion.is_running(), "revealed page must not enter again");
    assert_eq!(motion.overlay_gate, Duration::ZERO);
}

#[test]
fn keyboard_motion_cancellation_preserves_parent_overlay_origins() {
    let (state, mut motion, layout) = clock_overlay_covered_by_shell_modal();
    motion.overlay_frames[0].1 = ui::RevealOrigin::Button(Rect::new(8, 10, 12, 3));
    let origins = motion.overlay_frames.clone();
    motion.update_layout(&state, &layout, ui::ThemeTokens::glacier_night(), true);
    assert_eq!(motion.overlay_frames, origins);
    assert!(!motion.is_running());
}

#[test]
fn deferred_shell_modal_close_does_not_reenter_covered_page() {
    let (mut state, mut motion, layout) = clock_overlay_covered_by_shell_modal();
    let cancel = state
        .clone()
        .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
    assert_eq!(
        motion.intercept_input(&cancel),
        MotionInputDisposition::Defer
    );
    let mut frame = Buffer::filled(layout.bounds, Cell::new("P"));
    motion.process(Duration::ZERO, &mut frame, &state);
    motion.process(Duration::from_millis(180), &mut frame, &state);
    assert!(motion.take_deferred_close(&state).is_some());
    state.notification_dismiss_active_modal_without_response();
    state.refresh_hit_map();
    motion.update_layout(&state, &layout, ui::ThemeTokens::glacier_night(), false);
    assert_eq!(motion.overlay_gate, Duration::ZERO);
    motion.process(Duration::ZERO, &mut frame, &state);
    assert!(!motion.is_running());
}

#[test]
fn new_page_overlay_after_shell_close_still_enters() {
    let (mut state, mut motion, layout) = page_overlay_covered_by_shell_modal(ShellScreen::Editor);
    state.notification_dismiss_active_modal_without_response();
    state.pages.editor.editor_quick_menu_anchor = None;
    state.pages.editor.editor_open_menu = Some(ui::EditorMenu::Edit);
    state.refresh_hit_map();
    motion.update_layout(&state, &layout, ui::ThemeTokens::glacier_night(), false);
    assert_eq!(motion.overlay_gate, Duration::from_millis(340));
    assert!(motion.active_visual_outgoing.is_some());
}

#[test]
fn changing_pages_cancels_old_outgoing_cells_but_keeps_new_page_entry() {
    let (mut state, mut motion, layout) = clock_overlay_covered_by_shell_modal();
    let theme = ui::ThemeTokens::glacier_night();
    state.notification_dismiss_active_modal_without_response();
    state.refresh_hit_map();
    motion.update_layout(&state, &layout, theme, false);
    let mut old = Buffer::filled(layout.bounds, Cell::new("P"));
    motion.process(Duration::ZERO, &mut old, &state);
    assert!(motion.active_visual_outgoing.is_some());
    state.pages.clock.clock_create_state = None;
    state.set_navigation_path(vec![ShellScreen::Editor]);
    state.refresh_hit_map();
    let old_base = motion.base_snapshot.clone();
    motion.update_layout(&state, &layout, theme, false);
    assert!(motion.active_visual_outgoing.is_none());
    assert!(motion.base_snapshot.is_none());
    assert!(motion.overlay_snapshot.is_none());
    let mut actual = Buffer::filled(layout.bounds, Cell::new("N"));
    let mut expected = actual.clone();
    page_effect(
        layout.main,
        old_base,
        false,
        ui::MotionPhases::for_duration(ui::MotionTimings::PAGE),
        ui::RevealOrigin::Right,
    )
    .process(Duration::ZERO, &mut expected, layout.bounds);
    motion.process(Duration::ZERO, &mut actual, &state);
    assert_eq!(actual, expected);
}
