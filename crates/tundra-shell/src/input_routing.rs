impl ShellState {
    fn route_key_input(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        if !key.phase.is_press_like() {
            return (RoutedTarget::Global, ShellCommand::Noop);
        }

        if key.is_ctrl_c() {
            return (RoutedTarget::Global, ShellCommand::Shutdown);
        }

        if self.notifications.has_active_modal() {
            return self.route_notification_key(key);
        }

        if self.time_sync_dialog_visible {
            return self.route_time_sync_dialog_key(key);
        }

        if self.active_screen() == ShellScreen::ExitConfirm {
            return self.route_exit_confirm_key(key);
        }

        if self.active_screen() == ShellScreen::Clock {
            return self.route_clock_key(key);
        }

        if self.active_screen() == ShellScreen::Diagnostics {
            return self.route_diagnostics_key(key);
        }

        if self.active_screen() == ShellScreen::FirstRunSetup {
            return self.route_setup_key(key);
        }

        if self.active_screen() == ShellScreen::Login {
            return self.route_login_key(key);
        }

        if self.active_screen() == ShellScreen::BootstrapAdmin {
            return self.route_auth_key(key);
        }

        if self.active_screen() == ShellScreen::UserManagement {
            return self.route_user_management_key(key);
        }

        if self.active_popup.is_some() {
            return self.route_popup_key(key);
        }

        if self.active_screen() == ShellScreen::Explorer {
            return self.route_explorer_key(key);
        }

        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (RoutedTarget::Global, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab) {
            return (RoutedTarget::Global, ShellCommand::FocusNext);
        }

        match self.active_screen() {
            ShellScreen::Home
                if self.focused_component == ShellComponent::HomeLogout
                    && matches!(&key.key, InputKey::Enter | InputKey::Character(' ')) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::HomeLogout),
                    ShellCommand::Logout,
                )
            }
            _ if self.focused_component == ShellComponent::ClockButton
                && matches!(&key.key, InputKey::Enter | InputKey::Character(' ')) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::ClockButton),
                    self.clock_button_activation_command(),
                )
            }
            ShellScreen::Home if matches!(&key.key, InputKey::Left) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryLeft,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Right) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryRight,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Up) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryUp,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Down) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryDown,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Home) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeFirstEntry,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::End) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeLastEntry,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Enter | InputKey::Character(' ')) => {
                (
                    RoutedTarget::Component(ShellComponent::Home),
                    ShellCommand::ActivateSelectedHomeEntry,
                )
            }
            ShellScreen::Home if key.is_character('e') || key.is_character('E') => {
                (RoutedTarget::Global, ShellCommand::OpenExplorer)
            }
            ShellScreen::Home if key.is_character('u') || key.is_character('U') => {
                (RoutedTarget::Global, ShellCommand::OpenUserManagement)
            }
            ShellScreen::Home if key.is_character('d') || key.is_character('D') => {
                (RoutedTarget::Global, ShellCommand::OpenDiagnostics)
            }
            ShellScreen::Home
                if self.current_home_username().is_some()
                    && (key.is_character('l') || key.is_character('L')) =>
            {
                (RoutedTarget::Global, ShellCommand::Logout)
            }
            ShellScreen::Home if key.is_character('q') || matches!(&key.key, InputKey::Escape) => {
                (RoutedTarget::Global, ShellCommand::RequestExit)
            }
            _ => (
                RoutedTarget::Component(self.focused_component),
                ShellCommand::RecordInput,
            ),
        }
    }

    fn route_login_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(
            tundra_ui::compute_shell_layout(area),
            tundra_ui::ShellLayout::Compact(_)
        ) {
            return if matches!(&key.key, InputKey::Escape) {
                (RoutedTarget::Global, ShellCommand::RequestExit)
            } else {
                (
                    RoutedTarget::Component(ShellComponent::CompactHome),
                    ShellCommand::CaptureOverlayInput,
                )
            };
        }
        let target = RoutedTarget::Component(self.focused_component);
        if matches!(&key.key, InputKey::Escape) {
            return (RoutedTarget::Global, ShellCommand::RequestExit);
        }
        if matches!(&key.key, InputKey::Function(2)) {
            return (target, ShellCommand::ToggleLoginPasswordVisibility);
        }
        match self.focused_component {
            ShellComponent::LoginPassword => match &key.key {
                InputKey::BackTab => (target, ShellCommand::LoginFocusUserList),
                InputKey::Tab if key.modifiers.shift => (target, ShellCommand::LoginFocusUserList),
                InputKey::Tab => (target, ShellCommand::LoginFocusPasswordVisibility),
                InputKey::Up => (target, ShellCommand::LoginFocusUserList),
                InputKey::Enter => (target, ShellCommand::SubmitLogin),
                InputKey::Backspace => (target, ShellCommand::AuthBackspace),
                InputKey::Character(character) => {
                    (target, ShellCommand::AppendAuthChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            },
            ShellComponent::LoginPasswordVisibility => match &key.key {
                InputKey::BackTab => (target, ShellCommand::LoginFocusPassword),
                InputKey::Tab if key.modifiers.shift => (target, ShellCommand::LoginFocusPassword),
                InputKey::Tab | InputKey::Right | InputKey::Down => {
                    (target, ShellCommand::LoginFocusUserList)
                }
                InputKey::Left | InputKey::Up => (target, ShellCommand::LoginFocusPassword),
                InputKey::Enter | InputKey::Character(' ') => {
                    (target, ShellCommand::ToggleLoginPasswordVisibility)
                }
                _ => (target, ShellCommand::RecordInput),
            },
            _ => match &key.key {
                InputKey::BackTab => (target, ShellCommand::LoginFocusPasswordVisibility),
                InputKey::Tab if key.modifiers.shift => {
                    (target, ShellCommand::LoginFocusPasswordVisibility)
                }
                InputKey::Tab => (target, ShellCommand::LoginFocusPassword),
                InputKey::Enter => (target, ShellCommand::LoginFocusPassword),
                InputKey::Up => (target, ShellCommand::LoginPreviousUser),
                InputKey::Down => (target, ShellCommand::LoginNextUser),
                InputKey::PageUp => (target, ShellCommand::LoginPageUserUp),
                InputKey::PageDown => (target, ShellCommand::LoginPageUserDown),
                InputKey::Home => (target, ShellCommand::LoginFirstUser),
                InputKey::End => (target, ShellCommand::LoginLastUser),
                _ => (target, ShellCommand::RecordInput),
            },
        }
    }

    fn route_auth_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(self.focused_component);
        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab | InputKey::Down) {
            return (target, ShellCommand::FocusNext);
        }
        if matches!(&key.key, InputKey::Up) {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Escape) {
            return (RoutedTarget::Global, ShellCommand::RequestExit);
        }
        if matches!(&key.key, InputKey::Enter) {
            if matches!(
                self.focused_component,
                ShellComponent::LoginUsername | ShellComponent::BootstrapUsername
            ) {
                return (target, ShellCommand::FocusNext);
            }
            return (
                target,
                match self.active_screen() {
                    ShellScreen::BootstrapAdmin => ShellCommand::SubmitBootstrapAdmin,
                    _ => ShellCommand::SubmitLogin,
                },
            );
        }
        if matches!(&key.key, InputKey::Backspace) {
            return (target, ShellCommand::AuthBackspace);
        }
        if let InputKey::Character(character) = &key.key {
            return (target, ShellCommand::AppendAuthChar(*character));
        }

        (target, ShellCommand::RecordInput)
    }

    fn route_clock_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(
            tundra_ui::compute_shell_layout(area),
            tundra_ui::ShellLayout::Compact(_)
        ) {
            return match &key.key {
                InputKey::Escape if self.clock_create_state.is_some() => (
                    RoutedTarget::Modal(ShellComponent::ClockCreateDialog),
                    ShellCommand::ClockCloseCreate,
                ),
                InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseClock),
                _ => (
                    RoutedTarget::Component(ShellComponent::CompactHome),
                    ShellCommand::CaptureOverlayInput,
                ),
            };
        }

        if self.is_strict_guest() {
            let target = RoutedTarget::Component(ShellComponent::ClockButton);
            return match &key.key {
                InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseClock),
                InputKey::Enter | InputKey::Character(' ') => (target, ShellCommand::CloseClock),
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if let Some(create) = &self.clock_create_state {
            let target = RoutedTarget::Modal(ShellComponent::ClockCreateDialog);
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::ClockCloseCreate),
                InputKey::BackTab => (target, ShellCommand::ClockCreateFocusPrevious),
                InputKey::Tab if key.modifiers.shift => {
                    (target, ShellCommand::ClockCreateFocusPrevious)
                }
                InputKey::Tab => (target, ShellCommand::ClockCreateFocusNext),
                InputKey::Up | InputKey::Left => (target, ShellCommand::ClockCreateFocusPrevious),
                InputKey::Down | InputKey::Right => (target, ShellCommand::ClockCreateFocusNext),
                InputKey::Enter => match create.focus {
                    tundra_ui::ClockCreateDialogFocus::Input => {
                        (target, ShellCommand::ClockCreateFocusNext)
                    }
                    tundra_ui::ClockCreateDialogFocus::CreateAlarm => {
                        (target, ShellCommand::ClockCreateAlarm)
                    }
                    tundra_ui::ClockCreateDialogFocus::CreateCountdown => {
                        (target, ShellCommand::ClockCreateCountdown)
                    }
                },
                InputKey::Character(' ')
                    if create.focus == tundra_ui::ClockCreateDialogFocus::CreateAlarm =>
                {
                    (target, ShellCommand::ClockCreateAlarm)
                }
                InputKey::Character(' ')
                    if create.focus == tundra_ui::ClockCreateDialogFocus::CreateCountdown =>
                {
                    (target, ShellCommand::ClockCreateCountdown)
                }
                InputKey::Backspace if create.focus == tundra_ui::ClockCreateDialogFocus::Input => {
                    (target, ShellCommand::ClockCreateBackspace)
                }
                InputKey::Character(character)
                    if create.focus == tundra_ui::ClockCreateDialogFocus::Input =>
                {
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
            InputKey::Character('n' | 'N') => (target, ShellCommand::ClockOpenCreate),
            InputKey::Enter | InputKey::Character(' ')
                if self.focused_component == ShellComponent::ClockNewButton =>
            {
                (target, ShellCommand::ClockOpenCreate)
            }
            InputKey::Enter | InputKey::Character(' ')
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

    fn route_diagnostics_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(
            tundra_ui::compute_shell_layout(area),
            tundra_ui::ShellLayout::Compact(_)
        ) {
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
                InputKey::Up => (target, ShellCommand::DiagnosticsRepairPrevious),
                InputKey::Down => (target, ShellCommand::DiagnosticsRepairNext),
                InputKey::Tab | InputKey::BackTab | InputKey::Left | InputKey::Right => {
                    (target, ShellCommand::DiagnosticsRepairToggleAction)
                }
                InputKey::Enter | InputKey::Character(' ')
                    if self.diagnostics_repair_confirm_selected =>
                {
                    (target, ShellCommand::DiagnosticsConfirmRepair)
                }
                InputKey::Enter | InputKey::Character(' ') => {
                    (target, ShellCommand::DiagnosticsCancelRepair)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        let target = RoutedTarget::Component(ShellComponent::Diagnostics);
        if self.diagnostics_restart_is_required() {
            return match &key.key {
                InputKey::Enter => (RoutedTarget::Global, ShellCommand::RequestExit),
                InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseDiagnostics),
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseDiagnostics),
            InputKey::Tab | InputKey::Right => match self.diagnostics_tab {
                tundra_ui::DiagnosticsTab::Health => {
                    (target, ShellCommand::DiagnosticsIncidentsTab)
                }
                tundra_ui::DiagnosticsTab::Incidents => {
                    (target, ShellCommand::DiagnosticsHealthTab)
                }
            },
            InputKey::BackTab | InputKey::Left => match self.diagnostics_tab {
                tundra_ui::DiagnosticsTab::Health => {
                    (target, ShellCommand::DiagnosticsIncidentsTab)
                }
                tundra_ui::DiagnosticsTab::Incidents => {
                    (target, ShellCommand::DiagnosticsHealthTab)
                }
            },
            InputKey::Up => (target, ShellCommand::DiagnosticsPrevious),
            InputKey::Down => (target, ShellCommand::DiagnosticsNext),
            InputKey::PageUp => (target, ShellCommand::DiagnosticsPageUp),
            InputKey::PageDown => (target, ShellCommand::DiagnosticsPageDown),
            InputKey::Home => (target, ShellCommand::DiagnosticsFirst),
            InputKey::End => (target, ShellCommand::DiagnosticsLast),
            InputKey::Character('r' | 'R') => (target, ShellCommand::DiagnosticsRescan),
            InputKey::Character('f' | 'F') => {
                (target, ShellCommand::DiagnosticsPreviewSelectedRepair)
            }
            InputKey::Character('a' | 'A') => (target, ShellCommand::DiagnosticsPreviewAllRepairs),
            InputKey::Character('c' | 'C') => (target, ShellCommand::DiagnosticsCopySummary),
            InputKey::Character('o' | 'O') => (target, ShellCommand::DiagnosticsOpenReport),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    fn clock_button_activation_command(&self) -> ShellCommand {
        if self.active_screen() == ShellScreen::Clock {
            ShellCommand::CloseClock
        } else {
            ShellCommand::OpenClock
        }
    }

    fn route_notification_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target_component = self
            .notifications
            .active_modal_component()
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

        if let Some(index) = self.notifications.action_index_for_input(key) {
            return (target, ShellCommand::NotificationActivateAction(index));
        }

        match &key.key {
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
            InputKey::Enter | InputKey::Character(' ') => {
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

    fn route_time_sync_dialog_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Modal(ShellComponent::TimeSyncDialog);
        match &key.key {
            InputKey::Escape | InputKey::Enter | InputKey::Character(' ') => {
                (target, ShellCommand::CloseTimeSyncDialog)
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    fn route_setup_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target_component = self.setup_active_key_component();
        let target = RoutedTarget::Component(target_component);

        if matches!(&key.key, InputKey::Escape) {
            return (RoutedTarget::Global, ShellCommand::RequestExit);
        }

        match self.setup_step {
            tundra_ui::SetupStep::Language => match &key.key {
                InputKey::Up | InputKey::Left => (target, ShellCommand::SetupPreviousLanguage),
                InputKey::Down | InputKey::Right => (target, ShellCommand::SetupNextLanguage),
                InputKey::Enter | InputKey::Character(' ') => (target, ShellCommand::SetupContinue),
                _ => (target, ShellCommand::RecordInput),
            },
            tundra_ui::SetupStep::Timezone => match &key.key {
                InputKey::Up => (target, ShellCommand::SetupPreviousTimezone),
                InputKey::Down => (target, ShellCommand::SetupNextTimezone),
                InputKey::PageUp => (target, ShellCommand::SetupPageTimezoneUp),
                InputKey::PageDown => (target, ShellCommand::SetupPageTimezoneDown),
                InputKey::Home => (target, ShellCommand::SetupFirstTimezone),
                InputKey::End => (target, ShellCommand::SetupLastTimezone),
                InputKey::Enter => (target, ShellCommand::SetupContinue),
                _ => (target, ShellCommand::RecordInput),
            },
            tundra_ui::SetupStep::Admin => match &key.key {
                InputKey::BackTab => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab if key.modifiers.shift => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab => (target, ShellCommand::SetupFocusNext),
                InputKey::Up => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Down => (target, ShellCommand::SetupFocusNext),
                InputKey::Backspace if setup_admin_text_field(self.setup_focused_field) => {
                    (target, ShellCommand::SetupAdminBackspace)
                }
                InputKey::Enter if self.setup_focused_field == tundra_ui::SetupField::Submit => {
                    (target, ShellCommand::SubmitSetup)
                }
                InputKey::Enter => (target, ShellCommand::SetupFocusNext),
                InputKey::Character(character)
                    if setup_admin_text_field(self.setup_focused_field) =>
                {
                    (target, ShellCommand::AppendSetupAdminChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            },
        }
    }

    fn route_explorer_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Explorer);

        // Repeated action keys must not submit a dialog twice or open a file again while held.
        // Character/backspace repeats remain useful in name and search inputs.
        if key.phase != InputPhase::Press && matches!(key.key, InputKey::Enter | InputKey::Escape) {
            return (target, ShellCommand::CaptureOverlayInput);
        }

        if self
            .explorer_state
            .as_ref()
            .and_then(|state| state.pending_restore.as_ref())
            .is_some()
        {
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            return match &key.key {
                InputKey::Enter | InputKey::Character('k' | 'K') => {
                    (target, ShellCommand::ExplorerRestoreKeepBoth)
                }
                InputKey::Character('r' | 'R') => (target, ShellCommand::ExplorerRestoreReplace),
                InputKey::Escape | InputKey::Character('n' | 'N') => {
                    (target, ShellCommand::ExplorerRestoreCancel)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if self
            .explorer_state
            .as_ref()
            .and_then(|state| state.pending_conflict.as_ref())
            .is_some()
        {
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            return match &key.key {
                InputKey::Enter | InputKey::Character('k' | 'K') => {
                    (target, ShellCommand::ExplorerConflictKeepBoth)
                }
                InputKey::Character('r' | 'R') => (target, ShellCommand::ExplorerConflictReplace),
                InputKey::Character('s' | 'S') => (target, ShellCommand::ExplorerConflictSkip),
                InputKey::Character('a' | 'A') => {
                    (target, ShellCommand::ExplorerConflictToggleApplyToRemaining)
                }
                InputKey::Escape | InputKey::Character('n' | 'N') => {
                    (target, ShellCommand::ExplorerConflictCancel)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if self
            .explorer_state
            .as_ref()
            .and_then(|state| state.pending_dialog.as_ref())
            .is_some()
        {
            let confirm = self
                .explorer_state
                .as_ref()
                .and_then(|state| state.pending_dialog.as_ref())
                .map(|dialog| match dialog.kind {
                    tundra_apps::explorer::ExplorerDialogKind::DeleteToTrash => {
                        ShellCommand::ExplorerConfirmDelete
                    }
                    tundra_apps::explorer::ExplorerDialogKind::DumpTrash => {
                        ShellCommand::ExplorerConfirmDumpTrash
                    }
                })
                .unwrap_or(ShellCommand::ExplorerConfirmDelete);
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            return match &key.key {
                InputKey::Enter if key.is_unmodified_action_key() => (target, confirm.clone()),
                InputKey::Escape if key.is_unmodified_action_key() => {
                    (target, ShellCommand::CancelExplorerInput)
                }
                InputKey::Character('y' | 'Y') => (target, confirm),
                InputKey::Character('n' | 'N') => (target, ShellCommand::CancelExplorerInput),
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if self.explorer_input_mode != ExplorerInputMode::Browse {
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::CancelExplorerInput),
                InputKey::Enter => (target, ShellCommand::SubmitExplorerInput),
                InputKey::Backspace | InputKey::Delete => (target, ShellCommand::ExplorerBackspace),
                InputKey::Character(character) if !key.has_non_shift_modifier() => {
                    (target, ShellCommand::AppendExplorerChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            };
        }

        let is_trash = self
            .explorer_state
            .as_ref()
            .is_some_and(|state| state.current_location.is_trash());
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseExplorer),
            InputKey::Character('l' | 'L') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::BeginExplorerAddress)
            }
            InputKey::Left if key.modifiers.alt => (target, ShellCommand::ExplorerOpenBack),
            InputKey::Right if key.modifiers.alt => (target, ShellCommand::ExplorerOpenForward),
            InputKey::Up if key.modifiers.shift => (target, ShellCommand::ExplorerPreviousExtend),
            InputKey::Down if key.modifiers.shift => (target, ShellCommand::ExplorerNextExtend),
            InputKey::Up => (target, ShellCommand::ExplorerPrevious),
            InputKey::Down => (target, ShellCommand::ExplorerNext),
            InputKey::Enter if !is_trash => (target, ShellCommand::ExplorerOpenSelected),
            InputKey::Backspace => (target, ShellCommand::ExplorerOpenParent),
            InputKey::Delete if !is_trash => (target, ShellCommand::ExplorerDelete),
            InputKey::Function(2) if !is_trash => (target, ShellCommand::BeginExplorerRename),
            InputKey::Character('a' | 'A') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::ExplorerSelectAll)
            }
            InputKey::Character(' ') => (target, ShellCommand::ExplorerToggleFocused),
            InputKey::Character('f' | 'F') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::BeginExplorerSearch)
            }
            InputKey::Character('h' | 'H') => (target, ShellCommand::ExplorerToggleHidden),
            InputKey::Character('r' | 'R') if is_trash => (target, ShellCommand::ExplorerRestore),
            InputKey::Character('c' | 'C') if !is_trash => (target, ShellCommand::ExplorerCopy),
            InputKey::Character('x' | 'X') if !is_trash => (target, ShellCommand::ExplorerCut),
            InputKey::Character('v' | 'V') if !is_trash => (target, ShellCommand::ExplorerPaste),
            InputKey::Character('d' | 'D') if !is_trash => (target, ShellCommand::ExplorerDelete),
            InputKey::Character('n' | 'N' | 'f' | 'F') if !is_trash => {
                (target, ShellCommand::BeginExplorerNewFolder)
            }
            InputKey::Character('t' | 'T') if !is_trash => {
                (target, ShellCommand::BeginExplorerNewTextFile)
            }
            InputKey::Character('r' | 'R') if !is_trash => {
                (target, ShellCommand::BeginExplorerRename)
            }
            InputKey::Character('/') => (target, ShellCommand::BeginExplorerSearch),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    fn route_user_management_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::UserManagement);
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(
            tundra_ui::compute_shell_layout(area),
            tundra_ui::ShellLayout::Compact(_)
        ) {
            return match &key.key {
                InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseUserManagement),
                _ => (
                    RoutedTarget::Component(ShellComponent::CompactHome),
                    ShellCommand::CaptureOverlayInput,
                ),
            };
        }

        if self.user_management_mode != UserManagementMode::Browse {
            let field = self.user_management_form_field();
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::CancelUserManagementForm),
                InputKey::BackTab => (target, ShellCommand::UserManagementFocusPrevious),
                InputKey::Tab if key.modifiers.shift => {
                    (target, ShellCommand::UserManagementFocusPrevious)
                }
                InputKey::Tab | InputKey::Down => (target, ShellCommand::UserManagementFocusNext),
                InputKey::Up => (target, ShellCommand::UserManagementFocusPrevious),
                InputKey::Left | InputKey::Right
                    if field == Some(UserManagementFormField::Role) =>
                {
                    (target, ShellCommand::UserManagementToggleFormRole)
                }
                InputKey::Enter | InputKey::Character(' ')
                    if field == Some(UserManagementFormField::Role) =>
                {
                    (target, ShellCommand::UserManagementToggleFormRole)
                }
                InputKey::Enter | InputKey::Character(' ')
                    if field == Some(UserManagementFormField::Cancel) =>
                {
                    (target, ShellCommand::CancelUserManagementForm)
                }
                InputKey::Enter
                    if field == Some(UserManagementFormField::Submit)
                        || matches!(
                            field,
                            Some(
                                UserManagementFormField::Username
                                    | UserManagementFormField::DisplayName
                                    | UserManagementFormField::Password
                            )
                        ) =>
                {
                    (target, ShellCommand::SubmitUserManagementForm)
                }
                InputKey::Character(' ') if field == Some(UserManagementFormField::Submit) => {
                    (target, ShellCommand::SubmitUserManagementForm)
                }
                InputKey::Backspace => (target, ShellCommand::UserManagementBackspace),
                InputKey::Character(character)
                    if matches!(character, 'c' | 'C')
                        && field == Some(UserManagementFormField::Role) =>
                {
                    (target, ShellCommand::UserManagementToggleFormRole)
                }
                InputKey::Character(character) => {
                    (target, ShellCommand::AppendUserManagementChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            };
        }

        use tundra_ui::UserManagementAction;
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseUserManagement),
            InputKey::BackTab => (target, ShellCommand::UserManagementFocusPrevious),
            InputKey::Tab if key.modifiers.shift => {
                (target, ShellCommand::UserManagementFocusPrevious)
            }
            InputKey::Tab => (target, ShellCommand::UserManagementFocusNext),
            InputKey::Up => (target, ShellCommand::UserManagementPrevious),
            InputKey::Down => (target, ShellCommand::UserManagementNext),
            InputKey::PageUp => (target, ShellCommand::UserManagementPageUp),
            InputKey::PageDown => (target, ShellCommand::UserManagementPageDown),
            InputKey::Home => (target, ShellCommand::UserManagementFirst),
            InputKey::End => (target, ShellCommand::UserManagementLast),
            InputKey::Enter | InputKey::Character(' ') => {
                (target, ShellCommand::UserManagementActivateFocused)
            }
            InputKey::Character('n') | InputKey::Character('N') if self.can_manage_all_users() => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::NewUser),
            ),
            InputKey::Character('e') | InputKey::Character('E') => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::EditInfo),
            ),
            InputKey::Character('d') | InputKey::Character('D')
                if self
                    .user_management_users
                    .get(self.user_management_selected)
                    .is_some_and(|user| user.enabled && !user_is_locked(user)) =>
            {
                (
                    target,
                    ShellCommand::UserManagementActivateAction(UserManagementAction::ToggleEnabled),
                )
            }
            InputKey::Character('u') | InputKey::Character('U')
                if self
                    .user_management_users
                    .get(self.user_management_selected)
                    .is_some_and(|user| !user.enabled || user_is_locked(user)) =>
            {
                (
                    target,
                    ShellCommand::UserManagementActivateAction(UserManagementAction::ToggleEnabled),
                )
            }
            InputKey::Character('r') | InputKey::Character('R') => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::SetPassword),
            ),
            InputKey::Character('c') | InputKey::Character('C') if self.can_manage_all_users() => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::ToggleRole),
            ),
            InputKey::Character('x') | InputKey::Character('X') | InputKey::Delete => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::Delete),
            ),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    fn route_exit_confirm_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Modal(ShellComponent::ExitDialog);

        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab) {
            return (target, ShellCommand::FocusNext);
        }

        if key.is_character('y') || key.is_character('Y') || matches!(&key.key, InputKey::Enter) {
            return (target, ShellCommand::ConfirmExit);
        }

        if key.is_character('n') || key.is_character('N') || matches!(&key.key, InputKey::Escape) {
            return (target, ShellCommand::CancelExit);
        }

        (target, ShellCommand::CaptureOverlayInput)
    }

    fn route_popup_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Popup(ShellComponent::ContextMenu);

        if self.explorer_overlay_mode.is_some() {
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::ClosePopup),
                InputKey::Up | InputKey::BackTab => (target, ShellCommand::ExplorerOverlayPrevious),
                InputKey::Down | InputKey::Tab => (target, ShellCommand::ExplorerOverlayNext),
                InputKey::Enter | InputKey::Character(' ') => {
                    (target, ShellCommand::ExplorerOverlayActivate)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if matches!(&key.key, InputKey::Escape) {
            return (target, ShellCommand::ClosePopup);
        }
        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab) {
            return (target, ShellCommand::FocusNext);
        }

        (target, ShellCommand::CaptureOverlayInput)
    }

    fn route_mouse_input(
        &mut self,
        mouse: MouseInput,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let hit_target = self.hit_map.target_at(coordinates);
        let hit_layer = self.hit_map.layer_at(coordinates);

        if hit_layer == Some(ShellHitLayer::ShellModal) {
            if self.notifications.has_active_modal() {
                return self.route_notification_mouse(mouse, hit_target);
            }

            if self.time_sync_dialog_visible {
                return self.route_time_sync_dialog_mouse(mouse, hit_target);
            }

            if self.active_screen() == ShellScreen::ExitConfirm {
                return (
                    RoutedTarget::Modal(ShellComponent::ExitDialog),
                    ShellCommand::CaptureOverlayInput,
                );
            }
        }

        if self.active_screen() == ShellScreen::Diagnostics
            && !self.diagnostics_repair_preview.is_empty()
            && !self.notifications.has_active_modal()
            && !self.time_sync_dialog_visible
        {
            return self.route_diagnostics_mouse(mouse, hit_target);
        }

        if hit_layer == Some(ShellHitLayer::ShellChrome) {
            return self.route_shell_chrome_mouse(mouse, hit_target);
        }

        if self.notifications.has_active_modal() {
            return self.route_notification_mouse(mouse, hit_target);
        }

        if self.time_sync_dialog_visible {
            return self.route_time_sync_dialog_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::ExitConfirm {
            return (
                RoutedTarget::Modal(ShellComponent::ExitDialog),
                ShellCommand::CaptureOverlayInput,
            );
        }

        if self.active_screen() == ShellScreen::FirstRunSetup {
            return self.route_setup_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::Login {
            return self.route_login_mouse(mouse, hit_target);
        }

        if self.active_popup.is_some() {
            return self.route_popup_mouse(mouse, hit_target, received_at);
        }

        if self.active_screen() == ShellScreen::Clock {
            return self.route_clock_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::Diagnostics {
            return self.route_diagnostics_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::UserManagement {
            return self.route_user_management_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::Explorer {
            return self.route_explorer_mouse(mouse, hit_target, received_at);
        }

        match mouse {
            MouseInput::Moved { .. } => (target_route(hit_target), ShellCommand::Hover(hit_target)),
            MouseInput::Down {
                button: PointerButton::Right,
                ..
            } => {
                self.last_click = None;
                (
                    target_route(hit_target),
                    ShellCommand::OpenContextMenu {
                        target: hit_target,
                        coordinates,
                    },
                )
            }
            MouseInput::Down { button, .. } => {
                if let Some(target) = hit_target {
                    let click = self.register_click(hit_target, coordinates, button, received_at);
                    if target == ShellComponent::HomeLogout && button == PointerButton::Left {
                        return (RoutedTarget::Component(target), ShellCommand::Logout);
                    }
                    if target == ShellComponent::ClockButton {
                        return (
                            RoutedTarget::Component(target),
                            if button == PointerButton::Left {
                                self.clock_button_activation_command()
                            } else {
                                ShellCommand::Activate {
                                    target,
                                    coordinates,
                                    click,
                                }
                            },
                        );
                    }
                    if self.active_screen() == ShellScreen::Home && target == ShellComponent::Home {
                        return (
                            RoutedTarget::Component(target),
                            ShellCommand::ActivateHomeEntryAt(coordinates, click),
                        );
                    }

                    (
                        RoutedTarget::Component(target),
                        ShellCommand::Activate {
                            target,
                            coordinates,
                            click,
                        },
                    )
                } else {
                    (RoutedTarget::None, ShellCommand::RecordInput)
                }
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }

    fn route_shell_chrome_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target = target_route(hit_target);

        if self.active_screen() == ShellScreen::Explorer
            && matches!(
                mouse,
                MouseInput::Down { .. } | MouseInput::Up { .. } | MouseInput::Drag { .. }
            )
        {
            self.clear_explorer_pointer_capture();
        }

        match mouse {
            MouseInput::Moved { .. } => (target, ShellCommand::Hover(hit_target)),
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } if hit_target == Some(ShellComponent::ClockButton) => {
                self.last_click = None;
                (target, self.clock_button_activation_command())
            }
            MouseInput::Down { .. }
            | MouseInput::Up { .. }
            | MouseInput::Drag { .. }
            | MouseInput::Scroll { .. } => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    fn clear_explorer_pointer_capture(&mut self) {
        if let Some(state) = self.explorer_state.as_mut() {
            state.drag = None;
        }
        self.last_click = None;
    }

    fn route_explorer_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let target = RoutedTarget::Component(ShellComponent::Explorer);

        if hit_target != Some(ShellComponent::Explorer) {
            if matches!(
                mouse,
                MouseInput::Down { .. } | MouseInput::Up { .. } | MouseInput::Drag { .. }
            ) {
                self.clear_explorer_pointer_capture();
            }
            return match mouse {
                MouseInput::Moved { .. } => {
                    (target_route(hit_target), ShellCommand::Hover(hit_target))
                }
                MouseInput::Down { .. }
                | MouseInput::Up { .. }
                | MouseInput::Drag { .. }
                | MouseInput::Scroll { .. } => {
                    (target_route(hit_target), ShellCommand::CaptureOverlayInput)
                }
            };
        }

        match mouse {
            MouseInput::Moved { .. } => (target_route(hit_target), ShellCommand::Hover(hit_target)),
            MouseInput::Down {
                button: PointerButton::Right,
                ..
            } => {
                self.last_click = None;
                (
                    target,
                    ShellCommand::OpenContextMenu {
                        target: Some(ShellComponent::Explorer),
                        coordinates,
                    },
                )
            }
            MouseInput::Down {
                button: PointerButton::Left,
                modifiers,
                ..
            } => {
                let click = self.register_click(
                    Some(ShellComponent::Explorer),
                    coordinates,
                    PointerButton::Left,
                    received_at,
                );
                (
                    target,
                    ShellCommand::ExplorerPointerDown(coordinates, click, modifiers),
                )
            }
            MouseInput::Drag {
                button: PointerButton::Left,
                modifiers,
                ..
            } => (
                target,
                ShellCommand::ExplorerDragUpdate(coordinates, modifiers),
            ),
            MouseInput::Up {
                button: PointerButton::Left,
                modifiers,
                ..
            } => (target, ShellCommand::ExplorerDrop(coordinates, modifiers)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } => (target, ShellCommand::ExplorerScroll(-3)),
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } => (target, ShellCommand::ExplorerScroll(3)),
            MouseInput::Down { .. }
            | MouseInput::Up { .. }
            | MouseInput::Drag { .. }
            | MouseInput::Scroll { .. } => (target, ShellCommand::RecordInput),
        }
    }

    fn route_clock_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let modal_target = RoutedTarget::Modal(ShellComponent::ClockCreateDialog);

        if self.clock_create_state.is_some() {
            return match mouse {
                MouseInput::Moved { .. } => (modal_target, ShellCommand::Hover(hit_target)),
                MouseInput::Down {
                    button: PointerButton::Left,
                    ..
                } => match hit_target {
                    Some(ShellComponent::ClockCreateInput) => (
                        modal_target,
                        ShellCommand::ClockCreateSetFocus(tundra_ui::ClockCreateDialogFocus::Input),
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
        match mouse {
            MouseInput::Moved { .. } => (target, ShellCommand::Hover(hit_target)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if hit_target == Some(ShellComponent::ClockEntryList) => {
                (target, ShellCommand::ClockSelectPrevious)
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if hit_target == Some(ShellComponent::ClockEntryList) => {
                (target, ShellCommand::ClockSelectNext)
            }
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => match hit_target {
                Some(ShellComponent::ClockButton) => (target, ShellCommand::CloseClock),
                Some(ShellComponent::ClockNewButton) => (target, ShellCommand::ClockOpenCreate),
                Some(ShellComponent::ClockEntryList) => self
                    .clock_entry_id_at(coordinates)
                    .map(|id| (target, ShellCommand::ClockManageEntry(id)))
                    .unwrap_or((target, ShellCommand::RecordInput)),
                _ => (target, ShellCommand::RecordInput),
            },
            MouseInput::Down {
                button: PointerButton::Right,
                ..
            } => (target, ShellCommand::CaptureOverlayInput),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    fn route_diagnostics_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return (
                target_route(hit_target),
                if matches!(mouse, MouseInput::Moved { .. }) {
                    ShellCommand::Hover(hit_target)
                } else {
                    ShellCommand::CaptureOverlayInput
                },
            );
        };
        let model = self.to_diagnostics_view_model();
        let layout = tundra_ui::diagnostics_layout(main, &model);
        let diagnostic_target =
            tundra_ui::diagnostics_hit_test(&layout, (coordinates.0, coordinates.1));
        let routed = if self.diagnostics_repair_preview.is_empty() {
            RoutedTarget::Component(ShellComponent::Diagnostics)
        } else {
            RoutedTarget::Modal(ShellComponent::DiagnosticsRepairDialog)
        };

        match mouse {
            MouseInput::Moved { .. } => (routed, ShellCommand::Hover(hit_target)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if self.diagnostics_repair_preview.is_empty() => {
                (routed, ShellCommand::DiagnosticsPrevious)
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if self.diagnostics_repair_preview.is_empty() => {
                (routed, ShellCommand::DiagnosticsNext)
            }
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => match diagnostic_target {
                Some(tundra_ui::DiagnosticsHitTarget::Tab(tundra_ui::DiagnosticsTab::Health)) => {
                    (routed, ShellCommand::DiagnosticsHealthTab)
                }
                Some(tundra_ui::DiagnosticsHitTarget::Tab(
                    tundra_ui::DiagnosticsTab::Incidents,
                )) => (routed, ShellCommand::DiagnosticsIncidentsTab),
                Some(tundra_ui::DiagnosticsHitTarget::Check(index))
                | Some(tundra_ui::DiagnosticsHitTarget::Incident(index)) => {
                    (routed, ShellCommand::DiagnosticsSelectIndex(index))
                }
                Some(tundra_ui::DiagnosticsHitTarget::RepairConfirm) => {
                    (routed, ShellCommand::DiagnosticsConfirmRepair)
                }
                Some(tundra_ui::DiagnosticsHitTarget::RepairCancel) => {
                    (routed, ShellCommand::DiagnosticsCancelRepair)
                }
                Some(tundra_ui::DiagnosticsHitTarget::RepairItem(index)) => {
                    (routed, ShellCommand::DiagnosticsSelectRepairItem(index))
                }
                _ => (routed, ShellCommand::CaptureOverlayInput),
            },
            _ => (routed, ShellCommand::CaptureOverlayInput),
        }
    }

    fn route_user_management_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::UserManagement);
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(
            tundra_ui::compute_shell_layout(area),
            tundra_ui::ShellLayout::Compact(_)
        ) {
            return (
                RoutedTarget::Component(ShellComponent::CompactHome),
                ShellCommand::CaptureOverlayInput,
            );
        }

        if self.user_management_mode == UserManagementMode::Browse
            && hit_target == Some(ShellComponent::ClockButton)
            && matches!(
                mouse,
                MouseInput::Down {
                    button: PointerButton::Left,
                    ..
                }
            )
        {
            return (
                RoutedTarget::Component(ShellComponent::ClockButton),
                self.clock_button_activation_command(),
            );
        }

        let Some(layout) = self.user_management_layout() else {
            return (target, ShellCommand::CaptureOverlayInput);
        };
        let coordinates = mouse.coordinates();

        if self.user_management_mode != UserManagementMode::Browse {
            return match mouse {
                MouseInput::Moved { .. } => (target, ShellCommand::Hover(hit_target)),
                MouseInput::Down {
                    button: PointerButton::Left,
                    ..
                } => layout
                    .form_control_at(coordinates.0, coordinates.1)
                    .map(|field| {
                        let command = match field {
                            tundra_ui::UserManagementField::Role
                            | tundra_ui::UserManagementField::Submit
                            | tundra_ui::UserManagementField::Cancel => {
                                ShellCommand::UserManagementActivateFormControl(field)
                            }
                            _ => ShellCommand::UserManagementSetFormFocus(field),
                        };
                        (target, command)
                    })
                    .unwrap_or((target, ShellCommand::CaptureOverlayInput)),
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        match mouse {
            MouseInput::Moved { .. } => (target, ShellCommand::Hover(hit_target)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if rect_contains(layout.rows_area, coordinates) => {
                (target, ShellCommand::UserManagementPrevious)
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if rect_contains(layout.rows_area, coordinates) => {
                (target, ShellCommand::UserManagementNext)
            }
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => {
                if let Some(index) = layout.row_index_at(coordinates.0, coordinates.1) {
                    return (target, ShellCommand::UserManagementSelectRow(index));
                }
                if let Some(action) = layout.action_at(coordinates.0, coordinates.1) {
                    return (target, ShellCommand::UserManagementActivateAction(action));
                }
                (target, ShellCommand::RecordInput)
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    fn clock_entry_id_at(&self, coordinates: CellPosition) -> Option<u64> {
        let (width, height) = self.terminal_size;
        let area = Rect::new(0, 0, width, height);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return None;
        };
        let snapshot = self.network_clock.snapshot();
        let model = self.to_clock_view_model_at(&snapshot, Instant::now());
        tundra_ui::clock_page_layout(main, &model)
            .entry_rows
            .into_iter()
            .find(|row| rect_contains(row.area, coordinates))
            .map(|row| row.id)
    }

    fn route_time_sync_dialog_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        match mouse {
            MouseInput::Moved { .. } => (
                RoutedTarget::Modal(ShellComponent::TimeSyncDialog),
                ShellCommand::Hover(hit_target),
            ),
            _ if mouse.down_button().is_some() => (
                RoutedTarget::Modal(ShellComponent::TimeSyncDialog),
                ShellCommand::CloseTimeSyncDialog,
            ),
            _ => (
                RoutedTarget::Modal(ShellComponent::TimeSyncDialog),
                ShellCommand::CaptureOverlayInput,
            ),
        }
    }

    fn route_notification_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target_component = self
            .notifications
            .active_modal_component()
            .unwrap_or(ShellComponent::NotificationDialog);
        let target = RoutedTarget::Modal(target_component);

        if !self.notification_can_render() {
            self.notification_pointer_capture = None;
            return (target, ShellCommand::CaptureOverlayInput);
        }

        match mouse {
            MouseInput::Moved { .. } => (target, ShellCommand::Hover(hit_target)),
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => {
                let action_index = self.notification_action_index_at(mouse.coordinates());
                self.notification_pointer_capture = action_index.and_then(|action_index| {
                    self.notifications.active_modal_id().map(|notification_id| {
                        NotificationPointerCapture {
                            notification_id,
                            action_index,
                        }
                    })
                });
                if let Some(action_index) = action_index {
                    self.notifications.select_action(action_index);
                }
                (target, ShellCommand::CaptureOverlayInput)
            }
            MouseInput::Up {
                button: PointerButton::Left,
                ..
            } => {
                let pressed = self.notification_pointer_capture.take();
                let released_index = self.notification_action_index_at(mouse.coordinates());
                let current_id = self.notifications.active_modal_id();
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
            MouseInput::Drag {
                button: PointerButton::Left,
                ..
            } => {
                self.notification_pointer_capture = None;
                (target, ShellCommand::CaptureOverlayInput)
            }
            MouseInput::Down { .. }
            | MouseInput::Up { .. }
            | MouseInput::Drag { .. }
            | MouseInput::Scroll { .. } => {
                self.notification_pointer_capture = None;
                (target, ShellCommand::CaptureOverlayInput)
            }
        }
    }

    fn route_setup_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        match mouse {
            MouseInput::Moved { .. } => (target_route(hit_target), ShellCommand::Hover(hit_target)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if hit_target == Some(ShellComponent::SetupLanguage)
                && self.setup_step == tundra_ui::SetupStep::Language =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupLanguage),
                    ShellCommand::SetupPreviousLanguage,
                )
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if hit_target == Some(ShellComponent::SetupLanguage)
                && self.setup_step == tundra_ui::SetupStep::Language =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupLanguage),
                    ShellCommand::SetupNextLanguage,
                )
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if hit_target == Some(ShellComponent::SetupTimezone)
                && self.setup_step == tundra_ui::SetupStep::Timezone =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupTimezone),
                    ShellCommand::SetupPreviousTimezone,
                )
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if hit_target == Some(ShellComponent::SetupTimezone)
                && self.setup_step == tundra_ui::SetupStep::Timezone =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupTimezone),
                    ShellCommand::SetupNextTimezone,
                )
            }
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => {
                if let Some(target) = hit_target
                    && setup_field_for_component(target).is_some()
                    && setup_component_active_for_step(target, self.setup_step)
                {
                    return (
                        RoutedTarget::Component(target),
                        ShellCommand::ActivateSetup {
                            target,
                            coordinates,
                        },
                    );
                }

                (RoutedTarget::None, ShellCommand::RecordInput)
            }
            MouseInput::Down {
                button: PointerButton::Right,
                ..
            } => {
                self.last_click = None;
                (target_route(hit_target), ShellCommand::CaptureOverlayInput)
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }

    fn route_login_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        match mouse {
            MouseInput::Moved { .. } => (target_route(hit_target), ShellCommand::Hover(hit_target)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if hit_target == Some(ShellComponent::LoginUserList) => (
                RoutedTarget::Component(ShellComponent::LoginUserList),
                ShellCommand::LoginPreviousUser,
            ),
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if hit_target == Some(ShellComponent::LoginUserList) => (
                RoutedTarget::Component(ShellComponent::LoginUserList),
                ShellCommand::LoginNextUser,
            ),
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => {
                if hit_target == Some(ShellComponent::LoginPasswordVisibility) {
                    return (
                        RoutedTarget::Component(ShellComponent::LoginPasswordVisibility),
                        ShellCommand::ToggleLoginPasswordVisibility,
                    );
                }
                if let Some(
                    target @ (ShellComponent::LoginUserList
                    | ShellComponent::LoginUsername
                    | ShellComponent::LoginPassword),
                ) = hit_target
                {
                    return (
                        RoutedTarget::Component(target),
                        ShellCommand::ActivateLogin {
                            target,
                            coordinates,
                        },
                    );
                }

                (RoutedTarget::None, ShellCommand::RecordInput)
            }
            MouseInput::Down {
                button: PointerButton::Right,
                ..
            } => {
                self.last_click = None;
                (target_route(hit_target), ShellCommand::CaptureOverlayInput)
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }

    fn route_popup_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        if hit_target != Some(ShellComponent::ContextMenu) {
            if mouse.down_button().is_some() {
                return (RoutedTarget::OutsidePopup, ShellCommand::ClosePopup);
            }

            return (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::CaptureOverlayInput,
            );
        }

        match mouse {
            MouseInput::Moved { .. } => (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::Hover(Some(ShellComponent::ContextMenu)),
            ),
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => {
                let click = self.register_click(
                    Some(ShellComponent::ContextMenu),
                    coordinates,
                    PointerButton::Left,
                    received_at,
                );
                (
                    RoutedTarget::Popup(ShellComponent::ContextMenu),
                    ShellCommand::Activate {
                        target: ShellComponent::ContextMenu,
                        coordinates,
                        click,
                    },
                )
            }
            _ => (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::CaptureOverlayInput,
            ),
        }
    }

    fn register_click(
        &mut self,
        target: Option<ShellComponent>,
        coordinates: CellPosition,
        button: PointerButton,
        received_at: Instant,
    ) -> ClickKind {
        if button != PointerButton::Left {
            self.last_click = None;
            return ClickKind::Single;
        }

        let is_double_click = self
            .last_click
            .map(|last_click| {
                last_click.target == target
                    && coordinates_within_tolerance(last_click.coordinates, coordinates)
                    && received_at
                        .checked_duration_since(last_click.at)
                        .map(|elapsed| elapsed <= DOUBLE_CLICK_INTERVAL)
                        .unwrap_or(false)
            })
            .unwrap_or(false);

        if is_double_click {
            self.last_click = None;
            ClickKind::Double
        } else {
            self.last_click = Some(TimedClick {
                target,
                coordinates,
                at: received_at,
            });
            ClickKind::Single
        }
    }
}
