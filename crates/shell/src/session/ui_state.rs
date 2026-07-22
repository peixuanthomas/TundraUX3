use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ModalFocusContext {
    pub(super) screen: ShellScreen,
    pub(super) component: ShellComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NotificationPointerCapture {
    pub(super) notification_id: u64,
    pub(super) action_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DragTracker {
    pub(super) button: PointerButton,
    pub(super) origin_coordinates: CellPosition,
    pub(super) last_coordinates: CellPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScrollbarDragState {
    Explorer {
        grab_offset: u16,
    },
    Diagnostics {
        grab_offset: u16,
    },
    Editor {
        axis: ScrollbarAxis,
        grab_offset: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScrollbarAxis {
    Vertical,
    Horizontal,
}

pub(super) fn scrollbar_window_start(
    pointer_position: u16,
    grab_offset: u16,
    track_start: u16,
    track_length: u16,
    thumb_length: u16,
    content_length: usize,
    viewport_length: usize,
) -> usize {
    let travel = usize::from(track_length.saturating_sub(thumb_length));
    let maximum = content_length.saturating_sub(viewport_length);
    if travel == 0 || maximum == 0 {
        return 0;
    }
    let pointer_offset = usize::from(pointer_position.saturating_sub(track_start));
    let thumb_start = pointer_offset
        .saturating_sub(usize::from(grab_offset))
        .min(travel);
    maximum
        .saturating_mul(thumb_start)
        .saturating_add(travel / 2)
        / travel
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EditorTableResizeState {
    pub(super) table_id: ui::NodeId,
    pub(super) column_index: usize,
    pub(super) start_x: u16,
    pub(super) start_width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorCursorDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EditorCursorAccelerationState {
    pub(super) direction: EditorCursorDirection,
    pub(super) started_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EditorSettingsDialogState {
    pub(super) draft: storage::EditorConfig,
    pub(super) selected: ui::EditorSettingsField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsFocus {
    Categories,
    Fields,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsPickerState {
    pub(super) kind: ui::SettingsPickerKind,
    pub(super) query: String,
    pub(super) selected_index: usize,
    pub(super) window_start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsColorEditorState {
    pub(super) kind: ui::SettingsPickerKind,
    pub(super) value: String,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsWeatherLocationEditorState {
    pub(super) value: String,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsFileExtensionsEditorState {
    pub(super) value: String,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsTimeSyncServerEditorState {
    pub(super) value: String,
    pub(super) error: Option<String>,
    pub(super) validating: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsState {
    pub(super) category: ui::SettingsCategory,
    pub(super) selected_field: ui::SettingsField,
    pub(super) focus: SettingsFocus,
    pub(super) status: String,
    pub(super) scroll_offset: u16,
    pub(super) picker: Option<SettingsPickerState>,
    pub(super) color_editor: Option<SettingsColorEditorState>,
    pub(super) weather_location_editor: Option<SettingsWeatherLocationEditorState>,
    pub(super) file_extensions_editor: Option<SettingsFileExtensionsEditorState>,
    pub(super) time_sync_server_editor: Option<SettingsTimeSyncServerEditorState>,
    pub(super) time_sync_validation_request_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EditorReloadPolicy {
    Log { path: std::path::PathBuf },
    DiagnosticsReport { path: std::path::PathBuf },
}

impl EditorReloadPolicy {
    pub(in crate::session) fn path(&self) -> &std::path::Path {
        match self {
            Self::Log { path } | Self::DiagnosticsReport { path } => path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EditorReadSession {
    pub(super) reload: EditorReloadPolicy,
    pub(super) total_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorLoadNavigation {
    Explorer,
    EditorPicker,
    Diagnostics,
    Editor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EditorLoadOperation {
    Open {
        navigation: EditorLoadNavigation,
        reload: Option<EditorReloadPolicy>,
        replacing_dirty: bool,
    },
    Reload {
        session: EditorReadSession,
        was_at_bottom: bool,
        visible_capacity: usize,
        old_top_line: usize,
        old_left_column: usize,
        old_cursor: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EditorLoadState {
    pub(super) id: u64,
    pub(super) path: std::path::PathBuf,
    pub(super) stage: EditorTaskStage,
    pub(super) completed_bytes: u64,
    pub(super) total_bytes: Option<u64>,
    pub(super) operation: EditorLoadOperation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EditorSaveState {
    pub(super) id: u64,
    pub(super) path: std::path::PathBuf,
    pub(super) document_generation: u64,
    pub(super) revision: u64,
    pub(super) stage: EditorTaskStage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EditorRichRenderCache {
    pub(super) revision: u64,
    pub(super) blocks: std::sync::Arc<[ui::EditorRenderBlock]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UserManagementFormField {
    Username,
    DisplayName,
    Role,
    Password,
    Submit,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UserManagementCreateForm {
    pub(super) username: String,
    pub(super) display_name: String,
    pub(super) password: String,
    pub(super) role: UserRole,
    pub(super) focused_field: UserManagementFormField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UserManagementInfoForm {
    pub(super) username: String,
    pub(super) display_name: String,
    pub(super) focused_field: UserManagementFormField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UserManagementPasswordForm {
    pub(super) username: String,
    pub(super) password: String,
    pub(super) focused_field: UserManagementFormField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum UserManagementMode {
    Browse,
    Create(UserManagementCreateForm),
    EditInfo(UserManagementInfoForm),
    Password(UserManagementPasswordForm),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UserManagementPageFocus {
    UserList,
    Action(ui::UserManagementAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UserManagementFeedbackTone {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExplorerInputMode {
    Browse,
    Address,
    Search,
    NewFolder,
    NewTextFile,
    Rename,
    RestoreDestination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExplorerOverlayMode {
    ContextMenu { anchor: CellPosition },
    Sort { anchor: CellPosition },
    Options,
    Properties,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum ExplorerPurpose {
    #[default]
    Browse,
    DiagnosticsLogs,
    EditorOpen,
    EditorSaveAs {
        snapshot: app::editor::SaveSnapshot,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LauncherPendingConfirmation {
    Launch {
        id: String,
        path: std::path::PathBuf,
        kind: LauncherExecutableKind,
    },
    Remove {
        ids: Vec<String>,
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LauncherDragState {
    pub(super) item_id: String,
    pub(super) target: Option<ui::LauncherDropTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClockCreateState {
    pub(super) input: String,
    pub(super) error: Option<String>,
    pub(super) focus: ui::ClockCreateDialogFocus,
}

impl Default for ClockCreateState {
    fn default() -> Self {
        Self {
            input: String::new(),
            error: None,
            focus: ui::ClockCreateDialogFocus::Input,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TimedClick {
    pub(super) target: Option<ShellComponent>,
    pub(super) coordinates: CellPosition,
    pub(super) at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSessionState {
    pub(super) home_mode: ShellHomeMode,
    pub(super) ascii_assets: ui::RuntimeAsciiAssets,
    pub(super) screen_stack: Vec<ShellScreen>,
    pub(super) storage_manager: Option<StorageManager>,
    pub(super) last_time_sync_utc: Option<DateTime<Utc>>,
    pub(super) clock_scheduler: Option<ClockScheduler>,
    pub(super) clock_selected_entry_id: Option<u64>,
    pub(super) clock_entry_window_start: usize,
    pub(super) clock_create_state: Option<ClockCreateState>,
    pub(super) clock_persist_pending: bool,
    pub(super) clock_pending_due_summary: Option<String>,
    pub(super) clock_profile_pending_sync: Option<ClockProfile>,
    pub(super) time_sync_attempted: bool,
    pub(super) time_sync_dialog_visible: bool,
    pub(super) time_sync_failure_message: Option<String>,
    /// Retains the build-policy-selected debug home across authentication.
    /// This is internal session state and is not configurable by process args.
    pub(super) debug_home_after_login: bool,
    pub(super) debug_policy: DebugPolicy,
    pub(super) login_users: Vec<ShellLoginUser>,
    pub(super) login_selected_user: usize,
    pub(super) login_user_window_start: usize,
    pub(super) login_username: String,
    pub(super) login_password: String,
    pub(super) login_idle_deadline: Instant,
    pub(super) login_password_visible_until: Option<Instant>,
    pub(super) setup_step: ui::SetupStep,
    pub(super) setup_selected_language_index: usize,
    pub(super) setup_selected_timezone_index: usize,
    pub(super) setup_admin_username: String,
    pub(super) setup_admin_password: String,
    pub(super) setup_admin_password_confirm: String,
    pub(super) setup_admin_password_hint: String,
    pub(super) setup_focused_field: ui::SetupField,
    pub(super) setup_timezone_window_start: usize,
    pub(super) setup_border_shape: storage::BorderShape,
    pub(super) setup_theme_color: storage::BorderColor,
    pub(super) setup_accent_color: storage::AccentColor,
    pub(super) setup_custom_color_target: Option<ui::SetupCustomColorTarget>,
    pub(super) setup_custom_color_input: String,
    pub(super) setup_custom_color_error: Option<String>,
    pub(super) bootstrap_username: String,
    pub(super) bootstrap_password: String,
    pub(super) user_management_selected: usize,
    pub(super) user_management_window_start: usize,
    pub(super) user_management_focus: UserManagementPageFocus,
    pub(super) user_management_message: Option<String>,
    pub(super) user_management_feedback_tone: UserManagementFeedbackTone,
    pub(super) user_management_mode: UserManagementMode,
    pub(super) selected_home_entry_index: usize,
    pub(super) settings_state: Option<SettingsState>,
    pub(super) settings_task_runtime: ShellSettingsTaskRuntime,
    pub(super) launcher_selected_index: usize,
    pub(super) launcher_view_mode: app::launcher::LauncherViewMode,
    pub(super) launcher_viewport_offset: usize,
    pub(super) launcher_pending_confirmation: Option<LauncherPendingConfirmation>,
    pub(super) launcher_drag: Option<LauncherDragState>,
    pub(super) launcher_task_runtime: Option<ShellLauncherTaskRuntime>,
    pub(super) launcher_refresh_request: Option<u64>,
    pub(super) explorer_input_mode: ExplorerInputMode,
    pub(super) explorer_input: String,
    pub(super) explorer_input_replace_all: bool,
    pub(super) explorer_overlay_mode: Option<ExplorerOverlayMode>,
    pub(super) explorer_overlay_selection: usize,
    pub(super) explorer_conflict_apply_to_remaining: bool,
    pub(super) explorer_purpose: ExplorerPurpose,
    pub(super) explorer_task_runtime: Option<ShellExplorerTaskRuntime>,
    pub(super) editor_task_runtime: ShellEditorTaskRuntime,
    pub(super) editor_load_state: Option<EditorLoadState>,
    pub(super) editor_save_state: Option<EditorSaveState>,
    pub(super) editor_document_generation: u64,
    pub(super) editor_rich_render_cache: Option<EditorRichRenderCache>,
    pub(super) editor_cursor_acceleration: Option<EditorCursorAccelerationState>,
    pub(super) editor_settings_dialog: Option<EditorSettingsDialogState>,
    pub(super) editor_focus: ui::EditorFocus,
    pub(super) editor_open_menu: Option<ui::EditorMenu>,
    pub(super) editor_selected_toolbar_action: Option<ui::EditorToolbarAction>,
    pub(super) editor_quick_menu_anchor: Option<CellPosition>,
    pub(super) editor_drag_anchor: Option<app::editor::EditorPosition>,
    pub(super) editor_table_column_widths: std::collections::BTreeMap<ui::NodeId, Vec<usize>>,
    pub(super) editor_table_resize: Option<EditorTableResizeState>,
    pub(super) editor_fingerprint: Option<DocumentFingerprint>,
    pub(super) editor_close_after_save: bool,
    pub(super) editor_open_after_save: bool,
    pub(super) editor_discard_for_open: bool,
    pub(super) editor_message: Option<String>,
    pub(super) editor_recovery_dirty_since: Option<Instant>,
    pub(super) editor_last_recovery_write: Option<Instant>,
    pub(super) editor_read_session: Option<EditorReadSession>,
    pub(super) diagnostics_task_runtime: Option<ShellDiagnosticsTaskRuntime>,
    pub(super) diagnostics_tab: ui::DiagnosticsTab,
    pub(super) diagnostics_selected_check: usize,
    pub(super) diagnostics_selected_log: usize,
    pub(super) diagnostics_selected_incident: usize,
    pub(super) diagnostics_list_window_start: usize,
    pub(super) diagnostics_list_window_is_explicit: bool,
    pub(super) diagnostics_scanning: bool,
    pub(super) diagnostics_rescan_pending: bool,
    pub(super) diagnostics_repair_preview: Vec<app::diagnostics::DiagnosticsRepairAction>,
    pub(super) diagnostics_repair_selected: usize,
    pub(super) diagnostics_repair_scroll_offset: usize,
    pub(super) diagnostics_repair_confirm_selected: bool,
    pub(super) diagnostics_feedback: Option<String>,
    pub(super) diagnostics_restart_required: bool,
    pub(super) terminal_size: (u16, u16),
    pub(super) terminal_flags: ShellTerminalFlags,
    pub(super) focused_component: ShellComponent,
    pub(super) hovered_component: Option<ShellComponent>,
    pub(super) active_popup: Option<ShellPopup>,
    pub(super) hit_map: ShellHitMap,
    pub(super) hit_map_generation: u64,
    pub(super) tick_count: u64,
    pub(super) notification_bindings: NotificationBindings,
    pub(super) modal_focus_context: Option<ModalFocusContext>,
    pub(super) modal_focus_prepared_for_follow_up: bool,
    pub(super) notification_pointer_capture: Option<NotificationPointerCapture>,
    pub(super) pending_notification_commands: VecDeque<ShellCommand>,
    pub(super) error_message: Option<String>,
    pub(super) latest_watchdog_report: Option<std::path::PathBuf>,
    pub(super) latest_watchdog_summary: Option<String>,
    pub(super) shutdown_requested: bool,
    pub(super) return_to_lockscreen_requested: bool,
    pub(super) last_command: Option<ShellCommand>,
    pub(super) last_routed_target: Option<RoutedTarget>,
    pub(super) last_key_event: Option<String>,
    pub(super) last_mouse_event: Option<String>,
    pub(super) last_resize_event: Option<String>,
    pub(super) mouse_coordinates: Option<(u16, u16)>,
    pub(super) mouse_scroll_direction: Option<String>,
    pub(super) mouse_drag_direction: Option<String>,
    pub(super) platform_capability_summary: String,
    pub(super) last_click: Option<TimedClick>,
    pub(super) drag_tracker: Option<DragTracker>,
    pub(super) scrollbar_drag: Option<ScrollbarDragState>,
}
/// Coordinates the UI session with the UI-independent application state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSession {
    pub(super) app: app::AppState,
    pub(super) ui: UiSessionState,
}

impl std::ops::Deref for ShellSession {
    type Target = UiSessionState;

    fn deref(&self) -> &Self::Target {
        &self.ui
    }
}

impl std::ops::DerefMut for ShellSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ui
    }
}

impl ShellSession {
    pub fn app_state(&self) -> &app::AppState {
        &self.app
    }

    pub fn ui_state(&self) -> &UiSessionState {
        &self.ui
    }
}
