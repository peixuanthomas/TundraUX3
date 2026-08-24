use std::time::Instant;
use system_services_model::{
    WeatherCondition, WeatherConditions, WeatherData, WeatherLocation, WeatherUnits,
    format_temperature,
};

pub const BOTTOM_HUD_QUIT_PROMPT: &str = "Press Space to quit";
pub const BOTTOM_HUD_START_PROMPT: &str = "Press Space to start";

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
    fn text(self) -> &'static str {
        match self {
            Self::Quit => BOTTOM_HUD_QUIT_PROMPT,
            Self::Start => BOTTOM_HUD_START_PROMPT,
        }
    }
}

pub struct AppState {
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
    ) -> Self {
        Self::new_with_bottom_hud_prompt(
            location,
            city_name,
            location_display,
            hide_location,
            units,
            BottomHudPrompt::Quit,
        )
    }

    pub(crate) fn new_with_bottom_hud_prompt(
        location: WeatherLocation,
        city_name: Option<String>,
        location_display: LocationDisplay,
        hide_location: bool,
        units: WeatherUnits,
        bottom_hud_prompt: BottomHudPrompt,
    ) -> Self {
        Self {
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

    pub fn get_condition_text(&self) -> &str {
        if let Some(ref weather) = self.current_weather {
            match weather.condition {
                WeatherCondition::Clear => "Clear",
                WeatherCondition::Cloudy => "Cloudy",
                WeatherCondition::PartlyCloudy => "Partly Cloudy",
                WeatherCondition::Overcast => "Overcast",
                WeatherCondition::Fog => "Fog",
                WeatherCondition::Drizzle => "Drizzle",
                WeatherCondition::FreezingRain => "Freezing Rain",
                WeatherCondition::Rain => "Rain",
                WeatherCondition::Snow => "Snow",
                WeatherCondition::SnowGrains => "Snow Grains",
                WeatherCondition::RainShowers => "Rain Showers",
                WeatherCondition::SnowShowers => "Snow Showers",
                WeatherCondition::Thunderstorm => "Thunderstorm",
                WeatherCondition::ThunderstormHail => "Thunderstorm with Hail",
            }
        } else {
            "Loading"
        }
    }

    fn location_hud_suffix(&self) -> String {
        if self.hide_location {
            String::new()
        } else {
            let (lat_value, lat_dir) = if self.location.latitude >= 0.0 {
                (self.location.latitude, "N")
            } else {
                (-self.location.latitude, "S")
            };
            let (lon_value, lon_dir) = if self.location.longitude >= 0.0 {
                (self.location.longitude, "E")
            } else {
                (-self.location.longitude, "W")
            };
            let coords = format!("{:.2}°{}, {:.2}°{}", lat_value, lat_dir, lon_value, lon_dir);
            let label = match self.location_display {
                LocationDisplay::Coordinates => coords,
                LocationDisplay::City => match &self.city_name {
                    Some(city) => city.clone(),
                    None => coords,
                },
                LocationDisplay::Mixed => match &self.city_name {
                    Some(city) => format!("{} ({})", city, coords),
                    None => coords,
                },
            };
            format!(" | Location: {}", label)
        }
    }

    pub fn bottom_hud_text(&self) -> String {
        let location_str = self.location_hud_suffix();

        let offline_indicator = if self.is_offline { "OFFLINE | " } else { "" };

        if location_str.is_empty() {
            format!("{}{}", offline_indicator, self.bottom_hud_prompt.text())
        } else {
            format!(
                "{}{} | {}",
                offline_indicator,
                location_str.trim_start_matches(" | "),
                self.bottom_hud_prompt.text()
            )
        }
    }

    pub fn weather_summary_text(&self) -> Option<String> {
        let weather = self.current_weather.as_ref()?;
        let (temp, temp_unit) = format_temperature(weather.temperature, self.units.temperature);
        Some(format!(
            "{}  {:.1}{}",
            self.get_condition_text(),
            temp,
            temp_unit
        ))
    }

    pub fn update_cached_info(&mut self) {
        if !self.weather_info_needs_update {
            return;
        }

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
    use system_services_model::{
        CelestialEvents, PrecipitationUnit, TemperatureUnit, WindSpeedUnit,
    };

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
        let mut app =
            AppState::new_with_bottom_hud_prompt(location, city, display, false, units, prompt);

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
        assert!(hud.contains(BOTTOM_HUD_QUIT_PROMPT));
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
        assert!(hud.contains(BOTTOM_HUD_START_PROMPT));
        assert!(!hud.contains(BOTTOM_HUD_QUIT_PROMPT));
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
}
