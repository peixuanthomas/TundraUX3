use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ui::components::{
    ComponentEvent, DataTable, InputEvent, MouseButton, NavRail, NavRailItem, Toast, ToastTone,
};
use ui::{
    FrostMotion, MotionDirection, MotionFrame, MotionIdentity, MotionOverlayIdentity,
    MotionOverlayKind, MotionTimings, MotionTransitionKind, MouseEvent, RenderCapabilities,
    RenderContext, TundraTheme, schedule_motion, schedule_motion_range,
};

#[test]
fn pure_motion_schedule_covers_start_mid_end_interruption_reversal_and_idle() {
    let home = MotionIdentity {
        screen: Some("home"),
        focus: Some("a"),
        overlay: None,
    };
    let settings = MotionIdentity {
        screen: Some("settings"),
        focus: Some("b"),
        overlay: Some(MotionOverlayIdentity {
            kind: MotionOverlayKind::Dialog,
            id: "dialog",
        }),
    };
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
        animation_speed_percent: 100,
    };
    let start = schedule_motion(home, settings, Duration::from_millis(10), frame(10));
    assert!(
        start.active
            && start.screen_transition
            && start.focus_transition
            && start.overlay_transition
    );
    assert!(start.next_redraw_in <= Duration::from_nanos(16_666_667));
    let dialog_start = start.transitions.overlay.expect("dialog transition");
    assert_eq!(dialog_start.kind, MotionTransitionKind::Dialog);
    assert_eq!(dialog_start.direction, MotionDirection::Entering);
    assert_eq!(dialog_start.progress, 0);
    let dialog_mid = schedule_motion(home, settings, Duration::from_millis(10), frame(100))
        .transitions
        .overlay
        .expect("mid dialog transition");
    assert!(dialog_mid.active && dialog_mid.progress > 0 && dialog_mid.progress < 1_000);
    let dialog_final = schedule_motion(home, settings, Duration::from_millis(10), frame(190))
        .transitions
        .overlay
        .expect("final dialog transition");
    assert!(!dialog_final.active);
    assert_eq!(dialog_final.progress, 1_000);
    assert!(!schedule_motion(home, settings, Duration::from_millis(10), frame(231)).active);
    assert!(
        schedule_motion(settings, home, Duration::from_millis(120), frame(120)).active,
        "interruption/reversal restarts from its supplied change time"
    );
    assert!(!schedule_motion(home, home, Duration::ZERO, frame(1)).active);
    assert_eq!(
        schedule_motion(
            home,
            settings,
            Duration::ZERO,
            MotionFrame::reduced(Duration::ZERO)
        ),
        Default::default()
    );

    let popover = MotionIdentity {
        overlay: Some(MotionOverlayIdentity {
            kind: MotionOverlayKind::Popover,
            id: "menu",
        }),
        ..home
    };
    let popover_start = schedule_motion(home, popover, Duration::ZERO, frame(0))
        .transitions
        .overlay
        .expect("popover enter");
    assert_eq!(popover_start.kind, MotionTransitionKind::Popover);
    assert!(popover_start.active);
    assert!(
        !schedule_motion(home, popover, Duration::ZERO, frame(160))
            .transitions
            .overlay
            .expect("popover final")
            .active
    );

    let reversed = schedule_motion(popover, home, Duration::from_millis(80), frame(80))
        .transitions
        .overlay
        .expect("popover reversal");
    assert_eq!(reversed.direction, MotionDirection::Exiting);
    assert_eq!(reversed.progress, 1_000);
}

#[test]
fn ranged_motion_is_continuous_and_overlay_exit_projection_is_endpoint_neutral() {
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
        animation_speed_percent: 100,
    };
    for kind in [
        MotionTransitionKind::Page,
        MotionTransitionKind::Focus,
        MotionTransitionKind::Dialog,
        MotionTransitionKind::Popover,
    ] {
        let entering = schedule_motion_range(
            kind,
            MotionDirection::Entering,
            0,
            1_000,
            Duration::ZERO,
            frame(60),
        );
        let reversed = schedule_motion_range(
            kind,
            MotionDirection::Exiting,
            entering.progress,
            0,
            Duration::from_millis(60),
            frame(60),
        );
        assert_eq!(reversed.progress, entering.progress, "{kind:?} jumped");
        assert_eq!(reversed.phase_progress, 0);
        let settled = schedule_motion_range(
            kind,
            MotionDirection::Exiting,
            entering.progress,
            0,
            Duration::from_millis(60),
            frame(500),
        );
        assert_eq!(settled.progress, 0);
        assert_eq!(settled.phase_progress, 1_000);
        assert!(!settled.active);
        let reduced = schedule_motion_range(
            kind,
            MotionDirection::Exiting,
            entering.progress,
            0,
            Duration::from_millis(60),
            MotionFrame::reduced(Duration::from_millis(60)),
        );
        assert_eq!((reduced.progress, reduced.phase_progress), (0, 1_000));
        assert!(!reduced.active);
    }

    let theme = TundraTheme::default();
    let area = Rect::new(0, 0, 80, 24);
    for kind in [MotionTransitionKind::Dialog, MotionTransitionKind::Popover] {
        let context = |millis| {
            let motion = schedule_motion_range(
                kind,
                MotionDirection::Exiting,
                1_000,
                0,
                Duration::ZERO,
                frame(millis),
            );
            RenderContext::from_theme_with_transitions(
                &theme,
                frame(millis),
                ui::MotionTransitions {
                    overlay: Some(motion),
                    ..ui::MotionTransitions::default()
                },
                RenderCapabilities::default(),
            )
        };
        assert_eq!(context(0).page_area(area), area);
        assert_eq!(context(90).page_area(area), Rect::new(0, 1, 80, 23));
        assert_eq!(context(500).page_area(area), area);
    }
}

#[test]
fn animation_speed_scales_transition_duration_and_effect_delta() {
    let frame = |millis, speed| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::from_millis(16),
        reduced_motion: false,
        animation_speed_percent: speed,
    };
    let transition = |millis, speed| {
        schedule_motion_range(
            MotionTransitionKind::Page,
            MotionDirection::Entering,
            0,
            1_000,
            Duration::ZERO,
            frame(millis, speed),
        )
    };

    assert!(transition(110, 100).active);
    assert!(!transition(110, 200).active);
    assert!(transition(220, 50).active);
    assert_eq!(
        MotionTimings::resolve(frame(0, 200), MotionTimings::PAGE),
        Duration::from_millis(110)
    );
    assert_eq!(frame(0, 200).scaled_delta(), Duration::from_millis(32));
    assert_eq!(
        MotionFrame::reduced(Duration::ZERO).scaled_delta(),
        Duration::ZERO
    );
}

#[test]
fn replacement_interaction_uses_lifecycle_phase_not_carried_visible_progress() {
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
        animation_speed_percent: 100,
    };
    let start = schedule_motion_range(
        MotionTransitionKind::Dialog,
        MotionDirection::Replacing,
        800,
        1_000,
        Duration::from_millis(10),
        frame(10),
    );
    assert_eq!((start.progress, start.phase_progress), (800, 0));
    assert!(!start.interaction_ready());

    let before = ui::MotionTransition {
        phase_progress: 499,
        ..start
    };
    let threshold = ui::MotionTransition {
        phase_progress: 500,
        ..start
    };
    assert!(!before.interaction_ready());
    assert!(threshold.interaction_ready());

    let reduced = schedule_motion_range(
        MotionTransitionKind::Dialog,
        MotionDirection::Replacing,
        800,
        1_000,
        Duration::from_millis(10),
        MotionFrame::reduced(Duration::from_millis(10)),
    );
    assert!(!reduced.active);
    assert!(reduced.interaction_ready());

    let exiting = ui::MotionTransition {
        direction: MotionDirection::Exiting,
        phase_progress: 0,
        ..start
    };
    assert!(exiting.interaction_ready());
}

#[test]
fn dismissed_toast_is_immediately_invisible_in_reduced_motion() {
    let shown = MotionFrame::reduced(Duration::from_millis(10));
    let mut toast = Toast::new("Saved", ToastTone::Success, shown);
    toast.dismiss(MotionFrame::reduced(Duration::from_millis(20)));
    let dismissed = MotionFrame::reduced(Duration::from_millis(20));
    assert!(!toast.is_visible(dismissed));
    let area = Rect::new(0, 0, 12, 3);
    let mut buffer = Buffer::empty(area);
    let context = RenderContext {
        motion: dismissed,
        ..RenderContext::default()
    };
    toast.render(area, &mut buffer, &context);
    assert!(buffer.content().iter().all(|cell| cell.symbol() == " "));
}

#[test]
fn toast_enter_and_exit_progress_remain_self_scheduled_behind_other_overlays() {
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
        animation_speed_percent: 100,
    };
    let mut toast = Toast::new("Saved", ToastTone::Success, frame(0));
    assert_eq!(toast.visible_progress(frame(0)), 0);
    assert!(toast.requests_redraw(frame(0)));
    assert!(toast.visible_progress(frame(100)) > 0);
    let interrupted = toast.visible_progress(frame(75));
    toast.dismiss(frame(75));
    assert_eq!(toast.visible_progress(frame(75)), interrupted);
    assert_eq!(toast.visible_progress(frame(500)), 0);

    let mut toast = Toast::new("Saved", ToastTone::Success, frame(0));
    assert_eq!(toast.visible_progress(frame(200)), 1_000);
    assert!(!toast.requests_redraw(frame(200)));

    toast.dismiss(frame(200));
    assert_eq!(toast.visible_progress(frame(200)), 1_000);
    assert!(toast.requests_redraw(frame(200)));
    let exit_mid = toast.visible_progress(frame(275));
    assert!(exit_mid > 0 && exit_mid < 1_000);
    assert_eq!(toast.visible_progress(frame(350)), 0);
    assert!(!toast.requests_redraw(frame(350)));
    assert!(!toast.is_visible(frame(350)));
}

#[test]
fn toast_resume_reverses_from_the_visible_progress_in_full_and_reduced_motion() {
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
        animation_speed_percent: 100,
    };
    let mut toast = Toast::new("Saved", ToastTone::Info, frame(0));
    toast.dismiss(frame(200));
    let exiting = toast.visible_progress(frame(250));
    toast.resume(frame(250));
    assert_eq!(toast.visible_progress(frame(250)), exiting);
    assert_eq!(toast.visible_progress(frame(500)), 1_000);

    let mut reduced = Toast::new(
        "Saved",
        ToastTone::Info,
        MotionFrame::reduced(Duration::ZERO),
    );
    reduced.dismiss(MotionFrame::reduced(Duration::from_millis(1)));
    reduced.resume(MotionFrame::reduced(Duration::from_millis(2)));
    assert_eq!(
        reduced.visible_progress(MotionFrame::reduced(Duration::from_millis(2))),
        1_000
    );
    assert!(!reduced.requests_redraw(MotionFrame::reduced(Duration::from_millis(2))));
}

#[test]
fn data_table_render_and_hit_rows_agree_with_or_without_title() {
    for titled in [false, true] {
        let area = Rect::new(0, 0, 14, 5);
        let mut table = DataTable::new("table", ["Name"], [["First"], ["Second"]]);
        if titled {
            table = table.titled("Data");
        }
        let mut buffer = Buffer::empty(area);
        table.render(area, &mut buffer, &RenderContext::default());
        assert_eq!(buffer[(1, 2)].symbol(), "F");
        assert_eq!(
            table.handle_event(
                InputEvent::mouse(MouseEvent::down(1, 3, MouseButton::Left)),
                area
            ),
            ComponentEvent::Selected("table".into(), 1)
        );
    }
}

#[test]
fn nav_rail_render_and_hit_rows_agree_with_or_without_title() {
    for titled in [false, true] {
        let area = Rect::new(0, 0, 14, 3);
        let mut rail = NavRail::new(
            "nav",
            vec![
                NavRailItem::new("one", "One"),
                NavRailItem::new("two", "Two"),
            ],
        );
        if titled {
            rail = rail.titled("Nav");
        }
        let mut buffer = Buffer::empty(area);
        rail.render(area, &mut buffer, &RenderContext::default());
        let row_offset = u16::from(titled);
        let rendered_row = (0..area.width)
            .map(|x| buffer[(x, row_offset)].symbol())
            .collect::<String>();
        assert!(
            rendered_row.contains("One"),
            "titled={titled}: {rendered_row:?}"
        );
        assert_eq!(
            rail.handle_event(
                InputEvent::mouse(MouseEvent::down(1, row_offset + 1, MouseButton::Left)),
                area
            ),
            ComponentEvent::Selected("nav".into(), 1)
        );
    }
}

#[test]
fn data_table_padding_uses_terminal_cell_width_for_cjk_and_emoji() {
    let area = Rect::new(0, 0, 12, 4);
    let table = DataTable::new("table", ["甲", "B"], [["中", "🙂"]]);
    let mut buffer = Buffer::empty(area);
    table.render(area, &mut buffer, &RenderContext::default());
    assert_eq!(buffer[(6, 2)].symbol(), "🙂");
}

#[test]
fn frost_motion_only_requests_redraw_while_active_and_respects_reduced_motion() {
    let start = MotionFrame {
        now: Duration::ZERO,
        delta: Duration::ZERO,
        reduced_motion: false,
        animation_speed_percent: 100,
    };
    let mut motion = FrostMotion::default();
    motion.begin(start, MotionTimings::FOCUS);
    assert!(motion.requests_redraw(start));
    assert!(motion.progress(start, true) < 20);

    let halfway = MotionFrame {
        now: Duration::from_millis(60),
        delta: Duration::from_millis(16),
        reduced_motion: false,
        animation_speed_percent: 100,
    };
    assert!(motion.requests_redraw(halfway));
    assert_eq!(motion.progress(halfway, true), 500);
    motion.begin(halfway, MotionTimings::DIALOG);
    assert!(
        motion.requests_redraw(halfway),
        "an interrupted transition restarts at the current frame"
    );

    let finished = MotionFrame {
        now: Duration::from_millis(241),
        delta: Duration::from_millis(16),
        reduced_motion: false,
        animation_speed_percent: 100,
    };
    assert!(!motion.requests_redraw(finished));
    let reduced = MotionFrame::reduced(Duration::from_millis(242));
    motion.begin(reduced, MotionTimings::PAGE);
    assert!(!motion.requests_redraw(reduced));
    assert_eq!(motion.progress(reduced, true), 1_000);

    let context = RenderContext::from_theme_with_motion_preference(
        &TundraTheme::default(),
        Duration::from_millis(300),
        true,
        RenderCapabilities::default(),
    );
    assert!(context.motion.reduced_motion);
    assert_eq!(
        MotionTimings::resolve(context.motion, MotionTimings::DIALOG),
        Duration::ZERO
    );
}
