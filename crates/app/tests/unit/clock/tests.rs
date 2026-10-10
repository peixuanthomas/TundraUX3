use super::*;
use chrono::TimeZone;

fn snapshot(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> ClockSnapshot {
    let utc = Utc
        .with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .unwrap();
    ClockSnapshot {
        utc,
        date: utc.date_naive(),
        time: utc.time(),
        timezone: Some(chrono_tz::UTC),
        warning: None,
    }
}

#[test]
fn parses_only_strict_hh_mm_ss() {
    assert_eq!(
        parse_hh_mm_ss("23 59 58"),
        Ok(ClockTimeInput {
            hour: 23,
            minute: 59,
            second: 58,
        })
    );
    assert_eq!(
        parse_hh_mm_ss("1 02 03"),
        Err(ClockInputError::InvalidFormat)
    );
    assert_eq!(
        parse_hh_mm_ss("24 00 00"),
        Err(ClockInputError::HourOutOfRange)
    );
    assert_eq!(
        parse_hh_mm_ss("00 60 00"),
        Err(ClockInputError::MinuteOutOfRange)
    );
    assert_eq!(
        parse_hh_mm_ss("00 00 60"),
        Err(ClockInputError::SecondOutOfRange)
    );
}

#[test]
fn countdown_rejects_zero_and_rounds_display_up() {
    let start = snapshot(2026, 7, 10, 10, 0, 0);
    let base = Instant::now();
    let mut scheduler = ClockScheduler::empty();
    assert_eq!(
        scheduler.create_countdown("00 00 00", &start, base),
        Err(ClockSchedulerError::InvalidInput(
            ClockInputError::ZeroCountdown
        ))
    );
    scheduler
        .create_countdown("00 00 02", &start, base)
        .unwrap();
    assert_eq!(
        scheduler.entries(base + Duration::from_millis(1))[0].display_time,
        "00:00:02"
    );
}

#[test]
fn daily_alarm_crosses_midnight_and_fires_exactly_once() {
    let before = snapshot(2026, 7, 10, 23, 59, 58);
    let after = snapshot(2026, 7, 11, 0, 0, 2);
    let base = Instant::now();
    let mut scheduler = ClockScheduler::empty();
    let id = scheduler.create_daily_alarm("00 00 01", &before).unwrap();

    assert_eq!(
        scheduler.advance(&after, base + Duration::from_secs(4))[0].id,
        id
    );
    assert!(
        scheduler
            .advance(&after, base + Duration::from_secs(4))
            .is_empty()
    );
}

#[test]
fn forward_jump_fires_alarm_and_backward_jump_does_not_duplicate_it() {
    let start = snapshot(2026, 7, 10, 9, 55, 0);
    let forward = snapshot(2026, 7, 10, 10, 5, 0);
    let backward = snapshot(2026, 7, 10, 9, 58, 0);
    let forward_again = snapshot(2026, 7, 10, 10, 5, 0);
    let now = Instant::now();
    let mut scheduler = ClockScheduler::empty();
    scheduler.create_daily_alarm("10 00 00", &start).unwrap();

    assert_eq!(scheduler.advance(&forward, now).len(), 1);
    assert!(scheduler.advance(&backward, now).is_empty());
    assert!(scheduler.advance(&forward_again, now).is_empty());
}

#[test]
fn spring_dst_gap_fires_an_alarm_at_the_first_later_local_time() {
    let mut before = snapshot(2026, 3, 8, 6, 59, 59);
    before.date = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
    before.time = NaiveTime::from_hms_opt(1, 59, 59).unwrap();
    before.timezone = Some(chrono_tz::America::New_York);
    let mut after = snapshot(2026, 3, 8, 7, 0, 0);
    after.date = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
    after.time = NaiveTime::from_hms_opt(3, 0, 0).unwrap();
    after.timezone = Some(chrono_tz::America::New_York);
    let mut scheduler = ClockScheduler::empty();
    scheduler.create_daily_alarm("02 30 00", &before).unwrap();

    assert_eq!(scheduler.advance(&after, Instant::now()).len(), 1);
}

#[test]
fn fall_dst_repeated_time_fires_only_in_the_first_occurrence() {
    let mut before = snapshot(2026, 11, 1, 5, 29, 59);
    before.date = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    before.time = NaiveTime::from_hms_opt(1, 29, 59).unwrap();
    before.timezone = Some(chrono_tz::America::New_York);
    let mut first = snapshot(2026, 11, 1, 5, 30, 0);
    first.date = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    first.time = NaiveTime::from_hms_opt(1, 30, 0).unwrap();
    first.timezone = Some(chrono_tz::America::New_York);
    let mut repeated = snapshot(2026, 11, 1, 6, 30, 0);
    repeated.date = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    repeated.time = NaiveTime::from_hms_opt(1, 30, 0).unwrap();
    repeated.timezone = Some(chrono_tz::America::New_York);
    let mut scheduler = ClockScheduler::empty();
    scheduler.create_daily_alarm("01 30 00", &before).unwrap();

    assert_eq!(scheduler.advance(&first, Instant::now()).len(), 1);
    assert!(scheduler.advance(&repeated, Instant::now()).is_empty());
}

#[test]
fn restore_during_fall_back_second_fold_skips_the_missed_first_occurrence() {
    let mut second_fold = snapshot(2026, 11, 1, 6, 15, 0);
    second_fold.date = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    second_fold.time = NaiveTime::from_hms_opt(1, 15, 0).unwrap();
    second_fold.timezone = Some(chrono_tz::America::New_York);
    let profile = ClockProfile {
        next_id: 2,
        entries: vec![ClockEntryRecord::DailyAlarm {
            id: 1,
            hour: 1,
            minute: 30,
            second: 0,
            strong: false,
            snooze_deadline_epoch_ms: None,
        }],
    };
    let (mut scheduler, due) = ClockScheduler::restore(profile, &second_fold, Instant::now());
    let mut repeated_target = snapshot(2026, 11, 1, 6, 30, 0);
    repeated_target.date = second_fold.date;
    repeated_target.time = NaiveTime::from_hms_opt(1, 30, 0).unwrap();
    repeated_target.timezone = second_fold.timezone;

    assert!(due.is_empty());
    assert!(
        scheduler
            .advance(&repeated_target, Instant::now())
            .is_empty()
    );
}

#[test]
fn countdown_uses_monotonic_time_across_utc_corrections() {
    let start = snapshot(2026, 7, 10, 10, 0, 0);
    let corrected = snapshot(2026, 7, 10, 9, 0, 0);
    let base = Instant::now();
    let mut scheduler = ClockScheduler::empty();
    scheduler
        .create_countdown("00 00 10", &start, base)
        .unwrap();

    assert!(
        scheduler
            .advance(&corrected, base + Duration::from_secs(9))
            .is_empty()
    );
    assert_eq!(
        scheduler
            .advance(&corrected, base + Duration::from_secs(10))
            .len(),
        1
    );
    assert!(
        scheduler
            .advance(&corrected, base + Duration::from_secs(11))
            .is_empty()
    );
}

#[test]
fn restoring_expired_countdown_returns_due_and_removes_it() {
    let current = snapshot(2026, 7, 10, 10, 0, 0);
    let profile = ClockProfile {
        next_id: 4,
        entries: vec![ClockEntryRecord::Countdown {
            id: 3,
            deadline_epoch_ms: epoch_millis(current.utc - TimeDelta::seconds(1)),
            strong: true,
        }],
    };

    let (scheduler, due) = ClockScheduler::restore(profile, &current, Instant::now());

    assert!(scheduler.entries(Instant::now()).is_empty());
    assert_eq!(due, vec![DueEvent::countdown(3, true)]);
}

#[test]
fn restore_skips_missed_alarm_and_expired_snooze() {
    let current = snapshot(2026, 7, 10, 10, 5, 0);
    let profile = ClockProfile {
        next_id: 2,
        entries: vec![ClockEntryRecord::DailyAlarm {
            id: 1,
            hour: 10,
            minute: 0,
            second: 0,
            strong: true,
            snooze_deadline_epoch_ms: Some(epoch_millis(current.utc - TimeDelta::seconds(1))),
        }],
    };

    let (mut scheduler, due) = ClockScheduler::restore(profile, &current, Instant::now());

    assert!(due.is_empty());
    assert!(!scheduler.entries(Instant::now())[0].snoozed);
    assert!(scheduler.advance(&current, Instant::now()).is_empty());
}

#[test]
fn restore_normalizes_zero_and_duplicate_entry_ids() {
    let current = snapshot(2026, 7, 10, 10, 0, 0);
    let future = epoch_millis(current.utc + TimeDelta::minutes(5));
    let profile = ClockProfile {
        next_id: 1,
        entries: vec![
            ClockEntryRecord::DailyAlarm {
                id: 0,
                hour: 11,
                minute: 0,
                second: 0,
                strong: false,
                snooze_deadline_epoch_ms: None,
            },
            ClockEntryRecord::Countdown {
                id: 1,
                deadline_epoch_ms: future,
                strong: false,
            },
            ClockEntryRecord::Countdown {
                id: 1,
                deadline_epoch_ms: future,
                strong: true,
            },
        ],
    };

    let now = Instant::now();
    let (scheduler, due) = ClockScheduler::restore(profile, &current, now);
    let exported = scheduler.export_profile(&current, now);
    let ids = exported.entries.iter().map(|record| match record {
        ClockEntryRecord::DailyAlarm { id, .. } | ClockEntryRecord::Countdown { id, .. } => *id,
    });
    let unique = ids.clone().collect::<HashSet<_>>();

    assert!(due.is_empty());
    assert_eq!(unique.len(), 3);
    assert!(ids.clone().all(|id| id > 0));
    assert!(exported.next_id > ids.max().unwrap());
}

#[test]
fn restore_recovers_from_exhaustion_sentinel_and_maximum_ids() {
    let current = snapshot(2026, 7, 10, 10, 0, 0);
    let profile = ClockProfile {
        next_id: u64::MAX,
        entries: vec![
            ClockEntryRecord::DailyAlarm {
                id: u64::MAX,
                hour: 11,
                minute: 0,
                second: 0,
                strong: false,
                snooze_deadline_epoch_ms: None,
            },
            ClockEntryRecord::DailyAlarm {
                id: u64::MAX - 1,
                hour: 12,
                minute: 0,
                second: 0,
                strong: false,
                snooze_deadline_epoch_ms: None,
            },
        ],
    };

    let (mut scheduler, due) = ClockScheduler::restore(profile, &current, Instant::now());
    let created = scheduler
        .create_daily_alarm("13 00 00", &current)
        .expect("low unused IDs remain available");
    let ids = scheduler
        .entries(Instant::now())
        .into_iter()
        .map(|entry| entry.id)
        .collect::<HashSet<_>>();

    assert!(due.is_empty());
    assert!(created > 0 && created < u64::MAX);
    assert_eq!(ids.len(), 3);
    assert!(!ids.contains(&u64::MAX));
}

#[test]
fn strong_alarm_snoozes_for_exactly_five_monotonic_minutes() {
    let start = snapshot(2026, 7, 10, 9, 59, 0);
    let due_at = snapshot(2026, 7, 10, 10, 0, 0);
    let base = Instant::now();
    let mut scheduler = ClockScheduler::empty();
    let id = scheduler.create_daily_alarm("10 00 00", &start).unwrap();
    assert_eq!(scheduler.toggle_strong(id), Some(true));
    assert_eq!(scheduler.advance(&due_at, base).len(), 1);
    scheduler.snooze_five_minutes(id, &due_at, base).unwrap();

    assert!(
        scheduler
            .advance(&due_at, base + SNOOZE_DURATION - Duration::from_nanos(1))
            .is_empty()
    );
    assert_eq!(scheduler.advance(&due_at, base + SNOOZE_DURATION).len(), 1);
    assert!(
        scheduler
            .advance(&due_at, base + SNOOZE_DURATION)
            .is_empty()
    );
}

#[test]
fn export_reprojects_deadline_from_latest_synchronized_utc() {
    let start = snapshot(2026, 7, 10, 10, 0, 0);
    let corrected = snapshot(2026, 7, 10, 12, 0, 5);
    let base = Instant::now();
    let mut scheduler = ClockScheduler::empty();
    scheduler
        .create_countdown("00 00 10", &start, base)
        .unwrap();

    let profile = scheduler.export_profile(&corrected, base + Duration::from_secs(5));
    let ClockEntryRecord::Countdown {
        deadline_epoch_ms, ..
    } = profile.entries[0]
    else {
        panic!("expected countdown");
    };
    assert_eq!(
        deadline_epoch_ms,
        epoch_millis(corrected.utc + TimeDelta::seconds(5))
    );
}
