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
    super::identity::LinuxUserContext::current().map_err(|_| ServiceError::PermissionDenied)?;
    let connection = dbus::system()?;
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
    proxy.call(method, &(true,)).map_err(dbus::map_error)
}

#[cfg(test)]
mod tests {
    use super::*;
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
