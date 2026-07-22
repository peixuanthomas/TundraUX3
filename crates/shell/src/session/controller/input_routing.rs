use super::super::*;
impl ShellSession {
    pub(in crate::session) fn route_key_input(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        if !key.phase.is_press_like() {
            if self.active_screen() == ShellScreen::Editor
                && matches!(
                    key.key,
                    InputKey::Left | InputKey::Right | InputKey::Up | InputKey::Down
                )
            {
                return (
                    RoutedTarget::Component(ShellComponent::Editor),
                    ShellCommand::EditorKey(key.clone()),
                );
            }
            return (RoutedTarget::Global, ShellCommand::Noop);
        }

        if key.is_ctrl_c() && self.active_screen() == ShellScreen::Editor {
            return (
                RoutedTarget::Component(ShellComponent::Editor),
                ShellCommand::EditorKey(key.clone()),
            );
        }

        if key.is_ctrl_c() {
            return (RoutedTarget::Global, ShellCommand::Shutdown);
        }

        if self.notification_has_active_modal() {
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

        if self.active_screen() == ShellScreen::Launcher {
            return self.route_launcher_key(key);
        }

        if self.active_screen() == ShellScreen::Editor {
            return (
                RoutedTarget::Component(ShellComponent::Editor),
                ShellCommand::EditorKey(key.clone()),
            );
        }

        if self.active_screen() == ShellScreen::Settings {
            return (
                RoutedTarget::Component(ShellComponent::Settings),
                ShellCommand::SettingsKey(key.clone()),
            );
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
                    && matches!(&key.key, InputKey::Enter | InputKey::Char(' ')) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::HomeLogout),
                    ShellCommand::Logout,
                )
            }
            _ if self.focused_component == ShellComponent::ClockButton
                && matches!(&key.key, InputKey::Enter | InputKey::Char(' ')) =>
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
            ShellScreen::Home if matches!(&key.key, InputKey::Enter | InputKey::Char(' ')) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::ActivateSelectedHomeEntry,
            ),
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
                (RoutedTarget::Global, ShellCommand::LogoutToLockscreen)
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

    pub(in crate::session) fn route_login_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(ui::compute_shell_layout(area), ui::ShellLayout::Compact(_)) {
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
        if matches!(&key.key, InputKey::F(2)) {
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
                InputKey::Char(character) => (target, ShellCommand::AppendAuthChar(*character)),
                _ => (target, ShellCommand::RecordInput),
            },
            ShellComponent::LoginPasswordVisibility => match &key.key {
                InputKey::BackTab => (target, ShellCommand::LoginFocusPassword),
                InputKey::Tab if key.modifiers.shift => (target, ShellCommand::LoginFocusPassword),
                InputKey::Tab | InputKey::Right | InputKey::Down => {
                    (target, ShellCommand::LoginFocusUserList)
                }
                InputKey::Left | InputKey::Up => (target, ShellCommand::LoginFocusPassword),
                InputKey::Enter | InputKey::Char(' ') => {
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

    pub(in crate::session) fn route_auth_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
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
        if let InputKey::Char(character) = &key.key {
            return (target, ShellCommand::AppendAuthChar(*character));
        }

        (target, ShellCommand::RecordInput)
    }

    pub(in crate::session) fn route_clock_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(ui::compute_shell_layout(area), ui::ShellLayout::Compact(_)) {
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
                InputKey::Enter | InputKey::Char(' ') => (target, ShellCommand::CloseClock),
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

    pub(in crate::session) fn route_diagnostics_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(ui::compute_shell_layout(area), ui::ShellLayout::Compact(_)) {
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
                ui::DiagnosticsTab::Health => (target, ShellCommand::DiagnosticsLogsTab),
                ui::DiagnosticsTab::Logs => (target, ShellCommand::DiagnosticsIncidentsTab),
                ui::DiagnosticsTab::Incidents => (target, ShellCommand::DiagnosticsHealthTab),
            },
            InputKey::BackTab | InputKey::Left => match self.diagnostics_tab {
                ui::DiagnosticsTab::Health => (target, ShellCommand::DiagnosticsIncidentsTab),
                ui::DiagnosticsTab::Logs => (target, ShellCommand::DiagnosticsHealthTab),
                ui::DiagnosticsTab::Incidents => (target, ShellCommand::DiagnosticsLogsTab),
            },
            InputKey::Up => (target, ShellCommand::DiagnosticsPrevious),
            InputKey::Down => (target, ShellCommand::DiagnosticsNext),
            InputKey::PageUp => (target, ShellCommand::DiagnosticsPageUp),
            InputKey::PageDown => (target, ShellCommand::DiagnosticsPageDown),
            InputKey::Home => (target, ShellCommand::DiagnosticsFirst),
            InputKey::End => (target, ShellCommand::DiagnosticsLast),
            InputKey::Char('r' | 'R') => (target, ShellCommand::DiagnosticsRescan),
            InputKey::Char('f' | 'F') if self.diagnostics_tab == ui::DiagnosticsTab::Health => {
                (target, ShellCommand::DiagnosticsPreviewSelectedRepair)
            }
            InputKey::Char('a' | 'A') if self.diagnostics_tab == ui::DiagnosticsTab::Health => {
                (target, ShellCommand::DiagnosticsPreviewAllRepairs)
            }
            InputKey::Char('c' | 'C') => (target, ShellCommand::DiagnosticsCopySummary),
            InputKey::Char('e' | 'E') => (target, ShellCommand::DiagnosticsOpenLogsInExplorer),
            InputKey::Char('o' | 'O') => (target, ShellCommand::DiagnosticsOpenReport),
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

    pub(in crate::session) fn clock_button_activation_command(&self) -> ShellCommand {
        if self.active_screen() == ShellScreen::Clock {
            ShellCommand::CloseClock
        } else {
            ShellCommand::OpenClock
        }
    }

    pub(in crate::session) fn route_notification_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target_component = self
            .notification_active_modal_component()
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

        if let Some(index) = self.notification_action_index_for_input(key) {
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
            InputKey::Enter | InputKey::Char(' ') => {
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

    pub(in crate::session) fn route_setup_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target_component = self.setup_active_key_component();
        let target = RoutedTarget::Component(target_component);

        if self.setup_custom_color_target.is_some() {
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::CancelSetupCustomColor),
                InputKey::Enter => (target, ShellCommand::ApplySetupCustomColor),
                InputKey::Backspace => (target, ShellCommand::SetupCustomColorBackspace),
                InputKey::Char(character) => {
                    (target, ShellCommand::AppendSetupCustomColorChar(*character))
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if matches!(&key.key, InputKey::Escape) {
            return (RoutedTarget::Global, ShellCommand::RequestExit);
        }

        match self.setup_step {
            ui::SetupStep::Language => match &key.key {
                InputKey::Up | InputKey::Left => (target, ShellCommand::SetupPreviousLanguage),
                InputKey::Down | InputKey::Right => (target, ShellCommand::SetupNextLanguage),
                InputKey::Enter | InputKey::Char(' ') => (target, ShellCommand::SetupContinue),
                _ => (target, ShellCommand::RecordInput),
            },
            ui::SetupStep::Timezone => match &key.key {
                InputKey::Up => (target, ShellCommand::SetupPreviousTimezone),
                InputKey::Down => (target, ShellCommand::SetupNextTimezone),
                InputKey::PageUp => (target, ShellCommand::SetupPageTimezoneUp),
                InputKey::PageDown => (target, ShellCommand::SetupPageTimezoneDown),
                InputKey::Home => (target, ShellCommand::SetupFirstTimezone),
                InputKey::End => (target, ShellCommand::SetupLastTimezone),
                InputKey::Enter => (target, ShellCommand::SetupContinue),
                _ => (target, ShellCommand::RecordInput),
            },
            ui::SetupStep::Admin => match &key.key {
                InputKey::BackTab => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab if key.modifiers.shift => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab => (target, ShellCommand::SetupFocusNext),
                InputKey::Up => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Down => (target, ShellCommand::SetupFocusNext),
                InputKey::Backspace if setup_admin_text_field(self.setup_focused_field) => {
                    (target, ShellCommand::SetupAdminBackspace)
                }
                InputKey::Enter if self.setup_focused_field == ui::SetupField::Submit => {
                    (target, ShellCommand::SubmitSetup)
                }
                InputKey::Enter => (target, ShellCommand::SetupFocusNext),
                InputKey::Char(character) if setup_admin_text_field(self.setup_focused_field) => {
                    (target, ShellCommand::AppendSetupAdminChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            },
            ui::SetupStep::Appearance => match &key.key {
                InputKey::BackTab => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab if key.modifiers.shift => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab => (target, ShellCommand::SetupFocusNext),
                InputKey::Up => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Down => (target, ShellCommand::SetupFocusNext),
                InputKey::Left => (target, ShellCommand::SetupPreviousAppearanceChoice),
                InputKey::Right => (target, ShellCommand::SetupNextAppearanceChoice),
                InputKey::Enter | InputKey::Char(' ') => {
                    (target, ShellCommand::SubmitSetupAppearance)
                }
                _ => (target, ShellCommand::RecordInput),
            },
        }
    }

    pub(in crate::session) fn route_explorer_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Explorer);

        // Repeated action keys must not submit a dialog twice or open a file again while held.
        // Character/backspace repeats remain useful in name and search inputs.
        if key.phase != InputPhase::Press && matches!(key.key, InputKey::Enter | InputKey::Escape) {
            return (target, ShellCommand::CaptureOverlayInput);
        }

        if self
            .app
            .explorer_state()
            .and_then(|state| state.pending_restore.as_ref())
            .is_some()
        {
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            return match &key.key {
                InputKey::Enter | InputKey::Char('k' | 'K') => {
                    (target, ShellCommand::ExplorerRestoreKeepBoth)
                }
                InputKey::Char('r' | 'R') => (target, ShellCommand::ExplorerRestoreReplace),
                InputKey::Escape | InputKey::Char('n' | 'N') => {
                    (target, ShellCommand::ExplorerRestoreCancel)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if self
            .app
            .explorer_state()
            .and_then(|state| state.pending_conflict.as_ref())
            .is_some()
        {
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            return match &key.key {
                InputKey::Enter | InputKey::Char('k' | 'K') => {
                    (target, ShellCommand::ExplorerConflictKeepBoth)
                }
                InputKey::Char('r' | 'R') => (target, ShellCommand::ExplorerConflictReplace),
                InputKey::Char('s' | 'S') => (target, ShellCommand::ExplorerConflictSkip),
                InputKey::Char('a' | 'A') => {
                    (target, ShellCommand::ExplorerConflictToggleApplyToRemaining)
                }
                InputKey::Escape | InputKey::Char('n' | 'N') => {
                    (target, ShellCommand::ExplorerConflictCancel)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if self
            .app
            .explorer_state()
            .and_then(|state| state.pending_dialog.as_ref())
            .is_some()
        {
            let confirm = self
                .app
                .explorer_state()
                .and_then(|state| state.pending_dialog.as_ref())
                .map(|dialog| match dialog.kind {
                    app::explorer::ExplorerDialogKind::DeleteToTrash => {
                        ShellCommand::ExplorerConfirmDelete
                    }
                    app::explorer::ExplorerDialogKind::DumpTrash => {
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
                InputKey::Char('y' | 'Y') => (target, confirm),
                InputKey::Char('n' | 'N') => (target, ShellCommand::CancelExplorerInput),
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if self.explorer_input_mode != ExplorerInputMode::Browse {
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::CancelExplorerInput),
                InputKey::Enter => (target, ShellCommand::SubmitExplorerInput),
                InputKey::Backspace | InputKey::Delete => (target, ShellCommand::ExplorerBackspace),
                InputKey::Char(character) if !key.has_non_shift_modifier() => {
                    (target, ShellCommand::AppendExplorerChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            };
        }

        let is_trash = self
            .app
            .explorer_state()
            .is_some_and(|state| state.current_location.is_trash());
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseExplorer),
            InputKey::Char('l' | 'L') if key.modifiers.control || key.modifiers.super_key => {
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
            InputKey::F(2) if !is_trash => (target, ShellCommand::BeginExplorerRename),
            InputKey::Char('a' | 'A') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::ExplorerSelectAll)
            }
            InputKey::Char('a' | 'A') if !is_trash && self.can_manage_launcher() => {
                (target, ShellCommand::ExplorerAddToLauncher)
            }
            InputKey::Char(' ') => (target, ShellCommand::ExplorerToggleFocused),
            InputKey::Char('f' | 'F') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::BeginExplorerSearch)
            }
            InputKey::Char('h' | 'H') => (target, ShellCommand::ExplorerToggleHidden),
            InputKey::Char('r' | 'R') if is_trash => (target, ShellCommand::ExplorerRestore),
            InputKey::Char('c' | 'C') if !is_trash => (target, ShellCommand::ExplorerCopy),
            InputKey::Char('x' | 'X') if !is_trash => (target, ShellCommand::ExplorerCut),
            InputKey::Char('v' | 'V') if !is_trash => (target, ShellCommand::ExplorerPaste),
            InputKey::Char('d' | 'D') if !is_trash => (target, ShellCommand::ExplorerDelete),
            InputKey::Char('n' | 'N' | 'f' | 'F') if !is_trash => {
                (target, ShellCommand::BeginExplorerNewFolder)
            }
            InputKey::Char('t' | 'T') if !is_trash => {
                (target, ShellCommand::BeginExplorerNewTextFile)
            }
            InputKey::Char('r' | 'R') if !is_trash => (target, ShellCommand::BeginExplorerRename),
            InputKey::Char('/') => (target, ShellCommand::BeginExplorerSearch),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_launcher_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Launcher);
        if self.launcher_pending_confirmation.is_some() {
            return match key.key {
                InputKey::Enter | InputKey::Char('y' | 'Y') => {
                    (target, ShellCommand::LauncherConfirm)
                }
                InputKey::Escape | InputKey::Char('n' | 'N') => {
                    (target, ShellCommand::LauncherCancelConfirmation)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }
        if self.launcher_drag.is_some() && matches!(key.key, InputKey::Escape) {
            return (target, ShellCommand::LauncherCancelDrag);
        }
        match key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseLauncher),
            InputKey::Left | InputKey::Up => (target, ShellCommand::LauncherPrevious),
            InputKey::Right | InputKey::Down => (target, ShellCommand::LauncherNext),
            InputKey::PageUp => (target, ShellCommand::LauncherPageUp),
            InputKey::PageDown => (target, ShellCommand::LauncherPageDown),
            InputKey::Home => (target, ShellCommand::LauncherFirst),
            InputKey::End => (target, ShellCommand::LauncherLast),
            InputKey::Enter => (target, ShellCommand::LauncherActivate),
            InputKey::Delete => (target, ShellCommand::LauncherRemove),
            InputKey::Char('v' | 'V') => (target, ShellCommand::LauncherToggleView),
            InputKey::Char('r' | 'R') if key.modifiers.control || key.modifiers.super_key => {
                (target, ShellCommand::LauncherReapprove)
            }
            InputKey::Char('r' | 'R') => (target, ShellCommand::LauncherRefresh),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_user_management_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::UserManagement);
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(ui::compute_shell_layout(area), ui::ShellLayout::Compact(_)) {
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
                InputKey::Enter | InputKey::Char(' ')
                    if field == Some(UserManagementFormField::Role) =>
                {
                    (target, ShellCommand::UserManagementToggleFormRole)
                }
                InputKey::Enter | InputKey::Char(' ')
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
                InputKey::Char(' ') if field == Some(UserManagementFormField::Submit) => {
                    (target, ShellCommand::SubmitUserManagementForm)
                }
                InputKey::Backspace => (target, ShellCommand::UserManagementBackspace),
                InputKey::Char(character)
                    if matches!(character, 'c' | 'C')
                        && field == Some(UserManagementFormField::Role) =>
                {
                    (target, ShellCommand::UserManagementToggleFormRole)
                }
                InputKey::Char(character) => {
                    (target, ShellCommand::AppendUserManagementChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            };
        }

        use ui::UserManagementAction;
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
            InputKey::Enter | InputKey::Char(' ') => {
                (target, ShellCommand::UserManagementActivateFocused)
            }
            InputKey::Char('n') | InputKey::Char('N') if self.can_manage_all_users() => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::NewUser),
            ),
            InputKey::Char('e') | InputKey::Char('E') => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::EditInfo),
            ),
            InputKey::Char('d') | InputKey::Char('D')
                if self
                    .app
                    .managed_users()
                    .get(self.user_management_selected)
                    .is_some_and(|user| user.enabled && !user_is_locked(user)) =>
            {
                (
                    target,
                    ShellCommand::UserManagementActivateAction(UserManagementAction::ToggleEnabled),
                )
            }
            InputKey::Char('u') | InputKey::Char('U')
                if self
                    .app
                    .managed_users()
                    .get(self.user_management_selected)
                    .is_some_and(|user| !user.enabled || user_is_locked(user)) =>
            {
                (
                    target,
                    ShellCommand::UserManagementActivateAction(UserManagementAction::ToggleEnabled),
                )
            }
            InputKey::Char('r') | InputKey::Char('R') => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::SetPassword),
            ),
            InputKey::Char('c') | InputKey::Char('C') if self.can_manage_all_users() => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::ToggleRole),
            ),
            InputKey::Char('x') | InputKey::Char('X') | InputKey::Delete => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::Delete),
            ),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_exit_confirm_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
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

    pub(in crate::session) fn route_popup_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Popup(ShellComponent::ContextMenu);

        if self.explorer_overlay_mode.is_some() {
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            let can_add_to_launcher = matches!(
                self.to_explorer_view_model().overlay.as_ref(),
                Some(ui::ExplorerOverlayViewModel::ContextMenu(menu))
                    if menu.items.iter().any(|item| {
                        item.id == "add-to-launcher" && item.enabled
                    })
            );
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::ClosePopup),
                InputKey::Up | InputKey::BackTab => (target, ShellCommand::ExplorerOverlayPrevious),
                InputKey::Down | InputKey::Tab => (target, ShellCommand::ExplorerOverlayNext),
                InputKey::Enter | InputKey::Char(' ') => {
                    (target, ShellCommand::ExplorerOverlayActivate)
                }
                InputKey::Char('a' | 'A') if can_add_to_launcher => {
                    (target, ShellCommand::ExplorerAddToLauncher)
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

    pub(in crate::session) fn route_mouse_input(
        &mut self,
        mouse: MouseInput,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let hit_target = self.hit_map.target_at(coordinates);
        let hit_layer = self.hit_map.layer_at(coordinates);

        if hit_layer == Some(ShellHitLayer::ShellModal) {
            if self.notification_has_active_modal() {
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
            && !self.notification_has_active_modal()
            && !self.time_sync_dialog_visible
        {
            return self.route_diagnostics_mouse(mouse, hit_target);
        }

        if !self.notification_has_active_modal()
            && !self.time_sync_dialog_visible
            && self.active_popup.is_none()
        {
            let coordinates = mouse.coordinates();
            if self.launcher_drag.is_some() {
                match mouse.kind {
                    ui::MouseEventKind::Drag(PointerButton::Left) => {
                        return (
                            RoutedTarget::Component(ShellComponent::Launcher),
                            ShellCommand::LauncherDragUpdate(coordinates),
                        );
                    }
                    ui::MouseEventKind::Up(PointerButton::Left) => {
                        return (
                            RoutedTarget::Component(ShellComponent::Launcher),
                            ShellCommand::LauncherDrop(coordinates),
                        );
                    }
                    _ => {}
                }
            }
            match (self.scrollbar_drag, mouse.kind) {
                (
                    Some(ScrollbarDragState::Explorer { .. }),
                    ui::MouseEventKind::Drag(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::Explorer),
                        ShellCommand::ExplorerDragUpdate(coordinates, mouse.modifiers),
                    );
                }
                (
                    Some(ScrollbarDragState::Explorer { .. }),
                    ui::MouseEventKind::Up(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::Explorer),
                        ShellCommand::ExplorerDrop(coordinates, mouse.modifiers),
                    );
                }
                (
                    Some(ScrollbarDragState::Diagnostics { .. }),
                    ui::MouseEventKind::Drag(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::Diagnostics),
                        ShellCommand::DiagnosticsScrollbarDrag(coordinates),
                    );
                }
                (
                    Some(ScrollbarDragState::Diagnostics { .. }),
                    ui::MouseEventKind::Up(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::Diagnostics),
                        ShellCommand::DiagnosticsScrollbarPointerUp,
                    );
                }
                (
                    Some(ScrollbarDragState::Editor { .. }),
                    ui::MouseEventKind::Drag(PointerButton::Left)
                    | ui::MouseEventKind::Up(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::Editor),
                        ShellCommand::EditorPointer(mouse),
                    );
                }
                _ => {}
            }
        }

        if hit_layer == Some(ShellHitLayer::ShellChrome) {
            return self.route_shell_chrome_mouse(mouse, hit_target);
        }

        if self.notification_has_active_modal() {
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

        if self.active_screen() == ShellScreen::Launcher {
            return self.route_launcher_mouse(mouse, received_at);
        }

        if self.active_screen() == ShellScreen::Editor {
            return (
                RoutedTarget::Component(ShellComponent::Editor),
                ShellCommand::EditorPointer(mouse),
            );
        }

        if self.active_screen() == ShellScreen::Settings {
            return (
                RoutedTarget::Component(ShellComponent::Settings),
                ShellCommand::SettingsPointer(mouse),
            );
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => {
                (target_route(hit_target), ShellCommand::Hover(hit_target))
            }
            ui::MouseEventKind::Down(PointerButton::Right) => {
                self.last_click = None;
                (
                    target_route(hit_target),
                    ShellCommand::OpenContextMenu {
                        target: hit_target,
                        coordinates,
                    },
                )
            }
            ui::MouseEventKind::Down(button) => {
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

    pub(in crate::session) fn route_shell_chrome_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target = target_route(hit_target);

        if self.active_screen() == ShellScreen::Explorer
            && matches!(
                mouse.kind,
                ui::MouseEventKind::Down(_)
                    | ui::MouseEventKind::Up(_)
                    | ui::MouseEventKind::Drag(_)
            )
        {
            self.clear_explorer_pointer_capture();
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => (target, ShellCommand::Hover(hit_target)),
            ui::MouseEventKind::Down(PointerButton::Left)
                if hit_target == Some(ShellComponent::ClockButton) =>
            {
                self.last_click = None;
                (target, self.clock_button_activation_command())
            }
            ui::MouseEventKind::Down(_)
            | ui::MouseEventKind::Up(_)
            | ui::MouseEventKind::Click(_)
            | ui::MouseEventKind::DoubleClick(_)
            | ui::MouseEventKind::Drag(_)
            | ui::MouseEventKind::Scroll(_) => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    pub(in crate::session) fn clear_explorer_pointer_capture(&mut self) {
        let _ = self.update_explorer_state(|state| {
            state.drag = None;
        });
        self.clear_explorer_scrollbar_drag();
        self.last_click = None;
    }

    pub(in crate::session) fn route_explorer_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let target = RoutedTarget::Component(ShellComponent::Explorer);

        if hit_target != Some(ShellComponent::Explorer) {
            if matches!(
                mouse.kind,
                ui::MouseEventKind::Down(_)
                    | ui::MouseEventKind::Up(_)
                    | ui::MouseEventKind::Drag(_)
            ) {
                self.clear_explorer_pointer_capture();
            }
            return match mouse.kind {
                ui::MouseEventKind::Moved => {
                    (target_route(hit_target), ShellCommand::Hover(hit_target))
                }
                ui::MouseEventKind::Down(_)
                | ui::MouseEventKind::Up(_)
                | ui::MouseEventKind::Click(_)
                | ui::MouseEventKind::DoubleClick(_)
                | ui::MouseEventKind::Drag(_)
                | ui::MouseEventKind::Scroll(_) => {
                    (target_route(hit_target), ShellCommand::CaptureOverlayInput)
                }
            };
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => {
                (target_route(hit_target), ShellCommand::Hover(hit_target))
            }
            ui::MouseEventKind::Down(PointerButton::Right) => {
                self.last_click = None;
                (
                    target,
                    ShellCommand::OpenContextMenu {
                        target: Some(ShellComponent::Explorer),
                        coordinates,
                    },
                )
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
                let click = self.register_click(
                    Some(ShellComponent::Explorer),
                    coordinates,
                    PointerButton::Left,
                    received_at,
                );
                (
                    target,
                    ShellCommand::ExplorerPointerDown(coordinates, click, mouse.modifiers),
                )
            }
            ui::MouseEventKind::Drag(PointerButton::Left) => (
                target,
                ShellCommand::ExplorerDragUpdate(coordinates, mouse.modifiers),
            ),
            ui::MouseEventKind::Up(PointerButton::Left) => (
                target,
                ShellCommand::ExplorerDrop(coordinates, mouse.modifiers),
            ),
            ui::MouseEventKind::Scroll(ScrollDirection::Up) => {
                (target, ShellCommand::ExplorerScroll(-3))
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down) => {
                (target, ShellCommand::ExplorerScroll(3))
            }
            ui::MouseEventKind::Down(_)
            | ui::MouseEventKind::Up(_)
            | ui::MouseEventKind::Click(_)
            | ui::MouseEventKind::DoubleClick(_)
            | ui::MouseEventKind::Drag(_)
            | ui::MouseEventKind::Scroll(_) => (target, ShellCommand::RecordInput),
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
                        ShellCommand::ClockCreateSetFocus(ui::ClockCreateDialogFocus::Input),
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

    pub(in crate::session) fn route_diagnostics_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
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

    pub(in crate::session) fn route_user_management_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::UserManagement);
        let coordinates = mouse.coordinates();
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(ui::compute_shell_layout(area), ui::ShellLayout::Compact(_)) {
            return (
                RoutedTarget::Component(ShellComponent::CompactHome),
                ShellCommand::CaptureOverlayInput,
            );
        }

        if self.user_management_mode == UserManagementMode::Browse
            && hit_target == Some(ShellComponent::ClockButton)
            && matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
        {
            return (
                RoutedTarget::Component(ShellComponent::ClockButton),
                self.clock_button_activation_command(),
            );
        }

        let Some(layout) = self.user_management_layout() else {
            return (target, ShellCommand::CaptureOverlayInput);
        };
        if self.user_management_mode != UserManagementMode::Browse {
            return match mouse.kind {
                ui::MouseEventKind::Moved => (target, ShellCommand::Hover(hit_target)),
                ui::MouseEventKind::Down(PointerButton::Left) => layout
                    .form_control_at(coordinates.0, coordinates.1)
                    .map(|field| {
                        let command = match field {
                            ui::UserManagementField::Role
                            | ui::UserManagementField::Submit
                            | ui::UserManagementField::Cancel => {
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

        match mouse.kind {
            ui::MouseEventKind::Moved => (target, ShellCommand::Hover(hit_target)),
            ui::MouseEventKind::Scroll(ScrollDirection::Up)
                if rect_contains(layout.rows_area, coordinates) =>
            {
                (target, ShellCommand::UserManagementPrevious)
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down)
                if rect_contains(layout.rows_area, coordinates) =>
            {
                (target, ShellCommand::UserManagementNext)
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
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

    pub(in crate::session) fn clock_entry_id_at(&self, coordinates: CellPosition) -> Option<u64> {
        let (width, height) = self.terminal_size;
        let area = Rect::new(0, 0, width, height);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
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

    pub(in crate::session) fn route_notification_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target_component = self
            .notification_active_modal_component()
            .unwrap_or(ShellComponent::NotificationDialog);
        let target = RoutedTarget::Modal(target_component);
        let coordinates = mouse.coordinates();

        if !self.notification_can_render() {
            self.notification_pointer_capture = None;
            return (target, ShellCommand::CaptureOverlayInput);
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => (target, ShellCommand::Hover(hit_target)),
            ui::MouseEventKind::Down(PointerButton::Left) => {
                let action_index = self.notification_action_index_at(coordinates);
                self.notification_pointer_capture = action_index.and_then(|action_index| {
                    self.notification_active_modal_id().map(|notification_id| {
                        NotificationPointerCapture {
                            notification_id,
                            action_index,
                        }
                    })
                });
                if let Some(action_index) = action_index {
                    self.notification_select_action(action_index);
                }
                (target, ShellCommand::CaptureOverlayInput)
            }
            ui::MouseEventKind::Up(PointerButton::Left) => {
                let pressed = self.notification_pointer_capture.take();
                let released_index = self.notification_action_index_at(coordinates);
                let current_id = self.notification_active_modal_id();
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
            ui::MouseEventKind::Drag(PointerButton::Left) => {
                self.notification_pointer_capture = None;
                (target, ShellCommand::CaptureOverlayInput)
            }
            ui::MouseEventKind::Down(_)
            | ui::MouseEventKind::Up(_)
            | ui::MouseEventKind::Click(_)
            | ui::MouseEventKind::DoubleClick(_)
            | ui::MouseEventKind::Drag(_)
            | ui::MouseEventKind::Scroll(_) => {
                self.notification_pointer_capture = None;
                (target, ShellCommand::CaptureOverlayInput)
            }
        }
    }

    pub(in crate::session) fn route_setup_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        match mouse.kind {
            ui::MouseEventKind::Moved => {
                (target_route(hit_target), ShellCommand::Hover(hit_target))
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Up)
                if hit_target == Some(ShellComponent::SetupLanguage)
                    && self.setup_step == ui::SetupStep::Language =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupLanguage),
                    ShellCommand::SetupPreviousLanguage,
                )
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down)
                if hit_target == Some(ShellComponent::SetupLanguage)
                    && self.setup_step == ui::SetupStep::Language =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupLanguage),
                    ShellCommand::SetupNextLanguage,
                )
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Up)
                if hit_target == Some(ShellComponent::SetupTimezone)
                    && self.setup_step == ui::SetupStep::Timezone =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupTimezone),
                    ShellCommand::SetupPreviousTimezone,
                )
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down)
                if hit_target == Some(ShellComponent::SetupTimezone)
                    && self.setup_step == ui::SetupStep::Timezone =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupTimezone),
                    ShellCommand::SetupNextTimezone,
                )
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
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
            ui::MouseEventKind::Down(PointerButton::Right) => {
                self.last_click = None;
                (target_route(hit_target), ShellCommand::CaptureOverlayInput)
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_login_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        match mouse.kind {
            ui::MouseEventKind::Moved => {
                (target_route(hit_target), ShellCommand::Hover(hit_target))
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Up)
                if hit_target == Some(ShellComponent::LoginUserList) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::LoginUserList),
                    ShellCommand::LoginPreviousUser,
                )
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down)
                if hit_target == Some(ShellComponent::LoginUserList) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::LoginUserList),
                    ShellCommand::LoginNextUser,
                )
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
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
            ui::MouseEventKind::Down(PointerButton::Right) => {
                self.last_click = None;
                (target_route(hit_target), ShellCommand::CaptureOverlayInput)
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_popup_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        if hit_target != Some(ShellComponent::ContextMenu) {
            if matches!(mouse.kind, ui::MouseEventKind::Down(_)) {
                return (RoutedTarget::OutsidePopup, ShellCommand::ClosePopup);
            }

            return (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::CaptureOverlayInput,
            );
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::Hover(Some(ShellComponent::ContextMenu)),
            ),
            ui::MouseEventKind::Down(PointerButton::Left) => {
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

    pub(in crate::session) fn register_click(
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
