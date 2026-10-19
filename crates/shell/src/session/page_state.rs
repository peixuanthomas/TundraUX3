//! UI data belongs to a page instance, even while another page covers it.
use super::*;
use app::PageInstances;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommonPageState {
    pub(super) notification_message_scroll: usize,
    pub(super) overlay_manager: ShellOverlayManager,
    pub(super) table_sorts: std::collections::BTreeMap<String, ui::TableSort>,
    pub(super) page_touch: PageTouchState,
    pub(super) focused_component: ShellComponent,
    pub(super) active_popup: Option<ShellPopup>,
    pub(super) error_message: Option<i18n::LocalizedText>,
}
impl Default for CommonPageState {
    fn default() -> Self {
        Self {
            notification_message_scroll: 0,
            overlay_manager: ShellOverlayManager::default(),
            table_sorts: Default::default(),
            page_touch: PageTouchState::default(),
            focused_component: ShellComponent::Home,
            active_popup: None,
            error_message: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HomePageState {
    pub(super) selected_home_entry_index: usize,
    pub(super) home_viewport_offset: usize,
}
impl Default for HomePageState {
    fn default() -> Self {
        Self {
            selected_home_entry_index: 0,
            home_viewport_offset: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClockPageState {
    pub(super) clock_selected_entry_id: Option<u64>,
    pub(super) clock_entry_window_start: usize,
    pub(super) clock_create_state: Option<ClockCreateState>,
}
impl Default for ClockPageState {
    fn default() -> Self {
        Self {
            clock_selected_entry_id: None,
            clock_entry_window_start: 0,
            clock_create_state: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LoginPageState {
    pub(super) login_selected_user: usize,
    pub(super) login_user_window_start: usize,
    pub(super) login_username: String,
    pub(super) login_password: String,
    pub(super) login_idle_deadline: Instant,
    pub(super) login_password_visible_until: Option<Instant>,
}
impl Default for LoginPageState {
    fn default() -> Self {
        Self {
            login_selected_user: 0,
            login_user_window_start: 0,
            login_username: String::new(),
            login_password: String::new(),
            login_idle_deadline: Instant::now() + LOGIN_IDLE_TIMEOUT,
            login_password_visible_until: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetupPageState {
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
    pub(super) setup_custom_color_error: Option<i18n::LocalizedText>,
}
impl Default for SetupPageState {
    fn default() -> Self {
        Self {
            setup_step: ui::SetupStep::Language,
            setup_selected_language_index: 0,
            setup_selected_timezone_index: 0,
            setup_admin_username: String::new(),
            setup_admin_password: String::new(),
            setup_admin_password_confirm: String::new(),
            setup_admin_password_hint: String::new(),
            setup_focused_field: ui::SetupField::LanguageList,
            setup_timezone_window_start: 0,
            setup_border_shape: storage::AppearanceConfig::default().border_shape,
            setup_theme_color: storage::AppearanceConfig::default().border_color,
            setup_accent_color: storage::AppearanceConfig::default().accent_color,
            setup_custom_color_target: None,
            setup_custom_color_input: String::new(),
            setup_custom_color_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BootstrapPageState {
    pub(super) bootstrap_username: String,
    pub(super) bootstrap_password: String,
}
impl Default for BootstrapPageState {
    fn default() -> Self {
        Self {
            bootstrap_username: String::new(),
            bootstrap_password: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UserManagementPageState {
    pub(super) user_management_selected: usize,
    pub(super) user_management_window_start: usize,
    pub(super) user_management_focus: UserManagementPageFocus,
    pub(super) user_management_message: Option<i18n::LocalizedText>,
    pub(super) user_management_feedback_tone: UserManagementFeedbackTone,
    pub(super) user_management_mode: UserManagementMode,
    pub(super) user_management_job: Option<UserManagementJob>,
}
impl Default for UserManagementPageState {
    fn default() -> Self {
        Self {
            user_management_selected: 0,
            user_management_window_start: 0,
            user_management_focus: UserManagementPageFocus::UserList,
            user_management_message: None,
            user_management_feedback_tone: UserManagementFeedbackTone::Info,
            user_management_mode: UserManagementMode::Browse,
            user_management_job: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsPageState {
    pub(super) settings_state: Option<SettingsState>,
}
impl Default for SettingsPageState {
    fn default() -> Self {
        Self {
            settings_state: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SystemStatusPageState {
    pub(super) system_status_route: ui::SystemStatusRoute,
    pub(super) system_status_tab: ui::SystemStatusTab,
    pub(super) system_status_selected_widget: Option<storage::SystemStatusWidgetKind>,
    pub(super) system_status_dashboard_focus: ui::SystemStatusDashboardFocus,
    pub(super) system_status_dashboard_scroll_row: u16,
    pub(super) system_status_dashboard_draft: Option<storage::SystemStatusDashboardConfig>,
    pub(super) system_status_add_picker: Option<SystemStatusAddPickerState>,
    pub(super) system_status_size_picker: Option<SystemStatusSizePickerState>,
    pub(super) system_status_discard_dialog: bool,
    pub(super) system_status_discard_confirm_selected: bool,
    pub(super) system_status_dashboard_feedback: Option<i18n::LocalizedText>,
    pub(super) system_status_widget_drag: Option<SystemStatusWidgetDragState>,
    pub(super) system_status_process_sort: ui::SystemStatusProcessSort,
    pub(super) system_status_selected_row: usize,
    pub(super) system_status_scroll_offset: usize,
    pub(super) system_status_refresh_requested_revision: Option<u64>,
}
impl Default for SystemStatusPageState {
    fn default() -> Self {
        Self {
            system_status_route: ui::SystemStatusRoute::Dashboard,
            system_status_tab: ui::SystemStatusTab::Overview,
            system_status_selected_widget: None,
            system_status_dashboard_focus: ui::SystemStatusDashboardFocus::default(),
            system_status_dashboard_scroll_row: 0,
            system_status_dashboard_draft: None,
            system_status_add_picker: None,
            system_status_size_picker: None,
            system_status_discard_dialog: false,
            system_status_discard_confirm_selected: true,
            system_status_dashboard_feedback: None,
            system_status_widget_drag: None,
            system_status_process_sort: ui::SystemStatusProcessSort::default(),
            system_status_selected_row: 0,
            system_status_scroll_offset: 0,
            system_status_refresh_requested_revision: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LauncherPageState {
    pub(super) launcher_selected_index: usize,
    pub(super) launcher_view_mode: app::launcher::LauncherViewMode,
    pub(super) launcher_viewport_offset: usize,
    pub(super) launcher_confirm_selected: bool,
    pub(super) launcher_pending_confirmation: Option<LauncherPendingConfirmation>,
    pub(super) launcher_drag: Option<LauncherDragState>,
    pub(super) launcher_refresh_request: Option<u64>,
}
impl Default for LauncherPageState {
    fn default() -> Self {
        Self {
            launcher_selected_index: 0,
            launcher_view_mode: app::launcher::LauncherViewMode::LargeIcons,
            launcher_viewport_offset: 0,
            launcher_confirm_selected: true,
            launcher_pending_confirmation: None,
            launcher_drag: None,
            launcher_refresh_request: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandLinePageState {
    pub(super) command_line_start_directory: Option<PathBuf>,
}
impl Default for CommandLinePageState {
    fn default() -> Self {
        Self {
            command_line_start_directory: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExplorerPageState {
    pub(super) explorer_input_mode: ExplorerInputMode,
    pub(super) explorer_input: String,
    pub(super) explorer_address_cursor: usize,
    pub(super) explorer_address_selection_anchor: Option<usize>,
    pub(super) explorer_input_replace_all: bool,
    pub(super) explorer_overlay_mode: Option<ExplorerOverlayMode>,
    pub(super) explorer_overlay_selection: usize,
    pub(super) explorer_conflict_apply_to_remaining: bool,
    pub(super) explorer_purpose: ExplorerPurpose,
    pub(super) editor_picker_owner: Option<PageInstanceId>,
    pub(super) editor_picker_save_submitted: bool,
    pub(super) explorer_locations_scroll: Option<usize>,
}
impl Default for ExplorerPageState {
    fn default() -> Self {
        Self {
            explorer_input_mode: ExplorerInputMode::Browse,
            explorer_input: String::new(),
            explorer_address_cursor: 0,
            explorer_address_selection_anchor: None,
            explorer_input_replace_all: false,
            explorer_overlay_mode: None,
            explorer_overlay_selection: 0,
            explorer_conflict_apply_to_remaining: false,
            explorer_purpose: ExplorerPurpose::Browse,
            editor_picker_owner: None,
            editor_picker_save_submitted: false,
            explorer_locations_scroll: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EditorPageState {
    pub(super) editor_selected_menu: ui::EditorMenu,
    pub(super) editor_menu_index: usize,
    pub(super) editor_indent_tabs: bool,
    pub(super) editor_indent_width: usize,
    pub(super) editor_language: Option<String>,
    pub(super) editor_exit_group: bool,
    pub(super) editor_search_bar: Option<ui::EditorSearchBar>,
    pub(super) editor_search_memory: Option<ui::EditorSearchBar>,
    pub(super) editor_inline_replace_request: Option<(u64, u64)>,
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
    pub(super) editor_message: Option<i18n::LocalizedText>,
    pub(super) editor_recovery_dirty_since: Option<Instant>,
    pub(super) editor_last_recovery_write: Option<Instant>,
    pub(super) editor_read_session: Option<EditorReadSession>,
    pub(super) editor_config: ConfigEditorState,
    pub(super) editor_soft_wrap: bool,
    pub(super) editor_vim_mode: ui::EditorVimMode,
    pub(super) editor_vim_pending: Option<char>,
    pub(super) editor_vim_yank: Option<(String, bool)>,
    pub(super) editor_search_fields: Vec<platform::management::ManagementField>,
    pub(super) editor_preview_task: app::editor::search::PreviewTask,
    pub(super) editor_preview_request:
        Option<(u64, u64, Vec<platform::management::ManagementField>)>,
    pub(super) editor_replace_preview: Option<(u64, u64, Vec<app::editor::search::Replacement>)>,
    pub(super) editor_recovery_id: app::editor_recovery::RecoveryId,
    pub(super) editor_recovery_lease: Option<app::editor_recovery::EditorRecoveryLease>,
}
impl Default for EditorPageState {
    fn default() -> Self {
        Self {
            editor_selected_menu: ui::EditorMenu::File,
            editor_menu_index: 0,
            editor_indent_tabs: false,
            editor_indent_width: 4,
            editor_language: None,
            editor_exit_group: false,
            editor_search_bar: None,
            editor_search_memory: None,
            editor_inline_replace_request: None,
            editor_load_state: None,
            editor_save_state: None,
            editor_document_generation: 0,
            editor_rich_render_cache: None,
            editor_cursor_acceleration: None,
            editor_settings_dialog: None,
            editor_focus: ui::EditorFocus::Canvas,
            editor_open_menu: None,
            editor_selected_toolbar_action: None,
            editor_quick_menu_anchor: None,
            editor_drag_anchor: None,
            editor_table_column_widths: std::collections::BTreeMap::new(),
            editor_table_resize: None,
            editor_fingerprint: None,
            editor_close_after_save: false,
            editor_open_after_save: false,
            editor_discard_for_open: false,
            editor_message: None,
            editor_recovery_dirty_since: None,
            editor_last_recovery_write: None,
            editor_read_session: None,
            editor_config: ConfigEditorState::default(),
            editor_soft_wrap: false,
            editor_vim_mode: ui::EditorVimMode::Normal,
            editor_vim_pending: None,
            editor_vim_yank: None,
            editor_search_fields: Vec::new(),
            editor_replace_preview: None,
            editor_preview_task: Default::default(),
            editor_preview_request: None,
            editor_recovery_id: Default::default(),
            editor_recovery_lease: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiagnosticsPageState {
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
    pub(super) diagnostics_repair_log_context: Option<runtime_log::LogContext>,
    pub(super) diagnostics_repair_scroll_offset: usize,
    pub(super) diagnostics_repair_confirm_selected: bool,
    pub(super) diagnostics_feedback: Option<i18n::LocalizedText>,
    pub(super) diagnostics_detail_scroll: usize,
    pub(super) diagnostics_detail_drag: Option<u16>,
}
impl Default for DiagnosticsPageState {
    fn default() -> Self {
        Self {
            diagnostics_tab: ui::DiagnosticsTab::Health,
            diagnostics_selected_check: 0,
            diagnostics_selected_log: 0,
            diagnostics_selected_incident: 0,
            diagnostics_list_window_start: 0,
            diagnostics_list_window_is_explicit: false,
            diagnostics_scanning: false,
            diagnostics_rescan_pending: false,
            diagnostics_repair_preview: Vec::new(),
            diagnostics_repair_selected: 0,
            diagnostics_repair_log_context: None,
            diagnostics_repair_scroll_offset: 0,
            diagnostics_repair_confirm_selected: true,
            diagnostics_feedback: None,
            diagnostics_detail_scroll: 0,
            diagnostics_detail_drag: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LogsPageState {
    pub(super) logs_state: LogsUiState,
}
impl Default for LogsPageState {
    fn default() -> Self {
        Self {
            logs_state: LogsUiState::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManagementPageState {
    pub(super) management_state: ManagementState,
}
impl Default for ManagementPageState {
    fn default() -> Self {
        Self {
            management_state: ManagementState::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ShellPages {
    pub(super) screens: BTreeMap<PageInstanceId, ShellScreen>,
    pub(super) common: PageInstances<CommonPageState>,
    pub(super) home: PageInstances<HomePageState>,
    pub(super) clock: PageInstances<ClockPageState>,
    pub(super) login: PageInstances<LoginPageState>,
    pub(super) setup: PageInstances<SetupPageState>,
    pub(super) bootstrap: PageInstances<BootstrapPageState>,
    pub(super) user_management: PageInstances<UserManagementPageState>,
    pub(super) settings: PageInstances<SettingsPageState>,
    pub(super) system_status: PageInstances<SystemStatusPageState>,
    pub(super) launcher: PageInstances<LauncherPageState>,
    pub(super) command_line: PageInstances<CommandLinePageState>,
    pub(super) explorer: PageInstances<ExplorerPageState>,
    pub(super) editor: PageInstances<EditorPageState>,
    pub(super) diagnostics: PageInstances<DiagnosticsPageState>,
    pub(super) logs: PageInstances<LogsPageState>,
    pub(super) management: PageInstances<ManagementPageState>,
}

impl ShellPages {
    pub(super) fn create(&mut self, id: PageInstanceId, screen: ShellScreen) {
        self.screens.insert(id, screen);
        self.common.create(id);
        self.common.select(Some(id));
        self.common.focused_component = super::navigation::screen_focus(screen);
        match screen {
            ShellScreen::FirstRunSetup => {
                self.setup.create(id);
                self.setup.select(Some(id));
            }
            ShellScreen::BootstrapAdmin => {
                self.bootstrap.create(id);
                self.bootstrap.select(Some(id));
            }
            ShellScreen::Login => {
                self.login.create(id);
                self.login.select(Some(id));
            }
            ShellScreen::Home => {
                self.home.create(id);
                self.home.select(Some(id));
            }
            ShellScreen::Clock => {
                self.clock.create(id);
                self.clock.select(Some(id));
            }
            ShellScreen::Diagnostics => {
                self.diagnostics.create(id);
                self.diagnostics.select(Some(id));
            }
            ShellScreen::Logs => {
                self.logs.create(id);
                self.logs.select(Some(id));
            }
            ShellScreen::Management => {
                self.management.create(id);
                self.management.select(Some(id));
            }
            ShellScreen::SystemStatus => {
                self.system_status.create(id);
                self.system_status.select(Some(id));
                self.diagnostics.create(id);
                self.diagnostics.select(Some(id));
            }
            ShellScreen::Explorer => {
                self.explorer.create(id);
                self.explorer.select(Some(id));
            }
            ShellScreen::Launcher => {
                self.launcher.create(id);
                self.launcher.select(Some(id));
            }
            ShellScreen::CommandLine => {
                self.command_line.create(id);
                self.command_line.select(Some(id));
            }
            ShellScreen::Editor => {
                self.editor.create(id);
                self.editor.select(Some(id));
                self.management.create(id);
                self.management.select(Some(id));
                self.logs.create(id);
                self.logs.select(Some(id));
            }
            ShellScreen::Settings => {
                self.settings.create(id);
                self.settings.select(Some(id));
            }
            ShellScreen::UserManagement => {
                self.user_management.create(id);
                self.user_management.select(Some(id));
            }
            ShellScreen::ExitConfirm => {}
        }
    }
    pub(super) fn select(&mut self, id: Option<PageInstanceId>, screen: ShellScreen) {
        match screen {
            ShellScreen::FirstRunSetup => self.setup.select(id),
            ShellScreen::BootstrapAdmin => self.bootstrap.select(id),
            ShellScreen::Login => self.login.select(id),
            ShellScreen::Home => self.home.select(id),
            ShellScreen::Clock => self.clock.select(id),
            ShellScreen::Diagnostics => self.diagnostics.select(id),
            ShellScreen::Logs => self.logs.select(id),
            ShellScreen::Management => self.management.select(id),
            ShellScreen::SystemStatus => self.system_status.select(id),
            ShellScreen::Explorer => self.explorer.select(id),
            ShellScreen::Launcher => self.launcher.select(id),
            ShellScreen::CommandLine => self.command_line.select(id),
            ShellScreen::Editor => {
                self.editor.select(id);
                self.management.select(id);
                self.logs.select(id);
            }
            ShellScreen::Settings => self.settings.select(id),
            ShellScreen::UserManagement => self.user_management.select(id),
            ShellScreen::ExitConfirm => {}
        }
    }
    pub(super) fn remove(&mut self, id: PageInstanceId) {
        self.screens.remove(&id);
        self.common.remove(id);
        self.home.remove(id);
        self.clock.remove(id);
        self.login.remove(id);
        self.setup.remove(id);
        self.bootstrap.remove(id);
        self.user_management.remove(id);
        self.settings.remove(id);
        self.system_status.remove(id);
        self.launcher.remove(id);
        self.command_line.remove(id);
        self.explorer.remove(id);
        self.editor.remove(id);
        self.diagnostics.remove(id);
        self.logs.remove(id);
        self.management.remove(id);
    }
}

#[derive(Clone, Copy)]
pub(super) struct PageSelection {
    pub(super) common: Option<PageInstanceId>,
    pub(super) home: Option<PageInstanceId>,
    pub(super) clock: Option<PageInstanceId>,
    pub(super) login: Option<PageInstanceId>,
    pub(super) setup: Option<PageInstanceId>,
    pub(super) bootstrap: Option<PageInstanceId>,
    pub(super) user_management: Option<PageInstanceId>,
    pub(super) settings: Option<PageInstanceId>,
    pub(super) system_status: Option<PageInstanceId>,
    pub(super) launcher: Option<PageInstanceId>,
    pub(super) command_line: Option<PageInstanceId>,
    pub(super) explorer: Option<PageInstanceId>,
    pub(super) editor: Option<PageInstanceId>,
    pub(super) diagnostics: Option<PageInstanceId>,
    pub(super) logs: Option<PageInstanceId>,
    pub(super) management: Option<PageInstanceId>,
}

impl ShellPages {
    pub(super) fn selection(&self) -> PageSelection {
        PageSelection {
            common: self.common.selected_id(),
            home: self.home.selected_id(),
            clock: self.clock.selected_id(),
            login: self.login.selected_id(),
            setup: self.setup.selected_id(),
            bootstrap: self.bootstrap.selected_id(),
            user_management: self.user_management.selected_id(),
            settings: self.settings.selected_id(),
            system_status: self.system_status.selected_id(),
            launcher: self.launcher.selected_id(),
            command_line: self.command_line.selected_id(),
            explorer: self.explorer.selected_id(),
            editor: self.editor.selected_id(),
            diagnostics: self.diagnostics.selected_id(),
            logs: self.logs.selected_id(),
            management: self.management.selected_id(),
        }
    }
    pub(super) fn restore_selection(&mut self, selected: PageSelection) {
        self.common
            .select(selected.common.filter(|id| self.common.get(*id).is_some()));
        self.home
            .select(selected.home.filter(|id| self.home.get(*id).is_some()));
        self.clock
            .select(selected.clock.filter(|id| self.clock.get(*id).is_some()));
        self.login
            .select(selected.login.filter(|id| self.login.get(*id).is_some()));
        self.setup
            .select(selected.setup.filter(|id| self.setup.get(*id).is_some()));
        self.bootstrap.select(
            selected
                .bootstrap
                .filter(|id| self.bootstrap.get(*id).is_some()),
        );
        self.user_management.select(
            selected
                .user_management
                .filter(|id| self.user_management.get(*id).is_some()),
        );
        self.settings.select(
            selected
                .settings
                .filter(|id| self.settings.get(*id).is_some()),
        );
        self.system_status.select(
            selected
                .system_status
                .filter(|id| self.system_status.get(*id).is_some()),
        );
        self.launcher.select(
            selected
                .launcher
                .filter(|id| self.launcher.get(*id).is_some()),
        );
        self.command_line.select(
            selected
                .command_line
                .filter(|id| self.command_line.get(*id).is_some()),
        );
        self.explorer.select(
            selected
                .explorer
                .filter(|id| self.explorer.get(*id).is_some()),
        );
        self.editor
            .select(selected.editor.filter(|id| self.editor.get(*id).is_some()));
        self.diagnostics.select(
            selected
                .diagnostics
                .filter(|id| self.diagnostics.get(*id).is_some()),
        );
        self.logs
            .select(selected.logs.filter(|id| self.logs.get(*id).is_some()));
        self.management.select(
            selected
                .management
                .filter(|id| self.management.get(*id).is_some()),
        );
    }
}
