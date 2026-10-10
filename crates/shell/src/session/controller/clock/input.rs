use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_clock_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(self.focused_component);
        if key.phase == InputPhase::Release {
            return (target, ShellCommand::Noop);
        }
        if key.has_non_shift_modifier()
            || (key.phase != InputPhase::Press
                && (matches!(
                    key.key,
                    InputKey::Enter | InputKey::Escape | InputKey::F(3) | InputKey::F(4)
                ) || (matches!(key.key, InputKey::Char(_))
                    && !self
                        .clock_create_state
                        .as_ref()
                        .is_some_and(|create| create.focus == ui::ClockCreateDialogFocus::Input))))
        {
            return (target, ShellCommand::RecordInput);
        }
        if self.is_strict_guest() {
            let target = RoutedTarget::Component(ShellComponent::ClockButton);
            return match &key.key {
                InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseClock),
                InputKey::Enter | InputKey::Char(' ') => (target, ShellCommand::CloseClock),
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if let Some(create) = &self.clock_create_state {
            let target = RoutedTarget::Modal(ShellComponent::ClockCreateDialog);
            return match &key.key {
                InputKey::F(3) => (target, ShellCommand::ClockCreateAlarm),
                InputKey::F(4) => (target, ShellCommand::ClockCreateCountdown),
                InputKey::Escape => (target, ShellCommand::ClockCloseCreate),
                InputKey::BackTab => (target, ShellCommand::ClockCreateFocusPrevious),
                InputKey::Tab if key.modifiers.shift => {
                    (target, ShellCommand::ClockCreateFocusPrevious)
                }
                InputKey::Tab => (target, ShellCommand::ClockCreateFocusNext),
                InputKey::Up if create.focus == ui::ClockCreateDialogFocus::Input => (
                    target,
                    ShellCommand::ClockCreateAdjust(create.active_field, 1),
                ),
                InputKey::Down if create.focus == ui::ClockCreateDialogFocus::Input => (
                    target,
                    ShellCommand::ClockCreateAdjust(create.active_field, -1),
                ),
                InputKey::Up | InputKey::Left => (target, ShellCommand::ClockCreateFocusPrevious),
                InputKey::Down | InputKey::Right => (target, ShellCommand::ClockCreateFocusNext),
                InputKey::Enter => match create.focus {
                    ui::ClockCreateDialogFocus::Input => {
                        (target, ShellCommand::ClockCreateFocusNext)
                    }
                    ui::ClockCreateDialogFocus::CreateAlarm => {
                        (target, ShellCommand::ClockCreateAlarm)
                    }
                    ui::ClockCreateDialogFocus::CreateCountdown => {
                        (target, ShellCommand::ClockCreateCountdown)
                    }
                },
                InputKey::Char(' ') if create.focus == ui::ClockCreateDialogFocus::CreateAlarm => {
                    (target, ShellCommand::ClockCreateAlarm)
                }
                InputKey::Char(' ')
                    if create.focus == ui::ClockCreateDialogFocus::CreateCountdown =>
                {
                    (target, ShellCommand::ClockCreateCountdown)
                }
                InputKey::Backspace if create.focus == ui::ClockCreateDialogFocus::Input => {
                    (target, ShellCommand::ClockCreateBackspace)
                }
                InputKey::Char(character) if create.focus == ui::ClockCreateDialogFocus::Input => {
                    (target, ShellCommand::ClockCreateAppend(*character))
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        let target = RoutedTarget::Component(self.focused_component);
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseClock),
            InputKey::BackTab => (target, ShellCommand::FocusPrevious),
            InputKey::Tab if key.modifiers.shift => (target, ShellCommand::FocusPrevious),
            InputKey::Tab => (target, ShellCommand::FocusNext),
            InputKey::Char('n' | 'N') => (target, ShellCommand::ClockOpenCreate),
            InputKey::Char('m' | 'M') => (target, ShellCommand::ClockActivateSelected),
            InputKey::Enter | InputKey::Char(' ')
                if self.focused_component == ShellComponent::ClockNewButton =>
            {
                (target, ShellCommand::ClockOpenCreate)
            }
            InputKey::Enter | InputKey::Char(' ')
                if self.focused_component == ShellComponent::ClockEntryList =>
            {
                (target, ShellCommand::ClockActivateSelected)
            }
            InputKey::Up if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectPrevious)
            }
            InputKey::Down if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectNext)
            }
            InputKey::PageUp if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectPageUp)
            }
            InputKey::PageDown if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectPageDown)
            }
            InputKey::Home if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectFirst)
            }
            InputKey::End if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectLast)
            }
            _ => (target, ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn clock_button_activation_command(&self) -> ShellCommand {
        if self.active_screen() == ShellScreen::Clock {
            ShellCommand::CloseClock
        } else {
            ShellCommand::OpenClock
        }
    }

    pub(in crate::session) fn route_time_sync_dialog_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Modal(ShellComponent::TimeSyncDialog);
        match &key.key {
            InputKey::Escape | InputKey::Enter | InputKey::Char(' ') => {
                (target, ShellCommand::CloseTimeSyncDialog)
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    pub(in crate::session) fn route_clock_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let modal_target = RoutedTarget::Modal(ShellComponent::ClockCreateDialog);

        if self.clock_create_state.is_some() {
            return match mouse.kind {
                ui::MouseEventKind::Moved => (modal_target, ShellCommand::Hover(hit_target)),
                ui::MouseEventKind::Down(PointerButton::Left) => match hit_target {
                    Some(ShellComponent::ClockCreateInput) => (
                        modal_target,
                        self.clock_create_control_at(coordinates)
                            .unwrap_or(ShellCommand::CaptureOverlayInput),
                    ),
                    Some(ShellComponent::ClockCreateAlarmButton) => {
                        (modal_target, ShellCommand::ClockCreateAlarm)
                    }
                    Some(ShellComponent::ClockCreateCountdownButton) => {
                        (modal_target, ShellCommand::ClockCreateCountdown)
                    }
                    _ => (modal_target, ShellCommand::CaptureOverlayInput),
                },
                _ => (modal_target, ShellCommand::CaptureOverlayInput),
            };
        }

        let target = target_route(hit_target);
        match mouse.kind {
            ui::MouseEventKind::Moved => (target, ShellCommand::Hover(hit_target)),
            ui::MouseEventKind::Scroll(ScrollDirection::Up)
                if hit_target == Some(ShellComponent::ClockEntryList) =>
            {
                (target, ShellCommand::ClockSelectPrevious)
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down)
                if hit_target == Some(ShellComponent::ClockEntryList) =>
            {
                (target, ShellCommand::ClockSelectNext)
            }
            ui::MouseEventKind::Down(PointerButton::Left) => match hit_target {
                Some(ShellComponent::ClockButton) => (target, ShellCommand::CloseClock),
                Some(ShellComponent::ClockNewButton) => (target, ShellCommand::ClockOpenCreate),
                Some(ShellComponent::ClockEntryList) => self
                    .clock_entry_id_at(coordinates)
                    .map(|id| (target, ShellCommand::ClockManageEntry(id)))
                    .unwrap_or((target, ShellCommand::RecordInput)),
                _ => (target, ShellCommand::RecordInput),
            },
            ui::MouseEventKind::Down(PointerButton::Right) => {
                (target, ShellCommand::CaptureOverlayInput)
            }
            _ => (target, ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn clock_create_control_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<ShellCommand> {
        let (width, height) = self.terminal_size;
        let ui::ShellLayout::Full { main, .. } =
            self.shell_layout_for(Rect::new(0, 0, width, height))
        else {
            return None;
        };
        let layout = ui::clock_page_layout(main, &self.to_clock_view_model()).create_dialog?;
        for field in 0..3 {
            if rect_contains(layout.increments[field], coordinates) {
                return Some(ShellCommand::ClockCreateAdjust(field, 1));
            }
            if rect_contains(layout.decrements[field], coordinates) {
                return Some(ShellCommand::ClockCreateAdjust(field, -1));
            }
            if rect_contains(layout.values[field], coordinates)
                || rect_contains(layout.labels[field], coordinates)
            {
                return Some(ShellCommand::ClockCreateSelectField(field));
            }
        }
        None
    }

    pub(in crate::session) fn clock_entry_id_at(&self, coordinates: CellPosition) -> Option<u64> {
        let (width, height) = self.terminal_size;
        let area = Rect::new(0, 0, width, height);
        let ui::ShellLayout::Full { main, .. } = self.shell_layout_for(area) else {
            return None;
        };
        let snapshot = self.app.snapshot().clock;
        let model = self.to_clock_view_model_at(&snapshot, Instant::now());
        ui::clock_page_layout(main, &model)
            .entry_rows
            .into_iter()
            .find(|row| rect_contains(row.area, coordinates))
            .map(|row| row.id)
    }

    pub(in crate::session) fn route_time_sync_dialog_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        match mouse.kind {
            ui::MouseEventKind::Moved => (
                RoutedTarget::Modal(ShellComponent::TimeSyncDialog),
                ShellCommand::Hover(hit_target),
            ),
            ui::MouseEventKind::Down(_) => (
                RoutedTarget::Modal(ShellComponent::TimeSyncDialog),
                ShellCommand::CloseTimeSyncDialog,
            ),
            _ => (
                RoutedTarget::Modal(ShellComponent::TimeSyncDialog),
                ShellCommand::CaptureOverlayInput,
            ),
        }
    }
}
