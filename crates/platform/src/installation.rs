//! Installation provenance, separate from update transport and write access.
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateBackend {
    PortableUser,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    pub backend: UpdateBackend,
    pub directory: Option<PathBuf>,
    pub reason: Option<String>,
}

impl Installation {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            backend: UpdateBackend::Unavailable,
            directory: None,
            reason: Some(reason.into()),
        }
    }
}

pub const PORTABLE_MARKER: &str = "tundra-installation.json";
pub const PORTABLE_MARKER_CONTENT: &str = "{\"format\":1,\"kind\":\"portable-user\"}\n";

pub fn current_installation() -> Installation {
    #[cfg(target_os = "linux")]
    {
        crate::linux::installation::detect_current()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Installation::unavailable(
            "Linux portable installation detection is unavailable on this platform",
        )
    }
}
