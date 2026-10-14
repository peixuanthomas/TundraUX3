//! One running computation and one replaceable request per editor feature.
//! Superseded jobs retain at most one Rope snapshot and cannot publish results.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use watchdog::{AppWatchdog, ManagedThreadHandle, TaskId, TaskSpec};

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

pub const SYNC_BYTES: usize = 64 * 1024;
pub type Cancel = Arc<AtomicBool>;
type Work<T> = Box<dyn FnOnce(&AtomicBool) -> T + Send>;

struct State<T> {
    pending: Option<(Cancel, Work<T>)>,
    active: Cancel,
    result: Option<T>,
    running: bool,
}

pub struct Latest<T>(Arc<Mutex<State<T>>>, Mutex<Option<ManagedThreadHandle<()>>>);

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self(
            Arc::new(Mutex::new(State {
                pending: None,
                active: Arc::new(AtomicBool::new(false)),
                result: None,
                running: false,
            })),
            Mutex::new(None),
        )
    }
}
impl<T> std::fmt::Debug for Latest<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Latest(..)")
    }
}
// Cloned editor snapshots do not share mutable job ownership.
impl<T> Clone for Latest<T> {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl<T> PartialEq for Latest<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl<T> Eq for Latest<T> {}
impl<T> Latest<T> {
    pub fn cancel(&self) {
        let mut state = self.0.lock().unwrap();
        state.active.store(true, Ordering::Relaxed);
        state.pending = None;
        state.result = None;
    }
    pub fn take(&self) -> Option<T> {
        self.0.lock().unwrap().result.take()
    }
    pub fn busy(&self) -> bool {
        let state = self.0.lock().unwrap();
        state.running || state.result.is_some()
    }
    pub fn ready(&self) -> bool {
        self.0.lock().unwrap().result.is_some()
    }
}
impl<T: Send + 'static> Latest<T> {
    pub fn submit(&self, work: impl FnOnce(&AtomicBool) -> T + Send + 'static) {
        let mut state = self.0.lock().unwrap();
        state.active.store(true, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        state.active = cancelled.clone();
        state.pending = Some((cancelled, Box::new(work)));
        state.result = None;
        if state.running {
            return;
        }
        state.running = true;
        let shared = self.0.clone();
        drop(state);
        let mut finish = Some(Finish {
            state: shared.clone(),
            armed: true,
        });
        let mut run = move |cancellation: Option<watchdog::ThreadCancellation>| {
            let mut finish = finish.take().expect("editor computation runs once");
            if let Some(cancellation) = &cancellation {
                let weak = Arc::downgrade(&shared);
                cancellation.on_cancel(move || {
                    if let Some(shared) = weak.upgrade() {
                        let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
                        state.active.store(true, Ordering::Release);
                        state.pending = None;
                        state.result = None;
                    }
                });
            }
            loop {
                if cancellation.as_ref().is_some_and(|c| c.is_cancelled()) {
                    return;
                }
                let job = {
                    let mut state = shared.lock().unwrap();
                    match state.pending.take() {
                        Some(job) => job,
                        None => {
                            state.running = false;
                            finish.armed = false;
                            return;
                        }
                    }
                };
                let (cancelled, work) = job;
                let result = work(&cancelled);
                let mut state = shared.lock().unwrap();
                if !cancelled.load(Ordering::Acquire)
                    && !cancellation.as_ref().is_some_and(|c| c.is_cancelled())
                {
                    state.result = Some(result);
                }
            }
        };
        let app = match editor_watchdog() {
            Ok(app) => app,
            // Dropping the unstarted closure runs Finish, clearing the busy
            // flag. A stopped watchdog must never fall back to UI-thread work.
            Err(_) => return,
        };
        if let Some(app) = app {
            let group = app.task_group("editor-computations");
            group.reap_finished();
            let id = TaskId::new(format!(
                "compute-{}",
                NEXT_JOB.fetch_add(1, Ordering::Relaxed)
            ))
            .unwrap();
            match group.spawn_thread_with_cancellation(TaskSpec::one_shot(id), move |cancel| {
                run(Some(cancel))
            }) {
                Ok(handle) => {
                    let mut worker = self.1.lock().unwrap();
                    if let Some(previous) = worker.take() {
                        // The old worker has no pending computation left.
                        // Its completion bookkeeping may still be finishing.
                        drop(previous);
                    }
                    *worker = Some(handle);
                }
                Err(_) => {
                    let mut state = self.0.lock().unwrap();
                    state.running = false;
                    state.pending = None;
                    state.active.store(true, Ordering::Release);
                }
            }
        } else {
            // Standalone library use has no process runtime. Never start an
            // unowned thread; the application installs watchdog before UI work.
            run(None);
        }
    }
}

struct Finish<T> {
    state: Arc<Mutex<State<T>>>,
    armed: bool,
}
impl<T> Drop for Finish<T> {
    fn drop(&mut self) {
        if self.armed {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.active.store(true, Ordering::Release);
            state.pending = None;
            state.result = None;
            state.running = false;
        }
    }
}

fn editor_watchdog() -> Result<Option<AppWatchdog>, watchdog::WatchdogError> {
    if let Some(app) = AppWatchdog::current() {
        return Ok(Some(app));
    }
    let descriptor = || {
        watchdog::AppDescriptor::new(
            watchdog::AppId::from_static("editor"),
            "Editor",
            env!("CARGO_PKG_VERSION"),
            watchdog::AppCriticality::Optional,
        )
    };
    if let Some(process) = watchdog::ProcessWatchdog::current() {
        return process.register_app(descriptor()).map(Some);
    }
    #[cfg(test)]
    {
        static TEST_RUNTIME: std::sync::OnceLock<(Mutex<watchdog::WatchdogRuntime>, AppWatchdog)> =
            std::sync::OnceLock::new();
        let (_, app) = TEST_RUNTIME.get_or_init(|| {
            let root =
                std::env::temp_dir().join(format!("tundra-editor-tests-{}", std::process::id()));
            let config = watchdog::WatchdogConfig::new(
                root.join("reports"),
                root.join("fallback"),
                root.join("state"),
                "editor-tests",
                env!("CARGO_PKG_VERSION"),
            );
            let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
            (
                Mutex::new(runtime),
                process.register_app(descriptor()).unwrap(),
            )
        });
        return Ok(Some(app.clone()));
    }
    #[cfg(not(test))]
    Ok(None)
}
impl<T> Drop for Latest<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    #[test]
    fn panic_clears_busy_state_and_allows_another_request() {
        let worker = Latest::<usize>::default();
        worker.submit(|_| panic!("editor computation fixture"));
        let deadline = Instant::now() + Duration::from_secs(3);
        while worker.busy() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        worker.submit(|_| 42);
        while !worker.ready() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(worker.take(), Some(42));
    }

    #[test]
    fn process_shutdown_cancels_editor_work_without_dropping_the_editor() {
        let root =
            std::env::temp_dir().join(format!("tundra-editor-cancel-{}", std::process::id()));
        let config = watchdog::WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("state"),
            "editor-cancel",
            "test",
        );
        let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
        let app = process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("editor-test"),
                "Editor",
                "test",
                watchdog::AppCriticality::Optional,
            ))
            .unwrap();
        let worker = Latest::default();
        let (entered, ready) = mpsc::channel();
        let (exited, finished) = mpsc::channel();
        assert!(
            app.run_boundary(
                watchdog::BoundarySpec::new("editor-submit", watchdog::BoundaryKind::UiSession),
                std::panic::AssertUnwindSafe(|| {
                    worker.submit(move |cancel| {
                        entered.send(()).unwrap();
                        while !cancel.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        exited.send(()).unwrap();
                        1
                    });
                })
            )
            .is_ok()
        );
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        runtime.shutdown().unwrap();
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!worker.busy());
        assert_eq!(worker.take(), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_latest_queued_request_runs_and_stale_completion_is_discarded() {
        let worker = Latest::default();
        let (entered, started) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        worker.submit(move |_| {
            entered.send(()).unwrap();
            blocked.recv().unwrap();
            1
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let obsolete_ran = Arc::new(AtomicBool::new(false));
        let obsolete = obsolete_ran.clone();
        worker.submit(move |_| {
            obsolete.store(true, Ordering::Relaxed);
            2
        });
        worker.submit(|_| 3);
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let result = loop {
            if let Some(result) = worker.take() {
                break result;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(result, 3);
        assert!(!obsolete_ran.load(Ordering::Relaxed));
    }

    #[test]
    fn dropping_owner_cancels_running_work() {
        let worker = Latest::default();
        let (entered, started) = mpsc::channel();
        let (exited, finished) = mpsc::channel();
        worker.submit(move |cancel| {
            entered.send(()).unwrap();
            while !cancel.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
            exited.send(()).unwrap();
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(worker);
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
