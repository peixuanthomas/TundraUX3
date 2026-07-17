use crate::home_icons::{AssetError, HomeIcon, HomeIconCatalog, RuntimeAsciiAssets};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeDisplayMode {
    Debug,
    User,
    Auth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusViewModel {
    pub status: String,
    pub toast: Option<String>,
    pub error: Option<String>,
    pub alert_tone: NotificationTone,
    pub time_button_label: Option<String>,
    pub time_button_selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Status,
    Toast,
    Alert,
    Modal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationTone {
    Info,
    Success,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationActionViewModel {
    pub id: String,
    pub label: String,
    pub shortcut: Option<String>,
    pub selected: bool,
}

impl NotificationActionViewModel {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            shortcut: None,
            selected: false,
        }
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationViewModel {
    pub id: String,
    pub level: NotificationLevel,
    pub tone: NotificationTone,
    pub title: String,
    pub message: String,
    pub actions: Vec<NotificationActionViewModel>,
}

impl NotificationViewModel {
    pub fn new(
        id: impl Into<String>,
        level: NotificationLevel,
        tone: NotificationTone,
        title: impl Into<String>,
        message: impl Into<String>,
        actions: Vec<NotificationActionViewModel>,
    ) -> Self {
        Self {
            id: id.into(),
            level,
            tone,
            title: title.into(),
            message: message.into(),
            actions,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClockCreateDialogFocus {
    #[default]
    Input,
    CreateAlarm,
    CreateCountdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockEntryViewModel {
    pub id: u64,
    pub label: String,
    pub strong: bool,
}

impl ClockEntryViewModel {
    pub fn new(id: u64, label: impl Into<String>, strong: bool) -> Self {
        Self {
            id,
            label: label.into(),
            strong,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockCreateDialogViewModel {
    pub input: String,
    pub error: Option<String>,
    pub focus: ClockCreateDialogFocus,
}

impl ClockCreateDialogViewModel {
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
            error: None,
            focus: ClockCreateDialogFocus::Input,
        }
    }
}

impl Default for ClockCreateDialogViewModel {
    fn default() -> Self {
        Self::new("")
    }
}

/// Physical height-to-width ratio of one terminal character cell.
///
/// Terminals may omit pixel dimensions, so the conventional 2:1 character
/// cell is used as a fallback. Keeping this value explicit lets circular
/// graphics compensate for fonts and line heights which use another ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalCellAspectRatio(f64);

impl TerminalCellAspectRatio {
    pub const FALLBACK: Self = Self(2.0);

    pub fn new(height_to_width: f64) -> Option<Self> {
        (height_to_width.is_finite() && height_to_width > 0.0).then_some(Self(height_to_width))
    }

    /// Derives the average cell ratio from terminal character and pixel sizes.
    ///
    /// Crossterm reports zero pixel dimensions on terminals which do not
    /// support them; those and all other invalid dimensions use the fallback.
    pub fn from_window_size(columns: u16, rows: u16, pixel_width: u16, pixel_height: u16) -> Self {
        if columns == 0 || rows == 0 || pixel_width == 0 || pixel_height == 0 {
            return Self::FALLBACK;
        }

        let height_to_width = (f64::from(pixel_height) * f64::from(columns))
            / (f64::from(pixel_width) * f64::from(rows));
        Self::new(height_to_width).unwrap_or(Self::FALLBACK)
    }

    pub fn height_to_width(self) -> f64 {
        self.0
    }
}

impl Eq for TerminalCellAspectRatio {}

impl Default for TerminalCellAspectRatio {
    fn default() -> Self {
        Self::FALLBACK
    }
}

#[derive(Debug, Clone)]
pub struct ClockViewModel {
    pub date: String,
    pub digital_time: String,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub alarms: Vec<ClockEntryViewModel>,
    pub countdowns: Vec<ClockEntryViewModel>,
    pub selected_entry_id: Option<u64>,
    /// Offset into the flattened `alarms` then `countdowns` display order.
    pub entry_window_start: usize,
    pub create_dialog: Option<ClockCreateDialogViewModel>,
    read_only: bool,
    ascii_assets: Option<RuntimeAsciiAssets>,
    terminal_cell_aspect_ratio: TerminalCellAspectRatio,
}

impl PartialEq for ClockViewModel {
    fn eq(&self, other: &Self) -> bool {
        self.date == other.date
            && self.digital_time == other.digital_time
            && self.hour == other.hour
            && self.minute == other.minute
            && self.second == other.second
            && self.alarms == other.alarms
            && self.countdowns == other.countdowns
            && self.selected_entry_id == other.selected_entry_id
            && self.entry_window_start == other.entry_window_start
            && self.create_dialog == other.create_dialog
            && self.read_only == other.read_only
            && self.terminal_cell_aspect_ratio == other.terminal_cell_aspect_ratio
    }
}

impl Eq for ClockViewModel {}

impl ClockViewModel {
    /// Compatibility constructor for callers which only have a formatted time label.
    /// New code should prefer [`ClockViewModel::at`].
    pub fn new(current_time: impl Into<String>) -> Self {
        let current_time = current_time.into();
        let mut date = String::new();
        let mut digital_time = current_time.clone();

        for part in current_time.split_whitespace() {
            if date.is_empty() && part.contains('-') {
                date = part.to_string();
            }
            if part.contains(':') {
                digital_time = part.to_string();
                break;
            }
        }

        let mut time_parts = digital_time
            .split(':')
            .filter_map(|part| part.parse::<u8>().ok());
        let hour = time_parts.next().unwrap_or(0);
        let minute = time_parts.next().unwrap_or(0);
        let second = time_parts.next().unwrap_or(0);

        Self::at(date, digital_time, hour, minute, second)
    }

    pub fn at(
        date: impl Into<String>,
        digital_time: impl Into<String>,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> Self {
        Self {
            date: date.into(),
            digital_time: digital_time.into(),
            hour,
            minute,
            second,
            alarms: Vec::new(),
            countdowns: Vec::new(),
            selected_entry_id: None,
            entry_window_start: 0,
            create_dialog: None,
            read_only: false,
            ascii_assets: None,
            terminal_cell_aspect_ratio: TerminalCellAspectRatio::default(),
        }
    }

    /// Marks the Clock page as view-only.
    ///
    /// Read-only pages omit all controls which create clock entries. Input
    /// routing should use the zero-sized `new_button` returned by
    /// `clock_page_layout` as the matching hit-test contract.
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn with_ascii_assets(mut self, ascii_assets: RuntimeAsciiAssets) -> Self {
        self.ascii_assets = Some(ascii_assets);
        self
    }

    pub fn with_terminal_cell_aspect_ratio(
        mut self,
        terminal_cell_aspect_ratio: TerminalCellAspectRatio,
    ) -> Self {
        self.terminal_cell_aspect_ratio = terminal_cell_aspect_ratio;
        self
    }

    pub(crate) fn terminal_cell_aspect_ratio(&self) -> TerminalCellAspectRatio {
        self.terminal_cell_aspect_ratio
    }

    pub(crate) fn clock_font(&self) -> Option<&crate::ClockFontAsset> {
        self.ascii_assets
            .as_ref()
            .map(RuntimeAsciiAssets::clock_font)
    }
}

impl Default for ClockViewModel {
    fn default() -> Self {
        Self::at("", "00:00:00", 0, 0, 0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimeSyncDialogViewModel;

impl TimeSyncDialogViewModel {
    pub const MESSAGE: &'static str = "联网校准时间失败";

    pub fn new() -> Self {
        Self
    }

    pub fn message(&self) -> &'static str {
        Self::MESSAGE
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellChromeViewModel {
    pub app_name: String,
    pub build_mode: String,
    pub display_mode: HomeDisplayMode,
    pub terminal_size: (u16, u16),
    pub screen_stack: Vec<String>,
    pub status: StatusViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellEntry {
    pub label: String,
    pub description: String,
}

impl ShellEntry {
    pub fn new(label: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugDiagnosticsViewModel {
    pub tick_count: u64,
    pub last_key_event: Option<String>,
    pub last_mouse_event: Option<String>,
    pub last_resize_event: Option<String>,
    pub mouse_coordinates: Option<(u16, u16)>,
    pub scroll_direction: Option<String>,
    pub drag_direction: Option<String>,
    pub terminal_flags: Vec<String>,
    pub platform_capability_summary: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiagnosticsTab {
    #[default]
    Health,
    Logs,
    Incidents,
}

impl DiagnosticsTab {
    pub const ALL: [Self; 3] = [Self::Health, Self::Logs, Self::Incidents];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Health => "Health",
            Self::Logs => "Logs",
            Self::Incidents => "Incidents",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiagnosticsStatus {
    #[default]
    Pass,
    Warning,
    Fail,
}

impl DiagnosticsStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pass => "Pass",
            Self::Warning => "Warning",
            Self::Fail => "Failure",
        }
    }

    pub const fn marker(self) -> &'static str {
        match self {
            Self::Pass => "[OK]",
            Self::Warning => "[!]",
            Self::Fail => "[X]",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsCheckViewModel {
    pub id: String,
    pub label: String,
    pub category: String,
    pub status: DiagnosticsStatus,
    pub summary: String,
    pub detail: String,
    pub remediation: String,
    pub repairable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsIncidentViewModel {
    pub id: String,
    pub occurred_at: String,
    pub app: String,
    pub severity: DiagnosticsStatus,
    pub recovery: String,
    pub summary: String,
    pub detail: String,
    pub report_path: String,
    pub restricted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsLogViewModel {
    pub relative_path: String,
    pub path: String,
    pub modified_at: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsRepairItemViewModel {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticsRepairDialogViewModel {
    pub items: Vec<DiagnosticsRepairItemViewModel>,
    pub selected: usize,
    pub confirm_selected: bool,
    pub scroll_offset: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticsViewModel {
    pub tab: DiagnosticsTab,
    pub checks: Vec<DiagnosticsCheckViewModel>,
    pub incidents: Vec<DiagnosticsIncidentViewModel>,
    pub logs: Vec<DiagnosticsLogViewModel>,
    pub selected_check: usize,
    pub selected_incident: usize,
    pub selected_log: usize,
    pub list_window_start: usize,
    pub scanning: bool,
    pub can_view_details: bool,
    pub can_repair: bool,
    pub restart_required: bool,
    pub repair_dialog: Option<DiagnosticsRepairDialogViewModel>,
    pub feedback: Option<String>,
    pub scanned_at: Option<String>,
}

impl DiagnosticsViewModel {
    pub fn selected_check(&self) -> Option<&DiagnosticsCheckViewModel> {
        self.checks.get(self.selected_check)
    }

    pub fn selected_incident(&self) -> Option<&DiagnosticsIncidentViewModel> {
        self.incidents.get(self.selected_incident)
    }

    pub fn selected_log(&self) -> Option<&DiagnosticsLogViewModel> {
        self.logs.get(self.selected_log)
    }

    pub fn item_count(&self) -> usize {
        match self.tab {
            DiagnosticsTab::Health => self.checks.len(),
            DiagnosticsTab::Incidents => self.incidents.len(),
            DiagnosticsTab::Logs if self.can_view_details => self.logs.len(),
            DiagnosticsTab::Logs => 0,
        }
    }

    pub fn selected_index(&self) -> usize {
        match self.tab {
            DiagnosticsTab::Health => self.selected_check,
            DiagnosticsTab::Incidents => self.selected_incident,
            DiagnosticsTab::Logs => self.selected_log,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HomeViewModel {
    display_mode: HomeDisplayMode,
    diagnostics: Option<DebugDiagnosticsViewModel>,
    home_icon_assets: Option<RuntimeAsciiAssets>,
    pub(crate) current_user: Option<String>,
    pub(crate) current_time: Option<String>,
    entries: Vec<ShellEntry>,
    selected_entry_index: usize,
    logout_visible: bool,
    logout_selected: bool,
}

impl PartialEq for HomeViewModel {
    fn eq(&self, other: &Self) -> bool {
        self.display_mode == other.display_mode
            && self.diagnostics == other.diagnostics
            && self.current_user == other.current_user
            && self.current_time == other.current_time
            && self.entries == other.entries
            && self.selected_entry_index == other.selected_entry_index
            && self.logout_visible == other.logout_visible
            && self.logout_selected == other.logout_selected
    }
}

impl Eq for HomeViewModel {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthField {
    Username,
    Password,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginField {
    UserList,
    Password,
    PasswordVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginUserOptionViewModel {
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub enabled: bool,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginViewModel {
    pub users: Vec<LoginUserOptionViewModel>,
    pub selected_index: usize,
    pub user_window_start: usize,
    pub password_len: usize,
    pub focused_field: LoginField,
    pub error: Option<String>,
    visible_password: Option<String>,
}

impl LoginViewModel {
    pub fn new(
        users: Vec<LoginUserOptionViewModel>,
        selected_index: usize,
        user_window_start: usize,
        password_len: usize,
        focused_field: LoginField,
        error: Option<String>,
    ) -> Self {
        let selected_index = if users.is_empty() {
            0
        } else {
            selected_index.min(users.len() - 1)
        };
        let user_window_start = user_window_start.min(selected_index);

        Self {
            users,
            selected_index,
            user_window_start,
            password_len,
            focused_field,
            error,
            visible_password: None,
        }
    }

    /// Supplies the plaintext to render while the password reveal control is
    /// active. The compatibility constructor never stores plaintext.
    pub fn with_visible_password(mut self, password: impl Into<String>) -> Self {
        self.visible_password = Some(password.into());
        self
    }

    pub fn visible_password(&self) -> Option<&str> {
        self.visible_password.as_deref()
    }

    pub fn password_is_visible(&self) -> bool {
        self.visible_password.is_some()
    }

    pub fn selected_user(&self) -> Option<&LoginUserOptionViewModel> {
        self.users.get(self.selected_index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapAdminViewModel {
    pub username: String,
    pub password_len: usize,
    pub focused_field: AuthField,
    pub error: Option<String>,
}

impl BootstrapAdminViewModel {
    pub fn new(
        username: impl Into<String>,
        password_len: usize,
        focused_field: AuthField,
        error: Option<String>,
    ) -> Self {
        Self {
            username: username.into(),
            password_len,
            focused_field,
            error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Language,
    Timezone,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupField {
    LanguageList,
    TimezoneList,
    AdminUsername,
    AdminPassword,
    AdminPasswordConfirm,
    PasswordHint,
    Submit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupLanguageOption {
    pub code: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetupTimezoneOption {
    pub id: String,
    pub label: String,
    pub description: String,
    pub longitude: f64,
    pub latitude: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupPasswordRequirementViewModel {
    pub label: String,
    pub met: bool,
}

impl SetupPasswordRequirementViewModel {
    pub fn new(label: impl Into<String>, met: bool) -> Self {
        Self {
            label: label.into(),
            met,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetupViewModel {
    pub step: SetupStep,
    pub languages: Vec<SetupLanguageOption>,
    pub timezones: Vec<SetupTimezoneOption>,
    pub selected_language_index: usize,
    pub selected_timezone_index: usize,
    pub timezone_window_start: usize,
    pub admin_username: String,
    pub admin_password_len: usize,
    pub admin_password_confirm_len: usize,
    pub password_requirements: Vec<SetupPasswordRequirementViewModel>,
    pub password_hint: String,
    pub focused_field: SetupField,
    pub can_submit: bool,
    pub error: Option<String>,
}

impl SetupViewModel {
    pub fn selected_language(&self) -> Option<&SetupLanguageOption> {
        self.languages.get(self.selected_language_index)
    }

    pub fn selected_timezone(&self) -> Option<&SetupTimezoneOption> {
        self.timezones.get(self.selected_timezone_index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserManagementUserViewModel {
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub enabled: bool,
    pub locked: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserManagementField {
    Username,
    DisplayName,
    Role,
    Password,
    Submit,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserManagementFormKind {
    Create,
    EditInfo,
    Password,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserManagementFormViewModel {
    pub kind: UserManagementFormKind,
    pub title: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub password_len: usize,
    pub focused_field: UserManagementField,
    pub error: Option<String>,
}

impl UserManagementFormViewModel {
    pub fn field_order(&self) -> &'static [UserManagementField] {
        match self.kind {
            UserManagementFormKind::Create => &[
                UserManagementField::Username,
                UserManagementField::DisplayName,
                UserManagementField::Role,
                UserManagementField::Password,
                UserManagementField::Submit,
                UserManagementField::Cancel,
            ],
            UserManagementFormKind::EditInfo => &[
                UserManagementField::DisplayName,
                UserManagementField::Submit,
                UserManagementField::Cancel,
            ],
            UserManagementFormKind::Password => &[
                UserManagementField::Password,
                UserManagementField::Submit,
                UserManagementField::Cancel,
            ],
        }
    }

    pub fn submit_label(&self) -> &'static str {
        match self.kind {
            UserManagementFormKind::Create => "Create",
            UserManagementFormKind::EditInfo => "Save",
            UserManagementFormKind::Password => "Set password",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserManagementAction {
    NewUser,
    EditInfo,
    SetPassword,
    ToggleEnabled,
    ToggleRole,
    Delete,
    Back,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserManagementActionViewModel {
    pub action: UserManagementAction,
    pub label: String,
    pub shortcut: Option<char>,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub dangerous: bool,
}

impl UserManagementActionViewModel {
    pub fn new(action: UserManagementAction, label: impl Into<String>) -> Self {
        Self {
            action,
            label: label.into(),
            shortcut: None,
            enabled: true,
            disabled_reason: None,
            dangerous: false,
        }
    }

    pub fn with_shortcut(mut self, shortcut: char) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn disabled(mut self, reason: impl Into<String>) -> Self {
        self.enabled = false;
        self.disabled_reason = Some(reason.into());
        self
    }

    pub fn dangerous(mut self, dangerous: bool) -> Self {
        self.dangerous = dangerous;
        self
    }

    pub fn button_label(&self) -> String {
        match self.shortcut {
            Some(shortcut) => format!("[{} {}]", shortcut.to_ascii_uppercase(), self.label),
            None => format!("[{}]", self.label),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum UserManagementFocus {
    #[default]
    UserList,
    Action(UserManagementAction),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UserManagementFeedbackTone {
    #[default]
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserManagementViewModel {
    pub current_user: String,
    pub users: Vec<UserManagementUserViewModel>,
    pub selected_index: usize,
    pub user_window_start: usize,
    pub message: Option<String>,
    pub feedback_tone: UserManagementFeedbackTone,
    pub can_manage_all: bool,
    pub focus: UserManagementFocus,
    pub actions: Vec<UserManagementActionViewModel>,
    pub form: Option<UserManagementFormViewModel>,
}

impl UserManagementViewModel {
    pub fn new(
        current_user: impl Into<String>,
        users: Vec<UserManagementUserViewModel>,
        selected_index: usize,
        message: Option<String>,
        can_manage_all: bool,
        form: Option<UserManagementFormViewModel>,
    ) -> Self {
        let mut model = Self {
            current_user: current_user.into(),
            users,
            selected_index,
            user_window_start: 0,
            message,
            feedback_tone: UserManagementFeedbackTone::Info,
            can_manage_all,
            focus: UserManagementFocus::UserList,
            actions: Vec::new(),
            form,
        };
        model.actions = model.default_actions();
        model
    }

    pub fn selected_user(&self) -> Option<&UserManagementUserViewModel> {
        self.users.get(self.selected_index)
    }

    /// Builds the standard action row from the selected account and caller permissions.
    ///
    /// Shells may replace `actions` when a backend policy provides a more specific
    /// disabled reason, but using this method keeps the common labels and shortcuts
    /// consistent.
    pub fn default_actions(&self) -> Vec<UserManagementActionViewModel> {
        let selected = self.selected_user();
        let no_selection = "No user selected";
        let target_action =
            |action: UserManagementAction, label: &str, shortcut: char, dangerous: bool| {
                let action = UserManagementActionViewModel::new(action, label)
                    .with_shortcut(shortcut)
                    .dangerous(dangerous);
                if selected.is_some() {
                    action
                } else {
                    action.disabled(no_selection)
                }
            };

        if !self.can_manage_all {
            return vec![
                target_action(UserManagementAction::EditInfo, "Edit profile", 'E', false),
                target_action(
                    UserManagementAction::SetPassword,
                    "Change password",
                    'R',
                    false,
                ),
                target_action(UserManagementAction::Delete, "Delete account", 'X', true),
                UserManagementActionViewModel::new(UserManagementAction::Back, "Back"),
            ];
        }

        let enabled_admin_count = self
            .users
            .iter()
            .filter(|user| user.enabled && user.role.eq_ignore_ascii_case("admin"))
            .count();
        let last_enabled_admin = selected.is_some_and(|user| {
            user.enabled && user.role.eq_ignore_ascii_case("admin") && enabled_admin_count <= 1
        });
        let protected_reason = "The last enabled administrator must remain available";

        let toggle_enabled_label = selected.map_or("Enable", |user| {
            if user.locked {
                "Unlock"
            } else if user.enabled {
                "Disable"
            } else {
                "Enable"
            }
        });
        let toggle_enabled_shortcut = selected.map_or('U', |user| {
            if user.enabled && !user.locked {
                'D'
            } else {
                'U'
            }
        });
        let toggle_role_label = selected.map_or("Make admin", |user| {
            if user.role.eq_ignore_ascii_case("admin") {
                "Make user"
            } else {
                "Make admin"
            }
        });

        let mut toggle_enabled = target_action(
            UserManagementAction::ToggleEnabled,
            toggle_enabled_label,
            toggle_enabled_shortcut,
            false,
        );
        let mut toggle_role = target_action(
            UserManagementAction::ToggleRole,
            toggle_role_label,
            'C',
            false,
        );
        let mut delete = target_action(UserManagementAction::Delete, "Delete", 'X', true);
        if last_enabled_admin {
            if selected.is_some_and(|user| user.enabled && !user.locked) {
                toggle_enabled = toggle_enabled.disabled(protected_reason);
            }
            toggle_role = toggle_role.disabled(protected_reason);
            delete = delete.disabled(protected_reason);
        }

        vec![
            UserManagementActionViewModel::new(UserManagementAction::NewUser, "New user")
                .with_shortcut('N'),
            target_action(UserManagementAction::EditInfo, "Edit", 'E', false),
            target_action(
                UserManagementAction::SetPassword,
                "Set password",
                'R',
                false,
            ),
            toggle_enabled,
            toggle_role,
            delete,
            UserManagementActionViewModel::new(UserManagementAction::Back, "Back"),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplorerSortColumn {
    #[default]
    Name,
    Type,
    Size,
    Modified,
}

impl ExplorerSortColumn {
    pub const ALL: [Self; 4] = [Self::Name, Self::Type, Self::Size, Self::Modified];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Type => "Type",
            Self::Size => "Size",
            Self::Modified => "Modified",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplorerSortDirection {
    #[default]
    Ascending,
    Descending,
}

impl ExplorerSortDirection {
    pub const fn icon_key(self) -> &'static str {
        match self {
            Self::Ascending => "sort_asc",
            Self::Descending => "sort_desc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerToolbarAction {
    Back,
    Forward,
    Up,
    Refresh,
    New,
    Cut,
    Copy,
    Paste,
    Rename,
    Delete,
    Restore,
    DumpTrash,
    Sort,
    Options,
}

impl ExplorerToolbarAction {
    pub const ALL: [Self; 14] = [
        Self::Back,
        Self::Forward,
        Self::Up,
        Self::Refresh,
        Self::New,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::Rename,
        Self::Delete,
        Self::Restore,
        Self::DumpTrash,
        Self::Sort,
        Self::Options,
    ];

    pub const REGULAR: [Self; 12] = [
        Self::Back,
        Self::Forward,
        Self::Up,
        Self::Refresh,
        Self::New,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::Rename,
        Self::Delete,
        Self::Sort,
        Self::Options,
    ];

    pub const TRASH: [Self; 7] = [
        Self::Back,
        Self::Forward,
        Self::Refresh,
        Self::Restore,
        Self::DumpTrash,
        Self::Sort,
        Self::Options,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::Up => "Up",
            Self::Refresh => "Refresh",
            Self::New => "New",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Rename => "Rename",
            Self::Delete => "Delete",
            Self::Restore => "Restore",
            Self::DumpTrash => "Dump Trash",
            Self::Sort => "Sort",
            Self::Options => "Options",
        }
    }

    pub const fn icon_key(self) -> &'static str {
        match self {
            Self::Back => "back",
            Self::Forward => "forward",
            Self::Up => "up",
            Self::Refresh => "refresh",
            Self::New => "new",
            Self::Cut => "cut",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::Rename => "rename",
            Self::Delete => "delete",
            Self::Restore => "refresh",
            Self::DumpTrash => "delete",
            Self::Sort => "sort_asc",
            Self::Options => "options",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerToolbarButtonViewModel {
    pub action: ExplorerToolbarAction,
    pub label: String,
    pub icon_key: String,
    pub enabled: bool,
    pub active: bool,
}

impl ExplorerToolbarButtonViewModel {
    pub fn new(action: ExplorerToolbarAction, enabled: bool) -> Self {
        Self {
            action,
            label: action.label().to_string(),
            icon_key: action.icon_key().to_string(),
            enabled,
            active: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExplorerToolbarViewModel {
    pub buttons: Vec<ExplorerToolbarButtonViewModel>,
}

impl ExplorerToolbarViewModel {
    pub fn standard(can_go_back: bool, can_go_forward: bool) -> Self {
        Self {
            buttons: ExplorerToolbarAction::REGULAR
                .into_iter()
                .map(|action| {
                    let enabled = match action {
                        ExplorerToolbarAction::Back => can_go_back,
                        ExplorerToolbarAction::Forward => can_go_forward,
                        _ => true,
                    };
                    ExplorerToolbarButtonViewModel::new(action, enabled)
                })
                .collect(),
        }
    }

    pub fn trash(
        can_go_back: bool,
        can_go_forward: bool,
        can_restore: bool,
        can_dump: bool,
    ) -> Self {
        Self {
            buttons: ExplorerToolbarAction::TRASH
                .into_iter()
                .map(|action| {
                    let enabled = match action {
                        ExplorerToolbarAction::Back => can_go_back,
                        ExplorerToolbarAction::Forward => can_go_forward,
                        ExplorerToolbarAction::Restore => can_restore,
                        ExplorerToolbarAction::DumpTrash => can_dump,
                        _ => true,
                    };
                    ExplorerToolbarButtonViewModel::new(action, enabled)
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerEntryViewModel {
    pub name: String,
    pub kind: String,
    pub size: Option<String>,
    pub modified: Option<String>,
    pub attributes: Vec<String>,
    pub selected: bool,
}

/// Optional stable identity and interaction state layered over a legacy entry.
/// The vector on [`ExplorerViewModel`] is index-aligned with `entries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerEntryPresentationViewModel {
    pub id: String,
    pub path: String,
    pub icon_key: String,
    pub is_directory: bool,
    pub selected: bool,
    pub focused: bool,
    pub cut: bool,
    pub drop_target: bool,
    pub metadata_warning: Option<String>,
    pub original_path: Option<String>,
}

impl ExplorerEntryPresentationViewModel {
    pub fn new(
        id: impl Into<String>,
        path: impl Into<String>,
        icon_key: impl Into<String>,
        is_directory: bool,
    ) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            icon_key: icon_key.into(),
            is_directory,
            selected: false,
            focused: false,
            cut: false,
            drop_target: false,
            metadata_warning: None,
            original_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerQuickLocationViewModel {
    pub id: String,
    pub label: String,
    pub path: String,
    pub icon_key: String,
    pub kind: ExplorerQuickLocationKind,
    pub current: bool,
    pub enabled: bool,
    pub drop_target: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplorerQuickLocationKind {
    #[default]
    Directory,
    Volume,
    Trash,
}

impl ExplorerQuickLocationViewModel {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        path: impl Into<String>,
        icon_key: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            path: path.into(),
            icon_key: icon_key.into(),
            kind: ExplorerQuickLocationKind::Directory,
            current: false,
            enabled: true,
            drop_target: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerBreadcrumbViewModel {
    pub id: String,
    pub label: String,
    pub path: String,
    pub enabled: bool,
    pub drop_target: bool,
}

impl ExplorerBreadcrumbViewModel {
    pub fn new(id: impl Into<String>, label: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            path: path.into(),
            enabled: true,
            drop_target: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerOperationPhase {
    Scanning,
    CheckingConflicts,
    Copying,
    Moving,
    Deleting,
    Finishing,
}

impl ExplorerOperationPhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Scanning => "Scanning",
            Self::CheckingConflicts => "Checking conflicts",
            Self::Copying => "Copying",
            Self::Moving => "Moving",
            Self::Deleting => "Deleting",
            Self::Finishing => "Finishing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOperationProgressViewModel {
    pub phase: ExplorerOperationPhase,
    pub label: String,
    pub completed_items: u64,
    pub total_items: Option<u64>,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub cancellable: bool,
    pub cancel_label: String,
}

impl ExplorerOperationProgressViewModel {
    pub fn percent(&self) -> Option<u16> {
        if let Some(total_bytes) = self.total_bytes.filter(|total| *total > 0) {
            return Some(
                self.completed_bytes
                    .saturating_mul(100)
                    .checked_div(total_bytes)
                    .unwrap_or(0)
                    .min(100) as u16,
            );
        }
        self.total_items.filter(|total| *total > 0).map(|total| {
            self.completed_items
                .saturating_mul(100)
                .checked_div(total)
                .unwrap_or(0)
                .min(100) as u16
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerContextMenuItemViewModel {
    pub id: String,
    pub label: String,
    pub shortcut: Option<String>,
    pub enabled: bool,
    pub dangerous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerContextMenuViewModel {
    pub x: u16,
    pub y: u16,
    pub title: String,
    pub items: Vec<ExplorerContextMenuItemViewModel>,
    pub selected_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerNameDialogKind {
    NewFolder,
    NewTextFile,
    Rename,
    RestoreDestination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerNameDialogViewModel {
    pub kind: ExplorerNameDialogKind,
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub error: Option<String>,
    pub confirm_label: String,
    pub cancel_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOptionViewModel {
    pub id: String,
    pub label: String,
    pub value: String,
    pub enabled: bool,
    pub selected: bool,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOptionsViewModel {
    pub title: String,
    pub options: Vec<ExplorerOptionViewModel>,
    pub close_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerConflictChoice {
    KeepBoth,
    Replace,
    Skip,
    Cancel,
}

impl ExplorerConflictChoice {
    pub const fn label(self) -> &'static str {
        match self {
            Self::KeepBoth => "Keep both",
            Self::Replace => "Replace",
            Self::Skip => "Skip",
            Self::Cancel => "Cancel",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerConflictViewModel {
    pub title: String,
    pub source: String,
    pub destination: String,
    pub choices: Vec<ExplorerConflictChoice>,
    pub selected_choice: ExplorerConflictChoice,
    pub apply_to_remaining: bool,
    pub allow_apply_to_remaining: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerPropertyViewModel {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerPropertiesViewModel {
    pub title: String,
    pub properties: Vec<ExplorerPropertyViewModel>,
    pub close_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerOverlayViewModel {
    ContextMenu(ExplorerContextMenuViewModel),
    Name(ExplorerNameDialogViewModel),
    Options(ExplorerOptionsViewModel),
    Conflict(ExplorerConflictViewModel),
    Properties(ExplorerPropertiesViewModel),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerSearchViewModel {
    pub query: String,
    pub active: bool,
    pub match_count: Option<usize>,
}

impl ExplorerSearchViewModel {
    pub fn new(query: impl Into<String>, active: bool, match_count: Option<usize>) -> Self {
        Self {
            query: query.into(),
            active,
            match_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerDialogViewModel {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
}

impl ExplorerDialogViewModel {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerViewModel {
    pub current_path: String,
    pub address_value: String,
    pub address_editing: bool,
    pub is_trash: bool,
    pub entries: Vec<ExplorerEntryViewModel>,
    pub selected_index: Option<usize>,
    pub search: Option<ExplorerSearchViewModel>,
    pub show_hidden: bool,
    pub message: Option<String>,
    pub error: Option<String>,
    pub pending_dialog: Option<ExplorerDialogViewModel>,
    pub ascii_assets: Option<RuntimeAsciiAssets>,
    pub entry_presentations: Vec<ExplorerEntryPresentationViewModel>,
    pub toolbar: ExplorerToolbarViewModel,
    pub quick_locations: Vec<ExplorerQuickLocationViewModel>,
    pub breadcrumbs: Vec<ExplorerBreadcrumbViewModel>,
    pub sort_column: ExplorerSortColumn,
    pub sort_direction: ExplorerSortDirection,
    pub viewport_offset: usize,
    pub viewport_follows_focus: bool,
    pub show_sidebar: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub selected_count: usize,
    pub listing_warning_count: usize,
    pub operation: Option<ExplorerOperationProgressViewModel>,
    pub overlay: Option<ExplorerOverlayViewModel>,
}

impl ExplorerViewModel {
    pub fn new(
        current_path: impl Into<String>,
        entries: Vec<ExplorerEntryViewModel>,
        selected_index: Option<usize>,
    ) -> Self {
        Self::try_new(current_path, entries, selected_index)
            .expect("default ASCII Explorer assets must load")
    }

    pub fn try_new(
        current_path: impl Into<String>,
        entries: Vec<ExplorerEntryViewModel>,
        selected_index: Option<usize>,
    ) -> Result<Self, AssetError> {
        let ascii_assets = RuntimeAsciiAssets::load_default()?;
        Ok(Self::with_ascii_assets(
            current_path,
            entries,
            selected_index,
            ascii_assets,
        ))
    }

    pub fn with_ascii_assets(
        current_path: impl Into<String>,
        entries: Vec<ExplorerEntryViewModel>,
        selected_index: Option<usize>,
        ascii_assets: RuntimeAsciiAssets,
    ) -> Self {
        let selected_count = entries.iter().filter(|entry| entry.selected).count();
        let current_path = current_path.into();
        Self {
            address_value: current_path.clone(),
            current_path,
            address_editing: false,
            is_trash: false,
            entries,
            selected_index,
            search: None,
            show_hidden: false,
            message: None,
            error: None,
            pending_dialog: None,
            ascii_assets: Some(ascii_assets),
            entry_presentations: Vec::new(),
            toolbar: ExplorerToolbarViewModel::standard(false, false),
            quick_locations: Vec::new(),
            breadcrumbs: Vec::new(),
            sort_column: ExplorerSortColumn::Name,
            sort_direction: ExplorerSortDirection::Ascending,
            viewport_offset: 0,
            viewport_follows_focus: true,
            show_sidebar: true,
            can_go_back: false,
            can_go_forward: false,
            selected_count,
            listing_warning_count: 0,
            operation: None,
            overlay: None,
        }
    }

    pub fn set_history_availability(&mut self, can_go_back: bool, can_go_forward: bool) {
        self.can_go_back = can_go_back;
        self.can_go_forward = can_go_forward;
        for button in &mut self.toolbar.buttons {
            match button.action {
                ExplorerToolbarAction::Back => button.enabled = can_go_back,
                ExplorerToolbarAction::Forward => button.enabled = can_go_forward,
                _ => {}
            }
        }
    }

    pub fn entry_presentation(&self, index: usize) -> Option<&ExplorerEntryPresentationViewModel> {
        self.entry_presentations.get(index)
    }

    pub fn effective_selected_count(&self) -> usize {
        if self.selected_count > 0 {
            self.selected_count
        } else if !self.entry_presentations.is_empty() {
            self.entry_presentations
                .iter()
                .filter(|entry| entry.selected)
                .count()
        } else {
            self.entries.iter().filter(|entry| entry.selected).count()
        }
    }

    pub fn selected_entry(&self) -> Option<&ExplorerEntryViewModel> {
        self.selected_index
            .and_then(|selected_index| self.entries.get(selected_index))
    }
}

impl HomeViewModel {
    pub fn debug(diagnostics: DebugDiagnosticsViewModel) -> Self {
        Self {
            display_mode: HomeDisplayMode::Debug,
            diagnostics: Some(diagnostics),
            home_icon_assets: None,
            current_user: None,
            current_time: None,
            entries: Vec::new(),
            selected_entry_index: 0,
            logout_visible: false,
            logout_selected: false,
        }
    }

    pub fn user(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
    ) -> Self {
        Self::user_with_selection(current_user, current_time, entries, 0)
    }

    pub fn user_with_selection(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
        selected_entry_index: usize,
    ) -> Self {
        Self::try_user_with_selection(current_user, current_time, entries, selected_entry_index)
            .expect("default ASCII home icon assets must load")
    }

    pub fn try_user(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
    ) -> Result<Self, AssetError> {
        Self::try_user_with_selection(current_user, current_time, entries, 0)
    }

    pub fn try_user_with_selection(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
        selected_entry_index: usize,
    ) -> Result<Self, AssetError> {
        let home_icon_assets = RuntimeAsciiAssets::load_default()?;
        Ok(Self::user_with_selection_and_icon_assets(
            current_user,
            current_time,
            entries,
            selected_entry_index,
            home_icon_assets,
        ))
    }

    pub fn user_with_icon_assets(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
        home_icon_assets: RuntimeAsciiAssets,
    ) -> Self {
        Self::user_with_selection_and_icon_assets(
            current_user,
            current_time,
            entries,
            0,
            home_icon_assets,
        )
    }

    pub fn user_with_selection_and_icon_assets(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
        selected_entry_index: usize,
        home_icon_assets: RuntimeAsciiAssets,
    ) -> Self {
        let selected_entry_index = if entries.is_empty() {
            0
        } else {
            selected_entry_index.min(entries.len() - 1)
        };

        Self {
            display_mode: HomeDisplayMode::User,
            diagnostics: None,
            home_icon_assets: Some(home_icon_assets),
            current_user: Some(current_user.into()),
            current_time: Some(current_time.into()),
            entries,
            selected_entry_index,
            logout_visible: false,
            logout_selected: false,
        }
    }

    /// Adds an authenticated account summary and its Logout control.
    ///
    /// This is also the opt-in path for authenticated Debug homes; a plain
    /// [`HomeViewModel::debug`] remains the storage-free development view.
    pub fn with_account_logout(mut self, current_user: impl Into<String>, selected: bool) -> Self {
        self.current_user = Some(current_user.into());
        self.logout_visible = true;
        self.logout_selected = selected;
        self
    }

    pub fn logout_visible(&self) -> bool {
        self.logout_visible
    }

    pub fn logout_selected(&self) -> bool {
        self.logout_selected
    }

    pub fn display_mode(&self) -> HomeDisplayMode {
        self.display_mode
    }

    pub fn diagnostics(&self) -> Option<&DebugDiagnosticsViewModel> {
        self.diagnostics.as_ref()
    }

    pub fn entries(&self) -> &[ShellEntry] {
        &self.entries
    }

    pub fn home_icon_catalog(&self) -> Option<&HomeIconCatalog> {
        self.home_icon_assets
            .as_ref()
            .map(RuntimeAsciiAssets::home_icon_catalog)
    }

    pub fn home_icon_for_label(&self, label: &str) -> Option<&HomeIcon> {
        self.home_icon_assets
            .as_ref()
            .and_then(|assets| assets.home_icon_for_label(label))
    }

    pub fn selected_entry_index(&self) -> usize {
        self.selected_entry_index
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitConfirmViewModel {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
}

impl ExitConfirmViewModel {
    pub fn new() -> Self {
        Self {
            title: "Exit TundraUX 3".to_string(),
            message: "Leave the shell and restore the terminal?".to_string(),
            confirm_label: "Y / Enter: exit".to_string(),
            cancel_label: "N / Esc: cancel".to_string(),
        }
    }
}

impl Default for ExitConfirmViewModel {
    fn default() -> Self {
        Self::new()
    }
}
