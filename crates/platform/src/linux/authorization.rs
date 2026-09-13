#[path = "authorization_tty.rs"]
mod tty;
pub use tty::{TextAgent, controlling_terminal};
// Authorization is evaluated by polkit for the connection making the service request.
use super::dbus;
use crate::service::ServiceError;
use std::{collections::HashMap, sync::Arc};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::Value,
};

#[derive(Clone, Copy)]
pub enum Action {
    Update,
}
impl Action {
    fn policy(self) -> &'static str {
        match self {
            Self::Update => "org.freedesktop.packagekit.system-update",
        }
    }
}

/// Implemented by the terminal owner. No password or prompt response crosses this interface.
pub trait Interaction: Send + Sync {
    fn begin(&self) -> Result<(), ServiceError>;
    fn fallback(&self) -> Result<(), ServiceError>;
    fn finish(&self);
    fn cancelled(&self) -> bool {
        false
    }
}

pub struct Lease {
    interaction: Arc<dyn Interaction>,
}
impl Lease {
    pub(super) fn begin(interaction: Arc<dyn Interaction>) -> Result<Self, ServiceError> {
        if interaction.cancelled() {
            return Err(ServiceError::AuthorizationCancelled);
        }
        interaction.begin()?;
        let lease = Self { interaction };
        if lease.interaction.cancelled() {
            return Err(ServiceError::AuthorizationCancelled);
        }
        Ok(lease)
    }

    pub(super) fn fallback(&self) -> Result<(), ServiceError> {
        if self.interaction.cancelled() {
            return Err(ServiceError::AuthorizationCancelled);
        }
        self.interaction.fallback()?;
        if self.interaction.cancelled() {
            return Err(ServiceError::AuthorizationCancelled);
        }
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.interaction.finish();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Allowed,
    Challenge,
    Denied,
    Cancelled,
}
fn decision(allowed: bool, challenge: bool, details: &HashMap<String, String>) -> Decision {
    if allowed {
        Decision::Allowed
    } else if details
        .get("polkit.dismissed")
        .is_some_and(|value| value == "true")
    {
        Decision::Cancelled
    } else if challenge {
        Decision::Challenge
    } else {
        Decision::Denied
    }
}

fn check(
    connection: &Connection,
    action: Action,
    interactive: bool,
) -> Result<Decision, ServiceError> {
    let name = connection
        .unique_name()
        .ok_or(ServiceError::BackendDisconnected)?;
    let subject = (
        "system-bus-name",
        HashMap::from([("name", Value::from(name.as_str()))]),
    );
    let authority_connection = dbus::authorization_system()?;
    let proxy = Proxy::new(
        &authority_connection,
        "org.freedesktop.PolicyKit1",
        "/org/freedesktop/PolicyKit1/Authority",
        "org.freedesktop.PolicyKit1.Authority",
    )
    .map_err(dbus::map_error)?;
    let (result,): ((bool, bool, HashMap<String, String>),) = proxy
        .call(
            "CheckAuthorization",
            &(
                subject,
                action.policy(),
                HashMap::<String, String>::new(),
                u32::from(interactive),
                "",
            ),
        )
        .map_err(dbus::map_error)?;
    Ok(decision(result.0, result.1, &result.2))
}

/// A challenge without an available agent remains a challenge after polkit's interactive check.
/// Denial or dismissal by an existing agent is final: never replace it with our own prompt.
pub fn prepare(
    connection: &Connection,
    action: Action,
    interaction: Option<Arc<dyn Interaction>>,
) -> Result<Option<Lease>, ServiceError> {
    match check(connection, action, false)? {
        Decision::Allowed => return Ok(None),
        Decision::Cancelled => return Err(ServiceError::AuthorizationCancelled),
        Decision::Denied => return Err(ServiceError::PermissionDenied),
        Decision::Challenge => {}
    }
    let Some(interaction) = interaction else {
        // Headless clients may use an existing system agent through the actual service request.
        return Ok(None);
    };
    let lease = Lease::begin(interaction)?;
    let decision = check(connection, action, true)?;
    if lease.interaction.cancelled() {
        return Err(ServiceError::AuthorizationCancelled);
    }
    match decision {
        Decision::Allowed => Ok(Some(lease)),
        Decision::Challenge => {
            lease.fallback()?;
            Ok(Some(lease))
        }
        Decision::Denied => Err(ServiceError::PermissionDenied),
        Decision::Cancelled => Err(ServiceError::AuthorizationCancelled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_an_unresolved_challenge_can_use_fallback() {
        let details = HashMap::new();
        assert_eq!(decision(true, false, &details), Decision::Allowed);
        assert_eq!(decision(false, true, &details), Decision::Challenge);
        assert_eq!(decision(false, false, &details), Decision::Denied);
        assert_eq!(
            decision(
                false,
                true,
                &HashMap::from([("polkit.dismissed".into(), "true".into())])
            ),
            Decision::Cancelled
        );
    }
}
