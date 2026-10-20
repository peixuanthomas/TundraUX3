//! Persistent interaction state, independent of page data and frame rendering.
use crate::session::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InputMode {
    #[default]
    Keyboard,
    Pointer,
    Unfocused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::session) struct ButtonPointerCapture {
    pub region: ui::components::ButtonRegion,
    pub page: PageInstanceId,
    pub overlay: Option<String>,
    pub input: MouseInput,
    pub pressed_at: Instant,
    pub native_release: bool,
    pub activate_on_release: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(in crate::session) struct InteractionState {
    pub pointer_motion: bool,
    mode: InputMode,
    hover_suppressed: bool,
    opening_key: Option<InputKey>,
    pub pointer_position: Option<CellPosition>,
    pub button_capture: Option<ButtonPointerCapture>,
    pub completed_button_click_at: Option<Instant>,
}

impl InteractionState {
    pub fn keyboard_focus_visible(&self) -> bool {
        self.mode == InputMode::Keyboard
    }

    pub fn hover_position(&self) -> Option<CellPosition> {
        self.pointer_position
            .filter(|_| self.mode == InputMode::Pointer && !self.hover_suppressed)
    }

    /// Observe physical input before pointer activation becomes a synthetic key.
    /// Terminals commonly encode touch as left-button down/up without hover.
    pub(super) fn observe(&mut self, input: &InputEvent, keyboard_action: bool) {
        match input {
            InputEvent::Mouse(mouse) => {
                if mouse.kind == ui::MouseEventKind::Moved
                    && self.pointer_position == Some(mouse.coordinates())
                {
                    return;
                }
                self.mode = InputMode::Pointer;
                self.hover_suppressed = matches!(
                    mouse.kind,
                    ui::MouseEventKind::Up(_)
                        | ui::MouseEventKind::Click(_)
                        | ui::MouseEventKind::DoubleClick(_)
                );
                self.pointer_position = Some(mouse.coordinates());
            }
            InputEvent::FocusLost => {
                self.mode = InputMode::Unfocused;
                self.hover_suppressed = true;
                self.pointer_position = None;
                self.button_capture = None;
            }
            InputEvent::Key(key) if key.phase.is_press_like() && keyboard_action => {
                self.mode = InputMode::Keyboard;
            }
            _ => {}
        }
    }

    pub(super) fn guard_opening_key(&mut self, key: InputKey) {
        self.opening_key = Some(key);
    }

    /// A shortcut that opens a text field must not repeat into that field.
    pub(super) fn consumes_opening_repeat(&mut self, input: &InputEvent) -> bool {
        match input {
            InputEvent::Key(key) if key.phase == InputPhase::Repeat => {
                return self.opening_key.as_ref() == Some(&key.key);
            }
            InputEvent::Key(key) if key.phase == InputPhase::Release => {
                if self.opening_key.as_ref() == Some(&key.key) {
                    self.opening_key = None;
                }
            }
            InputEvent::Key(_)
            | InputEvent::Mouse(_)
            | InputEvent::FocusLost
            | InputEvent::Paste(_)
            | InputEvent::Resize { .. } => self.opening_key = None,
            _ => {}
        }
        false
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/input_routing/interaction_tests.rs"]
mod tests;
