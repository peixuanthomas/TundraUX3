use super::*;
impl ShellSession {
    pub(super) fn poll_auto_admin_stop(&mut self, now: Instant) {
        if self.auto_admin.closing_page.is_some() {
            return;
        }
        if self
            .auto_admin
            .job
            .as_ref()
            .is_some_and(|job| job.poll_stop(now))
        {
            self.auto_admin.visible = true;
            self.auto_admin.button_focus = Some(0);
            self.auto_admin.scroll = 0;
            self.auto_admin.pointer = None;
            self.auto_admin.suppress_repeats = true;
            self.interaction.button_capture = None;
            self.last_key_event = None;
        }
    }
}
