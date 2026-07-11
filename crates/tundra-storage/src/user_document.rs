use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::schema::{USERS_SCHEMA_VERSION, VersionedDocument};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsersDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub users: Vec<UserRecord>,
}

impl Default for UsersDocument {
    fn default() -> Self {
        Self {
            schema_version: USERS_SCHEMA_VERSION,
            users: Vec::new(),
        }
    }
}

impl VersionedDocument for UsersDocument {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

impl UsersDocument {
    pub(crate) fn from_legacy_v1(legacy: UsersV1Document) -> Self {
        let now = unix_millis();
        let users = legacy
            .users
            .into_iter()
            .enumerate()
            .map(|(index, username)| {
                let id = format!("legacy-user-{}", index + 1);
                UserRecord {
                    id,
                    username: username.clone(),
                    display_name: username,
                    role: "User".to_string(),
                    password_hash: String::new(),
                    password_hint: None,
                    enabled: false,
                    failed_login_attempts: 0,
                    locked_until_epoch_ms: None,
                    created_at_epoch_ms: now,
                    updated_at_epoch_ms: now,
                    last_login_at_epoch_ms: None,
                }
            })
            .collect();

        Self {
            schema_version: USERS_SCHEMA_VERSION,
            users,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserRecord {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub password_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hint: Option<String>,
    pub enabled: bool,
    pub failed_login_attempts: u32,
    pub locked_until_epoch_ms: Option<u64>,
    pub created_at_epoch_ms: u64,
    pub updated_at_epoch_ms: u64,
    pub last_login_at_epoch_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct UsersV1Document {
    pub schema_version: u32,
    #[serde(default)]
    pub users: Vec<String>,
}

impl VersionedDocument for UsersV1Document {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .ok()
        .and_then(|millis| u64::try_from(millis).ok())
        .unwrap_or(0)
}
