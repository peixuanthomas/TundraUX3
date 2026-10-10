use super::overlays::ShellOverlayDescriptor;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverlayFocusFrame {
    id: String,
    restore: ShellComponent,
}

#[cfg(test)]
#[path = "../../tests/unit/session/overlay_management.rs"]
mod tests;

/// Remembers focus for each visible overlay without owning page navigation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ShellOverlayManager {
    screen: Option<ShellScreen>,
    popup_origin: Option<(ShellScreen, ShellPopup)>,
    frames: Vec<OverlayFocusFrame>,
    last_focus: Option<ShellComponent>,
}

impl ShellOverlayManager {
    fn synchronize(
        &mut self,
        screen: ShellScreen,
        overlays: &[ShellOverlayDescriptor],
        current: ShellComponent,
        ready: bool,
    ) -> (ShellComponent, bool) {
        let page_changed = self.screen != Some(screen);
        if page_changed {
            self.frames.clear();
            self.screen = Some(screen);
            self.last_focus = Some(current);
        }
        let common = self
            .frames
            .iter()
            .zip(overlays)
            .take_while(|(frame, overlay)| frame.id == overlay.id)
            .count();
        let changed = page_changed || common != self.frames.len() || common != overlays.len();
        let mut focus = current;
        if changed {
            // Restore before opening a replacement. Queued notifications must
            // retain the original page focus rather than a closed dialog.
            focus = self
                .frames
                .get(common)
                .map(|frame| frame.restore)
                .unwrap_or_else(|| self.last_focus.unwrap_or(current));
            self.frames.truncate(common);
            for overlay in &overlays[common..] {
                self.frames.push(OverlayFocusFrame {
                    id: overlay.id.clone(),
                    restore: focus,
                });
                if let Some(first) = overlay.focus_order().first() {
                    focus = *first;
                }
            }
        }
        if ready && let Some(overlay) = overlays.last() {
            let order = overlay.focus_order();
            if !order.is_empty() && !order.contains(&focus) {
                focus = order[0];
            }
        }
        self.last_focus = Some(focus);
        (focus, changed)
    }
}

impl ShellSession {
    pub(in crate::session) fn synchronize_overlay_focus(&mut self) -> bool {
        let screen = self.content_screen();
        if let Some((origin, popup)) = self.overlay_manager.popup_origin
            && origin != screen
            && self.active_popup == Some(popup)
        {
            self.active_popup = None;
        }
        self.overlay_manager.popup_origin = self.active_popup.map(|popup| (screen, popup));
        let overlays = self.interactive_overlays();
        let focus = self.focused_component;
        let ready = self.overlay_interaction_ready;
        let (focus, changed) = self
            .overlay_manager
            .synchronize(screen, &overlays, focus, ready);
        self.focused_component = focus;
        if let Some(field) = setup_field_for_component(focus) {
            self.setup_focused_field = field;
        }
        if changed {
            self.cancel_pointer_gestures_for_modal();
            self.system_status_widget_drag = None;
            self.editor_drag_anchor = None;
            self.button_regions.clear();
            self.hovered_component = None;
            self.last_click = None;
        }
        changed
    }

    /// Called before direct/programmatic opens as well as before routed input.
    pub(in crate::session) fn capture_modal_focus_context(&mut self) {
        self.synchronize_overlay_focus();
    }

    pub(in crate::session) fn prepare_modal_focus_for_follow_up(&mut self) {
        self.synchronize_overlay_focus();
    }

    pub(in crate::session) fn finish_modal_focus_transition(&mut self) {
        if !self.notification_has_active_modal() {
            self.notification_pointer_capture = None;
            self.notification_message_scroll = 0;
        }
        if self.synchronize_overlay_focus() {
            self.refresh_hit_map();
        }
    }

    pub(in crate::session) fn cancel_pointer_gestures_for_modal(&mut self) {
        self.button_pointer_capture = None;
        self.notification_pointer_capture = None;
        self.notification_scrollbar_drag = None;
        self.diagnostics_detail_drag = None;
        self.scrollbar_drag = None;
        self.launcher_drag = None;
        self.drag_tracker = None;
        self.editor_table_resize = None;
        self.cancel_auto_admin_pointer();
        self.cancel_touch_pages_pointer();
        self.cancel_management_pointer_gesture();
        self.cancel_logs_pointer_gesture();
    }
}
