//! Errors shared by system-service adapters. UI code never interprets D-Bus text.
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceError {
    PermissionDenied,
    AuthorizationCancelled,
    ServiceUnavailable,
    Busy,
    NetworkError,
    BackendDisconnected,
    Unsupported,
    UntrustedTransaction,
    Timeout,
    Unknown,
    AccountPasswordSetupFailed,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PermissionDenied => "Permission denied",
            Self::AuthorizationCancelled => "Authorization cancelled",
            Self::ServiceUnavailable => "Service unavailable",
            Self::Busy => "The system service is busy; try again later",
            Self::NetworkError => "The system service could not reach the network",
            Self::BackendDisconnected => "Connection to the system service was lost",
            Self::Unsupported => "This operation is unsupported",
            Self::UntrustedTransaction => "The transaction cannot be safely previewed or requires additional trust, removal, or license acceptance",
            Self::Timeout => "The system service did not respond in time",
            Self::Unknown => "The system service could not complete the operation",
            Self::AccountPasswordSetupFailed => "Account created, but password setup failed. Set its password before use.",
        })
    }
}
impl std::error::Error for ServiceError {}
