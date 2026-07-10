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
    ascii_assets: Option<RuntimeAsciiAssets>,
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
            ascii_assets: None,
        }
    }

    pub fn with_ascii_assets(mut self, ascii_assets: RuntimeAsciiAssets) -> Self {
        self.ascii_assets = Some(ascii_assets);
        self
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

#[derive(Debug, Clone)]
pub struct HomeViewModel {
    display_mode: HomeDisplayMode,
    diagnostics: Option<DebugDiagnosticsViewModel>,
    home_icon_assets: Option<RuntimeAsciiAssets>,
    pub(crate) current_user: Option<String>,
    pub(crate) current_time: Option<String>,
    entries: Vec<ShellEntry>,
    selected_entry_index: usize,
}

impl PartialEq for HomeViewModel {
    fn eq(&self, other: &Self) -> bool {
        self.display_mode == other.display_mode
            && self.diagnostics == other.diagnostics
            && self.current_user == other.current_user
            && self.current_time == other.current_time
            && self.entries == other.entries
            && self.selected_entry_index == other.selected_entry_index
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
        }
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserManagementField {
    Username,
    DisplayName,
    Password,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserManagementViewModel {
    pub current_user: String,
    pub users: Vec<UserManagementUserViewModel>,
    pub selected_index: usize,
    pub message: Option<String>,
    pub can_manage_all: bool,
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
        Self {
            current_user: current_user.into(),
            users,
            selected_index,
            message,
            can_manage_all,
            form,
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
    pub entries: Vec<ExplorerEntryViewModel>,
    pub selected_index: Option<usize>,
    pub search: Option<ExplorerSearchViewModel>,
    pub show_hidden: bool,
    pub message: Option<String>,
    pub error: Option<String>,
    pub pending_dialog: Option<ExplorerDialogViewModel>,
}

impl ExplorerViewModel {
    pub fn new(
        current_path: impl Into<String>,
        entries: Vec<ExplorerEntryViewModel>,
        selected_index: Option<usize>,
    ) -> Self {
        Self {
            current_path: current_path.into(),
            entries,
            selected_index,
            search: None,
            show_hidden: false,
            message: None,
            error: None,
            pending_dialog: None,
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
        }
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
