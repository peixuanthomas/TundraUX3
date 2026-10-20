use storage::{AnimationTimingsConfig, AppearanceConfig};

#[test]
fn animation_timings_keep_legacy_defaults_and_round_trip_custom_phases() {
    let legacy: AppearanceConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(legacy.animation_timings, AnimationTimingsConfig::default());
    let custom: AppearanceConfig =
        serde_json::from_str(r#"{"animation_timings":{"page":[80,40,80]}}"#).unwrap();
    assert_eq!(custom.animation_timings.page, [80, 40, 80]);
    assert_eq!(custom.animation_timings.dialog, [45, 90, 45]);
    assert_eq!(custom.animation_timings.menu, [40, 80, 40]);
    let saved = serde_json::to_string(&custom).unwrap();
    assert_eq!(
        serde_json::from_str::<AppearanceConfig>(&saved).unwrap(),
        custom
    );
    let bounded: AppearanceConfig =
        serde_json::from_str(r#"{"animation_timings":{"page":[0,65535,55]}}"#).unwrap();
    assert_eq!(bounded.animation_timings.page, [1, 2_000, 55]);
}
