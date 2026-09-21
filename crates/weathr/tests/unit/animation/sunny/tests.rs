use super::*;
use crate::animation::TerminalSize;
use crate::app_state::AppState;
use crate::app_state::LocationDisplay;
use chrono::NaiveTime;
use system_services::CelestialEvents;
use system_services::{
    WeatherCondition, WeatherConditions, WeatherData, WeatherLocation, WeatherUnits,
};

fn sample_celestial_events() -> CelestialEvents {
    CelestialEvents {
        is_day: true,
        begin_twilight: Some(NaiveTime::from_hms_opt(5, 30, 0).unwrap()),
        rise: Some(NaiveTime::from_hms_opt(6, 0, 0).unwrap()),
        upper_transit: Some(NaiveTime::from_hms_opt(12, 0, 0).unwrap()),
        set: Some(NaiveTime::from_hms_opt(18, 0, 0).unwrap()),
        end_twilight: Some(NaiveTime::from_hms_opt(20, 0, 0).unwrap()),
    }
}

#[test]
fn parses_rfc3339_timestamp() {
    let time = SunSystem::parse_weather_time("2024-01-01T12:34:56Z").unwrap();
    assert_eq!(time, NaiveTime::from_hms_opt(12, 34, 56).unwrap());
}

#[test]
fn parses_naive_timestamp() {
    let time = SunSystem::parse_weather_time("2024-01-01T06:15").unwrap();
    assert_eq!(time, NaiveTime::from_hms_opt(6, 15, 0).unwrap());
}

#[test]
fn resolved_y_uses_weather_time() {
    let sun = sample_celestial_events();
    let location = WeatherLocation {
        latitude: 0.0,
        longitude: 0.0,
        elevation: None,
    };
    let units = WeatherUnits::metric();
    let mut state = AppState::new(
        location,
        None,
        LocationDisplay::Coordinates,
        false,
        units,
        crate::localization::tests::english(),
    );
    state.current_weather = Some(WeatherData {
        condition: WeatherCondition::Clear,
        temperature: 20.0,
        precipitation: 0.0,
        wind_speed: 5.0,
        wind_direction: 0.0,
        sun,
        moon_phase: None,
        timestamp: "2024-01-01T21:00:00Z".to_string(),
        attribution: String::new(),
    });
    let conditions = WeatherConditions {
        sun,
        ..WeatherConditions::default()
    };

    let ctx = FrameContext {
        size: TerminalSize {
            width: 80,
            height: 24,
        },
        horizon_y: 18,
        conditions: &conditions,
        state: &state,
        show_leaves: false,
        chimney: None,
    };

    let y = SunSystem::resolved_sun_y(&ctx, 3);
    assert_eq!(y, ctx.size.height);
}

#[test]
fn resolved_y_defaults_without_time() {
    let sun = sample_celestial_events();
    let location = WeatherLocation {
        latitude: 0.0,
        longitude: 0.0,
        elevation: None,
    };
    let units = WeatherUnits::metric();
    let mut state = AppState::new(
        location,
        None,
        LocationDisplay::Coordinates,
        false,
        units,
        crate::localization::tests::english(),
    );
    state.current_weather = Some(WeatherData {
        condition: WeatherCondition::Clear,
        temperature: 20.0,
        precipitation: 0.0,
        wind_speed: 5.0,
        wind_direction: 0.0,
        sun,
        moon_phase: None,
        timestamp: "n/a".to_string(),
        attribution: String::new(),
    });
    let conditions = WeatherConditions {
        sun,
        ..WeatherConditions::default()
    };

    let ctx = FrameContext {
        size: TerminalSize {
            width: 80,
            height: 24,
        },
        horizon_y: 18,
        conditions: &conditions,
        state: &state,
        show_leaves: false,
        chimney: None,
    };

    let y = SunSystem::resolved_sun_y(&ctx, 4);
    assert_eq!(y, 4);
}
