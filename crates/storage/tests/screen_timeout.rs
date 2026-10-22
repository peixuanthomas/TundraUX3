use storage::{DEFAULT_SCREEN_TIMEOUT_SECONDS, StorageConfig};

#[test]
fn old_configs_get_five_minutes_and_saved_timeouts_round_trip() {
    let old: StorageConfig = toml::from_str("schema_version = 1").unwrap();
    assert_eq!(old.screen_timeout_seconds, DEFAULT_SCREEN_TIMEOUT_SECONDS);
    for seconds in [0, 30, 60, 120, 300, 600, 1800] {
        let config = StorageConfig {
            screen_timeout_seconds: seconds,
            ..old.clone()
        };
        let loaded: StorageConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(loaded.screen_timeout_seconds, seconds);
    }
}
