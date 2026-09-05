//! UI-independent application state for global clock and exit behavior.

use std::collections::VecDeque;
use std::time::Instant;

use super::notification::{
    Notification, NotificationCenter, NotificationCommand, NotificationResponse,
};
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
    pub storage: system_services_model::StorageState,
    pub network: system_services_model::NetworkState,
    pub metrics: system_services_model::SystemMetricsSnapshot,
}

impl From<&system_services_model::SystemSnapshot> for AppSystemStatusSnapshot {
    fn from(snapshot: &system_services_model::SystemSnapshot) -> Self {
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
}

/// The runtime effect requested after a state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    Redraw,
    Exit,
    PowerOff,
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
    managed_users: Vec<UserAccount>,
    storage_config: storage::StorageConfig,
    active_appearance: Option<storage::AppearanceConfig>,
    active_system_status_dashboard: Option<storage::SystemStatusDashboardConfig>,
    explorer_state: Option<ExplorerState>,
    launcher_state: Option<LauncherState>,
    diagnostics_snapshot: Option<DiagnosticsSnapshot>,
    system_status_snapshot: Option<AppSystemStatusSnapshot>,
    editor_state: Option<EditorState>,
    pending_editor_effects: VecDeque<EditorEffect>,
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
            notifications: NotificationCenter::new("Ready"),
            auth_session: None,
            managed_users: Vec::new(),
            storage_config,
            active_appearance: None,
            active_system_status_dashboard: None,
            explorer_state: None,
            launcher_state: None,
            diagnostics_snapshot: None,
            system_status_snapshot: None,
            editor_state: None,
            pending_editor_effects: VecDeque::new(),
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
                self.managed_users = managed_users;
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
                self.explorer_state = explorer_state;
                AppAction::Redraw
            }
            AppCommand::SetLauncherState(launcher_state) => {
                self.launcher_state = launcher_state;
                AppAction::Redraw
            }
            AppCommand::SetDiagnosticsSnapshot(diagnostics_snapshot) => {
                self.diagnostics_snapshot = diagnostics_snapshot;
                AppAction::Redraw
            }
            AppCommand::SetSystemStatusSnapshot(system_status_snapshot) => {
                self.system_status_snapshot = system_status_snapshot;
                AppAction::Redraw
            }
            AppCommand::SetEditorState(editor_state) => {
                self.editor_state = editor_state;
                self.pending_editor_effects.clear();
                AppAction::Redraw
            }
            AppCommand::Editor(command) => {
                if let Some(editor_state) = self.editor_state.as_mut() {
                    self.pending_editor_effects
                        .extend(editor_state.apply(command));
                }
                AppAction::Redraw
            }
            AppCommand::SetEditorViewport(viewport) => {
                if let Some(editor_state) = self.editor_state.as_mut() {
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
        }
    }

    pub fn snapshot(&self) -> AppSnapshot<'_> {
        AppSnapshot {
            clock_timezone_id: self.clock_timezone_id.as_deref(),
            clock: self.network_clock.snapshot(),
            exit_confirmation_requested: self.exit_confirmation_requested,
            notifications: &self.notifications,
            auth_session: self.auth_session.as_ref(),
            managed_users: &self.managed_users,
            storage_config: &self.storage_config,
            active_appearance: self.active_appearance.as_ref(),
            active_system_status_dashboard: self.active_system_status_dashboard.as_ref(),
            explorer: self.explorer_state.as_ref(),
            launcher: self.launcher_state.as_ref(),
            diagnostics: self.diagnostics_snapshot.as_ref(),
            system_status: self.system_status_snapshot.as_ref(),
            editor: self.editor_state.as_ref(),
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
            NotificationCommand::ResolveAlert(key) => {
                self.notifications.resolve_alert_at(&key, at);
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
        }
    }

    pub fn auth_session(&self) -> Option<&AuthSession> {
        self.auth_session.as_ref()
    }
    pub fn managed_users(&self) -> &[UserAccount] {
        &self.managed_users
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
    pub fn explorer_state(&self) -> Option<&ExplorerState> {
        self.explorer_state.as_ref()
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
            .explorer_state
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
        self.launcher_state.as_ref()
    }

    pub fn dispatch_launcher_at(
        &mut self,
        command: LauncherCommand,
        platform: &dyn platform::Platform,
        storage: &storage::StorageManager,
        _at: Instant,
    ) -> (AppAction, LauncherEffect) {
        let effect = self
            .launcher_state
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
        self.diagnostics_snapshot.as_ref()
    }
    pub fn system_status_snapshot(&self) -> Option<&AppSystemStatusSnapshot> {
        self.system_status_snapshot.as_ref()
    }
    pub fn editor_state(&self) -> Option<&EditorState> {
        self.editor_state.as_ref()
    }

    pub fn take_editor_effects(&mut self) -> Vec<EditorEffect> {
        self.pending_editor_effects.drain(..).collect()
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
            && self.managed_users == other.managed_users
            && self.storage_config == other.storage_config
            && self.active_appearance == other.active_appearance
            && self.active_system_status_dashboard == other.active_system_status_dashboard
            && self.explorer_state == other.explorer_state
            && self.launcher_state == other.launcher_state
            && self.diagnostics_snapshot == other.diagnostics_snapshot
            && self.system_status_snapshot == other.system_status_snapshot
            && self.editor_state == other.editor_state
            && self.pending_editor_effects == other.pending_editor_effects
    }
}

impl Eq for AppState {}
impl Default for AppState {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
#[path = "tests/state.rs"]
mod tests;
