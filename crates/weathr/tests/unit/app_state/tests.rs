use super::LocationDisplay;
use super::*;
use system_services::{CelestialEvents, PrecipitationUnit, TemperatureUnit, WindSpeedUnit};

fn create_app_state(lat: f64, lon: f64) -> AppState {
    create_app_state_full(lat, lon, None, LocationDisplay::Coordinates)
}

fn create_app_state_full(
    lat: f64,
    lon: f64,
    city: Option<String>,
    display: LocationDisplay,
) -> AppState {
    create_app_state_full_with_prompt(lat, lon, city, display, BottomHudPrompt::Quit)
}

fn create_app_state_full_with_prompt(
    lat: f64,
    lon: f64,
    city: Option<String>,
    display: LocationDisplay,
    prompt: BottomHudPrompt,
) -> AppState {
    let location = WeatherLocation {
        latitude: lat,
        longitude: lon,
        elevation: None,
    };
    let units = WeatherUnits {
        temperature: TemperatureUnit::Celsius,
        wind_speed: WindSpeedUnit::Kmh,
        precipitation: PrecipitationUnit::Mm,
    };
    let mut app = AppState::new_with_bottom_hud_prompt(
        location,
        city,
        display,
        false,
        units,
        prompt,
        crate::localization::tests::english(),
    );

    let weather = WeatherData {
        condition: WeatherCondition::Clear,
        temperature: 20.0,
        precipitation: 0.0,
        wind_speed: 10.0,
        wind_direction: 0.0,
        moon_phase: Some(0.5),
        timestamp: "2024-01-01T12:00:00Z".to_string(),
        attribution: "".to_string(),
        sun: CelestialEvents::from_bool(true),
    };
    app.update_weather(weather);

    app
}

#[test]
fn clear_weather_for_offline_hides_weather_summary() {
    let mut app = create_app_state_full(
        34.0754,
        -84.2941,
        Some("Alpharetta".to_string()),
        LocationDisplay::Mixed,
    );

    app.clear_weather_for_offline();

    assert!(app.current_weather.is_none());
    assert!(app.is_offline);
    assert_eq!(app.weather_summary_text(), None);
    assert!(!app.weather_conditions.is_raining);
    assert!(!app.weather_conditions.is_snowing);
    assert!(!app.weather_conditions.is_thunderstorm);
}

#[test]
fn host_localization_updates_hud_without_changing_weather_or_raw_city_data() {
    let mut app = create_app_state_full_with_prompt(
        31.23,
        121.47,
        Some("上海 / Shanghai".into()),
        LocationDisplay::Mixed,
        BottomHudPrompt::Start,
    );
    app.update_cached_info();
    assert!(app.cached_weather_info.contains("Press Space to start"));
    assert!(!app.weather_info_needs_update);
    app.localize = crate::localization::tests::chinese();
    app.update_cached_info();
    assert_eq!(
        app.cached_weather_info,
        "位置：上海 / Shanghai（北纬31.23°，东经121.47°） | 按空格键开始"
    );
    assert_eq!(app.weather_summary_text().as_deref(), Some("晴  20.0°C"));
    assert_eq!(app.current_weather.as_ref().unwrap().temperature, 20.0);
    app.clear_weather_for_offline();
    app.hide_location = true;
    assert_eq!(app.bottom_hud_text(), "离线 | 按空格键开始");
    assert_eq!(app.get_condition_text(), "加载中");
}

#[test]
fn host_provider_remains_bound_when_display_state_moves_to_another_thread() {
    let mut app = create_app_state(0.0, 0.0);
    app.hide_location = true;
    app.localize = crate::localization::tests::chinese();
    let rendered = std::thread::spawn(move || app.bottom_hud_text())
        .join()
        .unwrap();
    assert_eq!(rendered, "按空格键退出");
}
