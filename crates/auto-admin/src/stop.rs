use super::*;
use ui::AutoAdminStopState;

const STOP_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct StopRequest {
    pub(super) state: AutoAdminStopState,
    since: Instant,
}

pub fn status(display: &Display) -> String {
    match display.stop.as_ref().map(|stop| stop.state) {
        Some(AutoAdminStopState::Waiting) => i18n::tr!("aa-stop-pending"),
        Some(AutoAdminStopState::Warning) => i18n::tr!("aa-stop-timeout"),
        Some(AutoAdminStopState::Killing) => i18n::tr!("aa-stop-killing"),
        _ => display.status.clone(),
    }
}

impl AutoAdminJob {
    pub fn accepts_input(&self) -> bool {
        self.phase() == RUNNING && !self.0.stop_requested.load(Ordering::Acquire)
    }

    #[cfg(any(target_os = "linux", test, feature = "test-support"))]
    pub fn enable_helper_control(&self) {
        self.0.helper_control.store(true, Ordering::Release);
    }

    pub fn can_kill(&self) -> bool {
        if self.0.helper_control.load(Ordering::Acquire)
            && self.0.helper_connected.load(Ordering::Acquire)
        {
            return true;
        }
        #[cfg(target_os = "linux")]
        return self
            .0
            .process
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some();
        #[cfg(not(target_os = "linux"))]
        false
    }

    pub fn stop_warning(&self) -> bool {
        self.0
            .display
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stop
            .as_ref()
            .is_some_and(|s| s.state == AutoAdminStopState::Warning)
    }

    pub fn interaction_phase(&self) -> u8 {
        let d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        match d.stop.as_ref().map(|s| s.state) {
            Some(AutoAdminStopState::Waiting) => 4,
            Some(AutoAdminStopState::Warning) => 5,
            Some(AutoAdminStopState::Killing) => 6,
            _ => self.phase() as u8,
        }
    }

    pub fn request_stop(&self, force: bool) {
        let mut display = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if self.phase() != RUNNING || (force && !self.can_kill()) {
            return;
        }
        if force {
            if display
                .stop
                .as_ref()
                .is_none_or(|s| s.state != AutoAdminStopState::Warning)
            {
                return;
            }
        } else if display.stop.is_some() {
            return;
        }
        self.0.stop_requested.store(true, Ordering::Release);
        if force {
            self.0.force_requested.store(true, Ordering::Release);
        }
        display.question = None;
        display.stop = Some(StopRequest {
            state: if force {
                AutoAdminStopState::Killing
            } else {
                AutoAdminStopState::Waiting
            },
            since: Instant::now(),
        });
        display.revision += 1;
        let input = if self.0.helper_control.load(Ordering::Acquire) {
            if force {
                OperationInput::Kill
            } else {
                OperationInput::Terminate
            }
        } else {
            OperationInput::Cancel
        };
        let _ = self.0.responses.send(input);
        #[cfg(target_os = "linux")]
        if let Some(process) = self
            .0
            .process
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            && let Err(error) = process.signal(force)
        {
            print_line(
                &mut display.parser,
                &format!("{}: {error}", i18n::tr!("aa-stop-send-failed")),
            );
        }
    }

    pub fn continue_waiting(&self) {
        let mut display = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(stop) = &mut display.stop {
            stop.state = AutoAdminStopState::Waiting;
            stop.since = Instant::now();
            display.revision += 1;
        }
    }
}

impl AutoAdminJob {
    /// Returns true only when a stop timeout first needs the user's attention.
    pub fn poll_stop(&self, now: Instant) -> bool {
        let mut display = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if self.phase() != RUNNING {
            return false;
        }
        if let Some(stop) = &mut display.stop
            && matches!(
                stop.state,
                AutoAdminStopState::Waiting | AutoAdminStopState::Killing
            )
            && now.saturating_duration_since(stop.since) >= STOP_TIMEOUT
        {
            stop.state = AutoAdminStopState::Warning;
            display.revision += 1;
            return true;
        }
        false
    }
}
