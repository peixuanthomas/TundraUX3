use super::*;

fn legacy() -> AppearanceConfig {
    legacy_appearance_default()
}

#[test]
fn animation_speed_defaults_and_clamps_when_loaded() {
    let missing: AppearanceConfig = serde_json::from_str("{}").expect("default appearance");
    let below: AppearanceConfig =
        serde_json::from_str(r#"{"animation_speed_percent":0}"#).expect("lower bound appearance");
    let above: AppearanceConfig =
        serde_json::from_str(r#"{"animation_speed_percent":999}"#).expect("upper bound appearance");

    assert_eq!(
        missing.animation_speed_percent,
        DEFAULT_ANIMATION_SPEED_PERCENT
    );
    assert_eq!(below.animation_speed_percent, MIN_ANIMATION_SPEED_PERCENT);
    assert_eq!(above.animation_speed_percent, MAX_ANIMATION_SPEED_PERCENT);
}

#[test]
fn exact_legacy_config_default_migrates_to_glacier() {
    let mut config = StorageConfig {
        schema_version: 1,
        appearance: legacy(),
        ..StorageConfig::default()
    };
    assert!(config.normalize());
    assert_eq!(config.appearance, AppearanceConfig::default());
}

#[test]
fn every_single_legacy_appearance_override_is_preserved() {
    let variants = [
        AppearanceConfig {
            border_shape: BorderShape::Square,
            ..legacy()
        },
        AppearanceConfig {
            border_color: BorderColor::LightBlue,
            ..legacy()
        },
        AppearanceConfig {
            accent_color: BorderColor::LightMagenta,
            ..legacy()
        },
        AppearanceConfig {
            icon_display_mode: IconDisplayMode::Ascii,
            ..legacy()
        },
        AppearanceConfig {
            motion_preference: MotionPreference::Reduced,
            ..legacy()
        },
        AppearanceConfig {
            animation_speed_percent: 125,
            ..legacy()
        },
    ];
    for expected in variants {
        let mut config = StorageConfig {
            schema_version: 1,
            appearance: expected.clone(),
            ..StorageConfig::default()
        };
        config.normalize();
        assert_eq!(config.appearance, expected);
    }
}

#[test]
fn system_status_defaults_and_normalization_are_stable() {
    assert_eq!(
        SystemStatusConfig::default(),
        SystemStatusConfig {
            low_available_gib: 5,
            low_percentage: 10,
            critical_available_gib: 1,
            critical_percentage: 5,
        }
    );

    let mut config = SystemStatusConfig {
        low_available_gib: 0,
        low_percentage: 101,
        critical_available_gib: u16::MAX,
        critical_percentage: 0,
    };
    assert!(config.normalize());
    assert_eq!(config.low_available_gib, SYSTEM_STATUS_MIN_AVAILABLE_GIB);
    assert_eq!(config.low_percentage, SYSTEM_STATUS_MAX_PERCENTAGE);
    assert_eq!(config.critical_available_gib, config.low_available_gib);
    assert_eq!(config.critical_percentage, SYSTEM_STATUS_MIN_PERCENTAGE);

    let mut inverted = SystemStatusConfig {
        low_available_gib: 20,
        low_percentage: 30,
        critical_available_gib: 21,
        critical_percentage: 31,
    };
    assert!(inverted.normalize());
    assert_eq!(inverted.critical_available_gib, 20);
    assert_eq!(inverted.critical_percentage, 30);
    assert!(!inverted.normalize());
}
