//! Fixed logind operations. Authorization remains with the system service.
use super::dbus;
use crate::service::ServiceError;
use zbus::blocking::Proxy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    PowerOff,
    Reboot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAvailability {
    Allowed,
    AuthorizationRequired,
    Denied,
    Unavailable,
}

fn decode_availability(value: &str) -> Result<PowerAvailability, ServiceError> {
    match value {
        "yes" => Ok(PowerAvailability::Allowed),
        "challenge" => Ok(PowerAvailability::AuthorizationRequired),
        "no" => Ok(PowerAvailability::Denied),
        "na" => Ok(PowerAvailability::Unavailable),
        _ => Err(ServiceError::Unknown),
    }
}

pub fn availability(action: PowerAction) -> Result<PowerAvailability, ServiceError> {
    let connection = dbus::system()?;
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(dbus::map_error)?;
    let method = match action {
        PowerAction::PowerOff => "CanPowerOff",
        PowerAction::Reboot => "CanReboot",
    };
    let value: String = proxy.call(method, &()).map_err(dbus::map_error)?;
    decode_availability(&value)
}

pub fn execute(action: PowerAction) -> Result<(), ServiceError> {
    execute_with_interaction(action, None)
}

pub fn execute_with_interaction(
    action: PowerAction,
    interaction: Option<std::sync::Arc<dyn super::authorization::Interaction>>,
) -> Result<(), ServiceError> {
    super::identity::LinuxUserContext::current().map_err(|_| ServiceError::PermissionDenied)?;
    let connection = dbus::authorization_system()?;
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(dbus::map_error)?;
    let method = match action {
        PowerAction::PowerOff => "PowerOff",
        PowerAction::Reboot => "Reboot",
    };
    request_with_agent(
        || {
            proxy.call(method, &(true,)).map_err(|error| {
                if matches!(&error, zbus::Error::MethodError(name, _, _)
                    if needs_agent(name.as_str()))
                {
                    PowerRequestError::Challenge
                } else {
                    PowerRequestError::Failed(dbus::map_error(error))
                }
            })
        },
        interaction,
    )
}

fn needs_agent(error_name: &str) -> bool {
    error_name == "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired"
}

enum PowerRequestError {
    Challenge,
    Failed(ServiceError),
}

// logind selects its own base, multiple-session or inhibitor policy. Its explicit
// unresolved challenge means the operation was NOT performed. Only that reply
// permits registering a fallback and repeating this same fixed request once.
// Denial, cancellation, timeout and disconnection never replay a power request.
fn request_with_agent(
    mut request: impl FnMut() -> Result<(), PowerRequestError>,
    interaction: Option<std::sync::Arc<dyn super::authorization::Interaction>>,
) -> Result<(), ServiceError> {
    let lease = interaction
        .map(super::authorization::Lease::begin)
        .transpose()?;
    match request() {
        Ok(()) => Ok(()),
        Err(PowerRequestError::Failed(error)) => Err(error),
        Err(PowerRequestError::Challenge) => {
            let lease = lease.as_ref().ok_or(ServiceError::ServiceUnavailable)?;
            lease.fallback()?;
            request().map_err(|error| match error {
                PowerRequestError::Challenge => ServiceError::ServiceUnavailable,
                PowerRequestError::Failed(error) => error,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Agent {
        events: Arc<Mutex<Vec<&'static str>>>,
        cancelled: bool,
    }
    impl super::super::authorization::Interaction for Agent {
        fn begin(&self) -> Result<(), ServiceError> {
            self.events.lock().unwrap().push("pause");
            Ok(())
        }
        fn fallback(&self) -> Result<(), ServiceError> {
            self.events.lock().unwrap().push("fallback");
            Ok(())
        }
        fn finish(&self) {
            self.events.lock().unwrap().push("restore");
        }
        fn cancelled(&self) -> bool {
            self.cancelled
        }
    }

    #[test]
    fn logind_challenge_uses_existing_agent_before_one_fallback_request() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut calls = 0;
        assert_eq!(
            request_with_agent(
                || {
                    calls += 1;
                    events.lock().unwrap().push("logind");
                    if calls == 1 {
                        Err(PowerRequestError::Challenge)
                    } else {
                        Ok(())
                    }
                },
                Some(Arc::new(Agent {
                    events: events.clone(),
                    cancelled: false
                })),
            ),
            Ok(())
        );
        assert_eq!(
            *events.lock().unwrap(),
            ["pause", "logind", "fallback", "logind", "restore"]
        );
    }

    #[test]
    fn completed_denied_or_uncertain_power_requests_are_never_replayed() {
        for outcome in [
            Ok(()),
            Err(ServiceError::PermissionDenied),
            Err(ServiceError::AuthorizationCancelled),
            Err(ServiceError::Timeout),
            Err(ServiceError::BackendDisconnected),
        ] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let mut calls = 0;
            let result = request_with_agent(
                || {
                    calls += 1;
                    outcome.map_err(PowerRequestError::Failed)
                },
                Some(Arc::new(Agent {
                    events: events.clone(),
                    cancelled: false,
                })),
            );
            assert_eq!(result, outcome);
            assert_eq!(calls, 1);
            assert_eq!(*events.lock().unwrap(), ["pause", "restore"]);
        }
        assert!(needs_agent(
            "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired"
        ));
        assert!(!needs_agent("org.freedesktop.DBus.Error.AccessDenied"));
    }

    #[test]
    fn cancelled_handoff_does_not_retry_and_unresolved_fallback_is_bounded() {
        for cancelled in [false, true] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let mut calls = 0;
            let result = request_with_agent(
                || {
                    calls += 1;
                    Err(PowerRequestError::Challenge)
                },
                Some(Arc::new(Agent {
                    events: events.clone(),
                    cancelled,
                })),
            );
            assert_eq!(calls, if cancelled { 0 } else { 2 });
            assert_eq!(
                result,
                Err(if cancelled {
                    ServiceError::AuthorizationCancelled
                } else {
                    ServiceError::ServiceUnavailable
                })
            );
            if cancelled {
                assert!(events.lock().unwrap().is_empty());
            } else {
                assert_eq!(events.lock().unwrap().last(), Some(&"restore"));
            }
        }
    }
    #[test]
    fn availability_preserves_authorization_and_missing_service_states() {
        assert_eq!(decode_availability("yes"), Ok(PowerAvailability::Allowed));
        assert_eq!(
            decode_availability("challenge"),
            Ok(PowerAvailability::AuthorizationRequired)
        );
        assert_eq!(decode_availability("no"), Ok(PowerAvailability::Denied));
        assert_eq!(
            decode_availability("na"),
            Ok(PowerAvailability::Unavailable)
        );
        assert_eq!(
            decode_availability("unexpected"),
            Err(ServiceError::Unknown)
        );
    }
}
