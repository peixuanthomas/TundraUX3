use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum WatchdogError {
    AlreadyInstalled,
    RuntimeAlreadyStarted,
    RuntimeStopped,
    NotInstalled,
    InvalidIdentifier(String),
    ConflictingAppRegistration(String),
    InvalidTaskPolicy(String),
    ThreadSpawn(std::io::Error),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    Serialization(serde_json::Error),
    ChannelClosed,
    IncidentTimeout,
    TaskPanicked,
    OperationAlreadyExists(String),
    RecoveryBlocked(String),
    Writer(String),
}

impl fmt::Display for WatchdogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInstalled => {
                formatter.write_str("a process watchdog is already installed")
            }
            Self::RuntimeAlreadyStarted => {
                formatter.write_str("a watchdog runtime has already been started in this process")
            }
            Self::RuntimeStopped => formatter.write_str("the watchdog runtime has stopped"),
            Self::NotInstalled => formatter.write_str("the process watchdog is not installed"),
            Self::InvalidIdentifier(value) => {
                write!(formatter, "invalid watchdog identifier: {value}")
            }
            Self::ConflictingAppRegistration(id) => {
                write!(
                    formatter,
                    "app {id} is already registered with different metadata"
                )
            }
            Self::InvalidTaskPolicy(message) => formatter.write_str(message),
            Self::ThreadSpawn(error) => {
                write!(formatter, "could not spawn managed thread: {error}")
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "{operation} failed for {}: {source}",
                path.display()
            ),
            Self::Serialization(error) => {
                write!(formatter, "watchdog serialization failed: {error}")
            }
            Self::ChannelClosed => formatter.write_str("the watchdog writer thread has stopped"),
            Self::IncidentTimeout => formatter.write_str("timed out waiting for the crash report"),
            Self::TaskPanicked => formatter.write_str("the managed task panicked"),
            Self::OperationAlreadyExists(id) => {
                write!(formatter, "operation journal {id} already exists")
            }
            Self::RecoveryBlocked(message) => formatter.write_str(message),
            Self::Writer(message) => write!(formatter, "watchdog writer failed: {message}"),
        }
    }
}

impl std::error::Error for WatchdogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ThreadSpawn(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for WatchdogError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}
