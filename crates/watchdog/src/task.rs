use crate::report::ExecutionContext;
use crate::runtime::{self, AppWatchdog};
use crate::{
    BoundaryKind, PanicAction, RecoveryOutcome, ReplaySafety, TaskGroupShutdown, TaskSpec,
    WatchdogError,
};
#[cfg(feature = "tokio")]
use futures_util::FutureExt;
use std::collections::{HashMap, VecDeque};
#[cfg(feature = "tokio")]
use std::future::Future;
#[cfg(feature = "tokio")]
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) struct GroupState {
    tasks: Mutex<HashMap<String, Arc<TaskControl>>>,
    closed: AtomicBool,
}

struct TaskControl {
    cancelled: AtomicBool,
    completed: AtomicBool,
    aborts: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
}

impl TaskControl {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let aborts = self
            .aborts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        for abort in aborts {
            abort();
        }
    }

    fn add_abort(&self, abort: Arc<dyn Fn() + Send + Sync>) {
        self.aborts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(abort.clone());
        if self.cancelled.load(Ordering::Acquire) {
            abort();
        }
    }
}

struct CompletionGuard(Arc<TaskControl>);

impl Drop for CompletionGuard {
    fn drop(&mut self) {
        self.0.completed.store(true, Ordering::Release);
    }
}

impl GroupState {
    fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
        }
    }

    fn register(&self, id: &str) -> Result<Arc<TaskControl>, WatchdogError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(WatchdogError::InvalidTaskPolicy(format!(
                "managed task group is closed; task {id} cannot be started"
            )));
        }
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.closed.load(Ordering::Acquire) {
            return Err(WatchdogError::InvalidTaskPolicy(format!(
                "managed task group is closed; task {id} cannot be started"
            )));
        }
        if tasks
            .get(id)
            .is_some_and(|task| !task.completed.load(Ordering::Acquire))
        {
            return Err(WatchdogError::InvalidTaskPolicy(format!(
                "managed task {id} is already running"
            )));
        }
        let control = Arc::new(TaskControl {
            cancelled: AtomicBool::new(false),
            completed: AtomicBool::new(false),
            aborts: Mutex::new(Vec::new()),
        });
        tasks.insert(id.to_string(), control.clone());
        Ok(control)
    }

    pub(crate) fn cancel_all(&self) {
        let tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for task in tasks.values() {
            task.cancel();
        }
    }

    pub(crate) fn close_and_cancel(&self) {
        self.closed.store(true, Ordering::Release);
        self.cancel_all();
    }

    pub(crate) fn counts(&self) -> TaskGroupShutdown {
        let tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let completed = tasks
            .values()
            .filter(|task| task.completed.load(Ordering::Acquire))
            .count();
        TaskGroupShutdown {
            completed,
            still_running: tasks.len().saturating_sub(completed),
        }
    }

    pub(crate) fn wait(&self, timeout: Duration) -> TaskGroupShutdown {
        let deadline = Instant::now().checked_add(timeout);
        loop {
            let counts = self.counts();
            if counts.still_running == 0
                || deadline.is_some_and(|deadline| Instant::now() >= deadline)
            {
                return counts;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    pub(crate) fn running_task_names(&self) -> Vec<String> {
        self.tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|(_, task)| !task.completed.load(Ordering::Acquire))
            .map(|(name, _)| name.clone())
            .collect()
    }
}

#[derive(Clone)]
pub struct ManagedTaskGroup {
    app: AppWatchdog,
    name: String,
    state: Arc<GroupState>,
}

pub struct ManagedThreadHandle<T> {
    join: Option<JoinHandle<Option<T>>>,
    control: Arc<TaskControl>,
}

#[cfg(feature = "tokio")]
pub struct ManagedTaskHandle<T> {
    join: tokio::task::JoinHandle<Option<T>>,
    control: Arc<TaskControl>,
}

#[cfg(feature = "tokio")]
pub struct ManagedLocalTaskHandle<T> {
    join: tokio::task::JoinHandle<Option<T>>,
    control: Arc<TaskControl>,
}

impl ManagedTaskGroup {
    pub(crate) fn new(app: AppWatchdog, name: &str) -> Self {
        let key = format!("{}/{name}", app.component_path());
        let state = {
            let mut groups = app
                .process
                .shared
                .groups
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            groups
                .entry(key)
                .or_insert_with(|| Arc::new(GroupState::new()))
                .clone()
        };
        Self {
            app,
            name: name.to_string(),
            state,
        }
    }

    pub fn spawn_thread<T, F>(
        &self,
        spec: TaskSpec,
        mut factory: F,
    ) -> Result<ManagedThreadHandle<T>, WatchdogError>
    where
        T: Send + 'static,
        F: FnMut() -> T + Send + 'static,
    {
        self.validate_spec(&spec)?;
        let task_name = format!("{}/{}", self.name, spec.id);
        let control = self.state.register(&task_name)?;
        let worker_control = control.clone();
        let app = self.app.clone();
        let thread_spec = spec.clone();
        let join = thread::Builder::new()
            .name(format!("{}/{}", app.component_path(), spec.id))
            .spawn(move || {
                let _completion = CompletionGuard(worker_control.clone());
                let mut restarts = VecDeque::new();
                let mut attempt = 0_usize;
                loop {
                    if worker_control.cancelled.load(Ordering::Acquire) {
                        return None;
                    }
                    let context = task_context(&app, &thread_spec, &task_name, attempt);
                    match runtime::catch_factory(context.clone(), &mut factory) {
                        Ok(value) => {
                            return (!worker_control.cancelled.load(Ordering::Acquire))
                                .then_some(value);
                        }
                        Err(payload) => {
                            let caught = app
                                .caught_from_context(context, payload_to_string(payload.as_ref()));
                            let recovery = restart_recovery(
                                &app,
                                &thread_spec,
                                &mut restarts,
                                "managed thread is being restarted",
                                "managed thread stopped after panic",
                            );
                            let should_continue = recovery.is_recovered();
                            let _ = caught.finalize_detached(recovery);
                            if !should_continue {
                                return None;
                            }
                            let delay = thread_spec.restart_policy.delay_for(attempt);
                            attempt = attempt.saturating_add(1);
                            if !delay.is_zero() {
                                thread::sleep(delay);
                            }
                        }
                    }
                }
            })
            .map_err(|error| {
                control.completed.store(true, Ordering::Release);
                WatchdogError::ThreadSpawn(error)
            })?;
        Ok(ManagedThreadHandle {
            join: Some(join),
            control,
        })
    }

    fn validate_spec(&self, spec: &TaskSpec) -> Result<(), WatchdogError> {
        spec.validate()?;
        if spec.panic_action == PanicAction::RestartTask
            && let ReplaySafety::Checkpointed(kind) = &spec.replay_safety
        {
            let has_handler = self
                .app
                .process
                .shared
                .recovery_handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains_key(&(self.app.descriptor.id.clone(), kind.clone()));
            if !has_handler {
                return Err(WatchdogError::InvalidTaskPolicy(format!(
                    "task {} cannot restart because no recovery handler is registered for {}",
                    spec.id, kind
                )));
            }
        }
        Ok(())
    }

    #[cfg(feature = "tokio")]
    pub fn spawn_async<T, Fut, F>(
        &self,
        spec: TaskSpec,
        mut factory: F,
    ) -> Result<ManagedTaskHandle<T>, WatchdogError>
    where
        T: Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        F: FnMut() -> Fut + Send + 'static,
    {
        self.validate_spec(&spec)?;
        let runtime_handle = tokio::runtime::Handle::try_current().map_err(|_| {
            WatchdogError::InvalidTaskPolicy(format!(
                "async task {} requires an active Tokio runtime",
                spec.id
            ))
        })?;
        let task_name = format!("{}/{}", self.name, spec.id);
        let control = self.state.register(&task_name)?;
        let manager_control = control.clone();
        let completion = CompletionGuard(manager_control.clone());
        let app = self.app.clone();
        let async_spec = spec.clone();
        let join = runtime_handle.spawn(async move {
            let _completion = completion;
            let mut restarts = VecDeque::new();
            let mut attempt = 0_usize;
            loop {
                if manager_control.cancelled.load(Ordering::Acquire) {
                    return None;
                }
                let context = task_context(&app, &async_spec, &task_name, attempt);
                let future = match runtime::catch_factory(context.clone(), &mut factory) {
                    Ok(future) => future,
                    Err(payload) => {
                        let caught =
                            app.caught_from_context(context, payload_to_string(payload.as_ref()));
                        let recovery = restart_recovery(
                            &app,
                            &async_spec,
                            &mut restarts,
                            "async task factory is being restarted",
                            "async task factory panicked",
                        );
                        let should_continue = recovery.is_recovered();
                        let _ = caught.finalize_detached(recovery);
                        if !should_continue {
                            return None;
                        }
                        wait_async_backoff(&async_spec, attempt).await;
                        attempt = attempt.saturating_add(1);
                        continue;
                    }
                };
                let result = runtime::scope_task_context(
                    context.clone(),
                    AssertUnwindSafe(future).catch_unwind(),
                )
                .await;
                match result {
                    Ok(value) => {
                        return (!manager_control.cancelled.load(Ordering::Acquire))
                            .then_some(value);
                    }
                    Err(payload) => {
                        let caught =
                            app.caught_from_context(context, payload_to_string(payload.as_ref()));
                        let recovery = restart_recovery(
                            &app,
                            &async_spec,
                            &mut restarts,
                            "async task is being restarted",
                            "async task stopped after panic",
                        );
                        let should_continue = recovery.is_recovered();
                        let _ = caught.finalize_detached(recovery);
                        if !should_continue {
                            return None;
                        }
                        wait_async_backoff(&async_spec, attempt).await;
                        attempt = attempt.saturating_add(1);
                    }
                }
            }
        });
        let abort = join.abort_handle();
        control.add_abort(Arc::new(move || abort.abort()));
        Ok(ManagedTaskHandle { join, control })
    }

    #[cfg(feature = "tokio")]
    pub fn spawn_blocking<T, F>(
        &self,
        mut spec: TaskSpec,
        operation: F,
    ) -> Result<ManagedTaskHandle<T>, WatchdogError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        spec.kind = crate::TaskKind::BlockingIo;
        if spec.panic_action == PanicAction::RestartTask {
            return Err(WatchdogError::InvalidTaskPolicy(
                "spawn_blocking accepts a one-shot closure and cannot restart it".to_string(),
            ));
        }
        self.validate_spec(&spec)?;
        let runtime_handle = tokio::runtime::Handle::try_current().map_err(|_| {
            WatchdogError::InvalidTaskPolicy(format!(
                "blocking task {} requires an active Tokio runtime",
                spec.id
            ))
        })?;
        let task_name = format!("{}/{}", self.name, spec.id);
        let control = self.state.register(&task_name)?;
        let manager_control = control.clone();
        let completion = CompletionGuard(manager_control.clone());
        let app = self.app.clone();
        let context = task_context(&app, &spec, &task_name, 0);
        let join = runtime_handle.spawn(async move {
            let _completion = completion;
            if manager_control.cancelled.load(Ordering::Acquire) {
                return None;
            }
            let blocking_context = context.clone();
            let child = tokio::task::spawn_blocking(move || {
                runtime::catch_factory(blocking_context, operation)
            });
            let child_abort = child.abort_handle();
            manager_control.add_abort(Arc::new(move || child_abort.abort()));
            match child.await {
                Ok(Ok(value)) => {
                    (!manager_control.cancelled.load(Ordering::Acquire)).then_some(value)
                }
                Ok(Err(payload)) => {
                    let _ = app
                        .caught_from_context(context, payload_to_string(payload.as_ref()))
                        .finalize_detached(terminal_recovery(
                            &spec,
                            "blocking task stopped after panic",
                        ));
                    None
                }
                Err(error) if error.is_cancelled() => None,
                Err(error) => {
                    let payload = if error.is_panic() {
                        payload_to_string(error.into_panic().as_ref())
                    } else {
                        "blocking task failed without a panic payload".to_string()
                    };
                    let _ = app.caught_from_context(context, payload).finalize_detached(
                        terminal_recovery(&spec, "blocking task stopped after panic"),
                    );
                    None
                }
            }
        });
        let abort = join.abort_handle();
        control.add_abort(Arc::new(move || abort.abort()));
        Ok(ManagedTaskHandle { join, control })
    }

    #[cfg(feature = "tokio")]
    pub fn spawn_local<T, Fut, F>(
        &self,
        spec: TaskSpec,
        factory: F,
    ) -> Result<ManagedLocalTaskHandle<T>, WatchdogError>
    where
        T: 'static,
        Fut: Future<Output = T> + 'static,
        F: FnOnce() -> Fut + 'static,
    {
        if spec.panic_action == PanicAction::RestartTask {
            return Err(WatchdogError::InvalidTaskPolicy(
                "spawn_local currently supports report-only tasks".to_string(),
            ));
        }
        self.validate_spec(&spec)?;
        tokio::runtime::Handle::try_current().map_err(|_| {
            WatchdogError::InvalidTaskPolicy(format!(
                "local task {} requires an active Tokio runtime and LocalSet",
                spec.id
            ))
        })?;
        let task_name = format!("{}/{}", self.name, spec.id);
        let control = self.state.register(&task_name)?;
        let manager_control = control.clone();
        let completion = CompletionGuard(manager_control.clone());
        let app = self.app.clone();
        let context = task_context(&app, &spec, &task_name, 0);
        let join = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tokio::task::spawn_local(async move {
                let _completion = completion;
                let future = match runtime::catch_factory(context.clone(), factory) {
                    Ok(future) => future,
                    Err(payload) => {
                        let _ = app
                            .caught_from_context(context, payload_to_string(payload.as_ref()))
                            .finalize_detached(terminal_recovery(
                                &spec,
                                "local task factory panicked",
                            ));
                        return None;
                    }
                };
                let result = runtime::scope_task_context(
                    context.clone(),
                    AssertUnwindSafe(future).catch_unwind(),
                )
                .await;
                match result {
                    Ok(value) => {
                        (!manager_control.cancelled.load(Ordering::Acquire)).then_some(value)
                    }
                    Err(payload) => {
                        let _ = app
                            .caught_from_context(context, payload_to_string(payload.as_ref()))
                            .finalize_detached(terminal_recovery(
                                &spec,
                                "local task stopped after panic",
                            ));
                        None
                    }
                }
            })
        }))
        .map_err(|_| {
            control.completed.store(true, Ordering::Release);
            WatchdogError::InvalidTaskPolicy(
                "spawn_local must be called from inside a Tokio LocalSet".to_string(),
            )
        })?;
        let abort = join.abort_handle();
        control.add_abort(Arc::new(move || abort.abort()));
        Ok(ManagedLocalTaskHandle { join, control })
    }

    pub fn cancel_all(&self) {
        self.state.cancel_all();
    }

    pub fn shutdown(self, timeout: Duration) -> TaskGroupShutdown {
        self.state.close_and_cancel();
        #[cfg(feature = "tokio")]
        if tokio::runtime::Handle::try_current().is_ok() {
            return self.state.counts();
        }
        self.state.wait(timeout)
    }

    #[cfg(feature = "tokio")]
    pub async fn shutdown_async(self, timeout: Duration) -> TaskGroupShutdown {
        self.state.close_and_cancel();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let counts = self.state.counts();
            if counts.still_running == 0 || tokio::time::Instant::now() >= deadline {
                return counts;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

impl<T> ManagedThreadHandle<T> {
    pub fn cancel(&self) {
        self.control.cancel();
    }

    pub fn join(mut self) -> Result<Option<T>, WatchdogError> {
        self.join
            .take()
            .expect("managed thread handle is consumed only once")
            .join()
            .map_err(|_| WatchdogError::TaskPanicked)
    }
}

#[cfg(feature = "tokio")]
impl<T> ManagedTaskHandle<T> {
    pub fn cancel(&self) {
        self.control.cancel();
    }

    pub async fn join(self) -> Result<Option<T>, WatchdogError> {
        match self.join.await {
            Ok(value) => Ok(value),
            Err(error) if error.is_cancelled() => Ok(None),
            Err(_) => Err(WatchdogError::TaskPanicked),
        }
    }
}

#[cfg(feature = "tokio")]
impl<T> ManagedLocalTaskHandle<T> {
    pub fn cancel(&self) {
        self.control.cancel();
    }

    pub async fn join(self) -> Result<Option<T>, WatchdogError> {
        match self.join.await {
            Ok(value) => Ok(value),
            Err(error) if error.is_cancelled() => Ok(None),
            Err(_) => Err(WatchdogError::TaskPanicked),
        }
    }
}

fn task_context(
    app: &AppWatchdog,
    spec: &TaskSpec,
    boundary: &str,
    attempt: usize,
) -> ExecutionContext {
    let mut context = app.execution_context(
        boundary.to_string(),
        match spec.kind {
            crate::TaskKind::UiSession => BoundaryKind::UiSession,
            crate::TaskKind::BlockingIo => BoundaryKind::BlockingIo,
            crate::TaskKind::OneShot | crate::TaskKind::LongRunning => BoundaryKind::AsyncTask,
        },
        spec.kind == crate::TaskKind::UiSession,
        Some(spec.id.clone()),
        Some(spec.kind),
        match &spec.replay_safety {
            ReplaySafety::Checkpointed(kind) => Some(kind.clone()),
            ReplaySafety::Never | ReplaySafety::Idempotent => None,
        },
        attempt,
    );
    context.replay_safety = Some(spec.replay_safety.clone());
    context.task_group = boundary
        .rsplit_once('/')
        .map(|(group, _)| group.to_string());
    context.panic_action = Some(spec.panic_action);
    context.restart_policy = Some(spec.restart_policy.clone());
    context.expects_finalize = true;
    context
}

fn restart_recovery(
    app: &AppWatchdog,
    spec: &TaskSpec,
    restarts: &mut VecDeque<Instant>,
    restart_detail: &str,
    terminal_detail: &str,
) -> RecoveryOutcome {
    if spec.panic_action != PanicAction::RestartTask {
        return terminal_recovery(spec, terminal_detail);
    }

    let checkpoint_detail = match &spec.replay_safety {
        ReplaySafety::Checkpointed(kind) => {
            let outcome = app.reconcile_checkpointed(kind);
            if !outcome.is_recovered() {
                return outcome;
            }
            Some(outcome)
        }
        ReplaySafety::Idempotent => None,
        ReplaySafety::Never => {
            return RecoveryOutcome::Unrecoverable(format!(
                "{terminal_detail}; replay safety forbids restarting this task"
            ));
        }
    };

    if !should_restart(spec, restarts) {
        return RecoveryOutcome::Unrecoverable(format!(
            "{terminal_detail}; restart limit ({}) was exhausted",
            spec.restart_policy.max_restarts
        ));
    }

    let detail = match checkpoint_detail {
        Some(RecoveryOutcome::Recovered(message))
        | Some(RecoveryOutcome::RecoveredWithWarnings(message)) => {
            format!("{restart_detail}; {message}")
        }
        _ => restart_detail.to_string(),
    };
    RecoveryOutcome::RecoveredWithWarnings(detail)
}

fn terminal_recovery(spec: &TaskSpec, detail: &str) -> RecoveryOutcome {
    match spec.panic_action {
        PanicAction::ReportOnly | PanicAction::RestartTask => {
            RecoveryOutcome::Unrecoverable(detail.to_string())
        }
        PanicAction::RestartAppSession => RecoveryOutcome::ManualActionRequired(format!(
            "{detail}; the process host must restart the app session"
        )),
        PanicAction::EscalateProcess => RecoveryOutcome::Unrecoverable(format!(
            "{detail}; process-level escalation is required"
        )),
    }
}

fn should_restart(spec: &TaskSpec, restarts: &mut VecDeque<Instant>) -> bool {
    if spec.panic_action != PanicAction::RestartTask {
        return false;
    }
    let now = Instant::now();
    while restarts
        .front()
        .is_some_and(|restart| now.duration_since(*restart) > spec.restart_policy.window)
    {
        restarts.pop_front();
    }
    if restarts.len() >= spec.restart_policy.max_restarts {
        return false;
    }
    restarts.push_back(now);
    true
}

#[cfg(feature = "tokio")]
async fn wait_async_backoff(spec: &TaskSpec, attempt: usize) {
    let delay = spec.restart_policy.delay_for(attempt);
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

fn payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic payload was not a string".to_string()
    }
}
