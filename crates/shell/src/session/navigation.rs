use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct NavigationEntry {
    id: u64,
    focus: ShellComponent,
    explorer: Option<Box<ExplorerReturnState>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExplorerReturnState {
    state: ExplorerState,
    purpose: ExplorerPurpose,
    locations_scroll: Option<usize>,
}

/// Owns the actual opening path. Only this module can change its private storage.
/// `path` remains separate so the public read-only screen_stack() API can borrow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShellNavigation {
    path: Vec<ShellScreen>,
    entries: Vec<NavigationEntry>,
    next_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NavigationCheckpoint {
    before: ShellNavigation,
    after: Vec<u64>,
}

fn screen_focus(screen: ShellScreen) -> ShellComponent {
    match screen {
        ShellScreen::FirstRunSetup => ShellComponent::SetupLanguage,
        ShellScreen::BootstrapAdmin => ShellComponent::BootstrapUsername,
        ShellScreen::Login => ShellComponent::LoginUserList,
        ShellScreen::Home => ShellComponent::Home,
        ShellScreen::Clock => ShellComponent::ClockNewButton,
        ShellScreen::Diagnostics => ShellComponent::Diagnostics,
        ShellScreen::Logs => ShellComponent::Logs,
        ShellScreen::Management => ShellComponent::Management,
        ShellScreen::SystemStatus => ShellComponent::SystemStatus,
        ShellScreen::Explorer => ShellComponent::Explorer,
        ShellScreen::Launcher => ShellComponent::Launcher,
        ShellScreen::CommandLine => ShellComponent::CommandLine,
        ShellScreen::Editor => ShellComponent::Editor,
        ShellScreen::Settings => ShellComponent::Settings,
        ShellScreen::UserManagement => ShellComponent::UserManagement,
        ShellScreen::ExitConfirm => ShellComponent::ExitDialog,
    }
}

impl ShellNavigation {
    pub(super) fn new(root: ShellScreen) -> Self {
        let mut navigation = Self {
            path: Vec::new(),
            entries: Vec::new(),
            next_id: 0,
        };
        navigation.push(root);
        navigation
    }

    pub(super) fn path(&self) -> &[ShellScreen] {
        &self.path
    }

    fn push(&mut self, screen: ShellScreen) {
        self.next_id += 1;
        self.path.push(screen);
        self.entries.push(NavigationEntry {
            id: self.next_id,
            focus: screen_focus(screen),
            explorer: None,
        });
    }

    fn remember_focus(&mut self, focus: ShellComponent) {
        if let Some(entry) = self.entries.last_mut() {
            entry.focus = focus;
        }
    }

    fn enter(&mut self, screen: ShellScreen, focus: ShellComponent) {
        if self.path.last() != Some(&screen) {
            self.remember_focus(focus);
            self.push(screen);
        }
    }

    fn leave(&mut self, screen: ShellScreen) -> Option<ShellComponent> {
        if self.path.last() != Some(&screen) {
            return None;
        }
        self.path.pop();
        self.entries.pop();
        if self.path.is_empty() {
            self.push(ShellScreen::Home);
        }
        self.entries.last().map(|entry| entry.focus)
    }

    fn reset(&mut self, root: ShellScreen) {
        self.path.clear();
        self.entries.clear();
        self.push(root);
    }

    fn restore(&mut self, checkpoint: &NavigationCheckpoint) -> Option<ShellComponent> {
        // A late failure must not replace a page the user has since closed/reopened.
        if !self
            .entries
            .iter()
            .map(|entry| entry.id)
            .take(checkpoint.after.len())
            .eq(checkpoint.after.iter().copied())
        {
            return None;
        }
        let tail_path = self.path.split_off(checkpoint.after.len());
        let tail_entries = self.entries.split_off(checkpoint.after.len());
        self.path = checkpoint.before.path.clone();
        self.entries = checkpoint.before.entries.clone();
        self.path.extend(tail_path);
        self.entries.extend(tail_entries);
        self.entries.last().map(|entry| entry.focus)
    }
}

impl ShellSession {
    /// The chrome shortcut acts like Escape except in Command Line, where it
    /// uses the host's emergency termination shortcut to stop the child PTY.
    /// The shared pointer capture delivers this press only after release
    /// and leaves keyboard focus with the page being returned to.
    pub(in crate::session) fn normalize_shell_navigation_input(
        &self,
        input: InputEvent,
    ) -> InputEvent {
        if let InputEvent::Mouse(mouse) = &input
            && mouse.kind == ui::MouseEventKind::Down(PointerButton::Left)
            && !self.auto_admin_visible()
            && !self.notification_has_active_modal()
            && !self.time_sync_dialog_visible
            && self.active_popup.is_none()
            && self.diagnostics_repair_preview.is_empty()
            && matches!(
                self.active_screen(),
                ShellScreen::Diagnostics | ShellScreen::SystemStatus
            )
            && let Some(button) = self.button_at(mouse.coordinates())
            && let Some(code) = button
                .id
                .as_str()
                .strip_prefix("diagnostics.toolbar.")
                .and_then(|value| value.parse::<u32>().ok())
                .and_then(char::from_u32)
        {
            return InputEvent::Key(KeyInput::new(InputKey::Char(code)));
        }
        if let InputEvent::Mouse(mouse) = &input
            && mouse.kind == ui::MouseEventKind::Down(PointerButton::Left)
            && self.hit_map.target_at(mouse.coordinates()) == Some(ShellComponent::BackButton)
        {
            if self.active_screen() == ShellScreen::CommandLine
                && !self.notification_has_active_modal()
                && !self.auto_admin_visible()
            {
                return InputEvent::Key(KeyInput::with_modifiers(
                    InputKey::Char('x'),
                    InputModifiers::CTRL_SHIFT,
                ));
            }
            return InputEvent::Key(KeyInput::new(InputKey::Escape));
        }
        input
    }

    pub(in crate::session) fn enter_screen(&mut self, screen: ShellScreen) {
        if self.active_screen() != screen {
            self.button_pointer_capture = None;
        }
        // A file picker can open another Explorer above an existing browser.
        // Preserve the earlier visit before the picker replaces the shared model.
        if screen == ShellScreen::Explorer
            && self.active_screen() != screen
            && let Some(index) = self.screen_stack().iter().rposition(|page| *page == screen)
            && self.navigation.entries[index].explorer.is_none()
            && let Some(state) = self.app.explorer_state().cloned()
        {
            let saved = ExplorerReturnState {
                state,
                purpose: self.explorer_purpose.clone(),
                locations_scroll: self.explorer_locations_scroll,
            };
            self.navigation.entries[index].explorer = Some(Box::new(saved));
        }
        let focus = self.focused_component;
        self.navigation.enter(screen, focus);
        self.focused_component = screen_focus(screen);
    }

    pub(in crate::session) fn reset_navigation(&mut self, root: ShellScreen) {
        self.button_pointer_capture = None;
        self.navigation.reset(root);
        self.focused_component = screen_focus(root);
    }

    /// Called only after the page has cancelled/confirmed its local work and cleaned up.
    pub(in crate::session) fn return_from_screen(&mut self, screen: ShellScreen) {
        if let Some(focus) = self.navigation.leave(screen) {
            self.button_pointer_capture = None;
            self.focused_component = focus;
            self.restore_navigation_page();
            let message = match self.active_screen() {
                ShellScreen::Launcher => i18n::msg!("shell-launcher"),
                ShellScreen::Explorer => i18n::msg!("shell-explorer"),
                ShellScreen::Editor => i18n::msg!("shell-editor"),
                ShellScreen::Logs => i18n::msg!("shell-logs"),
                ShellScreen::Diagnostics => i18n::msg!("shell-diagnostics"),
                ShellScreen::SystemStatus => i18n::msg!("shell-system-status"),
                ShellScreen::Settings => i18n::msg!("settings-ready"),
                ShellScreen::Clock => i18n::msg!("shell-clock"),
                _ => i18n::msg!("shell-ready"),
            };
            self.notify_status(message);
            self.refresh_hit_map();
        }
    }

    fn restore_navigation_page(&mut self) {
        if self.active_screen() == ShellScreen::Explorer
            && let Some(saved) = self
                .navigation
                .entries
                .last_mut()
                .and_then(|entry| entry.explorer.take())
        {
            self.replace_explorer_state(Some(saved.state));
            self.explorer_purpose = saved.purpose;
            self.explorer_locations_scroll = saved.locations_scroll;
        }
    }

    pub(in crate::session) fn begin_editor_navigation(
        &mut self,
        from_picker: bool,
    ) -> NavigationCheckpoint {
        let focus = self.focused_component;
        self.navigation.remember_focus(focus);
        let before = self.navigation.clone();
        if from_picker {
            if let Some(focus) = self.navigation.leave(ShellScreen::Explorer) {
                self.focused_component = focus;
            }
        }
        self.enter_screen(ShellScreen::Editor);
        // Even an existing Editor gets a new visit ID for this load. Restoring
        // the old path then consumes the checkpoint and cannot insert it twice.
        self.navigation.next_id += 1;
        let id = self.navigation.next_id;
        self.navigation
            .entries
            .last_mut()
            .expect("navigation root")
            .id = id;
        NavigationCheckpoint {
            before,
            after: self
                .navigation
                .entries
                .iter()
                .map(|entry| entry.id)
                .collect(),
        }
    }

    pub(in crate::session) fn restore_editor_load_navigation(
        &mut self,
        operation: &EditorLoadOperation,
    ) {
        if let EditorLoadOperation::Open { rollback, .. } = operation {
            // Keep a later overlay's live focus while restoring the path beneath it.
            let focus = self.focused_component;
            self.navigation.remember_focus(focus);
            if let Some(focus) = self.navigation.restore(rollback) {
                self.button_pointer_capture = None;
                self.focused_component = focus;
                self.restore_navigation_page();
            }
        }
    }

    /// Esc and the shared Back button reach this policy before page key routing.
    /// Pages may cancel forms or request confirmation; only navigation changes the path.
    pub(in crate::session) fn route_back_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        if self.notification_has_active_modal() {
            return self.route_notification_key(key);
        }
        if self.active_screen() == ShellScreen::CommandLine {
            return (
                RoutedTarget::Component(ShellComponent::CommandLine),
                if key.phase.is_press_like() {
                    ShellCommand::CommandLineKey(key.clone())
                } else {
                    ShellCommand::Noop
                },
            );
        }
        if key.phase != InputPhase::Press {
            return (RoutedTarget::Global, ShellCommand::Noop);
        }
        if self.time_sync_dialog_visible {
            return self.route_time_sync_dialog_key(key);
        }
        if self.active_screen() == ShellScreen::ExitConfirm {
            return self.route_exit_confirm_key(key);
        }
        if self.resolved_overlay_owner() == Some(ShellComponent::Explorer) {
            return self.route_explorer_key(key);
        }
        if self.active_popup.is_some() {
            return self.route_popup_key(key);
        }
        match self.active_screen() {
            ShellScreen::FirstRunSetup => self.route_setup_key(key),
            ShellScreen::BootstrapAdmin => self.route_auth_key(key),
            ShellScreen::Login => self.route_login_key(key),
            ShellScreen::Home => (
                RoutedTarget::Global,
                if key.has_non_shift_modifier() {
                    ShellCommand::RecordInput
                } else {
                    ShellCommand::RequestExit
                },
            ),
            ShellScreen::Clock => self.route_clock_key(key),
            ShellScreen::Diagnostics => self.route_diagnostics_key(key),
            ShellScreen::SystemStatus => self.route_system_status_key(key),
            ShellScreen::Explorer => self.route_explorer_key(key),
            ShellScreen::Launcher => self.route_launcher_key(key),
            ShellScreen::UserManagement => self.route_user_management_key(key),
            ShellScreen::Editor => (
                RoutedTarget::Component(ShellComponent::Editor),
                ShellCommand::EditorKey(key.clone()),
            ),
            ShellScreen::Settings => (
                RoutedTarget::Component(ShellComponent::Settings),
                ShellCommand::SettingsKey(key.clone()),
            ),
            ShellScreen::Logs => (
                RoutedTarget::Component(ShellComponent::Logs),
                ShellCommand::LogsKey(key.clone()),
            ),
            ShellScreen::Management => (
                RoutedTarget::Component(ShellComponent::Management),
                ShellCommand::ManagementKey(key.clone()),
            ),
            ShellScreen::CommandLine | ShellScreen::ExitConfirm => unreachable!("handled above"),
        }
    }

    #[cfg(test)]
    pub(in crate::session) fn set_navigation_path(&mut self, path: Vec<ShellScreen>) {
        let mut screens = path.into_iter();
        self.reset_navigation(screens.next().unwrap_or(ShellScreen::Home));
        for screen in screens {
            self.enter_screen(screen);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/navigation.rs"]
mod tests;
