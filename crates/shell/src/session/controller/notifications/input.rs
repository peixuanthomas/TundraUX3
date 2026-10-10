use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_notification_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target_component = self
            .notification_active_modal_component()
            .unwrap_or(ShellComponent::NotificationDialog);
        let target = RoutedTarget::Modal(target_component);

        if !self.notification_can_render() {
            return if key.phase == InputPhase::Press
                && key.is_unmodified_action_key()
                && matches!(key.key, InputKey::Escape)
            {
                (target, ShellCommand::NotificationCancel)
            } else {
                (target, ShellCommand::CaptureOverlayInput)
            };
        }

        if let Some(index) = self.notification_action_index_for_input(key) {
            return (target, ShellCommand::NotificationActivateAction(index));
        }

        match &key.key {
            InputKey::Up | InputKey::Down | InputKey::Home | InputKey::End
                if self.status_details_visible() && key.is_unmodified_action_key() =>
            {
                (
                    target,
                    ShellCommand::NotificationScrollMessage {
                        delta: match key.key {
                            InputKey::Up => -1,
                            InputKey::Down => 1,
                            InputKey::Home => -isize::MAX,
                            _ => isize::MAX,
                        },
                        page: false,
                    },
                )
            }
            InputKey::BackTab if !key.has_non_shift_modifier() => {
                (target, ShellCommand::NotificationPreviousAction)
            }
            InputKey::Tab if key.modifiers.shift && !key.has_non_shift_modifier() => {
                (target, ShellCommand::NotificationPreviousAction)
            }
            InputKey::Tab if !key.modifiers.shift && !key.has_non_shift_modifier() => {
                (target, ShellCommand::NotificationNextAction)
            }
            InputKey::Right | InputKey::Down if key.is_unmodified_action_key() => {
                (target, ShellCommand::NotificationNextAction)
            }
            InputKey::Left | InputKey::Up if key.is_unmodified_action_key() => {
                (target, ShellCommand::NotificationPreviousAction)
            }
            InputKey::PageUp | InputKey::PageDown if key.is_unmodified_action_key() => (
                target,
                ShellCommand::NotificationScrollMessage {
                    delta: if key.key == InputKey::PageUp { -1 } else { 1 },
                    page: true,
                },
            ),
            InputKey::Enter | InputKey::Char(' ') => {
                if key.phase == InputPhase::Press && key.is_unmodified_action_key() {
                    (target, ShellCommand::NotificationActivateSelected)
                } else {
                    (target, ShellCommand::CaptureOverlayInput)
                }
            }
            InputKey::Escape => {
                if key.phase == InputPhase::Press && key.is_unmodified_action_key() {
                    (target, ShellCommand::NotificationCancel)
                } else {
                    (target, ShellCommand::CaptureOverlayInput)
                }
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    pub(in crate::session) fn route_notification_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target_component = self
            .notification_active_modal_component()
            .unwrap_or(ShellComponent::NotificationDialog);
        let target = RoutedTarget::Modal(target_component);
        let coordinates = mouse.coordinates();

        if !self.notification_can_render() {
            self.notification_pointer_capture = None;
            return (target, ShellCommand::CaptureOverlayInput);
        }

        if self.handle_notification_scrollbar(mouse) {
            return (target, ShellCommand::CaptureOverlayInput);
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => (target, ShellCommand::Hover(hit_target)),
            ui::MouseEventKind::Down(PointerButton::Left) => {
                let action_index = self.notification_action_index_at(coordinates);
                self.notification_pointer_capture = action_index.and_then(|action_index| {
                    self.notification_active_modal_id().map(|notification_id| {
                        NotificationPointerCapture {
                            notification_id,
                            action_index,
                        }
                    })
                });
                if let Some(action_index) = action_index {
                    self.notification_select_action(action_index);
                }
                (target, ShellCommand::CaptureOverlayInput)
            }
            ui::MouseEventKind::Up(PointerButton::Left) => {
                let pressed = self.notification_pointer_capture.take();
                let released_index = self.notification_action_index_at(coordinates);
                let current_id = self.notification_active_modal_id();
                match (pressed, current_id, released_index) {
                    (Some(pressed), Some(current_id), Some(released_index))
                        if pressed.notification_id == current_id
                            && pressed.action_index == released_index =>
                    {
                        (
                            target,
                            ShellCommand::NotificationActivateAction(released_index),
                        )
                    }
                    _ => (target, ShellCommand::CaptureOverlayInput),
                }
            }
            ui::MouseEventKind::Drag(PointerButton::Left) => {
                self.notification_pointer_capture = None;
                (target, ShellCommand::CaptureOverlayInput)
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Up | ScrollDirection::Down) => (
                target,
                ShellCommand::NotificationScrollMessage {
                    delta: if matches!(mouse.kind, ui::MouseEventKind::Scroll(ScrollDirection::Up))
                    {
                        -1
                    } else {
                        1
                    },
                    page: false,
                },
            ),
            ui::MouseEventKind::Down(_)
            | ui::MouseEventKind::Up(_)
            | ui::MouseEventKind::Click(_)
            | ui::MouseEventKind::DoubleClick(_)
            | ui::MouseEventKind::Drag(_)
            | ui::MouseEventKind::Scroll(_) => {
                self.notification_pointer_capture = None;
                (target, ShellCommand::CaptureOverlayInput)
            }
        }
    }
}
