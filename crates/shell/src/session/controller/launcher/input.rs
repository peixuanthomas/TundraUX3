use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_launcher_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Launcher);
        if self.launcher_pending_confirmation.is_some() {
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            return match key.key {
                InputKey::Tab
                | InputKey::BackTab
                | InputKey::Left
                | InputKey::Right
                | InputKey::Up
                | InputKey::Down => (target, ShellCommand::LauncherToggleConfirmationAction),
                InputKey::Enter | InputKey::Char(' ') if key.is_unmodified_action_key() => (
                    target,
                    if self.launcher_confirm_selected {
                        ShellCommand::LauncherConfirm
                    } else {
                        ShellCommand::LauncherCancelConfirmation
                    },
                ),
                InputKey::Char('y' | 'Y') => (target, ShellCommand::LauncherConfirm),
                InputKey::Escape | InputKey::Char('n' | 'N') => {
                    (target, ShellCommand::LauncherCancelConfirmation)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }
        if self.launcher_drag.is_some() && matches!(key.key, InputKey::Escape) {
            return (target, ShellCommand::LauncherCancelDrag);
        }
        if key.has_non_shift_modifier() {
            return (target, ShellCommand::RecordInput);
        }
        match key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseLauncher),
            InputKey::Left | InputKey::Up => (target, ShellCommand::LauncherPrevious),
            InputKey::Right | InputKey::Down => (target, ShellCommand::LauncherNext),
            InputKey::PageUp => (target, ShellCommand::LauncherPageUp),
            InputKey::PageDown => (target, ShellCommand::LauncherPageDown),
            InputKey::Home => (target, ShellCommand::LauncherFirst),
            InputKey::End => (target, ShellCommand::LauncherLast),
            InputKey::Enter | InputKey::Char(' ') if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherActivate)
            }
            InputKey::Delete if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherRemove)
            }
            InputKey::Char('v' | 'V') if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherToggleView)
            }
            InputKey::Char('r' | 'R') | InputKey::F(5) if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherRefresh)
            }
            _ => (target, ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_launcher_mouse(
        &mut self,
        mouse: MouseInput,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Launcher);
        let coordinates = mouse.coordinates();
        match mouse.kind {
            ui::MouseEventKind::Scroll(direction) => {
                let delta = match direction {
                    ScrollDirection::Up => -1,
                    ScrollDirection::Down => 1,
                    _ => 0,
                };
                (target, ShellCommand::LauncherScroll(delta))
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
                let click = self.register_click(
                    Some(ShellComponent::Launcher),
                    coordinates,
                    PointerButton::Left,
                    received_at,
                );
                (target, ShellCommand::LauncherPointer(coordinates, click))
            }
            ui::MouseEventKind::Click(PointerButton::Left)
            | ui::MouseEventKind::DoubleClick(PointerButton::Left) => {
                (target, ShellCommand::LauncherActivate)
            }
            ui::MouseEventKind::Drag(PointerButton::Left) => {
                (target, ShellCommand::LauncherDragUpdate(coordinates))
            }
            ui::MouseEventKind::Up(PointerButton::Left) => {
                (target, ShellCommand::LauncherDrop(coordinates))
            }
            ui::MouseEventKind::Moved => {
                (target, ShellCommand::Hover(Some(ShellComponent::Launcher)))
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
        }
    }
}
