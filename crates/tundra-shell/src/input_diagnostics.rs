impl ShellState {
    fn record_input_diagnostics(&mut self, routed: &RoutedEvent) {
        match &routed.input {
            InputEvent::Key(key) => {
                self.last_key_event = Some(key.label());
            }
            InputEvent::Mouse(mouse) => {
                let mut summary = self.record_mouse_drag_diagnostics(*mouse);
                if matches!(
                    &routed.command,
                    ShellCommand::Activate {
                        click: ClickKind::Double,
                        ..
                    }
                ) && let MouseInput::Down { button, .. } = *mouse
                {
                    summary = format!("Mouse DoubleClick {}", button.label());
                }

                self.last_mouse_event = Some(summary);
                self.mouse_coordinates = Some(mouse.coordinates());
                self.mouse_scroll_direction = mouse
                    .scroll_direction()
                    .map(|direction| direction.label().to_string());
            }
            InputEvent::Resize { width, height } => {
                self.last_resize_event = Some(format!("{width}x{height}"));
            }
            InputEvent::FocusGained => {
                self.last_key_event = Some("FocusGained".to_string());
            }
            InputEvent::FocusLost => {
                self.last_key_event = Some("FocusLost".to_string());
            }
            InputEvent::Paste(value) => {
                self.last_key_event = Some(format!("Paste({} chars)", value.chars().count()));
            }
            InputEvent::Tick | InputEvent::Shutdown => {}
        }
    }

    fn record_mouse_drag_diagnostics(&mut self, mouse: MouseInput) -> String {
        match mouse {
            MouseInput::Down {
                button,
                coordinates,
                ..
            } => {
                self.drag_tracker = Some(DragTracker {
                    button,
                    last_coordinates: coordinates,
                });
                self.mouse_drag_direction = None;
                mouse.summary()
            }
            MouseInput::Drag {
                button,
                coordinates,
                ..
            } => {
                let previous = self
                    .drag_tracker
                    .filter(|tracker| tracker.button == button)
                    .map(|tracker| tracker.last_coordinates);
                let direction =
                    previous.and_then(|previous| drag_direction_between(previous, coordinates));

                self.drag_tracker = Some(DragTracker {
                    button,
                    last_coordinates: coordinates,
                });
                self.mouse_drag_direction =
                    direction.map(|direction| direction.label().to_string());

                if let Some(direction) = direction {
                    format!("Mouse Drag {} to {}", button.label(), direction.label())
                } else {
                    mouse.summary()
                }
            }
            MouseInput::Up { .. } | MouseInput::Moved { .. } | MouseInput::Scroll { .. } => {
                self.drag_tracker = None;
                self.mouse_drag_direction = None;
                mouse.summary()
            }
        }
    }
}
