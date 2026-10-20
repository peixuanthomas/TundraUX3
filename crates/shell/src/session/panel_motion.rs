use super::*;

impl ShellSession {
    pub(super) fn interaction_motion_frame(&self, mut frame: ui::MotionFrame) -> ui::MotionFrame {
        frame.reduced_motion |= !self.interaction.pointer_motion;
        frame
    }

    pub(super) fn advance_management_details(&mut self, frame: ui::MotionFrame) -> bool {
        if self.content_screen() != ShellScreen::Management {
            self.details_motion = None;
            return false;
        }
        let page = self.active_page_id();
        let target = if self.to_management_view_model().details_only {
            1_000
        } else {
            0
        };
        if self
            .details_motion
            .as_ref()
            .is_none_or(|(id, _)| *id != page)
        {
            self.details_motion = Some((page, ui::PanelMotion::new(target)));
        }
        let frame = self.interaction_motion_frame(frame);
        let motion = &mut self.details_motion.as_mut().unwrap().1;
        let previous = motion.value();
        let running = motion.update(target, frame, ui::MotionTimings::DIALOG);
        if motion.value() != previous {
            self.cancel_pointer_gestures_for_modal();
        }
        running
    }
}
