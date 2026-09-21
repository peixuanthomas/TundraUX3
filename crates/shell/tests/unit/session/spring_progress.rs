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
