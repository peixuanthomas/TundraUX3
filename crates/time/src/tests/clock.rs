use super::*;
use crate::TimeSyncError;
use chrono::{TimeZone, Timelike};

#[test]
fn advances_anchor_by_elapsed_duration() {
    let anchor = Utc.with_ymd_and_hms(2026, 7, 9, 10, 0, 0).single().unwrap();

    let advanced = advance_utc(anchor, Duration::from_secs(125));

    assert_eq!(advanced.hour(), 10);
    assert_eq!(advanced.minute(), 2);
    assert_eq!(advanced.second(), 5);
}

#[test]
fn converts_synced_utc_to_target_timezone() {
    let mut clock = NetworkClock::new(Some("Asia/Shanghai".to_string()));
    let utc = Utc
        .with_ymd_and_hms(2026, 7, 9, 15, 30, 0)
        .single()
        .unwrap();

    clock.apply_sync(Ok(utc));
    let display = clock.current();

    assert_eq!(display.date.to_string(), "2026-07-09");
    assert_eq!(display.time.hour(), 23);
    assert_eq!(display.time.minute(), 30);
    assert!(display.warning.is_none());
}

#[test]
fn snapshot_keeps_utc_and_local_fields_from_one_read() {
    let mut clock = NetworkClock::new(Some("Asia/Shanghai".to_string()));
    let utc = Utc
        .with_ymd_and_hms(2026, 7, 9, 15, 30, 17)
        .single()
        .unwrap();

    clock.apply_sync(Ok(utc));
    let snapshot = clock.snapshot();

    let projected = snapshot.utc.with_timezone(&chrono_tz::Asia::Shanghai);
    assert_eq!(snapshot.date, projected.date_naive());
    assert_eq!(snapshot.time, projected.time());
    assert_eq!(snapshot.time.second(), 17);
    assert!(snapshot.warning.is_none());
}

#[test]
fn unsynced_clock_uses_target_timezone_instead_of_utc_default() {
    let clock = NetworkClock::new(Some("Asia/Shanghai".to_string()));
    let expected = Utc::now().with_timezone(&chrono_tz::Asia::Shanghai);

    let display = clock.current();

    assert_eq!(display.date, expected.date_naive());
    let delta = display
        .time
        .signed_duration_since(expected.time())
        .num_seconds()
        .abs();
    assert!(delta <= 2, "display time differed by {delta} seconds");
    assert!(display.warning.is_none());
}

#[test]
fn failed_sync_preserves_last_trusted_anchor() {
    let mut clock = NetworkClock::new(Some("UTC".to_string()));
    let utc = Utc
        .with_ymd_and_hms(2026, 7, 9, 15, 30, 0)
        .single()
        .unwrap();
    clock.apply_sync(Ok(utc));

    clock.apply_sync(Err(TimeSyncError::new(vec!["example failed".to_string()])));
    let snapshot = clock.snapshot();

    assert!(clock.anchor.is_some());
    assert_eq!(snapshot.utc.date_naive(), utc.date_naive());
    assert_eq!(snapshot.utc.hour(), utc.hour());
    assert_eq!(snapshot.utc.minute(), utc.minute());
    assert!(
        snapshot
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("continuing last synchronized time"))
    );
}

#[test]
fn first_failed_sync_reports_system_time_fallback() {
    let mut clock = NetworkClock::new(Some("UTC".to_string()));

    clock.apply_sync(Err(TimeSyncError::new(vec!["example failed".to_string()])));
    let snapshot = clock.snapshot();

    assert!(clock.anchor.is_none());
    assert!(
        snapshot
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("using system time"))
    );
}

#[test]
fn invalid_timezone_reports_system_time_fallback() {
    let clock = NetworkClock::new(Some("Not/AZone".to_string()));
    let display = clock.current();

    assert!(
        display
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("Invalid timezone Not/AZone"))
    );
}
