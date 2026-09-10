//! Shared Spring presentation state. Values never replace business data.

use std::time::Duration;

#[derive(Debug, Clone, Copy, Default)]
pub struct SpringValue {
    value: f64,
    velocity: f64,
    target: f64,
}

impl SpringValue {
    pub fn value(&self) -> f64 {
        self.value
    }
    pub fn target(&self) -> f64 {
        self.target
    }

    pub fn retarget(&mut self, target: f64) {
        self.target = target;
    }

    /// Sets a static presentation value without changing its business target.
    pub fn set_value(&mut self, value: f64) {
        self.value = value;
        self.velocity = 0.0;
    }

    pub fn is_running(&self) -> bool {
        (self.value - self.target).abs() > 0.0001 || self.velocity.abs() > 0.001
    }

    pub fn advance(&mut self, delta: Duration, reduced: bool) {
        if reduced {
            self.set_value(self.target);
            return;
        }
        // Exact underdamped oscillator; retargeting preserves velocity. A long
        // stalled frame resolves to the current target instead of replaying a backlog.
        let delta = delta.as_secs_f64();
        let omega: f64 = 12.0;
        let damping: f64 = 0.65;
        let decay = omega * damping;
        let frequency = omega * (1.0 - damping * damping).sqrt();
        let displacement = self.value - self.target;
        let (sin, cos) = (frequency * delta).sin_cos();
        let envelope = (-decay * delta).exp();
        self.value = self.target
            + envelope
                * (displacement * cos + (self.velocity + decay * displacement) / frequency * sin);
        self.velocity = envelope
            * (self.velocity * cos
                - (decay * self.velocity + omega * omega * displacement) / frequency * sin);
        if !self.is_running() {
            self.set_value(self.target);
        }
    }
}
