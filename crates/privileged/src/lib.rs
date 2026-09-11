//! Privileged mechanisms accept bounded typed operations, never commands or paths.
use session_protocol::{OperationStatus, SessionIdentity, SystemAction};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
pub mod linux;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub sender: String,
    pub pid: u32,
    pub birth: u64,
    pub session: SessionIdentity,
}

pub struct Operation {
    pub principal: Principal,
    pub action: SystemAction,
    pub status: OperationStatus,
    pub created: Instant,
    pub result: String,
}

impl Operation {
    pub fn authorize_execution(
        &mut self,
        principal: &Principal,
        confirmed: bool,
    ) -> Result<(), &'static str> {
        if self.status != OperationStatus::AwaitingConfirmation {
            return Err("request already consumed or cancelled");
        }
        if self.principal != *principal || self.created.elapsed() > Duration::from_secs(120) {
            self.status = OperationStatus::Cancelled;
            return Err("request identity changed or expired");
        }
        if !confirmed {
            self.status = OperationStatus::Cancelled;
            return Err("authorization cancelled");
        }
        self.status = OperationStatus::Running;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn principal() -> Principal {
        Principal {
            sender: ":1.2".into(),
            pid: 123,
            birth: 99,
            session: SessionIdentity {
                uid: 1001,
                logind_session_id: "9".into(),
            },
        }
    }
    fn operation() -> Operation {
        Operation {
            principal: principal(),
            action: SystemAction::Reboot,
            status: OperationStatus::AwaitingConfirmation,
            created: Instant::now(),
            result: String::new(),
        }
    }
    #[test]
    fn consent_is_single_use_and_bound_to_process_birth_and_session() {
        for alter in [0, 1, 2, 3] {
            let mut p = principal();
            match alter {
                0 => p.birth += 1,
                1 => p.session.uid += 1,
                2 => p.session.logind_session_id = "10".into(),
                _ => p.sender = ":1.3".into(),
            }
            assert!(operation().authorize_execution(&p, true).is_err());
        }
        let mut op = operation();
        op.authorize_execution(&principal(), true).unwrap();
        assert!(op.authorize_execution(&principal(), true).is_err());
    }
    #[test]
    fn cancellation_and_expiry_cannot_be_reapproved() {
        let mut op = operation();
        assert!(op.authorize_execution(&principal(), false).is_err());
        assert!(op.authorize_execution(&principal(), true).is_err());
        let mut op = operation();
        op.created -= Duration::from_secs(121);
        assert!(op.authorize_execution(&principal(), true).is_err());
    }
}
