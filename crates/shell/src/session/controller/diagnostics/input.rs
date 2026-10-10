use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_diagnostics_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Diagnostics);
        if !key.phase.is_press_like()
            || key.has_non_shift_modifier()
            || (key.phase != InputPhase::Press
                && matches!(
                    key.key,
                    InputKey::Char(_)
                        | InputKey::Enter
                        | InputKey::Escape
                        | InputKey::Tab
                        | InputKey::BackTab
                        | InputKey::F(_)
                ))
        {
            return (target, ShellCommand::Noop);
        }
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(self.shell_layout_for(area), ui::ShellLayout::Compact(_)) {
            return if matches!(&key.key, InputKey::Escape) {
                (RoutedTarget::Global, ShellCommand::CloseDiagnostics)
            } else {
                (
                    RoutedTarget::Component(ShellComponent::CompactHome),
                    ShellCommand::CaptureOverlayInput,
                )
            };
        }

        if !self.diagnostics_repair_preview.is_empty() {
            let target = RoutedTarget::Modal(ShellComponent::DiagnosticsRepairDialog);
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::DiagnosticsCancelRepair),
                InputKey::Char('y' | 'Y') => (target, ShellCommand::DiagnosticsConfirmRepair),
                InputKey::Char('r' | 'R') => (RoutedTarget::Global, ShellCommand::Restart),
                InputKey::Up => (target, ShellCommand::DiagnosticsRepairPrevious),
                InputKey::Down => (target, ShellCommand::DiagnosticsRepairNext),
                InputKey::Tab | InputKey::BackTab | InputKey::Left | InputKey::Right => {
                    (target, ShellCommand::DiagnosticsRepairToggleAction)
                }
                InputKey::Enter | InputKey::Char(' ')
                    if self.diagnostics_repair_confirm_selected =>
                {
                    (target, ShellCommand::DiagnosticsConfirmRepair)
                }
                InputKey::Enter | InputKey::Char(' ') => {
                    (target, ShellCommand::DiagnosticsCancelRepair)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if self.diagnostics_is_busy()
            && matches!(
                key.key,
                InputKey::Char(_) | InputKey::Enter | InputKey::F(_)
            )
        {
            return (target, ShellCommand::Noop);
        }
        if self.diagnostics_restart_is_required() {
            return match &key.key {
                InputKey::Enter | InputKey::Char('r' | 'R') => {
                    (RoutedTarget::Global, ShellCommand::Restart)
                }
                InputKey::Char('e' | 'E') => (RoutedTarget::Global, ShellCommand::RequestExit),
                InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseDiagnostics),
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseDiagnostics),
            InputKey::Tab | InputKey::BackTab | InputKey::Left | InputKey::Right => {
                (target, ShellCommand::Noop)
            }
            InputKey::Up => (target, ShellCommand::DiagnosticsPrevious),
            InputKey::Down => (target, ShellCommand::DiagnosticsNext),
            InputKey::PageUp => (target, ShellCommand::DiagnosticsPageUp),
            InputKey::PageDown => (target, ShellCommand::DiagnosticsPageDown),
            InputKey::Home => (target, ShellCommand::DiagnosticsFirst),
            InputKey::End => (target, ShellCommand::DiagnosticsLast),
            InputKey::Char('r' | 'R') | InputKey::F(5) => (target, ShellCommand::DiagnosticsRescan),
            InputKey::Char('x' | 'X') if !self.diagnostics_scanning => {
                (RoutedTarget::Global, ShellCommand::Restart)
            }
            InputKey::Char('f' | 'F') if self.diagnostics_tab == ui::DiagnosticsTab::Health => {
                (target, ShellCommand::DiagnosticsPreviewSelectedRepair)
            }
            InputKey::Char('a' | 'A') if self.diagnostics_tab == ui::DiagnosticsTab::Health => {
                (target, ShellCommand::DiagnosticsPreviewAllRepairs)
            }
            InputKey::Char('c' | 'C') => (target, ShellCommand::DiagnosticsCopySummary),

            InputKey::Char('o' | 'O') => (target, ShellCommand::DiagnosticsOpenReport),
            InputKey::Char('e' | 'E')
                if self.diagnostics_tab != ui::DiagnosticsTab::Health
                    && self.diagnostics_can_view_details() =>
            {
                (target, ShellCommand::DiagnosticsOpenLogsInExplorer)
            }
            InputKey::Enter
                if matches!(
                    self.diagnostics_tab,
                    ui::DiagnosticsTab::Logs | ui::DiagnosticsTab::Incidents
                ) =>
            {
                (target, ShellCommand::DiagnosticsOpenReport)
            }
            _ => (target, ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_diagnostics_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = self.shell_layout_for(area) else {
            return (
                target_route(hit_target),
                if matches!(mouse.kind, ui::MouseEventKind::Moved) {
                    ShellCommand::Hover(hit_target)
                } else {
                    ShellCommand::CaptureOverlayInput
                },
            );
        };
        let model = self.to_diagnostics_view_model();
        let layout = ui::diagnostics_layout(main, &model);
        let diagnostic_target = ui::diagnostics_hit_test(&layout, (coordinates.0, coordinates.1));
        let routed = if self.diagnostics_repair_preview.is_empty() {
            RoutedTarget::Component(ShellComponent::Diagnostics)
        } else {
            RoutedTarget::Modal(ShellComponent::DiagnosticsRepairDialog)
        };

        match mouse.kind {
            ui::MouseEventKind::Moved => (routed, ShellCommand::Hover(hit_target)),
            ui::MouseEventKind::Scroll(ScrollDirection::Up)
                if self.diagnostics_repair_preview.is_empty() =>
            {
                (routed, ShellCommand::DiagnosticsPrevious)
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down)
                if self.diagnostics_repair_preview.is_empty() =>
            {
                (routed, ShellCommand::DiagnosticsNext)
            }
            ui::MouseEventKind::Down(PointerButton::Left) => match diagnostic_target {
                Some(ui::DiagnosticsHitTarget::Tab(ui::DiagnosticsTab::Health)) => {
                    (routed, ShellCommand::DiagnosticsHealthTab)
                }
                Some(ui::DiagnosticsHitTarget::Tab(ui::DiagnosticsTab::Logs)) => {
                    (routed, ShellCommand::DiagnosticsLogsTab)
                }
                Some(ui::DiagnosticsHitTarget::Tab(ui::DiagnosticsTab::Incidents)) => {
                    (routed, ShellCommand::DiagnosticsIncidentsTab)
                }
                Some(ui::DiagnosticsHitTarget::Check(index))
                | Some(ui::DiagnosticsHitTarget::Log(index))
                | Some(ui::DiagnosticsHitTarget::Incident(index)) => {
                    (routed, ShellCommand::DiagnosticsSelectIndex(index))
                }
                Some(ui::DiagnosticsHitTarget::Scrollbar)
                    if self.diagnostics_repair_preview.is_empty() =>
                {
                    (
                        routed,
                        ShellCommand::DiagnosticsScrollbarPointerDown(coordinates),
                    )
                }
                Some(ui::DiagnosticsHitTarget::RepairConfirm) => {
                    (routed, ShellCommand::DiagnosticsConfirmRepair)
                }
                Some(ui::DiagnosticsHitTarget::RepairRestart) => {
                    (RoutedTarget::Global, ShellCommand::Restart)
                }
                Some(ui::DiagnosticsHitTarget::RepairCancel) => {
                    (routed, ShellCommand::DiagnosticsCancelRepair)
                }
                Some(ui::DiagnosticsHitTarget::RepairItem(index)) => {
                    (routed, ShellCommand::DiagnosticsSelectRepairItem(index))
                }
                _ => (routed, ShellCommand::CaptureOverlayInput),
            },
            _ => (routed, ShellCommand::CaptureOverlayInput),
        }
    }
}
