use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::schema::{USERS_SCHEMA_VERSION, VersionedDocument};
use crate::{AppearanceConfig, SystemStatusDashboardConfig};

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

    fn upgrade_schema(&mut self) {
        self.normalize();
    }
}

impl UsersDocument {
    pub(crate) fn normalize(&mut self) -> bool {
        if self.schema_version == USERS_SCHEMA_VERSION {
            return false;
        }

        if self.schema_version < 3 {
            for user in &mut self.users {
                if user.appearance.is_legacy_default() {
                    user.appearance = AppearanceConfig::default();
                }
            }
        }
        if self.schema_version < 4 {
            for user in &mut self.users {
                user.system_status_dashboard = SystemStatusDashboardConfig::for_role(&user.role);
            }
        }
        self.schema_version = USERS_SCHEMA_VERSION;
        true
    }

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
                    appearance: AppearanceConfig::default(),
                    system_status_dashboard: SystemStatusDashboardConfig::for_role("User"),
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
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub system_status_dashboard: SystemStatusDashboardConfig,
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

#[cfg(test)]
mod glacier_user_migration_tests {
    use super::*;
    use crate::{BorderColor, BorderShape, IconDisplayMode, MotionPreference};

    fn user(appearance: AppearanceConfig) -> UserRecord {
        UserRecord {
            id: "id".into(),
            username: "user".into(),
            display_name: "User".into(),
            role: "User".into(),
            password_hash: String::new(),
            password_hint: None,
            appearance,
            system_status_dashboard: SystemStatusDashboardConfig::for_role("User"),
            enabled: true,
            failed_login_attempts: 0,
            locked_until_epoch_ms: None,
            created_at_epoch_ms: 0,
            updated_at_epoch_ms: 0,
            last_login_at_epoch_ms: None,
        }
    }

    #[test]
    fn users_schema_two_exact_default_migrates_but_custom_value_does_not() {
        let legacy = AppearanceConfig {
            border_shape: BorderShape::Rounded,
            border_color: BorderColor::White,
            accent_color: BorderColor::Cyan,
            icon_display_mode: IconDisplayMode::Image,
            motion_preference: MotionPreference::Full,
            animation_speed_percent: crate::DEFAULT_ANIMATION_SPEED_PERCENT,
        };
        let custom = AppearanceConfig {
            accent_color: BorderColor::Blue,
            ..legacy.clone()
        };
        let mut document = UsersDocument {
            schema_version: 2,
            users: vec![user(legacy), user(custom.clone())],
        };
        assert!(document.normalize());
        assert_eq!(document.users[0].appearance, AppearanceConfig::default());
        assert_eq!(document.users[1].appearance, custom);
        assert_eq!(document.schema_version, USERS_SCHEMA_VERSION);
    }

    #[test]
    fn users_schema_three_receives_role_specific_dashboard_defaults() {
        let mut admin = user(AppearanceConfig::default());
        admin.role = "Admin".into();
        let mut ordinary = user(AppearanceConfig::default());
        ordinary.system_status_dashboard.widgets = vec![crate::SystemStatusWidgetKind::Activity];
        let mut document = UsersDocument {
            schema_version: 3,
            users: vec![admin, ordinary],
        };
        assert!(document.normalize());
        assert!(
            document.users[0]
                .system_status_dashboard
                .widgets
                .contains(&crate::SystemStatusWidgetKind::TopProcesses)
        );
        assert!(
            document.users[1]
                .system_status_dashboard
                .widgets
                .contains(&crate::SystemStatusWidgetKind::Diagnostics)
        );
        assert!(
            !document.users[1]
                .system_status_dashboard
                .widgets
                .contains(&crate::SystemStatusWidgetKind::Activity)
        );
    }

    #[test]
    fn missing_dashboard_field_uses_ordinary_default() {
        let value = serde_json::to_value(user(AppearanceConfig::default())).unwrap();
        let mut object = value.as_object().unwrap().clone();
        object.remove("system_status_dashboard");
        let record: UserRecord = serde_json::from_value(object.into()).unwrap();
        assert_eq!(
            record.system_status_dashboard,
            SystemStatusDashboardConfig::for_role("User")
        );
    }
}
