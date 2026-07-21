use ratatui::style::Color;

use crate::view_model::{SetupLanguageOption, SetupTimezoneOption};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupColorOption {
    pub label: &'static str,
    pub value: &'static str,
    pub color: Color,
}

const SETUP_STANDARD_COLORS: [SetupColorOption; 7] = [
    SetupColorOption {
        label: "White",
        value: "white",
        color: Color::White,
    },
    SetupColorOption {
        label: "Cyan",
        value: "cyan",
        color: Color::Cyan,
    },
    SetupColorOption {
        label: "Blue",
        value: "blue",
        color: Color::Blue,
    },
    SetupColorOption {
        label: "Green",
        value: "green",
        color: Color::Green,
    },
    SetupColorOption {
        label: "Yellow",
        value: "yellow",
        color: Color::Yellow,
    },
    SetupColorOption {
        label: "Magenta",
        value: "magenta",
        color: Color::Magenta,
    },
    SetupColorOption {
        label: "Red",
        value: "red",
        color: Color::Red,
    },
];

pub const fn setup_standard_color_options() -> &'static [SetupColorOption] {
    &SETUP_STANDARD_COLORS
}

pub fn setup_language_options() -> Vec<SetupLanguageOption> {
    vec![SetupLanguageOption {
        code: "en-US".to_string(),
        label: "English".to_string(),
    }]
}

pub fn setup_timezone_options() -> Vec<SetupTimezoneOption> {
    vec![
        timezone("UTC", "UTC", "Coordinated Universal Time", 0.0, 0.0),
        timezone(
            "America/Los_Angeles",
            "Los Angeles",
            "Pacific Time",
            -118.2437,
            34.0522,
        ),
        timezone(
            "America/Denver",
            "Denver",
            "Mountain Time",
            -104.9903,
            39.7392,
        ),
        timezone(
            "America/Chicago",
            "Chicago",
            "Central Time",
            -87.6298,
            41.8781,
        ),
        timezone(
            "America/New_York",
            "New York",
            "Eastern Time",
            -74.0060,
            40.7128,
        ),
        timezone(
            "America/Sao_Paulo",
            "Sao Paulo",
            "Brasilia Time",
            -46.6333,
            -23.5505,
        ),
        timezone(
            "Europe/London",
            "London",
            "United Kingdom",
            -0.1276,
            51.5072,
        ),
        timezone(
            "Europe/Berlin",
            "Berlin",
            "Central Europe",
            13.4050,
            52.5200,
        ),
        timezone(
            "Africa/Johannesburg",
            "Johannesburg",
            "South Africa",
            28.0473,
            -26.2041,
        ),
        timezone(
            "Asia/Dubai",
            "Dubai",
            "Gulf Standard Time",
            55.2708,
            25.2048,
        ),
        timezone(
            "Asia/Kolkata",
            "Kolkata",
            "India Standard Time",
            88.3639,
            22.5726,
        ),
        timezone(
            "Asia/Shanghai",
            "Shanghai",
            "China Standard Time",
            121.4737,
            31.2304,
        ),
        timezone(
            "Asia/Tokyo",
            "Tokyo",
            "Japan Standard Time",
            139.6917,
            35.6895,
        ),
        timezone(
            "Australia/Sydney",
            "Sydney",
            "Australian Eastern Time",
            151.2093,
            -33.8688,
        ),
        timezone(
            "Pacific/Auckland",
            "Auckland",
            "New Zealand Time",
            174.7633,
            -36.8485,
        ),
    ]
}

fn timezone(
    id: &'static str,
    label: &'static str,
    description: &'static str,
    longitude: f64,
    latitude: f64,
) -> SetupTimezoneOption {
    SetupTimezoneOption {
        id: id.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        longitude,
        latitude,
    }
}
