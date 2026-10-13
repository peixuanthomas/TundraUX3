//! One running computation and one replaceable request per editor feature.
//! Superseded jobs retain at most one Rope snapshot and cannot publish results.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub const SYNC_BYTES: usize = 64 * 1024;
pub type Cancel = Arc<AtomicBool>;
type Work<T> = Box<dyn FnOnce(&AtomicBool) -> T + Send>;

struct State<T> {
    pending: Option<(Cancel, Work<T>)>,
    active: Cancel,
    result: Option<T>,
    running: bool,
}

pub struct Latest<T>(Arc<Mutex<State<T>>>);

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(State {
            pending: None,
            active: Arc::new(AtomicBool::new(false)),
            result: None,
            running: false,
        })))
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
        std::thread::spawn(move || {
            loop {
                let job = {
                    let mut state = shared.lock().unwrap();
                    match state.pending.take() {
                        Some(job) => job,
                        None => {
                            state.running = false;
                            return;
                        }
                    }
                };
                let (cancelled, work) = job;
                let result = work(&cancelled);
                let mut state = shared.lock().unwrap();
                if !cancelled.load(Ordering::Relaxed) {
                    state.result = Some(result);
                }
            }
        });
    }
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
