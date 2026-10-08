//! Stable failure data shared by the UI and scripts. Native output is detail,
//! never the short user-facing explanation or the authority for a retry.
use super::ManagementError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationProblem {
    pub code: String,
    pub summary_key: String,
    pub next_action: String,
    pub detail: String,
    pub exit_code: i32,
    pub native_exit_code: Option<i32>,
    pub service: Option<String>,
    pub boot_id: Option<String>,
}

impl OperationProblem {
    pub fn from_error(error: &ManagementError) -> Self {
        let (code, next_action, exit_code) = match error {
            ManagementError::Unavailable(_) => ("unavailable", "requirements", 4),
            ManagementError::PermissionDenied(_) => ("permission", "authorize", 3),
            ManagementError::InvalidInput(_) => ("invalid-input", "edit", 2),
            ManagementError::Conflict(_) => ("conflict", "refresh", 5),
            ManagementError::Cancelled => ("cancelled", "close", 130),
            ManagementError::Failed(_) => ("failed", "details", 1),
        };
        Self {
            code: code.into(),
            summary_key: format!("management-problem-{code}"),
            next_action: next_action.into(),
            detail: runtime_log::sanitize_text(&error.to_string()),
            exit_code,
            native_exit_code: None,
            service: None,
            boot_id: None,
        }
    }
}
