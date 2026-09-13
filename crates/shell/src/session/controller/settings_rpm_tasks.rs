#[cfg(target_os = "linux")]
use super::super::*;
#[cfg(target_os = "linux")]
use platform::installation::UpdateBackend;
use platform::{installation::Installation, service::ServiceError, updates::*};

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
pub(in crate::session) enum RpmTaskEvent {
    Installation(Installation),
    Progress(UpdateProgress),
    Completed(Result<RpmTaskOutcome, ServiceError>),
}
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
pub(in crate::session) enum RpmTaskOutcome {
    Check(UpdateCheck),
    Preview(UpdatePreview),
    Result(Option<UpdateResult>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum RpmTask {
    Check,
    Preview,
    Execute,
    Query,
}

#[cfg(target_os = "linux")]
impl ShellSettingsTaskRuntime {
    pub(in crate::session) fn submit_rpm_task(
        &self,
        task: RpmTask,
    ) -> Result<(), i18n::LocalizedText> {
        let group = self.shared.task_group.clone().ok_or_else(|| {
            i18n::LocalizedText::from(ServiceError::ServiceUnavailable.to_string())
        })?;
        let mut slot = self
            .shared
            .update_worker
            .lock()
            .map_err(|_| ServiceError::Busy.to_string())?;
        if slot.is_some() {
            return Err(ServiceError::Busy.to_string().into());
        }
        let shared = self.shared.clone();
        let events = shared.update_event_tx.clone();
        let id = self
            .shared
            .next_request_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let task_id =
            TaskId::new(format!("rpm-update-{}", id % 64)).map_err(|error| error.to_string())?;
        let worker = group
            .spawn_thread(TaskSpec::one_shot(task_id), move || {
                let report = |event| {
                    let _ = events.send(SettingsUpdateTaskEvent::Rpm(event));
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if matches!(task, RpmTask::Check) {
                        let installation = platform::installation::current_installation();
                        let backend = installation.backend;
                        report(RpmTaskEvent::Installation(installation));
                        if backend == UpdateBackend::PortableUser {
                            let result = app::update::check_for_updates(
                                &app::update::current_build_identity(),
                            )
                            .map_err(|error| error.to_string().into());
                            let _ = events.send(SettingsUpdateTaskEvent::CheckCompleted(result));
                            return None;
                        }
                        if backend != UpdateBackend::SystemRpm {
                            return Some(Err(ServiceError::Unsupported));
                        }
                    }
                    Some((|| {
                        let mut client = match shared.rpm_client.lock() {
                            Ok(client) => client,
                            Err(poisoned) => {
                                let mut client = poisoned.into_inner();
                                *client = None;
                                shared.rpm_client.clear_poison();
                                client
                            }
                        };
                        if client.is_none() {
                            let mut updates = platform::linux::updates::RpmUpdates::current()?;
                            if let Some(interaction) = shared
                                .authorization
                                .lock()
                                .map_err(|_| ServiceError::Unknown)?
                                .clone()
                            {
                                updates.set_authorization_interaction(interaction);
                            }
                            *shared
                                .rpm_cancellation
                                .lock()
                                .map_err(|_| ServiceError::Unknown)? = Some(updates.cancellation());
                            *client = Some(updates);
                        }
                        let updates = client.as_mut().ok_or(ServiceError::Unsupported)?;
                        let mut progress = |value| report(RpmTaskEvent::Progress(value));
                        match task {
                            RpmTask::Check => {
                                if let Some(result) = updates.query(&mut progress)? {
                                    return Ok(RpmTaskOutcome::Result(Some(result)));
                                }
                                updates.check(&mut progress).map(RpmTaskOutcome::Check)
                            }
                            RpmTask::Preview => {
                                updates.preview(&mut progress).map(RpmTaskOutcome::Preview)
                            }
                            RpmTask::Execute => updates
                                .execute(&mut progress)
                                .map(|result| RpmTaskOutcome::Result(Some(result))),
                            RpmTask::Query => {
                                updates.query(&mut progress).map(RpmTaskOutcome::Result)
                            }
                        }
                    })())
                }));
                match result {
                    Ok(Some(result)) => report(RpmTaskEvent::Completed(result)),
                    Ok(None) => {}
                    Err(payload) => {
                        report(RpmTaskEvent::Completed(Err(ServiceError::Unknown)));
                        std::panic::resume_unwind(payload);
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        *slot = Some(worker);
        Ok(())
    }
    pub(in crate::session) fn cancel_rpm_task(&self) -> bool {
        self.shared
            .rpm_cancellation
            .lock()
            .is_ok_and(|cancellation| cancellation.as_ref().is_some_and(|value| value.request()))
    }
}
