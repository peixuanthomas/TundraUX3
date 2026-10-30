use super::*;

fn id(screen: &str, focus: &str, overlay: Option<&str>) -> RedrawIdentity {
    RedrawIdentity {
        screen_saver_active: false,
        language_generation: 1,
        screen: screen.into(),
        focus: focus.into(),
        keyboard_focus_visible: false,
        overlay: overlay.map(|id| RedrawOverlayIdentity {
            kind: if id.contains("toast") {
                ui::MotionOverlayKind::Toast
            } else if id.contains("popover") {
                ui::MotionOverlayKind::Popover
            } else {
                ui::MotionOverlayKind::Dialog
            },
            id: id.to_string(),
        }),
    }
}

#[test]
fn initial_idle_and_state_clock_are_event_driven() {
    let origin = Instant::now();
    let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
    assert!(scheduler.is_due(origin));
    scheduler.did_draw(origin);
    assert!(!scheduler.is_due(origin + Duration::from_millis(999)));
    assert_eq!(
        scheduler.poll_timeout(origin, Duration::MAX),
        Duration::from_secs(1)
    );
    assert!(scheduler.is_due(origin + Duration::from_secs(1)));
    scheduler.did_draw(origin + Duration::from_secs(1));
    assert!(!scheduler.is_due(origin + Duration::from_secs(1)));
}

#[test]
fn all_shell_identity_changes_redraw_immediately_without_transitions() {
    let origin = Instant::now();
    for changed in [
        id("settings", "one", None),
        id("home", "two", None),
        RedrawIdentity {
            keyboard_focus_visible: true,
            ..id("home", "one", None)
        },
        id("home", "one", Some("dialog")),
        id("home", "one", Some("popover:menu")),
        id("home", "one", Some("toast:notice")),
    ] {
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        scheduler.did_draw(origin);
        scheduler.request_animation_frame(origin);

        scheduler.observe(origin, changed, false);

        assert!(scheduler.is_due(origin));
        assert_eq!(
            scheduler.transitions(origin),
            ui::MotionTransitions::default()
        );
        scheduler.did_draw(origin);
        assert!(!scheduler.is_due(origin + FRAME_INTERVAL));
    }
}

#[test]
fn reduced_motion_is_forwarded_to_independent_widgets() {
    let origin = Instant::now();
    let scheduler = RedrawScheduler::new(origin, id("home", "one", None), true);
    let frame = scheduler.frame(origin, 125);
    assert!(frame.reduced_motion);
    assert_eq!(frame.animation_speed_percent, 125);
}

#[test]
fn independent_widget_animation_can_request_the_next_frame() {
    let origin = Instant::now();
    let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
    scheduler.did_draw(origin);
    scheduler.request_animation_frame(origin);
    assert_eq!(
        scheduler.poll_timeout(origin, Duration::MAX),
        Duration::from_nanos(33_333_334)
    );
    assert!(!scheduler.is_due(origin + Duration::from_millis(33)));
    assert!(scheduler.is_due(origin + FRAME_INTERVAL));
}

#[test]
fn continuous_color_frames_keep_their_clock_when_motion_is_reduced() {
    let origin = Instant::now();
    let identity = id("editor", "canvas", None);
    let mut scheduler = RedrawScheduler::new(origin, identity.clone(), true);
    scheduler.did_draw(origin);
    scheduler.request_animation_frame(origin);
    assert_eq!(
        scheduler.poll_timeout(origin, Duration::MAX),
        STATE_CLOCK_INTERVAL
    );

    for frame_number in 0..3 {
        let now = origin + FRAME_INTERVAL * frame_number;
        scheduler.request_continuous_frame(now);
        scheduler.observe(now, identity.clone(), true);
        assert_eq!(
            scheduler.poll_timeout(now, Duration::MAX),
            Duration::from_nanos(33_333_334)
        );
        assert!(!scheduler.is_due(now + Duration::from_millis(33)));
        let next = now + FRAME_INTERVAL;
        assert!(scheduler.is_due(next));
        for speed in [0, 25, 100, 200] {
            let frame = scheduler.frame(next, speed);
            assert_eq!(frame.now, next.duration_since(origin));
            assert!(frame.scaled_delta().is_zero());
        }
        scheduler.did_draw(next);
    }
    assert!(!scheduler.is_due(origin + FRAME_INTERVAL * 4));
}
