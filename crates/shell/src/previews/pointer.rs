use ui::{
    InputEvent, MouseButton, MouseEventKind,
    components::{ButtonRegion, contains_point},
};

pub(super) struct PreviewPointer {
    pub(super) hovered: Option<ButtonRegion>,
    pub(super) pressed: Option<ButtonRegion>,
    position: Option<(u16, u16)>,
    pub(super) keyboard: bool,
}

impl Default for PreviewPointer {
    fn default() -> Self {
        Self {
            hovered: None,
            pressed: None,
            position: None,
            keyboard: true,
        }
    }
}

impl PreviewPointer {
    pub(super) fn prepare(
        &mut self,
        input: InputEvent,
        regions: &[ButtonRegion],
    ) -> Option<InputEvent> {
        match input {
            InputEvent::Key(ref key) if key.is_press_like() => {
                self.hovered = None;
                self.pressed = None;
                self.keyboard = true;
            }
            InputEvent::FocusLost | InputEvent::Resize { .. } => {
                self.hovered = None;
                self.pressed = None;
                self.keyboard = false;
            }
            InputEvent::Mouse(mut mouse) => {
                let position = (mouse.column(), mouse.row());
                if mouse.kind == MouseEventKind::Move && self.position == Some(position) {
                    return None;
                }
                self.position = Some(position);
                self.keyboard = false;
                let target = regions
                    .iter()
                    .rev()
                    .find(|region| {
                        !region.disabled && contains_point(region.area, position.0, position.1)
                    })
                    .cloned();
                match mouse.kind {
                    MouseEventKind::Move => self.hovered = target,
                    MouseEventKind::Down(MouseButton::Left) => {
                        self.hovered = target.clone();
                        self.pressed = target;
                        if self.pressed.is_some() {
                            return None;
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        if self.pressed != target {
                            self.pressed = None;
                        }
                        self.hovered = target;
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        self.hovered = None;
                        let pressed = self.pressed.take();
                        if pressed.is_some() && pressed == target {
                            mouse.kind = MouseEventKind::Click(MouseButton::Left);
                            return Some(InputEvent::Mouse(mouse));
                        }
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        self.pressed = None;
                        self.hovered = None;
                    }
                    _ => {}
                }
                return Some(InputEvent::Mouse(mouse));
            }
            _ => {}
        }
        Some(input)
    }
}
