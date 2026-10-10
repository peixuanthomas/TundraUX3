use super::*;

impl ShellSession {
    /// Navigation owns Escape. All other overlay keys use the same owner as
    /// focus, pointer input and the compositor.
    pub(in crate::session) fn route_overlay_key(
        &self,
        key: &KeyInput,
    ) -> Option<(RoutedTarget, ShellCommand)> {
        let overlay = self.interactive_overlays().pop()?;
        let target = overlay.target?;
        if !key.phase.is_press_like() || !self.overlay_interaction_ready {
            return Some((target, ShellCommand::Noop));
        }
        if self.auto_admin_visible() {
            return Some((target, ShellCommand::CaptureOverlayInput));
        }
        if self.notification_has_active_modal() {
            return Some(self.route_notification_key(key));
        }
        if self.time_sync_dialog_visible {
            return Some(self.route_time_sync_dialog_key(key));
        }
        if self.active_screen() == ShellScreen::ExitConfirm {
            return Some(self.route_exit_confirm_key(key));
        }
        Some(match overlay.component()? {
            ShellComponent::ContextMenu => self.route_popup_key(key),
            ShellComponent::Explorer => self.route_explorer_key(key),
            ShellComponent::Launcher => self.route_launcher_key(key),
            ShellComponent::ClockCreateInput => self.route_clock_key(key),
            ShellComponent::SetupCustomColorDialog => self.route_setup_key(key),
            ShellComponent::DiagnosticsRepairDialog
                if self.content_screen() == ShellScreen::SystemStatus =>
            {
                self.route_system_status_key(key)
            }
            ShellComponent::DiagnosticsRepairDialog => self.route_diagnostics_key(key),
            ShellComponent::SystemStatus => self.route_system_status_key(key),
            ShellComponent::UserManagement => self.route_user_management_key(key),
            ShellComponent::Editor => (target, ShellCommand::EditorKey(key.clone())),
            ShellComponent::Settings => (target, ShellCommand::SettingsKey(key.clone())),
            ShellComponent::Logs => (target, ShellCommand::LogsKey(key.clone())),
            ShellComponent::Management => (target, ShellCommand::ManagementKey(key.clone())),
            _ => (target, ShellCommand::CaptureOverlayInput),
        })
    }

    pub(in crate::session) fn route_overlay_mouse(
        &mut self,
        mouse: MouseInput,
        hit: Option<ShellComponent>,
        at: Instant,
    ) -> Option<(RoutedTarget, ShellCommand)> {
        let overlay = self.interactive_overlays().pop()?;
        let target = overlay.target?;
        if !self.overlay_interaction_ready || self.auto_admin_visible() {
            return Some((target, ShellCommand::CaptureOverlayInput));
        }
        if self.notification_has_active_modal() {
            return Some(self.route_notification_mouse(mouse, hit));
        }
        if self.time_sync_dialog_visible {
            return Some(self.route_time_sync_dialog_mouse(mouse, hit));
        }
        Some(match overlay.component()? {
            ShellComponent::ContextMenu => self.route_popup_mouse(mouse, hit, at),
            ShellComponent::Explorer => self.route_explorer_mouse(mouse, hit, at),
            ShellComponent::Launcher => self.route_launcher_mouse(mouse, at),
            ShellComponent::ClockCreateInput => self.route_clock_mouse(mouse, hit),
            ShellComponent::SetupCustomColorDialog => self.route_setup_mouse(mouse, hit),
            ShellComponent::DiagnosticsRepairDialog
                if self.content_screen() == ShellScreen::SystemStatus =>
            {
                self.route_system_status_mouse(mouse, hit, at)
            }
            ShellComponent::DiagnosticsRepairDialog => self.route_diagnostics_mouse(mouse, hit),
            ShellComponent::SystemStatus => self.route_system_status_mouse(mouse, hit, at),
            ShellComponent::UserManagement => self.route_user_management_mouse(mouse, hit),
            ShellComponent::Editor => (target, ShellCommand::EditorPointer(mouse)),
            ShellComponent::Settings => (target, ShellCommand::SettingsPointer(mouse)),
            ShellComponent::Logs => (target, ShellCommand::LogsPointer(mouse)),
            ShellComponent::Management => (target, ShellCommand::ManagementPointer(mouse)),
            _ => (target, ShellCommand::CaptureOverlayInput),
        })
    }

    pub(in crate::session) fn route_overlay_paste(
        &self,
        value: &str,
    ) -> Option<(RoutedTarget, ShellCommand)> {
        let overlay = self.interactive_overlays().pop()?;
        let target = overlay.target?;
        // Only a form that explicitly handles paste may consume it. In
        // particular, an Editor menu must not paste into the covered document.
        let command = match overlay
            .component()
            .filter(|_| self.overlay_interaction_ready)
        {
            Some(ShellComponent::Management) => ShellCommand::ManagementPaste(value.to_owned()),
            Some(ShellComponent::Editor) if self.config_editor_form_visible() => {
                ShellCommand::EditorPaste(value.to_owned())
            }
            _ => ShellCommand::CaptureOverlayInput,
        };
        Some((target, command))
    }
}
