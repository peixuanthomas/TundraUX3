//! UI-independent application state for global clock and exit behavior.

use std::time::Instant;

use super::notification::{
    Notification, NotificationCenter, NotificationCommand, NotificationResponse,
};
use super::pages::{AppPageKind, AppPageModels, PageInstanceId};
use crate::diagnostics::DiagnosticsSnapshot;
use crate::editor::{EditorCommand, EditorEffect, EditorState, EditorViewport};
use crate::explorer::{ExplorerCommand, ExplorerController, ExplorerEffect, ExplorerState};
use crate::launcher::{LauncherCommand, LauncherController, LauncherEffect, LauncherState};
use identity::{AuthSession, UserAccount};
use time::{ClockDisplay, ClockSnapshot, NetworkClock, TimeSyncResult};

#[derive(Debug, Clone, PartialEq)]
pub struct AppSystemStatusSnapshot {
    pub revision: u64,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub storage: system_services::StorageState,
    pub network: system_services::NetworkState,
    pub metrics: system_services::SystemMetricsSnapshot,
}

impl From<&system_services::SystemSnapshot> for AppSystemStatusSnapshot {
    fn from(snapshot: &system_services::SystemSnapshot) -> Self {
        Self {
            revision: snapshot.revision,
            observed_at: snapshot.observed_at,
            storage: snapshot.storage.clone(),
            network: snapshot.network.clone(),
            metrics: snapshot.metrics.clone(),
        }
    }
}

/// Commands understood by the application state core.
#[derive(Debug, Clone, PartialEq)]
pub enum AppCommand {
    Tick,
    Notification(NotificationCommand),
    ApplyTimeSync(TimeSyncResult),
    SetAuthSession(Option<AuthSession>),
    SetManagedUsers(Vec<UserAccount>),
    SetStorageConfig {
        config: storage::StorageConfig,
        synchronized_utc: Option<chrono::DateTime<chrono::Utc>>,
    },
    SetActiveAppearance(Option<storage::AppearanceConfig>),
    SetActiveSystemStatusDashboard(Option<storage::SystemStatusDashboardConfig>),
    SetExplorerState(Option<ExplorerState>),
    SetLauncherState(Option<LauncherState>),
    SetDiagnosticsSnapshot(Option<DiagnosticsSnapshot>),
    SetSystemStatusSnapshot(Option<AppSystemStatusSnapshot>),
    SetEditorState(Option<EditorState>),
    Editor(EditorCommand),
    SetEditorViewport(EditorViewport),
    SetClockTimezone {
        timezone_id: Option<String>,
        synchronized_utc: Option<chrono::DateTime<chrono::Utc>>,
    },
    RequestExit,
    ConfirmExit,
    CancelExit,
    RequestPowerOff,
    RequestReboot,
}

/// The runtime effect requested after a state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    Redraw,
    Exit,
    PowerOff,
    Reboot,
}

/// A read-only view of the global application state.
#[derive(Debug, Clone)]
pub struct AppSnapshot<'a> {
    pub clock_timezone_id: Option<&'a str>,
    pub clock: ClockSnapshot,
    pub exit_confirmation_requested: bool,
    pub notifications: &'a NotificationCenter,
    pub auth_session: Option<&'a AuthSession>,
    pub managed_users: &'a [UserAccount],
    pub storage_config: &'a storage::StorageConfig,
    pub active_appearance: Option<&'a storage::AppearanceConfig>,
    pub active_system_status_dashboard: Option<&'a storage::SystemStatusDashboardConfig>,
    pub explorer: Option<&'a ExplorerState>,
    pub launcher: Option<&'a LauncherState>,
    pub diagnostics: Option<&'a DiagnosticsSnapshot>,
    pub system_status: Option<&'a AppSystemStatusSnapshot>,
    pub editor: Option<&'a EditorState>,
}

/// UI-independent state shared by application domains.
#[derive(Debug, Clone)]
pub struct AppState {
    network_clock: NetworkClock,
    clock_timezone_id: Option<String>,
    exit_confirmation_requested: bool,
    notifications: NotificationCenter,
    auth_session: Option<AuthSession>,
    storage_config: storage::StorageConfig,
    active_appearance: Option<storage::AppearanceConfig>,
    active_system_status_dashboard: Option<storage::SystemStatusDashboardConfig>,
    system_status_snapshot: Option<AppSystemStatusSnapshot>,
    page_models: AppPageModels,
}

impl AppState {
    pub fn new(clock_timezone_id: Option<String>) -> Self {
        let mut storage_config = storage::StorageConfig::default();
        if let Some(timezone_id) = clock_timezone_id {
            storage_config.timezone = timezone_id;
        }
        Self::with_storage_config(storage_config)
    }

    pub fn with_storage_config(storage_config: storage::StorageConfig) -> Self {
        let clock_timezone_id = Some(storage_config.timezone.clone());
        Self {
            network_clock: NetworkClock::new(clock_timezone_id.clone()),
            clock_timezone_id,
            exit_confirmation_requested: false,
            notifications: NotificationCenter::new(i18n::msg!("notifications-status-ready")),
            auth_session: None,
            storage_config,
            active_appearance: None,
            active_system_status_dashboard: None,
            system_status_snapshot: None,
            page_models: AppPageModels::default(),
        }
    }

    /// Applies a command at the supplied monotonic time.
    ///
    /// The time is part of the stable dispatch interface for future domains.
    /// The network clock maintains its own synchronization anchor.
    pub fn dispatch_at(&mut self, command: AppCommand, at: Instant) -> AppAction {
        match command {
            AppCommand::Tick => {
                self.notifications.expire(at);
                AppAction::Redraw
            }
            AppCommand::Notification(command) => {
                self.apply_notification_command(command, at);
                AppAction::Redraw
            }
            AppCommand::ApplyTimeSync(result) => {
                self.network_clock.apply_sync(result);
                AppAction::Redraw
            }
            AppCommand::SetAuthSession(auth_session) => {
                self.auth_session = auth_session;
                AppAction::Redraw
            }
            AppCommand::SetManagedUsers(managed_users) => {
                *self.page_models.managed_users = managed_users;
                AppAction::Redraw
            }
            AppCommand::SetStorageConfig {
                config,
                synchronized_utc,
            } => {
                if self.storage_config.timezone != config.timezone {
                    self.replace_clock_timezone(Some(config.timezone.clone()), synchronized_utc);
                }
                self.storage_config = config;
                AppAction::Redraw
            }
            AppCommand::SetActiveAppearance(active_appearance) => {
                self.active_appearance = active_appearance;
                AppAction::Redraw
            }
            AppCommand::SetActiveSystemStatusDashboard(dashboard) => {
                self.active_system_status_dashboard = dashboard;
                AppAction::Redraw
            }
            AppCommand::SetExplorerState(explorer_state) => {
                *self.page_models.explorer = explorer_state;
                AppAction::Redraw
            }
            AppCommand::SetLauncherState(launcher_state) => {
                *self.page_models.launcher = launcher_state;
                AppAction::Redraw
            }
            AppCommand::SetDiagnosticsSnapshot(diagnostics_snapshot) => {
                *self.page_models.diagnostics = diagnostics_snapshot;
                AppAction::Redraw
            }
            AppCommand::SetSystemStatusSnapshot(system_status_snapshot) => {
                self.system_status_snapshot = system_status_snapshot;
                AppAction::Redraw
            }
            AppCommand::SetEditorState(editor_state) => {
                self.page_models.editor.state = editor_state;
                self.page_models.editor.effects.clear();
                AppAction::Redraw
            }
            AppCommand::Editor(command) => {
                let editor = &mut *self.page_models.editor;
                if let Some(editor_state) = editor.state.as_mut() {
                    editor.effects.extend(editor_state.apply(command));
                }
                AppAction::Redraw
            }
            AppCommand::SetEditorViewport(viewport) => {
                if let Some(editor_state) = self.page_models.editor.state.as_mut() {
                    editor_state.viewport = viewport;
                }
                AppAction::Redraw
            }
            AppCommand::SetClockTimezone {
                timezone_id,
                synchronized_utc,
            } => {
                let timezone_id =
                    timezone_id.or_else(|| Some(self.storage_config.timezone.clone()));
                if let Some(timezone_id) = timezone_id.as_ref() {
                    self.storage_config.timezone = timezone_id.clone();
                }
                self.replace_clock_timezone(timezone_id, synchronized_utc);
                AppAction::Redraw
            }
            AppCommand::RequestExit => {
                self.exit_confirmation_requested = true;
                AppAction::Redraw
            }
            AppCommand::ConfirmExit => {
                self.exit_confirmation_requested = false;
                AppAction::Exit
            }
            AppCommand::CancelExit => {
                self.exit_confirmation_requested = false;
                AppAction::Redraw
            }
            AppCommand::RequestPowerOff => {
                self.exit_confirmation_requested = false;
                AppAction::PowerOff
            }
            AppCommand::RequestReboot => {
                self.exit_confirmation_requested = false;
                AppAction::Reboot
            }
        }
    }

    pub fn snapshot(&self) -> AppSnapshot<'_> {
        AppSnapshot {
            clock_timezone_id: self.clock_timezone_id.as_deref(),
            clock: self.network_clock.snapshot(),
            exit_confirmation_requested: self.exit_confirmation_requested,
            notifications: &self.notifications,
            auth_session: self.auth_session.as_ref(),
            managed_users: &self.page_models.managed_users,
            storage_config: &self.storage_config,
            active_appearance: self.active_appearance.as_ref(),
            active_system_status_dashboard: self.active_system_status_dashboard.as_ref(),
            explorer: self.page_models.explorer.as_ref(),
            launcher: self.page_models.launcher.as_ref(),
            diagnostics: self.page_models.diagnostics.as_ref(),
            system_status: self.system_status_snapshot.as_ref(),
            editor: self.page_models.editor.state.as_ref(),
        }
    }

    pub fn notification_center(&self) -> &NotificationCenter {
        &self.notifications
    }

    pub fn push_notification_modal(&mut self, notification: Notification) -> u64 {
        self.notifications.push_modal(notification)
    }

    pub fn push_critical_notification_modal(&mut self, notification: Notification) -> u64 {
        self.notifications.push_critical_modal(notification)
    }

    pub fn activate_notification_action(&mut self, index: usize) -> Option<NotificationResponse> {
        self.notifications.activate_action(index)
    }

    pub fn activate_selected_notification_action(&mut self) -> Option<NotificationResponse> {
        self.notifications.activate_selected_action()
    }

    pub fn take_notification_response(&mut self) -> Option<NotificationResponse> {
        self.notifications.take_response()
    }

    fn apply_notification_command(&mut self, command: NotificationCommand, at: Instant) {
        match command {
            NotificationCommand::Reset(status) => {
                self.notifications = NotificationCenter::new(status);
            }
            NotificationCommand::SetStatus(message) => self.notifications.notify_status(message),
            NotificationCommand::ShowToast(message) => {
                self.notifications.notify_toast_at(message, at);
            }
            NotificationCommand::ClearToast => self.notifications.clear_toast(),
            NotificationCommand::ShowAlert { key, message, tone } => {
                self.notifications.notify_alert_with_key(key, message, tone);
            }
            NotificationCommand::ShowPageAlert {
                page,
                key,
                message,
                tone,
            } => {
                self.notifications
                    .notify_page_alert_with_key(page, key, message, tone);
            }
            NotificationCommand::ResolveAlert(key) => {
                self.notifications.resolve_alert_at(&key, at);
            }
            NotificationCommand::ResolvePageAlert { page, key } => {
                self.notifications.resolve_page_alert_at(page, &key, at);
            }
            NotificationCommand::ClearAlerts => self.notifications.clear_alerts_at(at),
            NotificationCommand::SelectNextAction => self.notifications.select_next_action(),
            NotificationCommand::SelectPreviousAction => {
                self.notifications.select_previous_action();
            }
            NotificationCommand::SelectAction(index) => self.notifications.select_action(index),
            NotificationCommand::DismissActiveModal => {
                self.notifications.dismiss_active_modal_without_response();
            }
            NotificationCommand::DismissModalByKey(key) => {
                self.notifications.dismiss_modal_by_key(&key);
            }
            NotificationCommand::DismissPageModalByKey { key, page } => {
                self.notifications.dismiss_page_modal_by_key(&key, page);
            }
        }
    }

    pub fn auth_session(&self) -> Option<&AuthSession> {
        self.auth_session.as_ref()
    }
    pub fn managed_users(&self) -> &[UserAccount] {
        &self.page_models.managed_users
    }
    pub fn storage_config(&self) -> &storage::StorageConfig {
        &self.storage_config
    }
    pub fn active_appearance(&self) -> Option<&storage::AppearanceConfig> {
        self.active_appearance.as_ref()
    }
    pub fn active_system_status_dashboard(&self) -> Option<&storage::SystemStatusDashboardConfig> {
        self.active_system_status_dashboard.as_ref()
    }
    pub fn register_page(&mut self, id: PageInstanceId, kind: AppPageKind) {
        self.page_models.register(id, kind);
    }

    pub fn select_page(&mut self, id: Option<PageInstanceId>, kind: AppPageKind) {
        self.page_models.select(id, kind);
    }

    pub fn select_notification_page(&mut self, id: PageInstanceId) {
        self.notifications.set_visible_page(id);
    }

    pub fn remove_page(&mut self, id: PageInstanceId) {
        self.page_models.remove(id);
        self.notifications.remove_page(id);
    }

    pub fn explorer_state(&self) -> Option<&ExplorerState> {
        self.page_models.explorer.as_ref()
    }

    pub fn dispatch_explorer_at(
        &mut self,
        command: ExplorerCommand,
        platform: &dyn platform::Platform,
        storage: &storage::StorageManager,
        _at: Instant,
    ) -> (AppAction, ExplorerEffect) {
        let controller = ExplorerController::default()
            .with_editor_extensions(self.storage_config.editor.explorer_open_extensions.clone());
        let effect = self
            .page_models
            .explorer
            .as_mut()
            .map(|state| {
                controller.apply(
                    state,
                    command,
                    self.auth_session.as_ref(),
                    platform,
                    storage,
                )
            })
            .unwrap_or(ExplorerEffect::None);
        (AppAction::Redraw, effect)
    }
    pub fn launcher_state(&self) -> Option<&LauncherState> {
        self.page_models.launcher.as_ref()
    }

    pub fn dispatch_launcher_at(
        &mut self,
        command: LauncherCommand,
        platform: &dyn platform::Platform,
        storage: &storage::StorageManager,
        _at: Instant,
    ) -> (AppAction, LauncherEffect) {
        let effect = self
            .page_models
            .launcher
            .as_mut()
            .map(|state| {
                LauncherController::default().apply(
                    state,
                    command,
                    self.auth_session.as_ref(),
                    platform,
                    storage,
                )
            })
            .unwrap_or(LauncherEffect::None);
        (AppAction::Redraw, effect)
    }
    pub fn diagnostics_snapshot(&self) -> Option<&DiagnosticsSnapshot> {
        self.page_models.diagnostics.as_ref()
    }
    pub fn system_status_snapshot(&self) -> Option<&AppSystemStatusSnapshot> {
        self.system_status_snapshot.as_ref()
    }
    pub fn editor_state(&self) -> Option<&EditorState> {
        self.page_models.editor.state.as_ref()
    }

    pub fn editor_state_for_page(&self, id: PageInstanceId) -> Option<&EditorState> {
        self.page_models.editor.get(id)?.state.as_ref()
    }

    pub fn poll_editor_computations(&mut self) -> crate::editor::BackgroundUpdate {
        self.page_models
            .editor
            .state
            .as_mut()
            .map_or_else(Default::default, EditorState::poll_background)
    }

    pub fn take_editor_effects(&mut self) -> Vec<EditorEffect> {
        self.page_models.editor.effects.drain(..).collect()
    }
    pub fn current_clock_display(&self) -> ClockDisplay {
        self.network_clock.current()
    }

    fn replace_clock_timezone(
        &mut self,
        timezone_id: Option<String>,
        synchronized_utc: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        let mut network_clock = NetworkClock::new(timezone_id.clone());
        if let Some(utc) = synchronized_utc {
            network_clock.apply_sync(Ok(utc));
        }
        self.network_clock = network_clock;
        self.clock_timezone_id = timezone_id;
    }
}

impl PartialEq for AppState {
    fn eq(&self, other: &Self) -> bool {
        self.clock_timezone_id == other.clock_timezone_id
            && self.exit_confirmation_requested == other.exit_confirmation_requested
            && self.notifications == other.notifications
            && self.auth_session == other.auth_session
            && self.storage_config == other.storage_config
            && self.active_appearance == other.active_appearance
            && self.active_system_status_dashboard == other.active_system_status_dashboard
            && self.system_status_snapshot == other.system_status_snapshot
            && self.page_models == other.page_models
    }
}

impl Eq for AppState {}
impl Default for AppState {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/application/state.rs"]
mod tests;
