//! Internal messages for the guardian, the Shell, app hosts, and workspace services.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

mod service;
mod supervision;
pub mod transport;
mod window;

pub use service::*;
pub use supervision::*;
pub use window::*;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_CONTROL_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_WINDOW_FRAME_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_SERVICE_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// Identifies a guardian invocation, including across PID reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RunId(pub u128);

impl fmt::Display for RunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:032x}", self.0)
    }
}

impl From<RunId> for String {
    fn from(value: RunId) -> Self {
        value.to_string()
    }
}

impl TryFrom<String> for RunId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl FromStr for RunId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.len() > 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid workspace run ID");
        }
        u128::from_str_radix(value, 16)
            .map(Self)
            .map_err(|_| "invalid workspace run ID")
    }
}

/// Identifies a single app opening. IDs belong to one run and are never recycled within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub u64);

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for SessionId {
    type Err = std::num::ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

/// Identifies an incarnation of a worker independently of its operating-system PID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProcessGeneration(pub u64);

/// Advances when the Shell changes the window's geometry or instance of a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayoutGeneration(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerRole {
    Shell,
    App(SessionId),
    Services,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WorkerId {
    pub run: RunId,
    pub role: WorkerRole,
    pub generation: ProcessGeneration,
}

/// Confirms the expected identity after both peers check the operating-system connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub worker: WorkerId,
    pub pid: u32,
    pub protocol: u32,
}

impl Hello {
    pub fn new(worker: WorkerId) -> Self {
        Self {
            worker,
            pid: std::process::id(),
            protocol: PROTOCOL_VERSION,
        }
    }
}
