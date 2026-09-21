use chrono::{
    DateTime, Days, LocalResult, NaiveDate, NaiveTime, TimeDelta, TimeZone, Timelike, Utc,
};
use std::collections::HashSet;
use std::fmt;
use std::time::{Duration, Instant};
use storage::{ClockEntryRecord, ClockProfile};
use time::ClockSnapshot;

pub const SNOOZE_DURATION: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockTimeInput {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl ClockTimeInput {
    fn duration(self) -> Duration {
        Duration::from_secs(
            u64::from(self.hour) * 60 * 60 + u64::from(self.minute) * 60 + u64::from(self.second),
        )
    }

    fn naive_time(self) -> NaiveTime {
        NaiveTime::from_hms_opt(
            u32::from(self.hour),
            u32::from(self.minute),
            u32::from(self.second),
        )
        .expect("validated clock input must form a time")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockInputError {
    InvalidFormat,
    HourOutOfRange,
    MinuteOutOfRange,
    SecondOutOfRange,
    ZeroCountdown,
}

impl fmt::Display for ClockInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => formatter.write_str("Use the format hh mm ss"),
            Self::HourOutOfRange => formatter.write_str("Hours must be between 00 and 23"),
            Self::MinuteOutOfRange => formatter.write_str("Minutes must be between 00 and 59"),
            Self::SecondOutOfRange => formatter.write_str("Seconds must be between 00 and 59"),
            Self::ZeroCountdown => formatter.write_str("Countdown must be greater than 00 00 00"),
        }
    }
}

impl std::error::Error for ClockInputError {}

pub fn parse_hh_mm_ss(value: &str) -> Result<ClockTimeInput, ClockInputError> {
    let bytes = value.as_bytes();
    if bytes.len() != 8
        || bytes[2] != b' '
        || bytes[5] != b' '
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 2 || index == 5 || byte.is_ascii_digit())
    {
        return Err(ClockInputError::InvalidFormat);
    }

    let field = |start: usize| (bytes[start] - b'0') * 10 + (bytes[start + 1] - b'0');
    let input = ClockTimeInput {
        hour: field(0),
        minute: field(3),
        second: field(6),
    };
    if input.hour > 23 {
        return Err(ClockInputError::HourOutOfRange);
    }
    if input.minute > 59 {
        return Err(ClockInputError::MinuteOutOfRange);
    }
    if input.second > 59 {
        return Err(ClockInputError::SecondOutOfRange);
    }

    Ok(input)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockEntryKind {
    DailyAlarm,
    Countdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockEntryView {
    pub id: u64,
    pub kind: ClockEntryKind,
    pub strong: bool,
    pub display_time: String,
    pub snoozed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueEvent {
    pub id: u64,
    pub kind: ClockEntryKind,
    pub strong: bool,
    pub display_time: String,
}

impl DueEvent {
    fn alarm(id: u64, time: NaiveTime, strong: bool) -> Self {
        Self {
            id,
            kind: ClockEntryKind::DailyAlarm,
            strong,
            display_time: format_time(time),
        }
    }

    fn countdown(id: u64, strong: bool) -> Self {
        Self {
            id,
            kind: ClockEntryKind::Countdown,
            strong,
            display_time: "00:00:00".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockSchedulerError {
    InvalidInput(ClockInputError),
    IdSpaceExhausted,
    EntryNotFound,
    SnoozeRequiresStrongAlarm,
}

impl fmt::Display for ClockSchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(error) => error.fmt(formatter),
            Self::IdSpaceExhausted => formatter.write_str("No clock entry IDs remain"),
            Self::EntryNotFound => formatter.write_str("Clock entry no longer exists"),
            Self::SnoozeRequiresStrongAlarm => {
                formatter.write_str("Only a strong alarm can be snoozed")
            }
        }
    }
}

impl std::error::Error for ClockSchedulerError {}

impl From<ClockInputError> for ClockSchedulerError {
    fn from(error: ClockInputError) -> Self {
        Self::InvalidInput(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeDeadline {
    anchor: Instant,
    duration: Duration,
}

impl RuntimeDeadline {
    fn new(anchor: Instant, duration: Duration) -> Self {
        Self { anchor, duration }
    }

    fn from_utc(deadline: DateTime<Utc>, snapshot: &ClockSnapshot, now: Instant) -> Option<Self> {
        let duration = deadline.signed_duration_since(snapshot.utc).to_std().ok()?;
        (!duration.is_zero()).then_some(Self::new(now, duration))
    }

    fn elapsed(&self, now: Instant) -> Duration {
        now.checked_duration_since(self.anchor).unwrap_or_default()
    }

    fn remaining(&self, now: Instant) -> Duration {
        self.duration.saturating_sub(self.elapsed(now))
    }

    fn is_due(&self, now: Instant) -> bool {
        self.elapsed(now) >= self.duration
    }

    fn projected_epoch_ms(&self, snapshot: &ClockSnapshot, now: Instant) -> u64 {
        let remaining = TimeDelta::from_std(self.remaining(now)).unwrap_or(TimeDelta::MAX);
        let deadline = snapshot
            .utc
            .checked_add_signed(remaining)
            .unwrap_or(DateTime::<Utc>::MAX_UTC);
        epoch_millis(deadline)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeEntry {
    DailyAlarm {
        id: u64,
        time: NaiveTime,
        strong: bool,
        next_date: NaiveDate,
        snooze: Option<RuntimeDeadline>,
    },
    Countdown {
        id: u64,
        strong: bool,
        deadline: RuntimeDeadline,
    },
}

impl RuntimeEntry {
    fn id(&self) -> u64 {
        match self {
            Self::DailyAlarm { id, .. } | Self::Countdown { id, .. } => *id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockScheduler {
    next_id: u64,
    entries: Vec<RuntimeEntry>,
}

impl Default for ClockScheduler {
    fn default() -> Self {
        Self {
            next_id: 1,
            entries: Vec::new(),
        }
    }
}

impl ClockScheduler {
    #[cfg(test)]
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn restore(
        profile: ClockProfile,
        snapshot: &ClockSnapshot,
        now: Instant,
    ) -> (Self, Vec<DueEvent>) {
        Self::from_profile(profile, snapshot, now)
    }

    pub fn from_profile(
        profile: ClockProfile,
        snapshot: &ClockSnapshot,
        now: Instant,
    ) -> (Self, Vec<DueEvent>) {
        let mut due = Vec::new();
        let mut entries = Vec::with_capacity(profile.entries.len());
        let mut used_ids = HashSet::with_capacity(profile.entries.len());

        for record in profile.entries {
            match record {
                ClockEntryRecord::DailyAlarm {
                    id,
                    hour,
                    minute,
                    second,
                    strong,
                    snooze_deadline_epoch_ms,
                } => {
                    let Some(time) = NaiveTime::from_hms_opt(
                        u32::from(hour),
                        u32::from(minute),
                        u32::from(second),
                    ) else {
                        continue;
                    };
                    let Some(id) = normalize_record_id(id, &mut used_ids) else {
                        continue;
                    };
                    let snooze = snooze_deadline_epoch_ms
                        .and_then(utc_from_epoch_millis)
                        .and_then(|deadline| RuntimeDeadline::from_utc(deadline, snapshot, now));
                    entries.push(RuntimeEntry::DailyAlarm {
                        id,
                        time,
                        strong,
                        next_date: next_alarm_date(snapshot, time),
                        snooze,
                    });
                }
                ClockEntryRecord::Countdown {
                    id,
                    deadline_epoch_ms,
                    strong,
                } => {
                    let Some(deadline) = utc_from_epoch_millis(deadline_epoch_ms) else {
                        continue;
                    };
                    let Some(id) = normalize_record_id(id, &mut used_ids) else {
                        continue;
                    };
                    if deadline <= snapshot.utc {
                        due.push(DueEvent::countdown(id, strong));
                        continue;
                    }
                    if let Some(deadline) = RuntimeDeadline::from_utc(deadline, snapshot, now) {
                        entries.push(RuntimeEntry::Countdown {
                            id,
                            strong,
                            deadline,
                        });
                    }
                }
            }
        }

        let next_id = if is_allocatable_id(profile.next_id) && !used_ids.contains(&profile.next_id)
        {
            profile.next_id
        } else {
            first_available_id(|candidate| used_ids.contains(&candidate)).unwrap_or(u64::MAX)
        };
        (Self { next_id, entries }, due)
    }

    pub fn create_daily_alarm(
        &mut self,
        value: &str,
        snapshot: &ClockSnapshot,
    ) -> Result<u64, ClockSchedulerError> {
        let input = parse_hh_mm_ss(value)?;
        self.add_daily_alarm(input, snapshot)
    }

    pub fn add_daily_alarm(
        &mut self,
        input: ClockTimeInput,
        snapshot: &ClockSnapshot,
    ) -> Result<u64, ClockSchedulerError> {
        validate_input(input)?;
        let id = self.allocate_id()?;
        let time = input.naive_time();
        self.entries.push(RuntimeEntry::DailyAlarm {
            id,
            time,
            strong: false,
            next_date: next_alarm_date(snapshot, time),
            snooze: None,
        });
        Ok(id)
    }

    pub fn create_countdown(
        &mut self,
        value: &str,
        snapshot: &ClockSnapshot,
        now: Instant,
    ) -> Result<u64, ClockSchedulerError> {
        let input = parse_hh_mm_ss(value)?;
        self.add_countdown(input, snapshot, now)
    }

    pub fn add_countdown(
        &mut self,
        input: ClockTimeInput,
        _snapshot: &ClockSnapshot,
        now: Instant,
    ) -> Result<u64, ClockSchedulerError> {
        validate_input(input)?;
        let duration = input.duration();
        if duration.is_zero() {
            return Err(ClockInputError::ZeroCountdown.into());
        }
        let id = self.allocate_id()?;
        self.entries.push(RuntimeEntry::Countdown {
            id,
            strong: false,
            deadline: RuntimeDeadline::new(now, duration),
        });
        Ok(id)
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let previous_len = self.entries.len();
        self.entries.retain(|entry| entry.id() != id);
        self.entries.len() != previous_len
    }

    pub fn toggle_strong(&mut self, id: u64) -> Option<bool> {
        self.entries.iter_mut().find_map(|entry| match entry {
            RuntimeEntry::DailyAlarm {
                id: entry_id,
                strong,
                ..
            }
            | RuntimeEntry::Countdown {
                id: entry_id,
                strong,
                ..
            } if *entry_id == id => {
                *strong = !*strong;
                Some(*strong)
            }
            _ => None,
        })
    }

    pub fn snooze_five_minutes(
        &mut self,
        id: u64,
        _snapshot: &ClockSnapshot,
        now: Instant,
    ) -> Result<(), ClockSchedulerError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.id() == id)
            .ok_or(ClockSchedulerError::EntryNotFound)?;
        match entry {
            RuntimeEntry::DailyAlarm {
                strong: true,
                snooze,
                ..
            } => {
                *snooze = Some(RuntimeDeadline::new(now, SNOOZE_DURATION));
                Ok(())
            }
            _ => Err(ClockSchedulerError::SnoozeRequiresStrongAlarm),
        }
    }

    pub fn entries(&self, now: Instant) -> Vec<ClockEntryView> {
        self.entries
            .iter()
            .map(|entry| match entry {
                RuntimeEntry::DailyAlarm {
                    id,
                    time,
                    strong,
                    snooze,
                    ..
                } => ClockEntryView {
                    id: *id,
                    kind: ClockEntryKind::DailyAlarm,
                    strong: *strong,
                    display_time: format_time(*time),
                    snoozed: snooze.is_some(),
                },
                RuntimeEntry::Countdown {
                    id,
                    strong,
                    deadline,
                } => ClockEntryView {
                    id: *id,
                    kind: ClockEntryKind::Countdown,
                    strong: *strong,
                    display_time: format_duration_ceil(deadline.remaining(now)),
                    snoozed: false,
                },
            })
            .collect()
    }

    pub fn advance(&mut self, snapshot: &ClockSnapshot, now: Instant) -> Vec<DueEvent> {
        let current_local = snapshot.date.and_time(snapshot.time);
        let mut due = Vec::new();
        let mut index = 0;
        while index < self.entries.len() {
            match &mut self.entries[index] {
                RuntimeEntry::DailyAlarm {
                    id,
                    time,
                    strong,
                    next_date,
                    snooze,
                } => {
                    if snooze.as_ref().is_some_and(|deadline| deadline.is_due(now)) {
                        due.push(DueEvent::alarm(*id, *time, *strong));
                        *snooze = None;
                    }

                    let target = next_date.and_time(*time);
                    if current_local >= target {
                        due.push(DueEvent::alarm(*id, *time, *strong));
                        *next_date = next_alarm_date(snapshot, *time);
                    }
                    index += 1;
                }
                RuntimeEntry::Countdown {
                    id,
                    strong,
                    deadline,
                } if deadline.is_due(now) => {
                    due.push(DueEvent::countdown(*id, *strong));
                    self.entries.remove(index);
                }
                RuntimeEntry::Countdown { .. } => index += 1,
            }
        }
        due
    }

    pub fn export_profile(&self, snapshot: &ClockSnapshot, now: Instant) -> ClockProfile {
        let entries = self
            .entries
            .iter()
            .map(|entry| match entry {
                RuntimeEntry::DailyAlarm {
                    id,
                    time,
                    strong,
                    snooze,
                    ..
                } => ClockEntryRecord::DailyAlarm {
                    id: *id,
                    hour: time.hour() as u8,
                    minute: time.minute() as u8,
                    second: time.second() as u8,
                    strong: *strong,
                    snooze_deadline_epoch_ms: snooze
                        .as_ref()
                        .map(|deadline| deadline.projected_epoch_ms(snapshot, now)),
                },
                RuntimeEntry::Countdown {
                    id,
                    strong,
                    deadline,
                } => ClockEntryRecord::Countdown {
                    id: *id,
                    deadline_epoch_ms: deadline.projected_epoch_ms(snapshot, now),
                    strong: *strong,
                },
            })
            .collect();
        ClockProfile {
            next_id: self.next_id,
            entries,
        }
    }

    fn allocate_id(&mut self) -> Result<u64, ClockSchedulerError> {
        let id = if is_allocatable_id(self.next_id)
            && !self.entries.iter().any(|entry| entry.id() == self.next_id)
        {
            self.next_id
        } else {
            first_available_id(|candidate| self.entries.iter().any(|entry| entry.id() == candidate))
                .ok_or(ClockSchedulerError::IdSpaceExhausted)?
        };
        self.next_id = id.saturating_add(1);
        Ok(id)
    }
}

fn next_alarm_date(snapshot: &ClockSnapshot, alarm: NaiveTime) -> NaiveDate {
    let fires_later_today = snapshot.timezone.and_then(|timezone| {
        daily_trigger_utc(snapshot.date, alarm, timezone).map(|target| snapshot.utc < target)
    });
    if fires_later_today.unwrap_or(snapshot.time < alarm) {
        snapshot.date
    } else {
        snapshot
            .date
            .checked_add_days(Days::new(1))
            .unwrap_or(snapshot.date)
    }
}

fn daily_trigger_utc(
    date: NaiveDate,
    alarm: NaiveTime,
    timezone: chrono_tz::Tz,
) -> Option<DateTime<Utc>> {
    let mut local = date.and_time(alarm);
    for _ in 0..=2 * 24 * 60 * 60 {
        match timezone.from_local_datetime(&local) {
            LocalResult::Single(value) => return Some(value.with_timezone(&Utc)),
            LocalResult::Ambiguous(first, second) => {
                return Some(first.min(second).with_timezone(&Utc));
            }
            LocalResult::None => {
                local = local.checked_add_signed(TimeDelta::seconds(1))?;
            }
        }
    }
    None
}

fn validate_input(input: ClockTimeInput) -> Result<(), ClockInputError> {
    if input.hour > 23 {
        return Err(ClockInputError::HourOutOfRange);
    }
    if input.minute > 59 {
        return Err(ClockInputError::MinuteOutOfRange);
    }
    if input.second > 59 {
        return Err(ClockInputError::SecondOutOfRange);
    }
    Ok(())
}

fn normalize_record_id(raw_id: u64, used_ids: &mut HashSet<u64>) -> Option<u64> {
    if is_allocatable_id(raw_id) && used_ids.insert(raw_id) {
        return Some(raw_id);
    }

    let candidate = first_available_id(|candidate| used_ids.contains(&candidate))?;
    used_ids.insert(candidate);
    Some(candidate)
}

fn is_allocatable_id(id: u64) -> bool {
    id > 0 && id < u64::MAX
}

fn first_available_id(mut is_used: impl FnMut(u64) -> bool) -> Option<u64> {
    let mut candidate = 1_u64;
    while candidate < u64::MAX {
        if !is_used(candidate) {
            return Some(candidate);
        }
        candidate = candidate.saturating_add(1);
    }
    None
}

fn utc_from_epoch_millis(value: u64) -> Option<DateTime<Utc>> {
    i64::try_from(value)
        .ok()
        .and_then(DateTime::<Utc>::from_timestamp_millis)
}

fn epoch_millis(value: DateTime<Utc>) -> u64 {
    u64::try_from(value.timestamp_millis()).unwrap_or_default()
}

fn format_time(time: NaiveTime) -> String {
    time.format("%H:%M:%S").to_string()
}

fn format_duration_ceil(duration: Duration) -> String {
    let seconds = duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() != 0));
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
#[path = "../tests/unit/clock_scheduler/tests.rs"]
mod tests;
