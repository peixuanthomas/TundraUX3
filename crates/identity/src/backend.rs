/// The account authority. Local accounts remain available to other platforms
/// and isolated tests; the native Linux shell explicitly selects Linux.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IdentityBackend {
    #[default]
    Local,
    Linux,
}

impl IdentityBackend {
    pub fn usernames_match(self, left: &str, right: &str) -> bool {
        match self {
            Self::Linux => left == right,
            Self::Local => left.eq_ignore_ascii_case(right),
        }
    }

    pub(crate) fn require_local(self) -> Result<(), crate::CoreError> {
        if self == Self::Linux {
            Err(crate::CoreError::SystemAccountManaged)
        } else {
            Ok(())
        }
    }

    pub(crate) fn users(
        self,
        storage: &storage::StorageManager,
    ) -> Result<Vec<storage::UserRecord>, crate::CoreError> {
        match self {
            Self::Local => Ok(storage.load_users()?.users),
            #[cfg(target_os = "linux")]
            Self::Linux => crate::linux::users(storage),
            #[cfg(not(target_os = "linux"))]
            Self::Linux => Err(crate::CoreError::SystemIdentity(
                "Linux is unavailable".into(),
            )),
        }
    }
}
