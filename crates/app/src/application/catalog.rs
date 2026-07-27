#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupLanguageOption {
    pub code: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetupTimezoneOption {
    pub id: String,
    pub label: String,
    pub description: String,
    pub longitude: f64,
    pub latitude: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltInApplicationDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub type_label: &'static str,
    pub admin_only: bool,
    pub fixed_in_launcher: bool,
}

pub const COMMAND_LINE_APPLICATION: BuiltInApplicationDescriptor = BuiltInApplicationDescriptor {
    id: "builtin.command-line",
    name: "Command Line",
    description: "TundraUX3 CLI and operating-system commands",
    type_label: "Built-in application",
    admin_only: true,
    fixed_in_launcher: true,
};

pub const BUILT_IN_LAUNCHER_APPLICATIONS: &[BuiltInApplicationDescriptor] =
    &[COMMAND_LINE_APPLICATION];

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_is_a_fixed_admin_launcher_application() {
        assert_eq!(BUILT_IN_LAUNCHER_APPLICATIONS, &[COMMAND_LINE_APPLICATION]);
        let descriptor = BUILT_IN_LAUNCHER_APPLICATIONS
            .first()
            .expect("Command Line built-in descriptor");
        assert_eq!(descriptor.id, "builtin.command-line");
        assert!(descriptor.admin_only);
        assert!(descriptor.fixed_in_launcher);
    }
}
