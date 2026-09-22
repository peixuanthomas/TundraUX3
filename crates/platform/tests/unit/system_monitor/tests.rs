use super::*;

fn battery() -> BatterySample {
    BatterySample {
        vendor: None,
        model: None,
        state: BatterySampleState::Discharging,
        charge_percent: 75.0,
        energy_wh: 30.0,
        energy_full_wh: 40.0,
        time_to_empty_seconds: Some(3600),
        time_to_full_seconds: None,
    }
}

#[test]
fn absent_batteries_are_a_successful_empty_sample() {
    assert_eq!(collect_battery_samples::<&str>([]), Ok(Vec::new()));
}

#[test]
fn battery_read_errors_are_not_discarded_or_misreported_as_missing_devices() {
    for samples in [
        vec![Err("DesignCapacity")],
        vec![Ok(battery()), Err("device disconnected")],
        vec![Err("permission denied"), Ok(battery())],
    ] {
        let expected = samples
            .iter()
            .find_map(|sample| sample.as_ref().err())
            .unwrap();
        assert_eq!(
            collect_battery_samples(samples.clone()),
            Err(format!("could not read battery: {expected}"))
        );
    }
}

#[test]
fn successful_battery_samples_keep_all_devices() {
    let mut charging = battery();
    charging.state = BatterySampleState::Charging;
    let expected = vec![battery(), charging];
    assert_eq!(
        collect_battery_samples(expected.iter().cloned().map(Ok::<_, &str>)),
        Ok(expected)
    );
}
