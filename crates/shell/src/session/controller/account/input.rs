use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_login_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(self.focused_component);
        if key.phase == InputPhase::Release {
            return (target, ShellCommand::Noop);
        }
        if key.has_non_shift_modifier() {
            return (
                target,
                if key.modifiers.is_control()
                    && !key.modifiers.alt
                    && !key.modifiers.super_key
                    && !key.modifiers.hyper
                    && !key.modifiers.meta
                    && key.key == InputKey::Enter
                    && key.phase == InputPhase::Press
                    && !self.login_users.is_empty()
                {
                    ShellCommand::SubmitLogin
                } else {
                    ShellCommand::RecordInput
                },
            );
        }
        if key.phase != InputPhase::Press
            && (matches!(key.key, InputKey::Enter | InputKey::Escape | InputKey::F(2))
                || (key.key == InputKey::Char(' ')
                    && self.focused_component == ShellComponent::LoginPasswordVisibility))
        {
            return (target, ShellCommand::RecordInput);
        }
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
        if key.phase == InputPhase::Release {
            return (target, ShellCommand::Noop);
        }
        if key.has_non_shift_modifier() {
            return (
                target,
                if key.modifiers.is_control()
                    && !key.modifiers.alt
                    && !key.modifiers.super_key
                    && !key.modifiers.hyper
                    && !key.modifiers.meta
                    && key.key == InputKey::Enter
                    && key.phase == InputPhase::Press
                {
                    match self.active_screen() {
                        ShellScreen::BootstrapAdmin => ShellCommand::SubmitBootstrapAdmin,
                        _ => ShellCommand::SubmitLogin,
                    }
                } else {
                    ShellCommand::RecordInput
                },
            );
        }
        if key.phase != InputPhase::Press && matches!(key.key, InputKey::Enter | InputKey::Escape) {
            return (target, ShellCommand::RecordInput);
        }
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

    pub(in crate::session) fn route_setup_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target_component = self.setup_active_key_component();
        let target = RoutedTarget::Component(target_component);
        if key.phase == InputPhase::Release {
            return (target, ShellCommand::Noop);
        }
        if key.has_non_shift_modifier() {
            if key.phase == InputPhase::Press
                && key.modifiers.is_control()
                && !key.modifiers.alt
                && !key.modifiers.super_key
                && !key.modifiers.hyper
                && !key.modifiers.meta
                && key.key == InputKey::Enter
            {
                return (
                    target,
                    if self.setup_custom_color_target.is_some() {
                        ShellCommand::ApplySetupCustomColor
                    } else {
                        ShellCommand::SetupPrimaryAction
                    },
                );
            }
            if key.phase == InputPhase::Press
                && key.modifiers.alt
                && !key.modifiers.is_control()
                && !key.modifiers.super_key
                && !key.modifiers.hyper
                && !key.modifiers.meta
                && key.key == InputKey::Left
                && self.setup_custom_color_target.is_none()
                && self.setup_step == ui::SetupStep::Timezone
            {
                return (target, ShellCommand::SetupPreviousStep);
            }
            return (target, ShellCommand::RecordInput);
        }
        if key.phase != InputPhase::Press
            && (matches!(key.key, InputKey::Enter | InputKey::Escape)
                || (key.key == InputKey::Char(' ')
                    && self.setup_custom_color_target.is_none()
                    && self.setup_step != ui::SetupStep::Admin))
        {
            return (target, ShellCommand::RecordInput);
        }

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
                    (target, ShellCommand::SetupPrimaryAction)
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

    pub(in crate::session) fn route_user_management_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::UserManagement);
        if key.phase == InputPhase::Release {
            return (target, ShellCommand::Noop);
        }
        if key.has_non_shift_modifier() {
            return (
                target,
                if key.modifiers.is_control()
                    && !key.modifiers.alt
                    && !key.modifiers.super_key
                    && !key.modifiers.hyper
                    && !key.modifiers.meta
                    && key.key == InputKey::Enter
                    && key.phase == InputPhase::Press
                    && self.user_management_mode != UserManagementMode::Browse
                {
                    ShellCommand::SubmitUserManagementForm
                } else {
                    ShellCommand::RecordInput
                },
            );
        }
        let editing_text = self.user_management_mode != UserManagementMode::Browse
            && matches!(
                self.user_management_form_field(),
                Some(
                    UserManagementFormField::Username
                        | UserManagementFormField::DisplayName
                        | UserManagementFormField::Password
                )
            );
        if key.phase != InputPhase::Press
            && (matches!(key.key, InputKey::Enter | InputKey::Escape)
                || (matches!(key.key, InputKey::Char(_)) && !editing_text))
        {
            return (target, ShellCommand::RecordInput);
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

    pub(in crate::session) fn route_user_management_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::UserManagement);
        let coordinates = mouse.coordinates();

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
}
