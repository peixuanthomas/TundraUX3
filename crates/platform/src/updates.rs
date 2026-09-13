//! Fixed-package update models shared by the native adapter and shell.
use crate::service::ServiceError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageVersion {
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub repository: String,
}

#[cfg(any(target_os = "linux", test))]
impl PackageVersion {
    pub(crate) fn parse(id: &str) -> Result<Self, ServiceError> {
        let fields: Vec<_> = id.split(';').collect();
        if fields.len() != 4
            || fields
                .iter()
                .any(|s| s.is_empty() || s.len() > 512 || s.chars().any(char::is_control))
        {
            return Err(ServiceError::UntrustedTransaction);
        }
        if fields[0]
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !"._+-".contains(c))
        {
            return Err(ServiceError::UntrustedTransaction);
        }
        Ok(Self {
            name: fields[0].into(),
            version: fields[1].into(),
            architecture: fields[2].into(),
            repository: fields[3].into(),
        })
    }
    pub(crate) fn id(&self) -> String {
        format!(
            "{};{};{};{}",
            self.name, self.version, self.architecture, self.repository
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageChange {
    pub package: PackageVersion,
    pub old_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePreview {
    pub changes: Vec<PackageChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheck {
    pub installed_version: String,
    pub candidate: Option<PackageVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateProgress {
    pub stage: UpdateStage,
    pub percentage: Option<u32>,
    pub cancellable: bool,
    pub package: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateStage {
    #[default]
    Preparing,
    StartingTransaction,
    Waiting,
    Authorizing,
    Downloading,
    Installing,
    Verifying,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateResult {
    Installed {
        version: String,
        system_restart_recommended: bool,
    },
    Cancelled,
    Failed(ServiceError),
    Unknown {
        expected_version: String,
    },
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn same_rpm_version(left: &str, right: &str) -> bool {
    left.strip_prefix("0:").unwrap_or(left) == right.strip_prefix("0:").unwrap_or(right)
}

#[cfg(target_os = "linux")]
pub(crate) fn transaction_error(code: u32) -> ServiceError {
    match code {
        2 | 10 | 43 => ServiceError::NetworkError,
        3 => ServiceError::Unsupported,
        5 | 30 | 31 | 34 | 37 | 50 | 51 => ServiceError::UntrustedTransaction,
        17 | 65 => ServiceError::AuthorizationCancelled,
        25 | 26 | 67 => ServiceError::Busy,
        48 => ServiceError::PermissionDenied,
        _ => ServiceError::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_ids_reject_missing_repository_and_terminal_controls() {
        let id = "tundraux3;1.3.1-1;x86_64;updates";
        assert_eq!(PackageVersion::parse(id).unwrap().id(), id);
        assert!(PackageVersion::parse("tundraux3;1.3.1-1;x86_64;updates").is_ok());
        for id in [
            "tundraux3;1;x86_64;",
            "tundraux3;1;x86_64;repo\u{1b}[31m",
            "tundraux3;1;x86_64",
            "tundraux3;1;x86_64;repo;extra",
        ] {
            assert_eq!(
                PackageVersion::parse(id),
                Err(ServiceError::UntrustedTransaction)
            );
        }
        assert!(same_rpm_version("0:1.3.1-1", "1.3.1-1"));
        assert!(!same_rpm_version("1:1.3.1-1", "1.3.1-1"));
    }
}
