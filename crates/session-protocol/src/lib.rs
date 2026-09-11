//! Shared wire contracts. Client-supplied identity is never authorization.
use serde::{Deserialize, Serialize};

pub mod greeter;

pub const VERSION: u32 = 1;
pub const SESSION_BUS: &str = "org.tundra.Session1";
pub const SESSION_PATH: &str = "/org/tundra/Session1";
pub const PRIVILEGED_BUS: &str = "org.tundra.Privileged1";
pub const PRIVILEGED_PATH: &str = "/org/tundra/Privileged1";

#[cfg(target_os = "linux")]
pub mod linux;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionIdentity {
    pub uid: u32,
    pub logind_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemUser {
    pub uid: u32,
    pub gid: u32,
    pub username: String,
    pub home: std::path::PathBuf,
    pub shell: std::path::PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Opening,
    Active,
    Locking,
    Locked,
    Unlocking,
    Closing,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub identity: SessionIdentity,
    pub state: SessionState,
    pub revision: u64,
}

impl SessionSnapshot {
    /// Compare-and-transition prevents stale workers from unlocking a newer state.
    pub fn transition(&mut self, revision: u64, next: SessionState) -> Result<(), &'static str> {
        use SessionState::*;
        if revision != self.revision {
            return Err("stale session revision");
        }
        if !matches!(
            (self.state, next),
            (Opening, Active | Closing)
                | (Active, Locking | Closing)
                | (Locking, Locked | Closing)
                | (Locked, Unlocking | Closing)
                | (Unlocking, Active | Locked | Closing)
                | (Closing, Ended)
        ) {
            return Err("invalid session transition");
        }
        self.revision = self.revision.checked_add(1).ok_or("revision exhausted")?;
        self.state = next;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SystemAction {
    PowerOff,
    Reboot,
    ReadSystemLogs {
        max_records: u32,
        since_epoch_seconds: u64,
    },
    InstallUpdate {
        release_id: String,
    },
}

impl SystemAction {
    pub fn policy_id(&self) -> &'static str {
        match self {
            Self::PowerOff => "org.tundra.power-off",
            Self::Reboot => "org.tundra.reboot",
            Self::ReadSystemLogs { .. } => "org.tundra.read-system-logs",
            Self::InstallUpdate { .. } => "org.tundra.install-update",
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::ReadSystemLogs { max_records, .. } if !(1..=10_000).contains(max_records) => {
                Err("log record limit must be between 1 and 10000")
            }
            Self::InstallUpdate { release_id }
                if release_id.is_empty()
                    || release_id.len() > 128
                    || !release_id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c)) =>
            {
                Err("invalid release identifier")
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationStatus {
    AwaitingConfirmation,
    Running,
    Completed,
    Cancelled,
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_and_unlock_preserve_identity_and_reject_stale_completion() {
        let identity = SessionIdentity {
            uid: 1001,
            logind_session_id: "c5".into(),
        };
        let mut session = SessionSnapshot {
            identity: identity.clone(),
            state: SessionState::Opening,
            revision: 0,
        };
        for next in [
            SessionState::Active,
            SessionState::Locking,
            SessionState::Locked,
            SessionState::Unlocking,
            SessionState::Locked,
            SessionState::Unlocking,
            SessionState::Active,
            SessionState::Closing,
            SessionState::Ended,
        ] {
            let old = session.revision;
            session.transition(old, next).unwrap();
            assert!(session.transition(old, SessionState::Active).is_err());
            assert_eq!(session.identity, identity);
        }
        assert!(
            session
                .transition(session.revision, SessionState::Active)
                .is_err()
        );
    }
    #[test]
    fn lock_request_cannot_claim_lock_completion_or_skip_authentication() {
        let mut s = SessionSnapshot {
            identity: SessionIdentity {
                uid: 1001,
                logind_session_id: "9".into(),
            },
            state: SessionState::Active,
            revision: 0,
        };
        assert!(s.transition(0, SessionState::Locked).is_err());
        s.transition(0, SessionState::Locking).unwrap();
        assert!(s.transition(1, SessionState::Active).is_err());
    }
}
