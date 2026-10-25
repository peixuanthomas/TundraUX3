use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct NavigationEntry {
    id: PageInstanceId,
    focus: ShellComponent,
    opening_button: Option<(Rect, (u16, u16))>,
}

/// Owns the actual opening path. Only this module can change its private storage.
/// `path` remains separate so the public read-only screen_stack() API can borrow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShellNavigation {
    path: Vec<ShellScreen>,
    entries: Vec<NavigationEntry>,
    restored_checkpoints: HashSet<u64>,
    parked: HashMap<PageInstanceId, NavigationEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NavigationCheckpoint {
    before: ShellNavigation,
    after: Vec<PageInstanceId>,
    operation: u64,
}

impl NavigationCheckpoint {
    pub(in crate::session) fn origin_page_id(&self) -> PageInstanceId {
        self.before.entries.last().expect("navigation root").id
    }
}

pub(super) fn screen_focus(screen: ShellScreen) -> ShellComponent {
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

const PAGE_SCREENS: [ShellScreen; 16] = [
    ShellScreen::FirstRunSetup,
    ShellScreen::BootstrapAdmin,
    ShellScreen::Login,
    ShellScreen::Home,
    ShellScreen::Clock,
    ShellScreen::Diagnostics,
    ShellScreen::Logs,
    ShellScreen::Management,
    ShellScreen::SystemStatus,
    ShellScreen::Explorer,
    ShellScreen::Launcher,
    ShellScreen::CommandLine,
    ShellScreen::Editor,
    ShellScreen::Settings,
    ShellScreen::UserManagement,
    ShellScreen::ExitConfirm,
];

fn app_page_kind(screen: ShellScreen) -> Option<app::AppPageKind> {
    Some(match screen {
        ShellScreen::UserManagement => app::AppPageKind::UserManagement,
        ShellScreen::Explorer => app::AppPageKind::Explorer,
        ShellScreen::Launcher => app::AppPageKind::Launcher,
        ShellScreen::Editor => app::AppPageKind::Editor,
        ShellScreen::Diagnostics | ShellScreen::SystemStatus => app::AppPageKind::Diagnostics,
        _ => return None,
    })
}

impl ShellNavigation {
    pub(super) fn new(root: ShellScreen) -> Self {
        let mut navigation = Self {
            path: Vec::new(),
            entries: Vec::new(),
            restored_checkpoints: HashSet::new(),
            parked: HashMap::new(),
        };
        navigation.push(root);
        navigation
    }

    pub(super) fn path(&self) -> &[ShellScreen] {
        &self.path
    }

    fn push(&mut self, screen: ShellScreen) {
        self.path.push(screen);
        self.entries.push(NavigationEntry {
            id: PageInstanceId::new(),
            focus: screen_focus(screen),
            opening_button: None,
        });
    }

    fn remember_focus(&mut self, focus: ShellComponent) {
        if let Some(entry) = self.entries.last_mut() {
            entry.focus = focus;
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
        self.parked.clear();
        self.path.clear();
        self.entries.clear();
        self.push(root);
    }

    fn restore(&mut self, checkpoint: &NavigationCheckpoint) -> Option<ShellComponent> {
        // Each load may restore its saved path only once.
        if self.restored_checkpoints.contains(&checkpoint.operation) {
            return None;
        }
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
        self.restored_checkpoints.insert(checkpoint.operation);
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
    pub(in crate::session) fn park_script_page(&mut self, id: PageInstanceId) {
        let Some(index) = self
            .navigation
            .entries
            .iter()
            .position(|entry| entry.id == id)
        else {
            return;
        };
        if self.navigation.entries.len() == 1 {
            return;
        }
        self.cancel_pointer_gestures_for_modal();
        self.navigation.path.remove(index);
        let entry = self.navigation.entries.remove(index);
        self.navigation.parked.insert(id, entry);
        self.restore_navigation_page();
        self.refresh_hit_map();
    }

    pub(in crate::session) fn restore_script_page(&mut self, id: PageInstanceId) {
        if let Some(entry) = self.navigation.parked.remove(&id) {
            self.cancel_pointer_gestures_for_modal();
            self.navigation.path.push(ShellScreen::Editor);
            self.navigation.entries.push(entry);
            self.restore_navigation_page();
            self.refresh_hit_map();
        } else {
            self.activate_editor_document_page(id);
        }
    }

    /// Documents opened together share the same return destination. Keep their
    /// page instances (including file tasks and recovery) intact when switching.
    pub(in crate::session) fn editor_document_pages(&self) -> Vec<PageInstanceId> {
        self.navigation
            .path
            .iter()
            .zip(&self.navigation.entries)
            .rev()
            .take_while(|(screen, _)| **screen == ShellScreen::Editor)
            .map(|(_, entry)| entry.id)
            .collect()
    }

    pub(in crate::session) fn activate_editor_document_page(&mut self, id: PageInstanceId) {
        if self.content_page_id() == id || !self.editor_document_pages().contains(&id) {
            return;
        }
        if let Some(index) = self
            .navigation
            .entries
            .iter()
            .position(|entry| entry.id == id)
        {
            self.cancel_pointer_gestures_for_modal();
            let entry = self.navigation.entries.remove(index);
            self.navigation.entries.push(entry);
            self.restore_navigation_page();
            self.pages.common.focused_component = ShellComponent::Editor;
            self.refresh_hit_map();
        }
    }
    /// Ordinary pages map the Back button to Escape. Command Line routes the
    /// button separately so it can ask before terminating its child processes.
    /// The shared pointer capture delivers this press only after release
    /// and leaves keyboard focus with the page being returned to.
    pub(in crate::session) fn initialize_root_page(&mut self) {
        let id = self.active_page_id();
        let screen = self.active_screen();
        self.pages.create(id, screen);
        self.app.select_notification_page(id);
        if let Some(kind) = app_page_kind(screen) {
            self.app.register_page(id, kind);
        }
    }

    pub fn active_page_id(&self) -> PageInstanceId {
        self.navigation.entries.last().expect("navigation root").id
    }

    pub fn content_page_id(&self) -> PageInstanceId {
        let index = if self.active_screen() == ShellScreen::ExitConfirm
            && self.navigation.entries.len() > 1
        {
            self.navigation.entries.len() - 2
        } else {
            self.navigation.entries.len() - 1
        };
        self.navigation.entries[index].id
    }

    pub(super) fn content_opening_button(&self) -> Option<Rect> {
        self.navigation
            .entries
            .iter()
            .find(|entry| entry.id == self.content_page_id())?
            .opening_button
            .filter(|(_, size)| *size == self.terminal_size)
            .map(|(area, _)| area)
    }

    pub fn live_page_ids(&self) -> Vec<PageInstanceId> {
        self.navigation
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect()
    }

    pub fn page_screen(&self, id: PageInstanceId) -> Option<ShellScreen> {
        self.pages.screens.get(&id).copied()
    }

    pub fn command_line_start_directory_for(&self, id: PageInstanceId) -> Option<&std::path::Path> {
        self.pages
            .command_line
            .get(id)?
            .command_line_start_directory
            .as_deref()
    }

    pub(in crate::session) fn enter_screen(&mut self, screen: ShellScreen) {
        if self.active_screen() == screen {
            return;
        }
        self.enter_new_page(screen);
    }

    pub(in crate::session) fn enter_new_page(&mut self, screen: ShellScreen) {
        self.cancel_pointer_gestures_for_modal();
        let focus = self.pages.common.focused_component;
        self.navigation.remember_focus(focus);
        self.navigation.push(screen);
        self.navigation.entries.last_mut().unwrap().opening_button = self
            .interaction
            .motion_origin
            .map(|area| (area, self.terminal_size));
        let id = self.active_page_id();
        self.pages.create(id, screen);
        self.app.select_notification_page(id);
        if let Some(kind) = app_page_kind(screen) {
            self.app.register_page(id, kind);
        }
    }

    pub(in crate::session) fn reset_navigation(&mut self, root: ShellScreen) {
        self.cancel_pointer_gestures_for_modal();
        self.tasks.reset();
        self.pending_session_exit = None;
        self.pending_page_closes.clear();
        self.approved_editor_closes.clear();
        let old = self.pages.common.ids();
        for id in old {
            self.pages.remove(id);
            self.app.remove_page(id);
        }
        self.navigation.reset(root);
        self.initialize_root_page();
    }

    /// Select the nearest live instance of each kind for controller calls. An
    /// Editor remains available to its file picker without sharing another Editor.
    fn restore_navigation_page(&mut self) {
        for screen in PAGE_SCREENS {
            let id = self
                .navigation
                .path
                .iter()
                .zip(&self.navigation.entries)
                .rev()
                .find(|(kind, _)| **kind == screen)
                .map(|(_, entry)| entry.id);
            self.pages.select(id, screen);
            if let Some(kind) = app_page_kind(screen) {
                let id = self
                    .navigation
                    .path
                    .iter()
                    .zip(&self.navigation.entries)
                    .rev()
                    .find(|(screen, _)| app_page_kind(**screen) == Some(kind))
                    .map(|(_, entry)| entry.id);
                self.app.select_page(id, kind);
            }
        }
        let diagnostics = self
            .navigation
            .path
            .iter()
            .zip(&self.navigation.entries)
            .rev()
            .find(|(screen, _)| {
                matches!(screen, ShellScreen::Diagnostics | ShellScreen::SystemStatus)
            })
            .map(|(_, entry)| entry.id);
        self.pages.diagnostics.select(diagnostics);
        let management = self
            .navigation
            .path
            .iter()
            .zip(&self.navigation.entries)
            .rev()
            .find(|(screen, _)| matches!(screen, ShellScreen::Management | ShellScreen::Editor))
            .map(|(_, entry)| entry.id);
        self.pages.management.select(management);
        let logs = self
            .navigation
            .path
            .iter()
            .zip(&self.navigation.entries)
            .rev()
            .find(|(screen, _)| matches!(screen, ShellScreen::Logs | ShellScreen::Editor))
            .map(|(_, entry)| entry.id);
        self.pages.logs.select(logs);
        let id = self.active_page_id();
        self.pages.common.select(Some(id));
        self.app.select_notification_page(id);
    }

    /// Deliver a result to its owner without changing the visible navigation path.
    pub(in crate::session) fn with_page_context<R>(
        &mut self,
        id: PageInstanceId,
        action: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let screen = self.page_screen(id)?;
        let selection = self.pages.selection();
        let path = self.live_page_ids();
        self.pages.select(Some(id), screen);
        self.pages.common.select(Some(id));
        if matches!(screen, ShellScreen::SystemStatus | ShellScreen::Diagnostics) {
            self.pages.diagnostics.select(Some(id));
        }
        if let Some(kind) = app_page_kind(screen) {
            self.app.select_page(Some(id), kind);
        }
        let result = action(self);
        if path == self.live_page_ids() {
            self.pages.restore_selection(selection);
            self.app
                .select_page(selection.explorer, app::AppPageKind::Explorer);
            self.app
                .select_page(selection.launcher, app::AppPageKind::Launcher);
            self.app
                .select_page(selection.editor, app::AppPageKind::Editor);
            self.app
                .select_page(selection.diagnostics, app::AppPageKind::Diagnostics);
            self.app
                .select_page(selection.user_management, app::AppPageKind::UserManagement);
        } else {
            self.restore_navigation_page();
        }
        Some(result)
    }

    pub(in crate::session) fn return_from_screen(&mut self, screen: ShellScreen) {
        if self.active_screen() == screen {
            self.request_page_close(self.active_page_id());
        }
    }

    /// Close an exact instance; a late terminal exit cannot close a different page.
    pub fn close_page_instance(&mut self, id: PageInstanceId) {
        if self.navigation.parked.remove(&id).is_some() {
            self.tasks.close_queries(id);
            self.pages.remove(id);
            self.app.remove_page(id);
            return;
        }
        let Some(index) = self
            .navigation
            .entries
            .iter()
            .position(|entry| entry.id == id)
        else {
            return;
        };
        let was_visible = self.active_page_id() == id;
        if was_visible {
            self.cancel_pointer_gestures_for_modal();
        }
        self.navigation.path.remove(index);
        self.navigation.entries.remove(index);
        self.tasks.close_queries(id);
        self.pending_page_closes.remove(&id);
        self.approved_editor_closes.remove(&id);
        self.pages.remove(id);
        self.app.remove_page(id);
        if self.navigation.path.is_empty() {
            self.navigation.push(ShellScreen::Home);
            self.initialize_root_page();
        }
        self.restore_navigation_page();
        if !was_visible {
            return;
        }
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

    pub(in crate::session) fn begin_editor_navigation(
        &mut self,
        from_picker: bool,
    ) -> NavigationCheckpoint {
        let focus = self.pages.common.focused_component;
        self.navigation.remember_focus(focus);
        let before = self.navigation.clone();
        if from_picker {
            if let Some(focus) = self.navigation.leave(ShellScreen::Explorer) {
                self.pages.common.focused_component = focus;
                self.restore_navigation_page();
            }
        }
        if self.active_screen() == ShellScreen::Editor
            && self
                .app
                .editor_state()
                .is_some_and(|s| s.is_dirty() || s.document.path.is_some())
        {
            self.enter_new_page(ShellScreen::Editor);
        } else {
            self.enter_screen(ShellScreen::Editor);
        }
        NavigationCheckpoint {
            operation: PageInstanceId::new().get(),
            before,
            after: self
                .navigation
                .entries
                .iter()
                .map(|entry| entry.id)
                .collect(),
        }
    }

    pub(in crate::session) fn finish_editor_load_navigation(
        &mut self,
        operation: &EditorLoadOperation,
    ) {
        if let EditorLoadOperation::Open { rollback, .. } = operation {
            let live = self.live_page_ids();
            for entry in &rollback.before.entries {
                if !live.contains(&entry.id) {
                    self.tasks.close_queries(entry.id);
                    self.pages.remove(entry.id);
                    self.app.remove_page(entry.id);
                }
            }
            self.restore_navigation_page();
        }
    }

    pub(in crate::session) fn restore_editor_load_navigation(
        &mut self,
        operation: &EditorLoadOperation,
    ) {
        if let EditorLoadOperation::Open { rollback, .. } = operation {
            // Keep a later overlay's live focus while restoring the path beneath it.
            let focus = self
                .pages
                .common
                .get(self.active_page_id())
                .expect("visible page state")
                .focused_component;
            self.navigation.remember_focus(focus);
            let previous = self.live_page_ids();
            if let Some(focus) = self.navigation.restore(rollback) {
                let live = self.live_page_ids();
                for id in previous {
                    if !live.contains(&id) {
                        self.tasks.close_queries(id);
                        self.pages.remove(id);
                        self.app.remove_page(id);
                    }
                }
                self.interaction.button_capture = None;
                self.restore_navigation_page();
                self.pages.common.focused_component = focus;
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
        if self.help_open {
            return self.route_help_key(key);
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
        if self.pages.common.active_popup.is_some() {
            return self.route_popup_key(key);
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
