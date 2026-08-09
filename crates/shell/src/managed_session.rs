use serde::Serialize;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const MANAGED_SESSION_ENV: &str = "TUNDRA_MANAGED_SESSION";
pub const HOST_PROTOCOL_ENV: &str = "TUNDRA_HOST_PROTOCOL";
pub const SESSION_ID_ENV: &str = "TUNDRA_SESSION_ID";
pub const SESSION_OUTCOME_PATH_ENV: &str = "TUNDRA_SESSION_OUTCOME_PATH";
pub const SESSION_SHUTDOWN_PATH_ENV: &str = "TUNDRA_SESSION_SHUTDOWN_PATH";
pub const HOST_PROTOCOL_VERSION: &str = "1";
pub const MANAGED_RESTART_EXIT_CODE: i32 = 74;
pub const MANAGED_RESET_EXIT_CODE: i32 = 75;
pub const MANAGED_PROTOCOL_ERROR_EXIT_CODE: i32 = 78;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedSession {
    session_id: String,
    outcome_path: PathBuf,
    shutdown_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedSessionError {
    Missing(&'static str),
    Invalid(&'static str),
    UnsupportedProtocol(String),
}

impl std::fmt::Display for ManagedSessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(name) => write!(formatter, "managed session is missing {name}"),
            Self::Invalid(name) => write!(formatter, "managed session has an invalid {name}"),
            Self::UnsupportedProtocol(version) => {
                write!(formatter, "unsupported Tundra host protocol {version}")
            }
        }
    }
}

impl std::error::Error for ManagedSessionError {}

#[derive(Debug, Serialize)]
struct SessionOutcome<'a> {
    schema_version: u8,
    session_id: &'a str,
    origin: &'static str,
    kind: &'static str,
    code: i32,
}

impl ManagedSession {
    pub fn from_environment() -> Result<Option<Self>, ManagedSessionError> {
        Self::from_values(
            std::env::var_os(MANAGED_SESSION_ENV),
            std::env::var_os(HOST_PROTOCOL_ENV),
            std::env::var_os(SESSION_ID_ENV),
            std::env::var_os(SESSION_OUTCOME_PATH_ENV),
            std::env::var_os(SESSION_SHUTDOWN_PATH_ENV),
        )
    }

    fn from_values(
        enabled: Option<OsString>,
        protocol: Option<OsString>,
        session_id: Option<OsString>,
        outcome_path: Option<OsString>,
        shutdown_path: Option<OsString>,
    ) -> Result<Option<Self>, ManagedSessionError> {
        let Some(enabled) = enabled else {
            return Ok(None);
        };
        if enabled != "1" {
            return Err(ManagedSessionError::Invalid(MANAGED_SESSION_ENV));
        }

        let protocol = protocol.ok_or(ManagedSessionError::Missing(HOST_PROTOCOL_ENV))?;
        let protocol = protocol
            .into_string()
            .map_err(|_| ManagedSessionError::Invalid(HOST_PROTOCOL_ENV))?;
        if protocol != HOST_PROTOCOL_VERSION {
            return Err(ManagedSessionError::UnsupportedProtocol(protocol));
        }

        let session_id = session_id.ok_or(ManagedSessionError::Missing(SESSION_ID_ENV))?;
        let session_id = session_id
            .into_string()
            .map_err(|_| ManagedSessionError::Invalid(SESSION_ID_ENV))?;
        if !valid_session_id(&session_id) {
            return Err(ManagedSessionError::Invalid(SESSION_ID_ENV));
        }

        let outcome_path = PathBuf::from(
            outcome_path.ok_or(ManagedSessionError::Missing(SESSION_OUTCOME_PATH_ENV))?,
        );
        if !outcome_path.is_absolute() || outcome_path.file_name().is_none() {
            return Err(ManagedSessionError::Invalid(SESSION_OUTCOME_PATH_ENV));
        }
        let shutdown_path = shutdown_path
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() && path.file_name().is_some() {
                    Ok(path)
                } else {
                    Err(ManagedSessionError::Invalid(SESSION_SHUTDOWN_PATH_ENV))
                }
            })
            .transpose()?;

        Ok(Some(Self {
            session_id,
            outcome_path,
            shutdown_path,
        }))
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn outcome_path(&self) -> &Path {
        &self.outcome_path
    }

    pub fn shutdown_path(&self) -> Option<&Path> {
        self.shutdown_path.as_deref()
    }

    pub fn write_exit(&self, code: i32) -> io::Result<()> {
        let parent = self.outcome_path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "outcome path has no parent")
        })?;
        fs::create_dir_all(parent)?;

        let outcome = SessionOutcome {
            schema_version: 1,
            session_id: &self.session_id,
            origin: "shell",
            kind: "exit",
            code,
        };
        let bytes = serde_json::to_vec(&outcome)
            .map_err(|error| io::Error::other(format!("serialize session outcome: {error}")))?;
        let temporary = temporary_outcome_path(&self.outcome_path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);

        if self.outcome_path.exists() {
            fs::remove_file(&self.outcome_path)?;
        }
        fs::rename(&temporary, &self.outcome_path)
    }
}

fn valid_session_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn temporary_outcome_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("outcome.json");
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managed(path: &Path) -> ManagedSession {
        ManagedSession::from_values(
            Some(OsString::from("1")),
            Some(OsString::from(HOST_PROTOCOL_VERSION)),
            Some(OsString::from("session-123")),
            Some(path.as_os_str().to_os_string()),
            Some(path.with_extension("shutdown").into_os_string()),
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn absent_marker_keeps_direct_shell_mode() {
        assert_eq!(
            ManagedSession::from_values(None, None, None, None, None),
            Ok(None)
        );
    }

    #[test]
    fn managed_mode_requires_exact_protocol_and_safe_id() {
        let path = std::env::temp_dir().join("tundra-outcome.json");
        let unsupported = ManagedSession::from_values(
            Some(OsString::from("1")),
            Some(OsString::from("2")),
            Some(OsString::from("session")),
            Some(path.as_os_str().to_os_string()),
            None,
        );
        assert!(matches!(
            unsupported,
            Err(ManagedSessionError::UnsupportedProtocol(version)) if version == "2"
        ));

        let unsafe_id = ManagedSession::from_values(
            Some(OsString::from("1")),
            Some(OsString::from(HOST_PROTOCOL_VERSION)),
            Some(OsString::from("../session")),
            Some(path.as_os_str().to_os_string()),
            None,
        );
        assert_eq!(unsafe_id, Err(ManagedSessionError::Invalid(SESSION_ID_ENV)));

        let unsafe_shutdown = ManagedSession::from_values(
            Some(OsString::from("1")),
            Some(OsString::from(HOST_PROTOCOL_VERSION)),
            Some(OsString::from("session")),
            Some(path.as_os_str().to_os_string()),
            Some(OsString::from("relative/shutdown")),
        );
        assert_eq!(
            unsafe_shutdown,
            Err(ManagedSessionError::Invalid(SESSION_SHUTDOWN_PATH_ENV))
        );
    }

    #[test]
    fn outcome_is_written_atomically_with_the_fixed_schema() {
        let root = std::env::temp_dir().join(format!(
            "tundra-shell-managed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("outcome.json");
        let session = managed(&path);
        assert_eq!(
            session.shutdown_path(),
            Some(path.with_extension("shutdown").as_path())
        );
        session.write_exit(MANAGED_RESTART_EXIT_CODE).unwrap();

        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["session_id"], "session-123");
        assert_eq!(value["origin"], "shell");
        assert_eq!(value["kind"], "exit");
        assert_eq!(value["code"], MANAGED_RESTART_EXIT_CODE);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

        fs::remove_dir_all(root).unwrap();
    }
}
