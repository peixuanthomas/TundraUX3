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
#[path = "../../tests/unit/linux/power/tests.rs"]
mod tests;
