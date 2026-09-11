//! Private inherited greeter channel, never exposed on the system bus.
use serde::{Deserialize, Serialize};

pub const MAX_FRAME_BYTES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PamStyle {
    EchoOn,
    EchoOff,
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ServerMessage {
    Login {
        message: Option<String>,
    },
    PamPrompt {
        id: u64,
        style: PamStyle,
        text: String,
    },
    Consent {
        id: u64,
        title: String,
        description: String,
    },
    Locked {
        username: String,
    },
    Complete {},
}

/// Deliberately no Debug implementation: PAM responses contain secrets.
#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ClientMessage {
    Ready {},
    Login { username: String },
    PamResponse { id: u64, response: String },
    Consent { id: u64, approved: bool },
    Unlock {},
    Logout {},
    Cancel {},
}
