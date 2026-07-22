use std::io::Write;

use platform::Platform;
use storage::{
    AccentColor, BorderColor, BorderShape, DEFAULT_ACCENT_COLOR, StorageConfig, StorageLayout,
    StorageManager,
};

use crate::arguments::{ConfigAction, ConfigField, ConfigUpdate};

pub(crate) fn run_config<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    action: ConfigAction,
) -> i32 {
    let storage = match config_storage(platform) {
        Ok(storage) => storage,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            return 1;
        }
    };

    let mut config = match load_or_default_config(&storage) {
        Ok(config) => config,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not load config: {error}");
            return 1;
        }
    };

    match action {
        ConfigAction::Get(field) => {
            write_config_value(stdout, &config, field);
            0
        }
        ConfigAction::Set(update) => match apply_config_update(&mut config, update) {
            Ok(message) => match storage.save_config(&config) {
                Ok(()) => {
                    let _ = writeln!(stdout, "{message}");
                    0
                }
                Err(error) => {
                    let _ = writeln!(stderr, "ERROR: could not save config: {error}");
                    1
                }
            },
            Err(error) => {
                let _ = writeln!(stderr, "ERROR: {error}");
                1
            }
        },
    }
}

fn config_storage(platform: &dyn Platform) -> Result<StorageManager, String> {
    platform
        .app_paths()
        .map(|paths| StorageManager::from_layout(StorageLayout::from_app_paths(&paths)))
        .map_err(|error| error.to_string())
}

fn load_or_default_config(storage: &StorageManager) -> Result<StorageConfig, String> {
    if storage.layout().config_path.exists() {
        storage.load_config().map_err(|error| error.to_string())
    } else {
        Ok(StorageConfig::default())
    }
}

fn write_config_value(output: &mut impl Write, config: &StorageConfig, field: Option<ConfigField>) {
    match field {
        Some(ConfigField::Theme) => {
            write_theme_summary(output, config);
        }
        Some(ConfigField::BorderShape) => {
            let _ = writeln!(
                output,
                "border-shape = {}",
                border_shape_name(config.appearance.border_shape)
            );
        }
        Some(ConfigField::BorderColor) => {
            let _ = writeln!(output, "border-color = {}", config.appearance.border_color);
        }
        Some(ConfigField::AccentColor) => {
            let _ = writeln!(output, "accent-color = {}", config.appearance.accent_color);
        }
        Some(ConfigField::Language) => {
            let _ = writeln!(output, "language = {}", config.language);
        }
        Some(ConfigField::Timezone) => {
            let _ = writeln!(output, "timezone = {}", config.timezone);
        }
        Some(ConfigField::Address) => {
            let _ = writeln!(output, "address = {}", config_address_summary(config));
        }
        None => {
            write_theme_summary(output, config);
            let _ = writeln!(output, "language = {}", config.language);
            let _ = writeln!(output, "timezone = {}", config.timezone);
            let _ = writeln!(output, "address = {}", config_address_summary(config));
        }
    }
}

fn apply_config_update(config: &mut StorageConfig, update: ConfigUpdate) -> Result<String, String> {
    match update {
        ConfigUpdate::BorderShape(value) => {
            let value = clean_config_value("border-shape", value)?;
            let border_shape = match value.to_ascii_lowercase().as_str() {
                "rounded" => BorderShape::Rounded,
                "square" => BorderShape::Square,
                _ => {
                    return Err(format!(
                        "unsupported border shape {value:?}; available values: rounded, square"
                    ));
                }
            };
            config.appearance.border_shape = border_shape;
            Ok(format!(
                "Updated border shape: {}",
                border_shape_name(border_shape)
            ))
        }
        ConfigUpdate::BorderColor(value) => {
            let value = clean_config_value("border-color", value)?;
            let color = value
                .parse::<BorderColor>()
                .map_err(|error| error.to_string())?;
            config.appearance.border_color = color;
            Ok(format!("Updated border color: {color}"))
        }
        ConfigUpdate::AccentColor(value) => {
            let value = clean_config_value("accent-color", value)?;
            let color = parse_accent_color(&value)?;
            config.appearance.accent_color = color;
            Ok(format!("Updated accent color: {color}"))
        }
        ConfigUpdate::Language(value) => {
            let language = resolve_language(&value)?;
            config.language = language.code.clone();
            Ok(format!(
                "Updated language: {} ({})",
                language.label, language.code
            ))
        }
        ConfigUpdate::Timezone(value) => {
            let timezone = resolve_timezone(&value)?;
            config.timezone = timezone.id.clone();
            Ok(format!(
                "Updated timezone: {} ({})",
                timezone.label, timezone.id
            ))
        }
        ConfigUpdate::Address(value) => {
            let timezone = resolve_timezone(&value)?;
            config.timezone = timezone.id.clone();
            Ok(format!("Updated address: {}", timezone_summary(&timezone)))
        }
    }
}

fn write_theme_summary(output: &mut impl Write, config: &StorageConfig) {
    let _ = writeln!(
        output,
        "border-shape = {}",
        border_shape_name(config.appearance.border_shape)
    );
    let _ = writeln!(output, "border-color = {}", config.appearance.border_color);
    let _ = writeln!(output, "accent-color = {}", config.appearance.accent_color);
}

fn parse_accent_color(value: &str) -> Result<AccentColor, String> {
    if value.eq_ignore_ascii_case("default") {
        Ok(DEFAULT_ACCENT_COLOR)
    } else {
        value
            .parse::<AccentColor>()
            .map_err(|error| error.to_string())
    }
}

const fn border_shape_name(border_shape: BorderShape) -> &'static str {
    match border_shape {
        BorderShape::Rounded => "rounded",
        BorderShape::Square => "square",
    }
}

fn clean_config_value(name: &str, value: String) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{name} cannot be empty"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{name} cannot contain control characters"));
    }

    Ok(value)
}

fn resolve_language(value: &str) -> Result<app::SetupLanguageOption, String> {
    let value = clean_config_value("language", value.to_string())?;
    app::setup_language_options()
        .into_iter()
        .find(|language| {
            language.code == value || language.label.eq_ignore_ascii_case(value.as_str())
        })
        .ok_or_else(|| {
            format!(
                "unsupported language {value:?}; available values: {}",
                app::setup_language_options()
                    .into_iter()
                    .map(|language| language.code)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn resolve_timezone(value: &str) -> Result<app::SetupTimezoneOption, String> {
    let value = clean_config_value("address", value.to_string())?;
    app::setup_timezone_options()
        .into_iter()
        .find(|timezone| {
            timezone.id == value || timezone.label.eq_ignore_ascii_case(value.as_str())
        })
        .ok_or_else(|| {
            format!(
                "unsupported address/timezone {value:?}; available values: {}",
                app::setup_timezone_options()
                    .into_iter()
                    .map(|timezone| timezone.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn config_address_summary(config: &StorageConfig) -> String {
    app::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == config.timezone)
        .map(|timezone| timezone_summary(&timezone))
        .unwrap_or_else(|| format!("unmapped timezone ({})", config.timezone))
}

fn timezone_summary(timezone: &app::SetupTimezoneOption) -> String {
    format!(
        "{} ({}, {:.4}, {:.4})",
        timezone.label, timezone.id, timezone.latitude, timezone.longitude
    )
}
