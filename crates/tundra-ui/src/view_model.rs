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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeViewModel {
    display_mode: HomeDisplayMode,
    diagnostics: Option<DebugDiagnosticsViewModel>,
    pub(crate) current_user: Option<String>,
    pub(crate) current_time: Option<String>,
    entries: Vec<ShellEntry>,
}

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
            current_user: None,
            current_time: None,
            entries: Vec::new(),
        }
    }

    pub fn user(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
    ) -> Self {
        Self {
            display_mode: HomeDisplayMode::User,
            diagnostics: None,
            current_user: Some(current_user.into()),
            current_time: Some(current_time.into()),
            entries,
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
