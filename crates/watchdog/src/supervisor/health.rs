use std::time::{Duration, Instant};

const SUSPICION_AFTER: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) struct WorkerHealth {
    last_progress: Instant,
    counter: u64,
    probe: Option<(u64, Instant)>,
    unresponsive: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HealthEvent {
    Probe,
    NoResponse,
}

impl WorkerHealth {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            last_progress: now,
            counter: 0,
            probe: None,
            unresponsive: false,
        }
    }

    pub(super) fn started(&self) -> bool {
        self.counter != 0
    }

    pub(super) fn unresponsive(&self) -> bool {
        self.unresponsive
    }

    pub(super) fn healthy(&self, now: Instant) -> bool {
        self.started()
            && !self.unresponsive
            && self.probe.is_none()
            && now.duration_since(self.last_progress) < SUSPICION_AFTER
    }

    pub(super) fn progress(&mut self, counter: u64, pong: Option<u64>, now: Instant) -> bool {
        if counter <= self.counter {
            return false;
        }
        let expected = self.probe.map(|(nonce, _)| nonce);
        if pong.is_some() && pong != expected {
            return false;
        }
        self.counter = counter;
        if pong == expected {
            self.last_progress = now;
            self.probe = None;
            return std::mem::take(&mut self.unresponsive);
        }
        false
    }

    pub(super) fn tick(&mut self, now: Instant, resumed: bool, nonce: u64) -> Option<HealthEvent> {
        if now.duration_since(self.last_progress) < SUSPICION_AFTER {
            return None;
        }
        if resumed || self.probe.is_none() {
            // A paused guardian cannot tell whether the worker ran during the gap.
            self.probe = Some((nonce, now + PROBE_TIMEOUT));
            return Some(HealthEvent::Probe);
        }
        if !self.unresponsive && self.probe.is_some_and(|(_, deadline)| now >= deadline) {
            self.unresponsive = true;
            return Some(HealthEvent::NoResponse);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_probe_and_advancing_loop_counter_are_both_required() {
        let start = Instant::now();
        let mut health = WorkerHealth::new(start);
        health.progress(1, None, start);
        assert_eq!(health.tick(start + Duration::from_secs(1), false, 7), None);
        assert_eq!(
            health.tick(start + Duration::from_secs(2), false, 7),
            Some(HealthEvent::Probe)
        );
        health.progress(2, None, start + Duration::from_secs(3));
        health.progress(3, Some(6), start + Duration::from_secs(3));
        health.progress(2, Some(7), start + Duration::from_secs(3));
        assert_eq!(
            health.tick(start + Duration::from_secs(4), false, 8),
            Some(HealthEvent::NoResponse)
        );
        assert!(health.progress(3, Some(7), start + Duration::from_secs(5)));
        assert!(health.healthy(start + Duration::from_secs(5)));
    }

    #[test]
    fn paused_guardian_checks_again_before_accusing_the_worker() {
        let start = Instant::now();
        let mut health = WorkerHealth::new(start);
        assert_eq!(
            health.tick(start + Duration::from_secs(2), false, 7),
            Some(HealthEvent::Probe)
        );
        assert_eq!(
            health.tick(start + Duration::from_secs(9), true, 8),
            Some(HealthEvent::Probe)
        );
        health.progress(1, Some(7), start + Duration::from_secs(9));
        assert_eq!(health.tick(start + Duration::from_secs(10), false, 9), None);
        health.progress(1, Some(8), start + Duration::from_secs(10));
        assert!(health.healthy(start + Duration::from_secs(10)));
    }
}
