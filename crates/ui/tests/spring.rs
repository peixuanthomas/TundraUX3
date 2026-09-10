use std::time::Duration;
use ui::SpringValue;

#[test]
fn spring_retargets_without_resetting_visual_position_and_settles() {
    let mut value = SpringValue::default();
    value.retarget(0.75);
    value.advance(Duration::from_millis(100), false);
    assert!(value.value() > 0.0 && value.value() < 0.75);
    let before = value.value();
    value.retarget(0.25);
    assert_eq!(value.value(), before);
    // Arrival of another target must preserve the current direction of travel.
    value.advance(Duration::from_millis(1), false);
    assert!(value.value() > before);
    value.advance(Duration::from_secs(10), false);
    assert_eq!(value.value(), 0.25);
    assert!(!value.is_running());
}

#[test]
fn reduced_motion_finishes_immediately_and_restoring_motion_can_animate_again() {
    let mut value = SpringValue::default();
    value.retarget(1.0);
    value.advance(Duration::ZERO, true);
    assert_eq!(value.value(), 1.0);
    assert!(!value.is_running());
    value.retarget(0.0);
    value.advance(Duration::from_millis(100), false);
    assert!(value.value() > 0.0 && value.value() < 1.0);
}
