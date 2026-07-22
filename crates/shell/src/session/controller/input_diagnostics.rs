use super::super::*;
impl ShellSession {
    pub(in crate::session) fn record_input_diagnostics(&mut self, routed: &RoutedEvent) {
        match &routed.input {
            InputEvent::Key(key) => self.last_key_event = Some(key.label()),
            InputEvent::Mouse(mouse) => {
                let mut summary = self.record_mouse_drag_diagnostics(*mouse);
                if matches!(
                    &routed.command,
                    ShellCommand::Activate {
                        click: ClickKind::Double,
                        ..
                    }
                ) && let ui::MouseEventKind::Down(button) = mouse.kind
                {
                    summary = format!("Mouse DoubleClick {}", button.label());
                }
                self.last_mouse_event = Some(summary);
                self.mouse_coordinates = Some(mouse.coordinates());
                self.mouse_scroll_direction =
                    mouse.scroll_direction().map(|direction| match direction {
                        ScrollDirection::Up => "Up".to_string(),
                        ScrollDirection::Down => "Down".to_string(),
                        ScrollDirection::Left => "Left".to_string(),
                        ScrollDirection::Right => "Right".to_string(),
                    });
            }
            InputEvent::Resize { width, height } => {
                self.last_resize_event = Some(format!("{width}x{height}"))
            }
            InputEvent::FocusGained => self.last_key_event = Some("FocusGained".to_string()),
            InputEvent::FocusLost => self.last_key_event = Some("FocusLost".to_string()),
            InputEvent::Paste(value) => {
                self.last_key_event = Some(format!("Paste({} chars)", value.chars().count()))
            }
            InputEvent::Tick | InputEvent::Shutdown => {}
        }
    }

    pub(in crate::session) fn record_mouse_drag_diagnostics(
        &mut self,
        mouse: MouseInput,
    ) -> String {
        let coordinates = mouse.coordinates();
        match mouse.kind {
            ui::MouseEventKind::Down(button) => {
                self.drag_tracker = Some(DragTracker {
                    button,
                    origin_coordinates: coordinates,
                    last_coordinates: coordinates,
                });
                self.mouse_drag_direction = None;
                mouse.summary()
            }
            ui::MouseEventKind::Drag(button) => {
                let previous = self
                    .drag_tracker
                    .filter(|tracker| tracker.button == button)
                    .map(|tracker| tracker.last_coordinates);
                let origin = self
                    .drag_tracker
                    .filter(|tracker| tracker.button == button)
                    .map(|tracker| tracker.origin_coordinates)
                    .unwrap_or(coordinates);
                let direction =
                    previous.and_then(|previous| drag_direction_between(previous, coordinates));
                self.drag_tracker = Some(DragTracker {
                    button,
                    origin_coordinates: origin,
                    last_coordinates: coordinates,
                });
                self.mouse_drag_direction =
                    direction.map(|direction| direction.label().to_string());
                direction.map_or_else(
                    || mouse.summary(),
                    |direction| format!("Mouse Drag {} to {}", button.label(), direction.label()),
                )
            }
            ui::MouseEventKind::Up(_)
            | ui::MouseEventKind::Moved
            | ui::MouseEventKind::Click(_)
            | ui::MouseEventKind::DoubleClick(_)
            | ui::MouseEventKind::Scroll(_) => {
                self.drag_tracker = None;
                self.mouse_drag_direction = None;
                mouse.summary()
            }
        }
    }
}
