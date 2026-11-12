use storage::{AnimationTimingsConfig, AppearanceConfig};

#[test]
fn animation_timings_use_light_defaults_and_round_trip_explicit_old_phases() {
    let legacy: AppearanceConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(legacy.animation_timings, AnimationTimingsConfig::default());
    let custom: AppearanceConfig =
        serde_json::from_str(r#"{"animation_timings":{"page":[80,40,80]}}"#).unwrap();
    assert_eq!(custom.animation_timings.page, [80, 40, 80]);
    assert_eq!(custom.animation_timings.dialog, [45, 90, 45]);
    assert_eq!(custom.animation_timings.menu, [30, 60, 30]);
    let saved = serde_json::to_string(&custom).unwrap();
    assert_eq!(
        serde_json::from_str::<AppearanceConfig>(&saved).unwrap(),
        custom
    );
    let bounded: AppearanceConfig =
        serde_json::from_str(r#"{"animation_timings":{"page":[0,65535,55]}}"#).unwrap();
    assert_eq!(bounded.animation_timings.page, [1, 2_000, 55]);
}

#[test]
fn explicit_old_timings_and_colors_survive_round_trip() {
    let value: AppearanceConfig = serde_json::from_str(
        r##"{"border_color":"#29434E","animation_timings":{"page":[55,110,55],"dialog":[45,90,45],"menu":[40,80,40]}}"##,
    ).unwrap();
    assert_eq!(value.animation_timings.page, [55, 110, 55]);
    assert_eq!(value.animation_timings.menu, [40, 80, 40]);
    assert!(!value.animation_timings.is_default());
    let saved = serde_json::to_string(&value).unwrap();
    assert_eq!(
        serde_json::from_str::<AppearanceConfig>(&saved).unwrap(),
        value
    );
    assert_eq!(
        AppearanceConfig::default().border_color,
        storage::BorderColor::Rgb(0x38, 0x38, 0x38)
    );
    assert!(AnimationTimingsConfig::default().is_default());
}
