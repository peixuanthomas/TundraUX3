use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScreenSaverState {
    pub(super) last_activity: Instant,
    pub(super) active: bool,
    wake_key: Option<InputKey>,
    wake_pointer: Option<PointerButton>,
}

impl Default for ScreenSaverState {
    fn default() -> Self {
        Self {
            last_activity: Instant::now(),
            active: false,
            wake_key: None,
            wake_pointer: None,
        }
    }
}

impl ShellSession {
    pub fn screen_saver_active(&self) -> bool {
        self.screen_saver.active
    }

    pub(super) fn screen_saver_poll_timeout(&self, now: Instant, fallback: Duration) -> Duration {
        let seconds = self.app.storage_config().screen_timeout_seconds;
        if self.screen_saver.active || seconds == 0 {
            return fallback;
        }
        fallback.min(
            Duration::from_secs(seconds)
                .saturating_sub(now.saturating_duration_since(self.screen_saver.last_activity)),
        )
    }

    pub(super) fn advance_screen_saver_at(&mut self, now: Instant) {
        if !self.screen_saver.active && self.screen_saver_poll_timeout(now, Duration::MAX).is_zero()
        {
            self.screen_saver.active = true;
            self.screen_saver.wake_key = None;
            self.screen_saver.wake_pointer = None;
            self.cancel_pointer_gestures_for_modal();
            self.screen_keyboard.pressed = None;
            self.screen_keyboard.physical.clear();
            self.interaction = Default::default();
            self.last_click = None;
            self.hovered_component = None;
        }
    }

    pub(super) fn consume_screen_saver_input(&mut self, input: &InputEvent, now: Instant) -> bool {
        if matches!(input, InputEvent::FocusLost | InputEvent::Resize { .. }) {
            self.screen_saver.wake_pointer = None;
            self.screen_saver.wake_key = None;
        }
        if matches!(input, InputEvent::Tick) {
            self.advance_screen_saver_at(now);
        }
        if self.screen_saver.active {
            let wake = match input {
                InputEvent::Key(key) if key.phase == InputPhase::Press => {
                    self.screen_saver.wake_key = Some(key.key.clone());
                    true
                }
                InputEvent::Mouse(mouse) => match mouse.kind {
                    ui::MouseEventKind::Down(button) => {
                        self.screen_saver.wake_pointer = Some(button);
                        true
                    }
                    ui::MouseEventKind::Click(_) | ui::MouseEventKind::DoubleClick(_) => true,
                    _ => false,
                },
                InputEvent::Paste(_) => true,
                _ => false,
            };
            if wake {
                self.screen_saver.active = false;
                self.screen_saver.last_activity = now;
                self.update_button_input_mode(input);
            }
            return matches!(
                input,
                InputEvent::Key(_) | InputEvent::Mouse(_) | InputEvent::Paste(_)
            );
        }
        if let InputEvent::Key(key) = input {
            if self.screen_saver.wake_key.as_ref() == Some(&key.key)
                && key.phase != InputPhase::Press
            {
                if key.phase == InputPhase::Release {
                    self.screen_saver.wake_key = None;
                }
                return true;
            }
            self.screen_saver.wake_key = None;
        }
        if let InputEvent::Mouse(mouse) = input
            && let Some(button) = self.screen_saver.wake_pointer
        {
            self.update_button_input_mode(input);
            if mouse.kind == ui::MouseEventKind::Up(button) {
                self.screen_saver.wake_pointer = None;
            }
            return true;
        }
        let activity = match input {
            InputEvent::Key(key) => key.phase.is_press_like(),
            InputEvent::Mouse(mouse) => {
                mouse.kind != ui::MouseEventKind::Moved
                    || self.interaction.pointer_position != Some(mouse.coordinates())
            }
            InputEvent::Paste(_) | InputEvent::Resize { .. } | InputEvent::FocusGained => true,
            _ => false,
        };
        if activity {
            self.screen_saver.last_activity = now;
        }
        false
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/screen_saver.rs"]
mod tests;
