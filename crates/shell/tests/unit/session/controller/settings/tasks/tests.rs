use super::*;

#[test]
fn system_status_operations_report_unavailable_runtime() {
    let runtime = ShellSettingsTaskRuntime::unavailable();
    assert_eq!(
        runtime.refresh_system_status(),
        Err(system_services::SystemServicesError::Shutdown)
    );
    assert_eq!(
        runtime.set_system_status_active(true),
        Err(system_services::SystemServicesError::Shutdown)
    );
    assert_eq!(
        runtime.set_system_status_active(false),
        Err(system_services::SystemServicesError::Shutdown)
    );
}

#[test]
fn storage_mapping_preserves_runtime_only_configuration() {
    let base = system_services::SystemServicesConfig {
        cache_dir: Some(std::path::PathBuf::from("cache/system-services")),
        weather_refresh_interval: Duration::from_secs(17),
        location_refresh_interval: Duration::from_secs(18),
        time_sync_interval: Duration::from_secs(19),
        request_timeout: Duration::from_secs(20),
        fallback_location: system_services::GeoLocation {
            latitude: 1.0,
            longitude: 2.0,
            city: Some("fallback".into()),
        },
        ..system_services::SystemServicesConfig::default()
    };
    let mapped =
        system_services_config_for_storage_config(&base, &storage::StorageConfig::default());
    assert_eq!(mapped.cache_dir, base.cache_dir);
    assert_eq!(
        mapped.weather_refresh_interval,
        base.weather_refresh_interval
    );
    assert_eq!(
        mapped.location_refresh_interval,
        base.location_refresh_interval
    );
    assert_eq!(mapped.time_sync_interval, base.time_sync_interval);
    assert_eq!(mapped.request_timeout, base.request_timeout);
    assert_eq!(mapped.fallback_location, base.fallback_location);
}

#[test]
fn storage_mapping_maps_thresholds_to_exact_binary_bytes() {
    let storage_config = storage::StorageConfig {
        system_status: storage::SystemStatusConfig {
            low_available_gib: 7,
            low_percentage: 13,
            critical_available_gib: 2,
            critical_percentage: 6,
        },
        ..storage::StorageConfig::default()
    };
    let base = system_services::SystemServicesConfig {
        weather_refresh_interval: Duration::from_secs(5),
        time_sync_interval: Duration::from_secs(30),
        ..system_services::SystemServicesConfig::default()
    };
    let mapped = system_services_config_for_storage_config(&base, &storage_config);
    assert_eq!(
        mapped.storage_thresholds.low_available_bytes,
        7 * 1024_u64.pow(3)
    );
    assert_eq!(mapped.storage_thresholds.low_percentage, 13);
    assert_eq!(
        mapped.storage_thresholds.critical_available_bytes,
        2 * 1024_u64.pow(3)
    );
    assert_eq!(mapped.storage_thresholds.critical_percentage, 6);
    assert_eq!(mapped.weather_refresh_interval, Duration::from_secs(5));
    assert_eq!(mapped.time_sync_interval, Duration::from_secs(30));
}
