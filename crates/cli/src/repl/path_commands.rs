//! PATH discovery never runs on the line editor's input thread.
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use platform::ExecutableSearchPath;

use super::completion::system_candidates;

type ScanRequest = (u64, ExecutableSearchPath);
type ScanResult = (u64, Arc<[String]>);

struct Cache {
    commands: Arc<[String]>,
    results: mpsc::Receiver<ScanResult>,
    pending: bool,
}

pub(super) struct PathCommands {
    fallback: Arc<[String]>,
    cache: RefCell<Cache>,
    search: Option<ExecutableSearchPath>,
    generation: Arc<AtomicU64>,
    requests: mpsc::Sender<ScanRequest>,
    worker: Option<watchdog::ManagedThreadHandle<()>>,
}

impl PathCommands {
    pub(super) fn new(watchdog: Option<&watchdog::AppWatchdog>) -> Self {
        Self::with_scanner(watchdog, |search, cancelled| {
            search.executable_names(cancelled)
        })
    }

    fn with_scanner<F>(watchdog: Option<&watchdog::AppWatchdog>, mut scan: F) -> Self
    where
        F: FnMut(&ExecutableSearchPath, &dyn Fn() -> bool) -> Vec<String> + Send + 'static,
    {
        let fallback: Arc<[String]> = system_candidates(Vec::new()).into();
        let generation = Arc::new(AtomicU64::new(0));
        let (requests, incoming) = mpsc::channel::<ScanRequest>();
        let (completed, results) = mpsc::channel();
        // Library callers without a managed runtime still get static hints;
        // they must not fall back to scanning on the input thread.
        let worker = watchdog.and_then(|watchdog| {
            static NEXT_WORKER: AtomicU64 = AtomicU64::new(0);
            let group = watchdog.task_group(&format!(
                "repl-completion-{}",
                NEXT_WORKER.fetch_add(1, Ordering::Relaxed)
            ));
            let latest = Arc::clone(&generation);
            let mut spec = watchdog::TaskSpec::one_shot(watchdog::TaskId::from_static("scan-path"));
            spec.replay_safety = watchdog::ReplaySafety::Idempotent;
            group
                .spawn_thread_with_cancellation(spec, move |cancel| {
                    while !cancel.is_cancelled() {
                        let mut request = match incoming.recv_timeout(Duration::from_millis(50)) {
                            Ok(request) => request,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        };
                        // Rapid prompt/PATH changes need only the newest scan.
                        for newer in incoming.try_iter() {
                            request = newer;
                        }
                        let (generation, search) = request;
                        let cancelled = || {
                            cancel.is_cancelled() || latest.load(Ordering::Acquire) != generation
                        };
                        if cancelled() {
                            continue;
                        }
                        let names = scan(&search, &cancelled);
                        if !cancelled() {
                            let commands = system_candidates(names).into();
                            if completed.send((generation, commands)).is_err() {
                                return;
                            }
                        }
                    }
                })
                .ok()
        });
        Self {
            cache: RefCell::new(Cache {
                commands: Arc::clone(&fallback),
                results,
                pending: false,
            }),
            fallback,
            search: None,
            generation,
            requests,
            worker,
        }
    }

    pub(super) fn refresh(&mut self, search: ExecutableSearchPath) {
        self.commands();
        let changed = self.search.as_ref() != Some(&search);
        let cache = self.cache.get_mut();
        if changed {
            // An old PATH's late result must never reintroduce removed names.
            cache.commands = Arc::clone(&self.fallback);
        } else if cache.pending {
            return;
        }
        self.search = Some(search.clone());
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        cache.pending = self.requests.send((generation, search)).is_ok();
    }

    pub(super) fn commands(&self) -> Arc<[String]> {
        let mut cache = self.cache.borrow_mut();
        while let Ok((generation, commands)) = cache.results.try_recv() {
            if generation == self.generation.load(Ordering::Acquire) {
                cache.commands = commands;
                cache.pending = false;
            }
        }
        Arc::clone(&cache.commands)
    }
}

impl Drop for PathCommands {
    fn drop(&mut self) {
        if let Some(worker) = &self.worker {
            // Do not join here: even exiting must not wait for a slow disk.
            // The process watchdog continues to track the cancelled worker.
            worker.cancel();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/repl/path_commands.rs"]
mod tests;
