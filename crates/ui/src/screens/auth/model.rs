pub use app::{SetupLanguageOption, SetupTimezoneOption};

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
    Appearance,
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
    AppearanceShape,
    AppearanceThemeColor,
    AppearanceThemeCustom,
    AppearanceAccentColor,
    AppearanceAccentCustom,
    AppearanceSubmit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCustomColorTarget {
    Theme,
    Accent,
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
    pub border_shape: crate::BorderShape,
    pub theme_color: ratatui::style::Color,
    pub theme_color_value: String,
    pub accent_color: ratatui::style::Color,
    pub accent_color_value: String,
    pub custom_color_target: Option<SetupCustomColorTarget>,
    pub custom_color_input: String,
    pub custom_color_valid: bool,
    pub custom_color_conflicts_with_theme: bool,
    pub custom_color_error: Option<String>,
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
