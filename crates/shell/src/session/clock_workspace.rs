use super::{
    window_launch::{WindowApplication, WindowOwner},
    *,
};
use crate::session_services::{
    SessionServicesClient,
    clock::{ClockAnchor, SharedClockProfile},
};
use workspace_protocol::{ServiceOperation, WorkerRole};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WorkspaceClock {
    profile: Option<(String, u64, ClockProfile)>,
    time: Option<u64>,
    publish: Option<ClockAnchor>,
}

impl ShellSession {
    pub(super) fn reset_workspace_clock_profile(&mut self) {
        self.workspace_clock.profile = None;
    }

    pub(super) fn publish_workspace_clock(&mut self, utc: Option<DateTime<Utc>>) {
        if self.window_owner == WindowOwner::Shell {
            self.workspace_clock.publish = Some(ClockAnchor {
                utc,
                observed_at: Utc::now(),
            });
        }
    }

    pub(super) fn write_workspace_clock_profile(
        &mut self,
        client: &SessionServicesClient,
        previous: Option<&ClockScheduler>,
        reproject: bool,
        snapshot: &time::ClockSnapshot,
        now: Instant,
    ) -> Result<(), i18n::LocalizedText> {
        let unavailable =
            || i18n::LocalizedText::from(i18n::msg!("workspace-services-unavailable"));
        let Some((session, revision, stored)) = &mut self.ui.workspace_clock.profile else {
            return Err(unavailable());
        };
        if self
            .app
            .auth_session()
            .is_none_or(|auth| &auth.session_id != session)
        {
            return Err(unavailable());
        }
        let Some(scheduler) = self.ui.clock_scheduler.as_ref() else {
            return Err(unavailable());
        };
        let profile = match previous.filter(|_| !reproject) {
            Some(previous) => scheduler.export_profile_mutation(previous, stored, snapshot, now),
            None => scheduler.export_profile(snapshot, now),
        };
        let updated = SharedClockProfile {
            profile,
            alarm_dates: previous
                .map(|previous| scheduler.changed_alarm_dates(previous))
                .unwrap_or_default(),
        };
        let (next_revision, ()): (_, ()) = client
            .read(ServiceOperation::WriteClockProfile {
                revision: *revision,
                profile: SessionServicesClient::encode(&updated)
                    .map_err(|error| error.to_string())?,
            })
            .map_err(|error| error.to_string())?;
        *revision = next_revision;
        *stored = updated.profile;
        Ok(())
    }

    pub(super) fn advance_workspace_clock(
        &mut self,
        client: &SessionServicesClient,
        snapshot: &time::ClockSnapshot,
        now: Instant,
    ) {
        if self.ui.workspace_clock.profile.is_none() {
            return;
        }
        let Some(scheduler) = self.ui.clock_scheduler.as_mut() else {
            return;
        };
        let previous = scheduler.clone();
        let due = scheduler.advance(snapshot, now);
        let reproject = self.clock_persist_pending;
        if due.is_empty() && !reproject {
            return;
        }
        if let Err(error) =
            self.write_workspace_clock_profile(client, Some(&previous), reproject, snapshot, now)
        {
            self.clock_scheduler = Some(previous);
            self.report_clock_storage_error(error);
            return;
        }
        self.clock_persist_pending = false;
        self.sync_clock_selection_at(now);
        let _ = self.handle_clock_due_events(due);
        self.resolve_notification_alert(CLOCK_STORAGE_ALERT_KEY);
        self.refresh_hit_map();
    }
}

pub(super) fn sync_clock_workspace(
    client: &SessionServicesClient,
    state: &mut ShellSession,
) -> io::Result<()> {
    let shell = client.worker().role == WorkerRole::Shell;
    if shell {
        if let Some(anchor) = state.workspace_clock.publish {
            client.request(ServiceOperation::WriteClockTime {
                anchor: SessionServicesClient::encode(&anchor)?,
            })?;
            state.workspace_clock.publish = None;
        }
    } else if state.window_owner == WindowOwner::App(WindowApplication::Clock) {
        let (revision, anchor): (_, Option<ClockAnchor>) =
            client.read(ServiceOperation::ReadClockTime)?;
        if let Some(anchor) = anchor
            && state.workspace_clock.time != Some(revision)
        {
            state.workspace_clock.time = Some(revision);
            state.time_sync_attempted = true;
            if let Some(utc) = anchor.current_utc() {
                state.last_time_sync_utc = Some(utc);
                state
                    .app
                    .dispatch_at(app::AppCommand::ApplyTimeSync(Ok(utc)), Instant::now());
            }
        }
    } else {
        return Ok(());
    }
    let Some(auth) = state
        .app
        .auth_session()
        .filter(|auth| auth.role != UserRole::Guest)
    else {
        state.workspace_clock.profile = None;
        state.clock_scheduler = None;
        state.clock_profile_pending_sync = None;
        state.pages.clock.clock_create_state = None;
        return Ok(());
    };
    let session = auth.session_id.clone();
    if state
        .workspace_clock
        .profile
        .as_ref()
        .is_some_and(|(old, _, _)| old != &session)
    {
        state.workspace_clock.profile = None;
        state.clock_scheduler = None;
        state.pages.clock.clock_create_state = None;
    }
    let (
        revision,
        SharedClockProfile {
            profile,
            alarm_dates,
        },
    ): (_, SharedClockProfile) = client.read(ServiceOperation::ReadClockProfile)?;
    if state
        .workspace_clock
        .profile
        .as_ref()
        .is_some_and(|(_, old, _)| *old == revision)
    {
        return Ok(());
    }
    if !state.time_sync_attempted && !profile.entries.is_empty() {
        state.clock_profile_pending_sync = Some(profile);
        return Ok(());
    }
    state.clock_profile_pending_sync = None;
    let snapshot = state.app.snapshot().clock;
    let now = Instant::now();
    let previous = state
        .ui
        .workspace_clock
        .profile
        .as_ref()
        .map(|(_, _, profile)| profile);
    state
        .ui
        .clock_scheduler
        .get_or_insert_with(ClockScheduler::default)
        .synchronize_profile(previous, &profile, &alarm_dates, &snapshot, now);
    state.workspace_clock.profile = Some((session, revision, profile));
    state.sync_clock_selection_at(now);
    state.resolve_notification_alert(CLOCK_STORAGE_ALERT_KEY);
    Ok(())
}
