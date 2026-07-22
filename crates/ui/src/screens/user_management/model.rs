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
