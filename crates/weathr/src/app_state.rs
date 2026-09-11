use crate::localization::{LocalizationProvider, localize};
use std::time::Instant;
use system_services::{
    WeatherCondition, WeatherConditions, WeatherData, WeatherLocation, WeatherUnits,
    format_temperature,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LocationDisplay {
    #[default]
    Coordinates,
    City,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BottomHudPrompt {
    Quit,
    Start,
}

impl BottomHudPrompt {
    fn text(self, localize: &LocalizationProvider) -> String {
        match self {
            Self::Quit => localize!(localize, "weathr-quit-prompt"),
            Self::Start => localize!(localize, "weathr-start-prompt"),
        }
    }
}

pub struct AppState {
    pub(crate) localize: LocalizationProvider,
    pub current_weather: Option<WeatherData>,
    pub is_offline: bool,
    pub weather_conditions: WeatherConditions,
    pub loading_state: LoadingState,
    pub cached_weather_info: String,
    pub weather_info_needs_update: bool,
    pub location: WeatherLocation,
    pub city_name: Option<String>,
    pub location_display: LocationDisplay,
    pub hide_location: bool,
    pub units: WeatherUnits,
    bottom_hud_prompt: BottomHudPrompt,
}

impl AppState {
    pub fn new(
        location: WeatherLocation,
        city_name: Option<String>,
        location_display: LocationDisplay,
        hide_location: bool,
        units: WeatherUnits,
        localize: LocalizationProvider,
    ) -> Self {
        Self::new_with_bottom_hud_prompt(
            location,
            city_name,
            location_display,
            hide_location,
            units,
            BottomHudPrompt::Quit,
            localize,
        )
    }

    pub(crate) fn new_with_bottom_hud_prompt(
        location: WeatherLocation,
        city_name: Option<String>,
        location_display: LocationDisplay,
        hide_location: bool,
        units: WeatherUnits,
        bottom_hud_prompt: BottomHudPrompt,
        localize: LocalizationProvider,
    ) -> Self {
        Self {
            localize,
            current_weather: None,
            is_offline: false,
            weather_conditions: WeatherConditions::default(),
            loading_state: LoadingState::new(),
            cached_weather_info: String::new(),
            weather_info_needs_update: true,
            location,
            city_name,
            location_display,
            hide_location,
            units,
            bottom_hud_prompt,
        }
    }

    pub fn update_weather(&mut self, weather: WeatherData) {
        self.weather_conditions.is_thunderstorm = weather.condition.is_thunderstorm();
        self.weather_conditions.is_snowing = weather.condition.is_snowing();
        self.weather_conditions.is_raining =
            weather.condition.is_raining() && !self.weather_conditions.is_thunderstorm;
        self.weather_conditions.is_cloudy = weather.condition.is_cloudy();
        self.weather_conditions.is_foggy = weather.condition.is_foggy();
        self.weather_conditions.sun = weather.sun;

        self.current_weather = Some(weather);
        self.is_offline = false;
        self.weather_info_needs_update = true;
    }

    pub fn update_snapshot(
        &mut self,
        weather: WeatherData,
        location: WeatherLocation,
        city: Option<String>,
        units: WeatherUnits,
    ) {
        self.location = location;
        self.city_name = city;
        self.units = units;
        self.update_weather(weather);
    }

    pub fn clear_weather_for_offline(&mut self) {
        self.current_weather = None;
        self.weather_conditions = WeatherConditions::default();
        self.is_offline = true;
        self.weather_info_needs_update = true;
    }

    pub fn set_offline_mode(&mut self, offline: bool) {
        self.is_offline = offline;
        self.weather_info_needs_update = true;
    }

    pub fn update_loading_animation(&mut self) {
        if self.loading_state.should_update() {
            self.loading_state.next_frame();
            self.weather_info_needs_update = true;
        }
    }

    pub fn get_condition_text(&self) -> String {
        if let Some(ref weather) = self.current_weather {
            localize!(
                self.localize,
                match weather.condition {
                    WeatherCondition::Clear => "weathr-condition-clear",
                    WeatherCondition::Cloudy => "weathr-condition-cloudy",
                    WeatherCondition::PartlyCloudy => "weathr-condition-partly-cloudy",
                    WeatherCondition::Overcast => "weathr-condition-overcast",
                    WeatherCondition::Fog => "weathr-condition-fog",
                    WeatherCondition::Drizzle => "weathr-condition-drizzle",
                    WeatherCondition::FreezingRain => "weathr-condition-freezing-rain",
                    WeatherCondition::Rain => "weathr-condition-rain",
                    WeatherCondition::Snow => "weathr-condition-snow",
                    WeatherCondition::SnowGrains => "weathr-condition-snow-grains",
                    WeatherCondition::RainShowers => "weathr-condition-rain-showers",
                    WeatherCondition::SnowShowers => "weathr-condition-snow-showers",
                    WeatherCondition::Thunderstorm => "weathr-condition-thunderstorm",
                    WeatherCondition::ThunderstormHail => "weathr-condition-thunderstorm-hail",
                }
            )
        } else {
            localize!(self.localize, "weathr-loading")
        }
    }

    fn location_hud_text(&self) -> String {
        if self.hide_location {
            String::new()
        } else {
            let (lat_value, lat_dir) = if self.location.latitude >= 0.0 {
                (
                    self.location.latitude,
                    localize!(self.localize, "weathr-north"),
                )
            } else {
                (
                    -self.location.latitude,
                    localize!(self.localize, "weathr-south"),
                )
            };
            let (lon_value, lon_dir) = if self.location.longitude >= 0.0 {
                (
                    self.location.longitude,
                    localize!(self.localize, "weathr-east"),
                )
            } else {
                (
                    -self.location.longitude,
                    localize!(self.localize, "weathr-west"),
                )
            };
            let coords = localize!(
                self.localize,
                "weathr-coordinates",
                latitude = format!("{lat_value:.2}"),
                latitude_direction = lat_dir,
                longitude = format!("{lon_value:.2}"),
                longitude_direction = lon_dir
            );
            let label = match self.location_display {
                LocationDisplay::Coordinates => coords,
                LocationDisplay::City => match &self.city_name {
                    Some(city) => city.clone(),
                    None => coords,
                },
                LocationDisplay::Mixed => match &self.city_name {
                    Some(city) => localize!(
                        self.localize,
                        "weathr-city-coordinates",
                        city = city,
                        coordinates = coords
                    ),
                    None => coords,
                },
            };
            localize!(self.localize, "weathr-location", location = label)
        }
    }

    pub fn bottom_hud_text(&self) -> String {
        let location = self.location_hud_text();
        let prompt = self.bottom_hud_prompt.text(&self.localize);
        let content = if location.is_empty() {
            prompt
        } else {
            localize!(
                self.localize,
                "weathr-hud-location",
                location = location,
                prompt = prompt
            )
        };
        if self.is_offline {
            localize!(self.localize, "weathr-hud-offline", content = content)
        } else {
            content
        }
    }

    pub fn weather_summary_text(&self) -> Option<String> {
        let weather = self.current_weather.as_ref()?;
        let (temp, temp_unit) = format_temperature(weather.temperature, self.units.temperature);
        Some(localize!(
            self.localize,
            "weathr-summary",
            condition = self.get_condition_text(),
            temperature = format!("{temp:.1}"),
            unit = temp_unit
        ))
    }

    pub fn update_cached_info(&mut self) {
        // This is presentation text: refresh it under the active locale even when
        // the weather snapshot itself has not changed.
        self.cached_weather_info = self.bottom_hud_text();

        self.weather_info_needs_update = false;
    }

    pub fn should_show_sun(&self) -> bool {
        if !self.weather_conditions.sun.is_day {
            return false;
        }

        if let Some(ref weather) = self.current_weather {
            matches!(
                weather.condition,
                WeatherCondition::Clear | WeatherCondition::PartlyCloudy | WeatherCondition::Cloudy
            )
        } else {
            false
        }
    }

    pub fn should_show_fireflies(&self) -> bool {
        if self.weather_conditions.sun.is_day {
            return false;
        }

        if let Some(ref weather) = self.current_weather {
            let is_warm = weather.temperature > 15.0;
            let is_clear_night = matches!(
                weather.condition,
                WeatherCondition::Clear | WeatherCondition::PartlyCloudy
            );
            is_warm
                && is_clear_night
                && !self.weather_conditions.is_raining
                && !self.weather_conditions.is_thunderstorm
                && !self.weather_conditions.is_snowing
        } else {
            false
        }
    }
}

pub struct LoadingState {
    pub frame: usize,
    pub last_update: Instant,
    pub loading_chars: [char; 4],
}

impl LoadingState {
    pub fn new() -> Self {
        Self {
            frame: 0,
            last_update: Instant::now(),
            loading_chars: ['|', '/', '-', '\\'],
        }
    }

    pub fn should_update(&self) -> bool {
        self.last_update.elapsed() >= std::time::Duration::from_millis(100)
    }

    pub fn next_frame(&mut self) {
        self.frame = (self.frame + 1) % self.loading_chars.len();
        self.last_update = Instant::now();
    }

    pub fn current_char(&self) -> char {
        self.loading_chars[self.frame]
    }
}

impl Default for LoadingState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
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
    fn coordinate_display_contract_covers_hemispheres_and_zero() {
        let cases = [
            ("New York", 40.7128, -74.0060, "40.71°N", "74.01°W"),
            ("Sydney", -33.8688, 151.2093, "33.87°S", "151.21°E"),
            ("London", 51.5074, -0.1278, "51.51°N", "0.13°W"),
            ("São Paulo", -23.5505, -46.6333, "23.55°S", "46.63°W"),
            ("Tokyo", 35.6762, 139.6503, "35.68°N", "139.65°E"),
            ("Null Island", 0.0, 0.0, "0.00°N", "0.00°E"),
        ];

        for (name, latitude, longitude, expected_latitude, expected_longitude) in cases {
            let mut app = create_app_state(latitude, longitude);
            app.update_cached_info();

            assert!(
                app.cached_weather_info.contains(expected_latitude),
                "{name}: expected {expected_latitude:?} in {:?}",
                app.cached_weather_info
            );
            assert!(
                app.cached_weather_info.contains(expected_longitude),
                "{name}: expected {expected_longitude:?} in {:?}",
                app.cached_weather_info
            );
        }
    }

    #[test]
    fn location_display_mode_contract() {
        let cases = [
            (
                "coordinates with city",
                Some("Alpharetta"),
                LocationDisplay::Coordinates,
                "Location: 34.08°N, 84.29°W",
                Some("Alpharetta"),
            ),
            (
                "city with city",
                Some("Alpharetta"),
                LocationDisplay::City,
                "Location: Alpharetta",
                Some("34.08°N"),
            ),
            (
                "city without city",
                None,
                LocationDisplay::City,
                "Location: 34.08°N, 84.29°W",
                None,
            ),
            (
                "mixed with city",
                Some("Alpharetta"),
                LocationDisplay::Mixed,
                "Location: Alpharetta (34.08°N, 84.29°W)",
                None,
            ),
            (
                "mixed without city",
                None,
                LocationDisplay::Mixed,
                "Location: 34.08°N, 84.29°W",
                Some("("),
            ),
        ];

        for (name, city, display, expected, unexpected) in cases {
            let mut app =
                create_app_state_full(34.0754, -84.2941, city.map(str::to_owned), display);
            app.update_cached_info();

            assert!(
                app.cached_weather_info.contains(expected),
                "{name}: expected {expected:?} in {:?}",
                app.cached_weather_info
            );
            if let Some(unexpected) = unexpected {
                assert!(
                    !app.cached_weather_info.contains(unexpected),
                    "{name}: did not expect {unexpected:?} in {:?}",
                    app.cached_weather_info
                );
            }
        }
    }

    #[test]
    fn bottom_hud_text_includes_location_and_space_prompt_only() {
        let app = create_app_state_full(
            34.0754,
            -84.2941,
            Some("Alpharetta".to_string()),
            LocationDisplay::Mixed,
        );

        let hud = app.bottom_hud_text();

        assert!(hud.contains("Location: Alpharetta (34.08°N, 84.29°W)"));
        assert!(hud.contains("Press Space to quit"));
        assert!(!hud.contains("Weather: Clear"));
        assert!(!hud.contains("Temp: 20.0°C"));
        assert!(!hud.contains("Wind:"));
        assert!(!hud.contains("Precip:"));
        assert!(!hud.contains("Press 'q' to quit"));
    }

    #[test]
    fn bottom_hud_text_uses_start_prompt_when_requested() {
        let app = create_app_state_full_with_prompt(
            34.0754,
            -84.2941,
            Some("Alpharetta".to_string()),
            LocationDisplay::Mixed,
            BottomHudPrompt::Start,
        );

        let hud = app.bottom_hud_text();

        assert!(hud.contains("Location: Alpharetta (34.08°N, 84.29°W)"));
        assert!(hud.contains("Press Space to start"));
        assert!(!hud.contains("Press Space to quit"));
    }

    #[test]
    fn weather_summary_text_includes_condition_and_temperature() {
        let app = create_app_state_full(
            34.0754,
            -84.2941,
            Some("Alpharetta".to_string()),
            LocationDisplay::Mixed,
        );

        let summary = app.weather_summary_text();

        assert_eq!(summary.as_deref(), Some("Clear  20.0°C"));
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
}
