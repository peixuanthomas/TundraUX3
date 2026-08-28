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
        let mut changed = self.schema_version != USERS_SCHEMA_VERSION;

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
        for user in &mut self.users {
            let dashboard_before = user.system_status_dashboard.clone();
            user.system_status_dashboard.normalize();
            changed |= user.system_status_dashboard != dashboard_before;
        }
        self.schema_version = USERS_SCHEMA_VERSION;
        changed
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
    use crate::{
        BorderColor, BorderShape, DashboardLayout, IconDisplayMode, MotionPreference,
        SystemStatusWidgetKind, SystemStatusWidgetSize, WidgetPlacement,
    };

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

    #[test]
    fn current_schema_repairs_dashboard_once_without_replacing_custom_layout_or_user_fields() {
        let appearance = AppearanceConfig {
            border_shape: BorderShape::Square,
            accent_color: BorderColor::LightMagenta,
            ..AppearanceConfig::default()
        };
        let mut record = user(appearance.clone());
        record.id = "custom-id".into();
        record.username = "custom-user".into();
        record.display_name = "Custom User".into();
        record.role = "Admin".into();
        record.password_hash = "preserved-hash".into();
        record.password_hint = Some("preserved hint".into());
        record.failed_login_attempts = 2;
        record.locked_until_epoch_ms = Some(55);
        record.created_at_epoch_ms = 11;
        record.updated_at_epoch_ms = 22;
        record.last_login_at_epoch_ms = Some(33);
        record.system_status_dashboard = SystemStatusDashboardConfig {
            widgets: vec![
                SystemStatusWidgetKind::Cpu,
                SystemStatusWidgetKind::Cpu,
                SystemStatusWidgetKind::Memory,
                SystemStatusWidgetKind::Storage,
            ],
            wide: DashboardLayout {
                placements: vec![
                    WidgetPlacement {
                        kind: SystemStatusWidgetKind::Cpu,
                        column: 0,
                        row: 0,
                        size: SystemStatusWidgetSize::Wide,
                    },
                    WidgetPlacement {
                        kind: SystemStatusWidgetKind::Cpu,
                        column: 4,
                        row: 0,
                        size: SystemStatusWidgetSize::Small,
                    },
                    WidgetPlacement {
                        kind: SystemStatusWidgetKind::Memory,
                        column: 7,
                        row: 0,
                        size: SystemStatusWidgetSize::Small,
                    },
                    WidgetPlacement {
                        kind: SystemStatusWidgetKind::Storage,
                        column: 6,
                        row: 0,
                        size: SystemStatusWidgetSize::Small,
                    },
                ],
            },
            narrow: DashboardLayout {
                placements: vec![
                    WidgetPlacement {
                        kind: SystemStatusWidgetKind::Memory,
                        column: 9,
                        row: 0,
                        size: SystemStatusWidgetSize::Wide,
                    },
                    WidgetPlacement {
                        kind: SystemStatusWidgetKind::Memory,
                        column: 0,
                        row: 2,
                        size: SystemStatusWidgetSize::Small,
                    },
                    WidgetPlacement {
                        kind: SystemStatusWidgetKind::Cpu,
                        column: 0,
                        row: 4,
                        size: SystemStatusWidgetSize::Large,
                    },
                ],
            },
        };
        let mut document = UsersDocument {
            schema_version: USERS_SCHEMA_VERSION,
            users: vec![record],
        };

        assert!(document.normalize());
        let repaired = &document.users[0];
        assert_eq!(
            repaired.system_status_dashboard.widgets,
            vec![
                SystemStatusWidgetKind::Cpu,
                SystemStatusWidgetKind::Memory,
                SystemStatusWidgetKind::Storage,
            ]
        );
        assert_eq!(
            repaired.system_status_dashboard.wide.placements,
            vec![
                WidgetPlacement {
                    kind: SystemStatusWidgetKind::Cpu,
                    column: 0,
                    row: 0,
                    size: SystemStatusWidgetSize::Wide,
                },
                WidgetPlacement {
                    kind: SystemStatusWidgetKind::Storage,
                    column: 6,
                    row: 0,
                    size: SystemStatusWidgetSize::Small,
                },
                WidgetPlacement {
                    kind: SystemStatusWidgetKind::Memory,
                    column: 4,
                    row: 1,
                    size: SystemStatusWidgetSize::Small,
                },
            ]
        );
        assert_eq!(
            repaired.system_status_dashboard.narrow.placements,
            vec![
                WidgetPlacement {
                    kind: SystemStatusWidgetKind::Memory,
                    column: 0,
                    row: 0,
                    size: SystemStatusWidgetSize::Wide,
                },
                WidgetPlacement {
                    kind: SystemStatusWidgetKind::Storage,
                    column: 0,
                    row: 2,
                    size: SystemStatusWidgetSize::Small,
                },
                WidgetPlacement {
                    kind: SystemStatusWidgetKind::Cpu,
                    column: 0,
                    row: 4,
                    size: SystemStatusWidgetSize::Large,
                },
            ]
        );
        assert_eq!(repaired.id, "custom-id");
        assert_eq!(repaired.username, "custom-user");
        assert_eq!(repaired.display_name, "Custom User");
        assert_eq!(repaired.role, "Admin");
        assert_eq!(repaired.password_hash, "preserved-hash");
        assert_eq!(repaired.password_hint.as_deref(), Some("preserved hint"));
        assert_eq!(repaired.appearance, appearance);
        assert!(repaired.enabled);
        assert_eq!(repaired.failed_login_attempts, 2);
        assert_eq!(repaired.locked_until_epoch_ms, Some(55));
        assert_eq!(repaired.created_at_epoch_ms, 11);
        assert_eq!(repaired.updated_at_epoch_ms, 22);
        assert_eq!(repaired.last_login_at_epoch_ms, Some(33));
        assert!(!document.normalize());
    }
}
