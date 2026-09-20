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
#[path = "../tests/unit/app_state/tests.rs"]
mod tests;
