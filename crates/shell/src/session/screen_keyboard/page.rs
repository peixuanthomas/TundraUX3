use super::*;

impl ShellSession {
    pub(in crate::session) fn page_keyboard_identity(&self) -> Option<String> {
        let overlays = self.interactive_overlays();
        let overlay = overlays.last();
        if let Some(overlay) = overlay {
            let accepts_text = match overlay.component() {
                Some(ShellComponent::Launcher) => self
                    .pages
                    .launcher
                    .launcher_run_settings
                    .as_ref()
                    .is_some_and(|state| state.embedded && matches!(state.selected, 1 | 2)),
                Some(ShellComponent::Explorer) => matches!(
                    self.resolved_explorer_overlay(),
                    Some(overlays::ResolvedExplorerOverlay::Input(_))
                ),
                Some(ShellComponent::Settings) => {
                    (overlay.id.starts_with("settings-editor:")
                        && overlay.id != "settings-editor:animation")
                        || self
                            .pages
                            .settings
                            .settings_state
                            .as_ref()
                            .and_then(|state| state.picker.as_ref())
                            .is_some_and(|picker| {
                                matches!(
                                    picker.kind,
                                    ui::SettingsPickerKind::Language
                                        | ui::SettingsPickerKind::Timezone
                                )
                            })
                }
                Some(ShellComponent::UserManagement) => !matches!(
                    self.pages.user_management.user_management_mode,
                    UserManagementMode::Browse
                ),
                Some(ShellComponent::SetupCustomColorDialog | ShellComponent::ClockCreateInput) => {
                    true
                }
                _ => false,
            };
            return accepts_text.then(|| overlay.id.clone());
        }
        match self.active_screen() {
            ShellScreen::Login | ShellScreen::BootstrapAdmin | ShellScreen::Editor => {
                Some(format!("{:?}", self.active_screen()))
            }
            ShellScreen::Explorer => Some(format!(
                "explorer:{:?}",
                self.pages.explorer.explorer_input_mode
            )),
            ShellScreen::FirstRunSetup if self.pages.setup.setup_step == ui::SetupStep::Admin => {
                Some("setup-admin".into())
            }
            _ => None,
        }
    }

    pub(super) fn page_keyboard_target(&self) -> String {
        let mut target = format!("{:?}", self.pages.common.focused_component);
        if self.config_editor_form_visible() {
            target.push_str(&self.management_keyboard_target());
        }
        match self.active_screen() {
            ShellScreen::Launcher => {
                if let Some(state) = &self.pages.launcher.launcher_run_settings {
                    target.push_str(&format!(":{}:{}", state.id, state.selected));
                }
            }
            ShellScreen::Editor => {
                target.push_str(&format!(":{:?}", self.pages.editor.editor_focus));
                if let Some(search) = &self.pages.editor.editor_search_bar {
                    target.push_str(&format!(":{:?}", search.focus));
                }
            }
            ShellScreen::Management => {
                target.push_str(&self.management_keyboard_target());
            }
            ShellScreen::UserManagement => match &self.pages.user_management.user_management_mode {
                UserManagementMode::Create(form) => {
                    target.push_str(&format!(":{:?}", form.focused_field))
                }
                UserManagementMode::EditInfo(form) => {
                    target.push_str(&format!(":{:?}", form.focused_field))
                }
                UserManagementMode::Browse => {}
            },
            ShellScreen::Clock => {
                if let Some(form) = &self.pages.clock.clock_create_state {
                    target.push_str(&format!(":{:?}:{}", form.focus, form.active_field));
                }
            }
            _ => {}
        }
        target
    }

    pub(super) fn focus_page_keyboard_input(&mut self) {
        match self.active_screen() {
            ShellScreen::Launcher => {
                self.pages.common.focused_component = ShellComponent::Launcher;
                if let Some(state) = &mut self.pages.launcher.launcher_run_settings
                    && state.embedded
                    && !matches!(state.selected, 1 | 2)
                {
                    state.selected = 1;
                }
            }
            ShellScreen::Login => {
                self.pages.common.focused_component = ShellComponent::LoginPassword
            }
            ShellScreen::BootstrapAdmin => {
                if !matches!(
                    self.pages.common.focused_component,
                    ShellComponent::BootstrapUsername | ShellComponent::BootstrapPassword
                ) {
                    self.pages.common.focused_component = ShellComponent::BootstrapUsername;
                }
            }
            ShellScreen::FirstRunSetup => {
                if self.pages.setup.setup_custom_color_target.is_none()
                    && !matches!(
                        self.pages.common.focused_component,
                        ShellComponent::SetupAdminUsername
                            | ShellComponent::SetupAdminPassword
                            | ShellComponent::SetupAdminPasswordConfirm
                            | ShellComponent::SetupAdminHint
                    )
                {
                    self.pages.common.focused_component = ShellComponent::SetupAdminUsername;
                    if let Some(field) =
                        setup_field_for_component(ShellComponent::SetupAdminUsername)
                    {
                        self.pages.setup.setup_focused_field = field;
                    }
                }
            }
            ShellScreen::Explorer => {
                if self.pages.explorer.explorer_input_mode == ExplorerInputMode::Browse {
                    self.begin_explorer_input(ExplorerInputMode::Search);
                }
                self.pages.explorer.explorer_overlay_selection = 0;
                self.pages.common.focused_component = ShellComponent::Explorer;
            }
            ShellScreen::Editor => {
                self.pages.common.focused_component = ShellComponent::Editor;
                if self
                    .pages
                    .editor
                    .editor_search_bar
                    .as_ref()
                    .is_none_or(|search| search.focus.is_none())
                {
                    self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                    self.pages.editor.editor_selected_toolbar_action = None;
                }
            }
            ShellScreen::Clock => {
                if let Some(form) = &mut self.pages.clock.clock_create_state {
                    form.focus = ui::ClockCreateDialogFocus::Input;
                    self.pages.common.focused_component = ShellComponent::ClockCreateInput;
                }
            }
            ShellScreen::Settings => {
                if let Some(editor) = self
                    .pages
                    .settings
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.number_editor.as_mut())
                {
                    editor.selected_index = 0;
                }
            }
            ShellScreen::UserManagement => {
                match &mut self.pages.user_management.user_management_mode {
                    UserManagementMode::Create(form) => {
                        if !matches!(
                            form.focused_field,
                            UserManagementFormField::Username
                                | UserManagementFormField::DisplayName
                                | UserManagementFormField::Password
                        ) {
                            form.focused_field = UserManagementFormField::Username;
                        }
                    }
                    UserManagementMode::EditInfo(form) => {
                        form.focused_field = UserManagementFormField::DisplayName
                    }
                    UserManagementMode::Browse => {}
                }
            }
            _ => {}
        }
    }

    pub(in crate::session) fn paste_native_text(
        &mut self,
        value: &str,
        platform: &dyn Platform,
        at: Instant,
    ) {
        if self.page_keyboard_identity().is_none() {
            return;
        }
        let page = self.active_page_id();
        let owner = self.page_keyboard_identity();
        // Pasted line breaks must never submit a password or a management form.
        for character in value.chars().filter(|character| !character.is_control()) {
            if self.active_page_id() != page || self.page_keyboard_identity() != owner {
                break;
            }
            let input = InputEvent::Key(KeyInput::new(InputKey::Char(character)));
            let (target, command) = self.route_key_input(match &input {
                InputEvent::Key(key) => key,
                _ => unreachable!(),
            });
            let _ = self.apply_routed_event_once(
                RoutedEvent {
                    input,
                    target,
                    command,
                },
                platform,
                at,
            );
        }
    }
}
