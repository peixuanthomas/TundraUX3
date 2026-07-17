use std::fmt;

use serde::{Deserialize, Serialize};

use crate::identity::AuthSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    Guest,
    User,
    Admin,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Guest => "Guest",
            Self::User => "User",
            Self::Admin => "Admin",
        }
    }

    pub(crate) fn from_storage(value: &str) -> Self {
        match value {
            "Admin" => Self::Admin,
            "Guest" => Self::Guest,
            _ => Self::User,
        }
    }
}

impl fmt::Display for UserRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionAction {
    ReadFile,
    WriteFile,
    DeleteFile,
    MoveFile,
    OpenExternal,
    ManageOwnUser,
    ManageUsers,
    ViewDiagnosticsDetails,
    RepairDiagnostics,
    ChangeSettings,
    EnterDebugMode,
}

impl PermissionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadFile => "ReadFile",
            Self::WriteFile => "WriteFile",
            Self::DeleteFile => "DeleteFile",
            Self::MoveFile => "MoveFile",
            Self::OpenExternal => "OpenExternal",
            Self::ManageOwnUser => "ManageOwnUser",
            Self::ManageUsers => "ManageUsers",
            Self::ViewDiagnosticsDetails => "ViewDiagnosticsDetails",
            Self::RepairDiagnostics => "RepairDiagnostics",
            Self::ChangeSettings => "ChangeSettings",
            Self::EnterDebugMode => "EnterDebugMode",
        }
    }
}

impl fmt::Display for PermissionAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl Authorization {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugPolicy {
    pub debug_build: bool,
    pub allow_release_debug: bool,
}

impl DebugPolicy {
    pub fn current_build(allow_release_debug: bool) -> Self {
        Self {
            debug_build: cfg!(debug_assertions),
            allow_release_debug,
        }
    }

    fn allows_debug(self) -> bool {
        self.debug_build || self.allow_release_debug
    }
}

impl Default for DebugPolicy {
    fn default() -> Self {
        Self::current_build(false)
    }
}

#[derive(Debug, Clone)]
pub struct PermissionService {
    debug_policy: DebugPolicy,
}

impl PermissionService {
    pub fn new(debug_policy: DebugPolicy) -> Self {
        Self { debug_policy }
    }

    pub fn authorize(
        &self,
        session: Option<&AuthSession>,
        action: PermissionAction,
        _resource: Option<&str>,
    ) -> Authorization {
        let role = session
            .map(|session| session.role)
            .unwrap_or(UserRole::Guest);
        match action {
            PermissionAction::ReadFile
            | PermissionAction::WriteFile
            | PermissionAction::DeleteFile
            | PermissionAction::MoveFile
            | PermissionAction::OpenExternal => match role {
                UserRole::User | UserRole::Admin => Authorization::allow(),
                UserRole::Guest => Authorization::deny("not_authenticated"),
            },
            PermissionAction::ManageOwnUser => match role {
                UserRole::User | UserRole::Admin => Authorization::allow(),
                UserRole::Guest => Authorization::deny("not_authenticated"),
            },
            PermissionAction::ManageUsers
            | PermissionAction::ViewDiagnosticsDetails
            | PermissionAction::RepairDiagnostics
            | PermissionAction::ChangeSettings => match role {
                UserRole::Admin => Authorization::allow(),
                UserRole::Guest => Authorization::deny("not_authenticated"),
                UserRole::User => Authorization::deny("insufficient_role"),
            },
            PermissionAction::EnterDebugMode => match role {
                UserRole::Admin if self.debug_policy.allows_debug() => Authorization::allow(),
                UserRole::Admin => Authorization::deny("debug_policy_denied"),
                UserRole::Guest => Authorization::deny("not_authenticated"),
                UserRole::User => Authorization::deny("insufficient_role"),
            },
        }
    }
}

impl Default for PermissionService {
    fn default() -> Self {
        Self::new(DebugPolicy::default())
    }
}
