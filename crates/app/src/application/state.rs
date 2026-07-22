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

/// Commands understood by the application state core.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    SetExplorerState(Option<ExplorerState>),
    SetLauncherState(Option<LauncherState>),
    SetDiagnosticsSnapshot(Option<DiagnosticsSnapshot>),
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
    pub explorer: Option<&'a ExplorerState>,
    pub launcher: Option<&'a LauncherState>,
    pub diagnostics: Option<&'a DiagnosticsSnapshot>,
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
    explorer_state: Option<ExplorerState>,
    launcher_state: Option<LauncherState>,
    diagnostics_snapshot: Option<DiagnosticsSnapshot>,
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
            explorer_state: None,
            launcher_state: None,
            diagnostics_snapshot: None,
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
            explorer: self.explorer_state.as_ref(),
            launcher: self.launcher_state.as_ref(),
            diagnostics: self.diagnostics_snapshot.as_ref(),
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
            && self.explorer_state == other.explorer_state
            && self.launcher_state == other.launcher_state
            && self.diagnostics_snapshot == other.diagnostics_snapshot
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
mod tests {
    use chrono::{TimeZone, Timelike, Utc};

    use super::*;

    fn session(id: &str, username: &str) -> AuthSession {
        AuthSession {
            session_id: format!("session-{id}"),
            user_id: format!("user-{id}"),
            username: username.to_string(),
            role: identity::UserRole::User,
            started_at_epoch_ms: 1,
        }
    }

    fn diagnostics_snapshot(warning: &str) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot {
            scanned_at: Utc
                .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
                .single()
                .unwrap(),
            checks: Vec::new(),
            incidents: Vec::new(),
            logs: Vec::new(),
            warnings: vec![warning.to_string()],
        }
    }

    fn user(id: &str, username: &str) -> UserAccount {
        UserAccount {
            id: id.to_string(),
            username: username.to_string(),
            display_name: username.to_string(),
            role: identity::UserRole::User,
            enabled: true,
            failed_login_attempts: 0,
            locked_until_epoch_ms: None,
            password_hint: None,
            appearance: storage::AppearanceConfig::default(),
            created_at_epoch_ms: 1,
            updated_at_epoch_ms: 1,
            last_login_at_epoch_ms: None,
        }
    }

    #[test]
    fn managed_users_can_be_replaced_cleared_and_borrowed_from_a_snapshot() {
        let mut state = AppState::default();
        let first = user("first", "ada");
        let replacement = user("replacement", "lin");

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetManagedUsers(vec![first.clone()]),
                Instant::now()
            ),
            AppAction::Redraw
        );
        assert_eq!(state.managed_users(), &[first.clone()]);
        assert_eq!(state.snapshot().managed_users, &[first]);

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetManagedUsers(vec![replacement.clone()]),
                Instant::now(),
            ),
            AppAction::Redraw
        );
        assert_eq!(state.managed_users(), &[replacement]);

        assert_eq!(
            state.dispatch_at(AppCommand::SetManagedUsers(Vec::new()), Instant::now()),
            AppAction::Redraw
        );
        assert!(state.managed_users().is_empty());
        assert!(state.snapshot().managed_users.is_empty());
    }

    #[test]
    fn explorer_state_can_be_set_cleared_and_borrowed_from_a_snapshot() {
        let mut state = AppState::default();
        let explorer = ExplorerState::new("workspace", false);

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetExplorerState(Some(explorer.clone())),
                Instant::now(),
            ),
            AppAction::Redraw
        );
        assert_eq!(state.explorer_state(), Some(&explorer));
        assert_eq!(state.snapshot().explorer, Some(&explorer));

        assert_eq!(
            state.dispatch_at(AppCommand::SetExplorerState(None), Instant::now()),
            AppAction::Redraw
        );
        assert!(state.explorer_state().is_none());
        assert!(state.snapshot().explorer.is_none());
    }

    #[test]
    fn explorer_dispatch_without_state_is_a_safe_noop() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let root = std::env::temp_dir().join(format!(
            "tundra-app-state-explorer-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("Documents")).expect("test documents directory");
        let app_paths = platform::build_windows_app_paths(
            root.join("Roaming"),
            root.join("Local"),
            root.join("Temp"),
        )
        .expect("test paths");
        let user_dirs = platform::UserDirs::new(
            root.join("Desktop"),
            root.join("Documents"),
            root.join("Downloads"),
            root.join("Pictures"),
            root.join("Videos"),
            root.join("Music"),
            root.join("Roaming"),
        )
        .expect("test user directories");
        let platform = platform::mock::MockPlatform::new(user_dirs, app_paths.clone());
        let storage = storage::StorageManager::open(app_paths)
            .expect("test storage")
            .manager;
        let mut state = AppState::default();

        assert_eq!(
            state.dispatch_explorer_at(
                ExplorerCommand::Refresh,
                &platform,
                &storage,
                Instant::now()
            ),
            (AppAction::Redraw, ExplorerEffect::None)
        );
        let _ = platform::cleanup_temp_path(&root);
    }

    #[test]
    fn launcher_state_can_be_set_cleared_and_borrowed_from_a_snapshot() {
        let mut state = AppState::default();
        let launcher = LauncherState {
            message: Some("ready".to_string()),
            ..LauncherState::default()
        };

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetLauncherState(Some(launcher.clone())),
                Instant::now(),
            ),
            AppAction::Redraw
        );
        assert_eq!(state.launcher_state(), Some(&launcher));
        assert_eq!(state.snapshot().launcher, Some(&launcher));

        assert_eq!(
            state.dispatch_at(AppCommand::SetLauncherState(None), Instant::now()),
            AppAction::Redraw
        );
        assert!(state.launcher_state().is_none());
        assert!(state.snapshot().launcher.is_none());
    }

    #[test]
    fn launcher_dispatch_without_state_is_a_safe_noop() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let root = std::env::temp_dir().join(format!(
            "tundra-app-state-launcher-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("Documents")).expect("test documents directory");
        let app_paths = platform::build_windows_app_paths(
            root.join("Roaming"),
            root.join("Local"),
            root.join("Temp"),
        )
        .expect("test paths");
        let user_dirs = platform::UserDirs::new(
            root.join("Desktop"),
            root.join("Documents"),
            root.join("Downloads"),
            root.join("Pictures"),
            root.join("Videos"),
            root.join("Music"),
            root.join("Roaming"),
        )
        .expect("test user directories");
        let platform = platform::mock::MockPlatform::new(user_dirs, app_paths.clone());
        let storage = storage::StorageManager::open(app_paths)
            .expect("test storage")
            .manager;
        let mut state = AppState::default();

        assert_eq!(
            state.dispatch_launcher_at(
                LauncherCommand::Refresh,
                &platform,
                &storage,
                Instant::now()
            ),
            (AppAction::Redraw, LauncherEffect::None)
        );
        let _ = platform::cleanup_temp_path(&root);
    }

    #[test]
    fn diagnostics_snapshot_can_be_set_replaced_cleared_and_borrowed() {
        let mut state = AppState::default();
        let first = diagnostics_snapshot("first");
        let replacement = diagnostics_snapshot("replacement");

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetDiagnosticsSnapshot(Some(first.clone())),
                Instant::now(),
            ),
            AppAction::Redraw
        );
        assert_eq!(state.diagnostics_snapshot(), Some(&first));
        assert_eq!(state.snapshot().diagnostics, Some(&first));

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetDiagnosticsSnapshot(Some(replacement.clone())),
                Instant::now(),
            ),
            AppAction::Redraw
        );
        assert_eq!(state.diagnostics_snapshot(), Some(&replacement));

        assert_eq!(
            state.dispatch_at(AppCommand::SetDiagnosticsSnapshot(None), Instant::now()),
            AppAction::Redraw
        );
        assert!(state.diagnostics_snapshot().is_none());
        assert!(state.snapshot().diagnostics.is_none());
    }

    #[test]
    fn editor_viewport_is_applied_when_an_editor_is_open_and_is_safe_otherwise() {
        let mut state = AppState::default();
        let viewport = EditorViewport {
            top_line: 12,
            left_column: 4,
        };
        let now = Instant::now();

        assert_eq!(
            state.dispatch_at(AppCommand::SetEditorViewport(viewport), now),
            AppAction::Redraw
        );
        assert!(state.editor_state().is_none());

        state.dispatch_at(AppCommand::SetEditorState(Some(EditorState::new())), now);
        assert_eq!(
            state.dispatch_at(AppCommand::SetEditorViewport(viewport), now),
            AppAction::Redraw
        );
        assert_eq!(
            state.editor_state().map(|editor| editor.viewport),
            Some(viewport)
        );
    }

    #[test]
    fn editor_commands_preserve_graphemes_and_queue_effects_in_order() {
        let mut state = AppState::default();
        let now = Instant::now();

        assert_eq!(
            state.dispatch_at(AppCommand::SetEditorState(Some(EditorState::new())), now),
            AppAction::Redraw
        );
        assert_eq!(
            state.dispatch_at(
                AppCommand::Editor(EditorCommand::InsertText("A好e\u{301}🙂".to_string())),
                now,
            ),
            AppAction::Redraw
        );
        assert_eq!(
            state
                .snapshot()
                .editor
                .map(EditorState::export_text)
                .as_deref(),
            Some("A好e\u{301}🙂")
        );

        state.dispatch_at(AppCommand::Editor(EditorCommand::RequestOpen), now);
        state.dispatch_at(AppCommand::Editor(EditorCommand::RequestPaste), now);
        assert_eq!(
            state.take_editor_effects(),
            vec![EditorEffect::OpenFilePicker, EditorEffect::ReadClipboard]
        );
    }

    #[test]
    fn replacing_or_clearing_editor_state_discards_pending_effects() {
        let mut state = AppState::default();
        let now = Instant::now();
        state.dispatch_at(AppCommand::SetEditorState(Some(EditorState::new())), now);
        state.dispatch_at(AppCommand::Editor(EditorCommand::RequestOpen), now);

        state.dispatch_at(AppCommand::SetEditorState(Some(EditorState::new())), now);
        assert!(state.take_editor_effects().is_empty());
        assert!(state.snapshot().editor.is_some());

        state.dispatch_at(AppCommand::Editor(EditorCommand::RequestPaste), now);
        state.dispatch_at(AppCommand::SetEditorState(None), now);
        assert!(state.take_editor_effects().is_empty());
        assert!(state.editor_state().is_none());
        assert!(state.snapshot().editor.is_none());
    }

    #[test]
    fn auth_session_can_be_set_replaced_cleared_and_borrowed_from_a_snapshot() {
        let mut state = AppState::default();
        let first = session("first", "ada");
        let replacement = session("replacement", "lin");

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetAuthSession(Some(first.clone())),
                Instant::now()
            ),
            AppAction::Redraw
        );
        assert_eq!(state.auth_session(), Some(&first));
        let snapshot = state.snapshot();
        assert_eq!(snapshot.auth_session, Some(&first));
        drop(snapshot);
        assert_eq!(state, state.clone());

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetAuthSession(Some(replacement.clone())),
                Instant::now(),
            ),
            AppAction::Redraw
        );
        assert_eq!(state.auth_session(), Some(&replacement));
        assert_ne!(state, AppState::default());

        assert_eq!(
            state.dispatch_at(AppCommand::SetAuthSession(None), Instant::now()),
            AppAction::Redraw
        );
        assert!(state.auth_session().is_none());
        assert!(state.snapshot().auth_session.is_none());
    }

    #[test]
    fn commands_request_the_expected_runtime_actions() {
        let mut state = AppState::default();
        let now = Instant::now();
        let synced_utc = Utc
            .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
            .single()
            .unwrap();

        assert_eq!(state.dispatch_at(AppCommand::Tick, now), AppAction::Redraw);
        assert_eq!(
            state.dispatch_at(AppCommand::ApplyTimeSync(Ok(synced_utc)), now),
            AppAction::Redraw
        );
        assert_eq!(
            state.dispatch_at(AppCommand::RequestExit, now),
            AppAction::Redraw
        );
        assert_eq!(
            state.dispatch_at(AppCommand::ConfirmExit, now),
            AppAction::Exit
        );
        assert_eq!(
            state.dispatch_at(AppCommand::RequestPowerOff, now),
            AppAction::PowerOff
        );
    }

    #[test]
    fn cancelling_exit_hides_the_confirmation() {
        let mut state = AppState::default();
        let now = Instant::now();

        state.dispatch_at(AppCommand::RequestExit, now);
        assert!(state.snapshot().exit_confirmation_requested);

        assert_eq!(
            state.dispatch_at(AppCommand::CancelExit, now),
            AppAction::Redraw
        );
        assert!(!state.snapshot().exit_confirmation_requested);
    }

    #[test]
    fn snapshot_keeps_timezone_identity_and_dst_projection_together() {
        let mut state = AppState::new(Some("America/New_York".to_string()));
        let now = Instant::now();
        let dst_boundary_utc = Utc.with_ymd_and_hms(2026, 3, 8, 6, 30, 0).single().unwrap();

        state.dispatch_at(AppCommand::ApplyTimeSync(Ok(dst_boundary_utc)), now);
        let snapshot = state.snapshot();
        let timezone = snapshot.clock.timezone.expect("known timezone is resolved");
        let projected = snapshot.clock.utc.with_timezone(&timezone);

        assert_eq!(snapshot.clock_timezone_id, Some("America/New_York"));
        assert_eq!(snapshot.clock.date, projected.date_naive());
        assert_eq!(snapshot.clock.time, projected.time());
        assert_eq!(snapshot.clock.time.hour(), 1);
        assert!(snapshot.clock.warning.is_none());
    }

    #[test]
    fn clone_equality_ignores_the_network_clock_anchor() {
        let mut synchronized = AppState::new(Some("UTC".to_string()));
        let utc = Utc
            .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
            .single()
            .unwrap();
        synchronized.dispatch_at(AppCommand::ApplyTimeSync(Ok(utc)), Instant::now());

        let unsynchronized = AppState::new(Some("UTC".to_string()));
        assert_eq!(synchronized, unsynchronized);
        assert_eq!(synchronized.clone(), synchronized);
    }

    #[test]
    fn resetting_timezone_handles_invalid_ids_and_installs_a_sync_anchor() {
        let mut state = AppState::new(Some("Asia/Shanghai".to_string()));
        let now = Instant::now();
        let synchronized_utc = Utc
            .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
            .single()
            .unwrap();

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetClockTimezone {
                    timezone_id: Some("Not/AZone".to_string()),
                    synchronized_utc: None,
                },
                now,
            ),
            AppAction::Redraw
        );
        let invalid_snapshot = state.snapshot();
        assert_eq!(invalid_snapshot.clock_timezone_id, Some("Not/AZone"));
        assert!(invalid_snapshot.clock.timezone.is_none());
        assert!(
            invalid_snapshot
                .clock
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("Invalid timezone Not/AZone"))
        );

        assert_eq!(
            state.dispatch_at(
                AppCommand::SetClockTimezone {
                    timezone_id: Some("UTC".to_string()),
                    synchronized_utc: Some(synchronized_utc),
                },
                now,
            ),
            AppAction::Redraw
        );
        let utc_snapshot = state.snapshot();
        assert_eq!(utc_snapshot.clock_timezone_id, Some("UTC"));
        assert_eq!(
            utc_snapshot
                .clock
                .timezone
                .map(|timezone| timezone.to_string()),
            Some("UTC".to_string())
        );
        assert_eq!(
            utc_snapshot.clock.utc.date_naive(),
            synchronized_utc.date_naive()
        );
        assert_eq!(utc_snapshot.clock.utc.hour(), synchronized_utc.hour());
        assert_eq!(utc_snapshot.clock.utc.minute(), synchronized_utc.minute());
        assert!(utc_snapshot.clock.warning.is_none());
    }

    #[test]
    fn storage_config_and_active_appearance_are_canonical_app_state() {
        let mut config = storage::StorageConfig::default();
        config.timezone = "Asia/Shanghai".to_string();
        config.language = "en-US".to_string();
        let mut state = AppState::with_storage_config(config.clone());
        let appearance = storage::AppearanceConfig {
            border_shape: storage::BorderShape::Square,
            ..storage::AppearanceConfig::default()
        };

        assert_eq!(state.storage_config(), &config);
        assert_eq!(state.snapshot().storage_config, &config);
        assert_eq!(state.snapshot().clock_timezone_id, Some("Asia/Shanghai"));
        assert!(state.active_appearance().is_none());

        state.dispatch_at(
            AppCommand::SetActiveAppearance(Some(appearance.clone())),
            Instant::now(),
        );
        assert_eq!(state.active_appearance(), Some(&appearance));
        assert_eq!(state.snapshot().active_appearance, Some(&appearance));

        state.dispatch_at(AppCommand::SetActiveAppearance(None), Instant::now());
        assert!(state.active_appearance().is_none());
    }

    #[test]
    fn replacing_non_timezone_config_preserves_clock_anchor() {
        let mut config = storage::StorageConfig::default();
        config.timezone = "UTC".to_string();
        let mut state = AppState::with_storage_config(config.clone());
        let synchronized_utc = Utc
            .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
            .single()
            .unwrap();
        state.dispatch_at(
            AppCommand::ApplyTimeSync(Ok(synchronized_utc)),
            Instant::now(),
        );

        config.weather_location = Some("Pudong, Shanghai, China".to_string());
        state.dispatch_at(
            AppCommand::SetStorageConfig {
                config: config.clone(),
                synchronized_utc: None,
            },
            Instant::now(),
        );

        let snapshot = state.snapshot();
        assert_eq!(snapshot.storage_config, &config);
        assert_eq!(snapshot.clock_timezone_id, Some("UTC"));
        assert_eq!(
            snapshot.clock.utc.date_naive(),
            synchronized_utc.date_naive()
        );
        assert_eq!(snapshot.clock.utc.hour(), synchronized_utc.hour());
        assert_eq!(snapshot.clock.utc.minute(), synchronized_utc.minute());
    }

    #[test]
    fn replacing_timezone_updates_config_and_clock_together() {
        let mut state = AppState::default();
        let mut config = state.storage_config().clone();
        config.timezone = "America/New_York".to_string();
        let synchronized_utc = Utc
            .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
            .single()
            .unwrap();

        state.dispatch_at(
            AppCommand::SetStorageConfig {
                config: config.clone(),
                synchronized_utc: Some(synchronized_utc),
            },
            Instant::now(),
        );

        let snapshot = state.snapshot();
        assert_eq!(snapshot.storage_config, &config);
        assert_eq!(snapshot.clock_timezone_id, Some("America/New_York"));
        assert_eq!(
            snapshot.clock.utc.date_naive(),
            synchronized_utc.date_naive()
        );
        assert_eq!(snapshot.clock.utc.hour(), synchronized_utc.hour());
    }
    #[test]
    fn notifications_are_owned_dispatched_and_snapshotted_by_app_state() {
        let started_at = Instant::now();
        let mut state = AppState::default();
        assert_eq!(state.snapshot().notifications.status(), "Ready");

        state.dispatch_at(
            AppCommand::Notification(NotificationCommand::ShowToast("Saved".to_string())),
            started_at,
        );
        assert_eq!(state.notification_center().toast(), Some("Saved"));

        state.dispatch_at(AppCommand::Tick, started_at + crate::DEFAULT_TOAST_DURATION);
        assert_eq!(state.notification_center().toast(), None);
    }

    #[test]
    fn notification_modal_response_stays_in_the_app_domain_queue() {
        let mut state = AppState::default();
        let id = state.push_notification_modal(Notification::modal(
            "Confirm",
            "Continue?",
            crate::NotificationTone::Warning,
            vec![crate::NotificationAction::new("continue", "Continue")],
        ));

        let activated = state.activate_selected_notification_action().unwrap();
        assert_eq!(activated.notification_id, id);
        assert_eq!(activated.action_id, "continue");
        assert_eq!(state.take_notification_response(), Some(activated));
    }
    #[test]
    fn clock_display_remains_available_after_a_sync_result() {
        let mut state = AppState::new(Some("UTC".to_string()));
        let synced_utc = Utc
            .with_ymd_and_hms(2026, 7, 22, 12, 0, 0)
            .single()
            .unwrap();

        state.dispatch_at(AppCommand::ApplyTimeSync(Ok(synced_utc)), Instant::now());

        let display = state.current_clock_display();
        assert!(display.warning.is_none());
    }
}
