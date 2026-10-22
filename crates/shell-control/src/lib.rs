//! The local Shell control protocol. Coordinates count terminal cells, not pixels.
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(self.width)
            && y < self.y.saturating_add(self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub parent: Option<String>,
    pub role: String,
    pub name: String,
    pub text: String,
    pub rect: Rect,
    pub focused: bool,
    pub selected: bool,
    pub enabled: bool,
    pub actionable: bool,
    pub sensitive: bool,
    pub actions: Vec<String>,
}

impl Node {
    pub fn new(
        id: impl Into<String>,
        role: impl Into<String>,
        name: impl Into<String>,
        rect: Rect,
    ) -> Self {
        Self {
            id: id.into(),
            parent: None,
            role: role.into(),
            name: name.into(),
            text: String::new(),
            rect,
            focused: false,
            selected: false,
            enabled: true,
            actionable: true,
            sensitive: false,
            actions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub session: String,
    pub number: u64,
    pub page: String,
    pub width: u16,
    pub height: u16,
    pub animation_running: bool,
    #[serde(default)]
    pub background_tasks_running: bool,
    pub nodes: Vec<Node>,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Target {
    Id { id: String },
    Position { x: u16, y: u16 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Press,
    Repeat,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Action {
    Snapshot,
    Click {
        target: Target,
        #[serde(default)]
        button: Button,
        #[serde(default)]
        double: bool,
    },
    Move {
        target: Target,
    },
    Drag {
        from: Target,
        to: Target,
        #[serde(default = "default_drag_ms")]
        duration_ms: u64,
        #[serde(default)]
        button: Button,
    },
    Scroll {
        target: Target,
        direction: String,
        #[serde(default = "default_count")]
        count: u16,
    },
    Key {
        key: String,
        #[serde(default)]
        modifiers: Vec<String>,
        #[serde(default)]
        phase: Phase,
    },
    Text {
        text: String,
    },
}
fn default_drag_ms() -> u64 {
    300
}
fn default_count() -> u16 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub protocol: u32,
    pub snapshot: Option<u64>,
    #[serde(flatten)]
    pub action: Action,
}

impl Request {
    pub fn new(action: Action) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            snapshot: None,
            action,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub protocol: u32,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ControlError>,
}
impl Response {
    pub fn success(snapshot: Snapshot) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            ok: true,
            closed: false,
            snapshot: Some(snapshot),
            error: None,
        }
    }
    pub fn closed() -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            ok: true,
            closed: true,
            snapshot: None,
            error: None,
        }
    }
    pub fn failure(code: &str, message: impl Into<String>) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            ok: false,
            closed: false,
            snapshot: None,
            error: Some(ControlError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session: String,
    pub pid: u32,
    pub endpoint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_round_trip_and_defaults() {
        let value = r#"{"protocol":1,"command":"click","target":{"id":"settings.system"}}"#;
        let request: Request = serde_json::from_str(value).unwrap();
        assert!(matches!(
            request.action,
            Action::Click {
                button: Button::Left,
                double: false,
                ..
            }
        ));
        assert_eq!(
            request,
            serde_json::from_str(&serde_json::to_string(&request).unwrap()).unwrap()
        );
    }
}
