use std::collections::{HashMap, HashSet};

use ui::{MotionFrame, SettingsViewModel, SpringValue, SystemStatusViewModel};

/// Frame-persistent visual state, kept out of AppState and persisted preferences.
#[derive(Default)]
pub(super) struct SpringProgress {
    values: HashMap<String, SpringValue>,
}

#[cfg(test)]
#[path = "tests/spring_progress.rs"]
mod tests;

impl SpringProgress {
    pub(super) fn update(
        &mut self,
        settings: Option<&mut SettingsViewModel>,
        status: Option<&mut SystemStatusViewModel>,
        frame: MotionFrame,
    ) -> bool {
        let mut seen = HashSet::new();
        if let Some(activity) = settings
            .and_then(|model| model.update.as_mut())
            .and_then(|update| update.activity.as_mut())
        {
            for (key, meter) in [
                ("update.download", &mut activity.download),
                ("update.compilation", &mut activity.compilation),
            ] {
                meter.display_basis_points =
                    self.present(key.into(), meter.percent, frame, &mut seen);
            }
        }
        if let Some(status) = status {
            for widget in status
                .dashboard
                .wide_widgets
                .iter_mut()
                .chain(&mut status.dashboard.narrow_widgets)
                .chain(&mut status.dashboard.overview_metrics)
            {
                widget.display_basis_points = self.present(
                    format!("status.{:?}", widget.kind),
                    widget.progress_percent,
                    frame,
                    &mut seen,
                );
            }
        }
        // Leaving a screen or losing a sample cancels its animations and timer demand.
        self.values.retain(|key, _| seen.contains(key));
        self.values.values().any(SpringValue::is_running)
    }

    fn present(
        &mut self,
        key: String,
        percent: Option<u16>,
        frame: MotionFrame,
        seen: &mut HashSet<String>,
    ) -> Option<u16> {
        let target = f64::from(percent?.min(100)) / 100.0;
        let first_visit = seen.insert(key.clone());
        let value = self.values.entry(key).or_default();
        if first_visit {
            let changed = value.target() != target;
            value.retarget(target);
            // A new target starts now, not at the time of the previous idle frame.
            value.advance(
                if changed {
                    std::time::Duration::ZERO
                } else {
                    frame.scaled_delta()
                },
                frame.reduced_motion,
            );
        }
        Some((value.value().clamp(0.0, 1.0) * 10_000.0).round() as u16)
    }
}
