use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_help_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let command = match key.key {
            InputKey::Escape | InputKey::F(1) if key.phase == InputPhase::Press => {
                ShellCommand::ClosePageHelp
            }
            InputKey::Enter | InputKey::Char(' ') if key.is_unmodified_action_key() => {
                ShellCommand::ClosePageHelp
            }
            InputKey::Up => ShellCommand::ScrollPageHelp(-1, false),
            InputKey::Down => ShellCommand::ScrollPageHelp(1, false),
            InputKey::PageUp => ShellCommand::ScrollPageHelp(-1, true),
            InputKey::PageDown => ShellCommand::ScrollPageHelp(1, true),
            _ => ShellCommand::CaptureOverlayInput,
        };
        (RoutedTarget::Modal(ShellComponent::HelpDialog), command)
    }

    pub(in crate::session) fn route_help_mouse(
        &mut self,
        mouse: MouseInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Modal(ShellComponent::HelpDialog);
        let ui::NotificationLayout::Dialog(layout) = self.page_help_layout() else {
            return (target, ShellCommand::CaptureOverlayInput);
        };
        let command = match mouse.kind {
            ui::MouseEventKind::Scroll(ui::ScrollDirection::Up) => {
                ShellCommand::ScrollPageHelp(-1, false)
            }
            ui::MouseEventKind::Scroll(ui::ScrollDirection::Down) => {
                ShellCommand::ScrollPageHelp(1, false)
            }
            ui::MouseEventKind::Down(PointerButton::Left)
                if layout
                    .actions
                    .iter()
                    .any(|action| rect_contains(action.area, mouse.coordinates())) =>
            {
                ShellCommand::ClosePageHelp
            }
            _ => ShellCommand::CaptureOverlayInput,
        };
        (target, command)
    }

    pub(in crate::session) fn scroll_page_help(&mut self, delta: isize, page: bool) -> ShellAction {
        if let ui::NotificationLayout::Dialog(layout) = self.page_help_layout() {
            let step = if page {
                usize::from(layout.message.height).max(1)
            } else {
                1
            };
            self.help_scroll = self
                .help_scroll
                .saturating_add_signed(delta.saturating_mul(step as isize))
                .min(layout.max_scroll_offset);
        }
        ShellAction::Redraw
    }

    pub(in crate::session) fn handle_help_scrollbar(&mut self, mouse: MouseInput) -> bool {
        if matches!(mouse.kind, ui::MouseEventKind::Up(PointerButton::Left)) {
            return self.help_scrollbar_drag.take().is_some();
        }
        let ui::NotificationLayout::Dialog(layout) = self.page_help_layout() else {
            return false;
        };
        let Some(track) = layout.scrollbar else {
            return false;
        };
        let scrollbar = ui::components::Scrollbar::new(
            layout.message_line_count,
            usize::from(layout.message.height),
            layout.scroll_offset,
        );
        let (start, length) = scrollbar.thumb_range(track);
        match mouse.kind {
            ui::MouseEventKind::Down(PointerButton::Left)
                if rect_contains(track, mouse.coordinates()) =>
            {
                let relative = mouse.row().saturating_sub(track.y);
                self.help_scrollbar_drag = Some(if (start..start + length).contains(&relative) {
                    relative - start
                } else {
                    length / 2
                });
            }
            ui::MouseEventKind::Drag(PointerButton::Left) if self.help_scrollbar_drag.is_some() => {
            }
            _ => return false,
        }
        self.help_scroll = scrollbar_window_start(
            mouse.row(),
            self.help_scrollbar_drag.unwrap(),
            track.y,
            track.height,
            length,
            layout.message_line_count,
            usize::from(layout.message.height),
        );
        true
    }
}
