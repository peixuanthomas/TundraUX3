use super::*;
#[test]
fn weather_helpers_preserve_precipitation_semantics() {
    assert!(WeatherCondition::RainShowers.is_raining());
    assert!(WeatherCondition::SnowGrains.is_snowing());
    assert_eq!(
        format_temperature(0.0, TemperatureUnit::Fahrenheit),
        (32.0, "°F")
    );
}

#[test]
fn storage_pressure_classification_covers_boundaries_and_precedence() {
    let thresholds = StorageThresholds {
        low_available_bytes: 200,
        low_percentage: 20,
        critical_available_bytes: 100,
        critical_percentage: 5,
    };

    assert_eq!(
        thresholds.classify(Some(2_000), Some(100)),
        StoragePressure::Critical
    );
    assert_eq!(
        thresholds.classify(Some(3_000), Some(200)),
        StoragePressure::Low
    );
    assert_eq!(
        thresholds.classify(Some(2_000), Some(201)),
        StoragePressure::Low
    );
    assert_eq!(
        thresholds.classify(Some(1_000), Some(50)),
        StoragePressure::Critical
    );
    assert_eq!(
        thresholds.classify(Some(1_000), Some(300)),
        StoragePressure::Normal
    );
}

#[test]
fn storage_pressure_rejects_unknown_and_invalid_capacities() {
    let thresholds = StorageThresholds {
        low_available_bytes: 1,
        low_percentage: 10,
        critical_available_bytes: 1,
        critical_percentage: 5,
    };
    assert_eq!(thresholds.classify(None, Some(1)), StoragePressure::Unknown);
    assert_eq!(thresholds.classify(Some(1), None), StoragePressure::Unknown);
    assert_eq!(
        thresholds.classify(Some(0), Some(0)),
        StoragePressure::Unknown
    );
    assert_eq!(
        thresholds.classify(Some(1), Some(2)),
        StoragePressure::Unknown
    );
}

#[test]
fn storage_pressure_percentage_math_does_not_overflow() {
    let thresholds = StorageThresholds {
        low_available_bytes: 0,
        low_percentage: 50,
        critical_available_bytes: 0,
        critical_percentage: 1,
    };
    assert_eq!(
        thresholds.classify(Some(u64::MAX), Some(u64::MAX / 2)),
        StoragePressure::Low
    );
}
