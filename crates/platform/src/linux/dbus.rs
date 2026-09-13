//! All connections used by Linux service adapters have a bounded method timeout.
use crate::service::ServiceError;
use std::time::Duration;
use zbus::blocking::{Connection, connection::Builder};

pub fn system() -> Result<Connection, ServiceError> {
    Builder::system()
        .map_err(map_error)?
        .method_timeout(Duration::from_secs(30))
        .build()
        .map_err(map_error)
}

pub fn session() -> Result<Connection, ServiceError> {
    Builder::session()
        .map_err(map_error)?
        .method_timeout(Duration::from_secs(5))
        .build()
        .map_err(map_error)
}

pub fn map_error(error: zbus::Error) -> ServiceError {
    match error {
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.AccessDenied"
            | "org.freedesktop.PolicyKit1.Error.NotAuthorized"
            | "org.freedesktop.PackageKit.Transaction.NotAuthorized"
            | "org.freedesktop.login1.NotAuthorized" => ServiceError::PermissionDenied,
            "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.Spawn.ServiceNotFound" => {
                ServiceError::ServiceUnavailable
            }
            "org.freedesktop.DBus.Error.NoReply" | "org.freedesktop.DBus.Error.Timeout" => {
                ServiceError::Timeout
            }
            "org.freedesktop.DBus.Error.Disconnected" => ServiceError::BackendDisconnected,
            "org.freedesktop.DBus.Error.UnknownMethod"
            | "org.freedesktop.DBus.Error.NotSupported" => ServiceError::Unsupported,
            _ => ServiceError::Unknown,
        },
        zbus::Error::InputOutput(error) => match error.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                ServiceError::ServiceUnavailable
            }
            std::io::ErrorKind::PermissionDenied => ServiceError::PermissionDenied,
            std::io::ErrorKind::TimedOut => ServiceError::Timeout,
            _ => ServiceError::BackendDisconnected,
        },
        _ => ServiceError::Unknown,
    }
}

/// Report active names without activating missing services.
pub fn has_owner(connection: &Connection, name: &str) -> Result<bool, ServiceError> {
    let bus = zbus::blocking::fdo::DBusProxy::new(connection).map_err(map_error)?;
    bus.name_has_owner(name.try_into().map_err(|_| ServiceError::Unknown)?)
        .map_err(|error| map_error(error.into()))
}

/// Read activatable names as well as active owners; diagnosis must not start services.
pub fn name_available(connection: &Connection, name: &str) -> Result<bool, ServiceError> {
    if has_owner(connection, name)? {
        return Ok(true);
    }
    let bus = zbus::blocking::fdo::DBusProxy::new(connection).map_err(map_error)?;
    Ok(bus
        .list_activatable_names()
        .map_err(|e| map_error(e.into()))?
        .iter()
        .any(|n| n.as_str() == name))
}
