use super::*;
use ui::AutoAdminStopState;

const STOP_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct StopRequest {
    pub(super) state: AutoAdminStopState,
    since: Instant,
}

pub(super) fn status(display: &Display) -> String {
    match display.stop.as_ref().map(|stop| stop.state) {
        Some(AutoAdminStopState::Waiting) => i18n::tr!("aa-stop-pending"),
        Some(AutoAdminStopState::Warning) => i18n::tr!("aa-stop-timeout"),
        Some(AutoAdminStopState::Killing) => i18n::tr!("aa-stop-killing"),
        _ => display.status.clone(),
    }
}

impl AutoAdminJob {
    pub(super) fn accepts_input(&self) -> bool {
        self.phase() == RUNNING && !self.0.stop_requested.load(Ordering::Acquire)
    }

    #[cfg(any(target_os = "linux", test))]
    pub(in crate::session) fn enable_helper_control(&self) {
        self.0.helper_control.store(true, Ordering::Release);
    }

    pub(super) fn can_kill(&self) -> bool {
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

    pub(super) fn stop_warning(&self) -> bool {
        self.0
            .display
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stop
            .as_ref()
            .is_some_and(|s| s.state == AutoAdminStopState::Warning)
    }

    pub(super) fn interaction_phase(&self) -> u8 {
        let d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        match d.stop.as_ref().map(|s| s.state) {
            Some(AutoAdminStopState::Waiting) => 4,
            Some(AutoAdminStopState::Warning) => 5,
            Some(AutoAdminStopState::Killing) => 6,
            _ => self.phase(),
        }
    }

    pub(super) fn request_stop(&self, force: bool) {
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

    pub(super) fn continue_waiting(&self) {
        let mut display = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(stop) = &mut display.stop {
            stop.state = AutoAdminStopState::Waiting;
            stop.since = Instant::now();
            display.revision += 1;
        }
    }
}

impl ShellSession {
    pub(super) fn poll_auto_admin_stop(&mut self, now: Instant) {
        let Some(job) = self.auto_admin.job.clone() else {
            return;
        };
        let mut display = job.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if job.phase() != RUNNING {
            return;
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
            self.auto_admin.visible = true;
            self.auto_admin.button_focus = Some(0);
            self.auto_admin.scroll = 0;
            self.auto_admin.pointer = None;
            self.auto_admin.suppress_repeats = true;
            self.button_pointer_capture = None;
            self.last_key_event = None;
        }
    }
}
