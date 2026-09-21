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
