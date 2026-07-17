#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModalFocusContext {
    screen: ShellScreen,
    component: ShellComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NotificationPointerCapture {
    notification_id: u64,
    action_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DragTracker {
    button: PointerButton,
    origin_coordinates: CellPosition,
    last_coordinates: CellPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EditorTableResizeState {
    table_id: tundra_ui::NodeId,
    column_index: usize,
    start_x: u16,
    start_width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorReloadPolicy {
    DiagnosticsLog {
        path: std::path::PathBuf,
        max_bytes: usize,
    },
    DiagnosticsReport {
        path: std::path::PathBuf,
    },
}

impl EditorReloadPolicy {
    fn path(&self) -> &std::path::Path {
        match self {
            Self::DiagnosticsLog { path, .. } | Self::DiagnosticsReport { path } => path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditorDiagnosticSession {
    reload: EditorReloadPolicy,
    start_byte: u64,
    total_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserManagementFormField {
    Username,
    DisplayName,
    Role,
    Password,
    Submit,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserManagementCreateForm {
    username: String,
    display_name: String,
    password: String,
    role: UserRole,
    focused_field: UserManagementFormField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserManagementInfoForm {
    username: String,
    display_name: String,
    focused_field: UserManagementFormField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserManagementPasswordForm {
    username: String,
    password: String,
    focused_field: UserManagementFormField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UserManagementMode {
    Browse,
    Create(UserManagementCreateForm),
    EditInfo(UserManagementInfoForm),
    Password(UserManagementPasswordForm),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserManagementPageFocus {
    UserList,
    Action(tundra_ui::UserManagementAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserManagementFeedbackTone {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExplorerInputMode {
    Browse,
    Address,
    Search,
    NewFolder,
    NewTextFile,
    Rename,
    RestoreDestination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExplorerOverlayMode {
    ContextMenu { anchor: CellPosition },
    Sort { anchor: CellPosition },
    Options,
    Properties,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum ExplorerPurpose {
    #[default]
    Browse,
    EditorOpen,
    EditorSaveAs {
        snapshot: tundra_apps::editor::SaveSnapshot,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClockCreateState {
    input: String,
    error: Option<String>,
    focus: tundra_ui::ClockCreateDialogFocus,
}

impl Default for ClockCreateState {
    fn default() -> Self {
        Self {
            input: String::new(),
            error: None,
            focus: tundra_ui::ClockCreateDialogFocus::Input,
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimedClick {
    target: Option<ShellComponent>,
    coordinates: CellPosition,
    at: Instant,
}

#[derive(Debug, Clone)]
struct ShellNetworkClock(NetworkClock);

impl ShellNetworkClock {
    fn new(timezone_id: Option<String>) -> Self {
        Self(NetworkClock::new(timezone_id))
    }

    fn apply_sync(&mut self, result: TimeSyncResult) {
        self.0.apply_sync(result);
    }

    fn current(&self) -> tundra_weathr::network_clock::ClockDisplay {
        self.0.current()
    }

    fn snapshot(&self) -> tundra_weathr::network_clock::ClockSnapshot {
        self.0.snapshot()
    }
}

impl PartialEq for ShellNetworkClock {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ShellNetworkClock {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellState {
    home_mode: ShellHomeMode,
    launch_target: ShellLaunchTarget,
    ascii_assets: tundra_ui::RuntimeAsciiAssets,
    screen_stack: Vec<ShellScreen>,
    storage_manager: Option<StorageManager>,
    network_clock: ShellNetworkClock,
    clock_timezone_id: Option<String>,
    last_time_sync_utc: Option<DateTime<Utc>>,
    clock_scheduler: Option<ClockScheduler>,
    clock_selected_entry_id: Option<u64>,
    clock_entry_window_start: usize,
    clock_create_state: Option<ClockCreateState>,
    clock_persist_pending: bool,
    clock_pending_due_summary: Option<String>,
    clock_profile_pending_sync: Option<ClockProfile>,
    time_sync_attempted: bool,
    time_sync_dialog_visible: bool,
    time_sync_failure_message: Option<String>,
    auth_session: Option<AuthSession>,
    requested_debug_mode: bool,
    debug_policy: DebugPolicy,
    login_users: Vec<ShellLoginUser>,
    login_selected_user: usize,
    login_user_window_start: usize,
    login_username: String,
    login_password: String,
    login_idle_deadline: Instant,
    login_password_visible_until: Option<Instant>,
    setup_step: tundra_ui::SetupStep,
    setup_selected_language_index: usize,
    setup_selected_timezone_index: usize,
    setup_admin_username: String,
    setup_admin_password: String,
    setup_admin_password_confirm: String,
    setup_admin_password_hint: String,
    setup_focused_field: tundra_ui::SetupField,
    setup_timezone_window_start: usize,
    bootstrap_username: String,
    bootstrap_password: String,
    user_management_users: Vec<UserAccount>,
    user_management_selected: usize,
    user_management_window_start: usize,
    user_management_focus: UserManagementPageFocus,
    user_management_message: Option<String>,
    user_management_feedback_tone: UserManagementFeedbackTone,
    user_management_mode: UserManagementMode,
    selected_home_entry_index: usize,
    explorer_state: Option<ExplorerState>,
    explorer_input_mode: ExplorerInputMode,
    explorer_input: String,
    explorer_input_replace_all: bool,
    explorer_overlay_mode: Option<ExplorerOverlayMode>,
    explorer_overlay_selection: usize,
    explorer_conflict_apply_to_remaining: bool,
    explorer_purpose: ExplorerPurpose,
    explorer_task_runtime: Option<ShellExplorerTaskRuntime>,
    editor_state: Option<EditorState>,
    editor_focus: tundra_ui::EditorFocus,
    editor_open_menu: Option<tundra_ui::EditorMenu>,
    editor_selected_toolbar_action: Option<tundra_ui::EditorToolbarAction>,
    editor_quick_menu_anchor: Option<CellPosition>,
    editor_drag_anchor: Option<tundra_apps::editor::EditorPosition>,
    editor_table_column_widths: std::collections::BTreeMap<tundra_ui::NodeId, Vec<usize>>,
    editor_table_resize: Option<EditorTableResizeState>,
    editor_fingerprint: Option<DocumentFingerprint>,
    editor_close_after_save: bool,
    editor_open_after_save: bool,
    editor_discard_for_open: bool,
    editor_message: Option<String>,
    editor_recovery_dirty_since: Option<Instant>,
    editor_last_recovery_write: Option<Instant>,
    editor_diagnostic_session: Option<EditorDiagnosticSession>,
    diagnostics_task_runtime: Option<ShellDiagnosticsTaskRuntime>,
    diagnostics_snapshot: Option<tundra_apps::diagnostics::DiagnosticsSnapshot>,
    diagnostics_tab: tundra_ui::DiagnosticsTab,
    diagnostics_selected_check: usize,
    diagnostics_selected_log: usize,
    diagnostics_selected_incident: usize,
    diagnostics_list_window_start: usize,
    diagnostics_scanning: bool,
    diagnostics_rescan_pending: bool,
    diagnostics_repair_preview: Vec<tundra_apps::diagnostics::DiagnosticsRepairAction>,
    diagnostics_repair_selected: usize,
    diagnostics_repair_scroll_offset: usize,
    diagnostics_repair_confirm_selected: bool,
    diagnostics_feedback: Option<String>,
    diagnostics_restart_required: bool,
    terminal_size: (u16, u16),
    terminal_flags: ShellTerminalFlags,
    focused_component: ShellComponent,
    hovered_component: Option<ShellComponent>,
    active_popup: Option<ShellPopup>,
    hit_map: ShellHitMap,
    hit_map_generation: u64,
    tick_count: u64,
    notifications: NotificationCenter,
    modal_focus_context: Option<ModalFocusContext>,
    modal_focus_prepared_for_follow_up: bool,
    notification_pointer_capture: Option<NotificationPointerCapture>,
    pending_notification_commands: VecDeque<ShellCommand>,
    error_message: Option<String>,
    latest_watchdog_report: Option<std::path::PathBuf>,
    latest_watchdog_summary: Option<String>,
    shutdown_requested: bool,
    return_to_lockscreen_requested: bool,
    last_command: Option<ShellCommand>,
    last_routed_target: Option<RoutedTarget>,
    last_key_event: Option<String>,
    last_mouse_event: Option<String>,
    last_resize_event: Option<String>,
    mouse_coordinates: Option<(u16, u16)>,
    mouse_scroll_direction: Option<String>,
    mouse_drag_direction: Option<String>,
    platform_capability_summary: String,
    last_click: Option<TimedClick>,
    drag_tracker: Option<DragTracker>,
}
