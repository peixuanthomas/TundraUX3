use super::*;
use std::time::Duration;

#[test]
fn new_samples_start_after_idle_and_disappearing_values_cancel_their_timers() {
    let mut progress = SpringProgress::default();
    let mut seen = HashSet::new();
    let mut frame = MotionFrame {
        delta: Duration::from_secs(10),
        ..Default::default()
    };
    assert_eq!(
        progress.present("download".into(), Some(70), frame, &mut seen),
        Some(0)
    );
    seen.clear();
    frame.delta = Duration::from_millis(30);
    frame.now = Duration::from_millis(30);
    let displayed = progress
        .present("download".into(), Some(70), frame, &mut seen)
        .unwrap();
    assert!(displayed > 0 && displayed < 7000);
    // Duplicate dashboard representations must not advance twice in one frame.
    assert_eq!(
        progress.present("download".into(), Some(70), frame, &mut seen),
        Some(displayed)
    );
    frame.reduced_motion = true;
    seen.clear();
    assert_eq!(
        progress.present("download".into(), Some(70), frame, &mut seen),
        Some(7000)
    );
    assert!(!progress.update(None, None, frame));
    assert!(progress.values.is_empty());
}

#[test]
fn tween_retargets_displayed_value_without_overshoot_and_completion_is_immediate() {
    let mut progress = SpringProgress::default();
    let frame = |now| MotionFrame {
        now: Duration::from_millis(now),
        ..Default::default()
    };
    let mut sample = |percent, now| {
        progress
            .present(
                "download".into(),
                Some(percent),
                frame(now),
                &mut HashSet::new(),
            )
            .unwrap()
    };
    assert_eq!(sample(80, 0), 0);
    assert_eq!(sample(80, 100), 4_000);
    assert_eq!(sample(20, 100), 4_000);
    assert_eq!(sample(20, 200), 3_000);
    assert_eq!(sample(20, 300), 2_000);
    assert_eq!(sample(100, 301), 10_000);
    assert!(progress.next_redraw_in(frame(301)).is_none());
}

#[test]
fn tween_reports_its_end_deadline_and_does_not_use_idle_delta_to_jump() {
    let mut progress = SpringProgress::default();
    let frame = |now| MotionFrame {
        now: Duration::from_millis(now),
        delta: Duration::from_secs(10),
        ..Default::default()
    };
    assert_eq!(
        progress.present(
            "status".into(),
            Some(50),
            frame(10_000),
            &mut HashSet::new()
        ),
        Some(0)
    );
    progress.present(
        "status".into(),
        Some(50),
        frame(10_199),
        &mut HashSet::new(),
    );
    assert_eq!(
        progress.next_redraw_in(frame(10_199)),
        Some(Duration::from_millis(1))
    );
    assert_eq!(
        progress.present(
            "status".into(),
            Some(50),
            frame(10_200),
            &mut HashSet::new()
        ),
        Some(5_000)
    );
    assert!(progress.next_redraw_in(frame(10_200)).is_none());
}
