use super::*;

impl AutoAdminJob {
    pub fn run_approved<T, E>(
        &self,
        approval_error: impl FnOnce(ManagementError) -> E,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        self.wait_for_approval().map_err(approval_error)?;
        operation()
    }

    pub fn finish_result<T, E: fmt::Display>(
        &self,
        result: &Result<T, E>,
        success_message: impl FnOnce(&T) -> String,
    ) {
        self.finish(
            result
                .as_ref()
                .map(success_message)
                .map_err(ToString::to_string),
        );
    }
}

/// Queries may omit AA; mutations always pass their approval job. Keeping the
/// task one-shot prevents account, package and power operations from replaying.
pub fn spawn_task(
    group: &ManagedTaskGroup,
    id: TaskId,
    language: Arc<i18n::LanguageSnapshot>,
    job: Option<&AutoAdminJob>,
    operation: impl FnOnce() + Send + 'static,
) -> Result<ManagedThreadHandle<()>, watchdog::WatchdogError> {
    let worker_job = job.cloned();
    let mut operation = Some(operation);
    let result = group.spawn_thread(TaskSpec::one_shot(id), move || {
        let _language = i18n::enter_snapshot(language.clone());
        let _completion = TaskCompletion(worker_job.clone());
        operation
            .take()
            .expect("an AutoAdmin operation task runs only once")();
    });
    if let Err(error) = &result
        && let Some(job) = job
    {
        job.finish(Err(error.to_string()));
    }
    result
}

struct TaskCompletion(Option<AutoAdminJob>);

impl Drop for TaskCompletion {
    fn drop(&mut self) {
        if let Some(job) = &self.0
            && job.running()
        {
            // Let watchdog capture the panic. AA only closes the unfinished UI,
            // without exposing the panic payload or claiming a write succeeded.
            let message = if std::thread::panicking() {
                watchdog::WatchdogError::TaskPanicked.to_string()
            } else {
                platform::service::ServiceError::Unknown.to_string()
            };
            job.finish(Err(message));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/tasks.rs"]
pub(crate) mod tests;
