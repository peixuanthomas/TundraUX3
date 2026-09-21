use super::*;
#[test]
fn old_configs_receive_log_retention_defaults_and_invalid_sizes_normalize() {
    let mut value = serde_json::to_value(StorageConfig::default()).unwrap();
    value.as_object_mut().unwrap().remove("runtime_logs");
    let mut config: StorageConfig = serde_json::from_value(value).unwrap();
    assert_eq!(config.runtime_logs, RuntimeLogsConfig::default());
    config.runtime_logs.max_total_mib = 0;
    config.runtime_logs.segment_mib = 10;
    assert!(config.normalize());
    assert_eq!(config.runtime_logs.max_total_mib, 1);
    assert_eq!(config.runtime_logs.segment_mib, 1);
}
