use storage::{AutoAdminPolicy, StorageConfig};

#[test]
fn auto_admin_defaults_to_manual_and_round_trips_all_choices() {
    let mut config = StorageConfig::default();
    assert_eq!(config.auto_admin, AutoAdminPolicy::Manual);
    for policy in [
        AutoAdminPolicy::Automatic,
        AutoAdminPolicy::Manual,
        AutoAdminPolicy::Deny,
    ] {
        config.auto_admin = policy;
        let saved = toml::to_string(&config).unwrap();
        assert_eq!(
            toml::from_str::<StorageConfig>(&saved).unwrap().auto_admin,
            policy
        );
        let absent = saved
            .lines()
            .filter(|line| !line.starts_with("auto_admin ="))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            toml::from_str::<StorageConfig>(&absent).unwrap().auto_admin,
            AutoAdminPolicy::Manual
        );
    }
}

#[test]
fn invalid_policy_is_not_treated_as_automatic_approval() {
    let text = toml::to_string(&StorageConfig::default()).unwrap().replace(
        "auto_admin = \"manual\"",
        "auto_admin = \"allow_everything\"",
    );
    assert!(toml::from_str::<StorageConfig>(&text).is_err());
}
