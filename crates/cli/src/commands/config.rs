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
    if let ConfigAction::Options(field) = action {
        write_config_options(stdout, field);
        return 0;
    }
    if platform.kind() != platform::PlatformKind::Linux
        && matches!(
            action,
            ConfigAction::Set(ConfigUpdate::UpdateMode(_))
                | ConfigAction::Reset(ConfigField::UpdateMode)
        )
    {
        let _ = writeln!(stderr, "ERROR: update-mode is only supported on Linux");
        return 1;
    }
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

    let action = if let ConfigAction::Reset(field) = action {
        reset_config_field(&mut config, field);
        return save_config(&storage, &config, stdout, stderr);
    } else {
        action
    };
    match action {
        ConfigAction::Options(_) | ConfigAction::Reset(_) => unreachable!(),
        ConfigAction::Get(field) => {
            write_config_value(stdout, &config, field);
            0
        }
        ConfigAction::Set(update) => match apply_config_update(&mut config, update) {
            Ok(message) => {
                let code = save_config(&storage, &config, stdout, stderr);
                if code == 0 {
                    let _ = writeln!(stdout, "{message}");
                }
                code
            }
            Err(error) => {
                let _ = writeln!(stderr, "ERROR: {error}");
                1
            }
        },
    }
}

pub(crate) fn config_storage(platform: &dyn Platform) -> Result<StorageManager, String> {
    platform
        .app_paths()
        .map(|paths| StorageManager::from_layout(StorageLayout::from_app_paths(&paths)))
        .map_err(|error| error.to_string())
}

pub(crate) fn load_or_default_config(storage: &StorageManager) -> Result<StorageConfig, String> {
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
        Some(ConfigField::IconMode) => {
            let _ = writeln!(
                output,
                "icon-mode = {}",
                match config.appearance.icon_display_mode {
                    storage::IconDisplayMode::Ascii => "ascii",
                    storage::IconDisplayMode::Image => "image",
                }
            );
        }
        Some(ConfigField::Motion) => {
            let _ = writeln!(
                output,
                "motion = {}",
                match config.appearance.motion_preference {
                    storage::MotionPreference::Full => "full",
                    storage::MotionPreference::Reduced => "reduced",
                }
            );
        }
        Some(ConfigField::AnimationSpeed) => {
            let _ = writeln!(
                output,
                "animation-speed = {}",
                config.appearance.animation_speed_percent
            );
        }
        Some(ConfigField::WeatherLocation) => {
            let _ = writeln!(
                output,
                "weather-location = {}",
                config.weather_location.as_deref().unwrap_or("auto")
            );
        }
        Some(ConfigField::UpdateMode) => {
            let _ = writeln!(
                output,
                "update-mode = {} (Linux)",
                match config.linux_update_mode {
                    storage::LinuxUpdateMode::Release => "release",
                    storage::LinuxUpdateMode::Beta => "beta",
                }
            );
        }
        None => {
            for field in [
                ConfigField::IconMode,
                ConfigField::Motion,
                ConfigField::AnimationSpeed,
                ConfigField::WeatherLocation,
                ConfigField::UpdateMode,
            ] {
                write_config_value(output, config, Some(field));
            }
            write_theme_summary(output, config);
            let _ = writeln!(output, "language = {}", config.language);
            let _ = writeln!(output, "timezone = {}", config.timezone);
            let _ = writeln!(output, "address = {}", config_address_summary(config));
        }
    }
}

fn apply_config_update(config: &mut StorageConfig, update: ConfigUpdate) -> Result<String, String> {
    match update {
        ConfigUpdate::IconMode(value) => {
            config.appearance.icon_display_mode =
                match clean_config_value("icon-mode", value)?.as_str() {
                    "ascii" => storage::IconDisplayMode::Ascii,
                    "image" => storage::IconDisplayMode::Image,
                    _ => return Err("icon-mode must be ascii or image".into()),
                };
            Ok("Updated icon mode".into())
        }
        ConfigUpdate::Motion(value) => {
            config.appearance.motion_preference =
                match clean_config_value("motion", value)?.as_str() {
                    "full" => storage::MotionPreference::Full,
                    "reduced" => storage::MotionPreference::Reduced,
                    _ => return Err("motion must be full or reduced".into()),
                };
            Ok("Updated motion preference".into())
        }
        ConfigUpdate::AnimationSpeed(value) => {
            let speed = value
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|speed| {
                    (storage::MIN_ANIMATION_SPEED_PERCENT..=storage::MAX_ANIMATION_SPEED_PERCENT)
                        .contains(speed)
                        && (speed - storage::MIN_ANIMATION_SPEED_PERCENT)
                            % storage::ANIMATION_SPEED_STEP_PERCENT
                            == 0
                })
                .ok_or("animation-speed must be 50, 75, 100, 125, 150, 175, or 200 (percent)")?;
            config.appearance.animation_speed_percent = speed;
            Ok(format!("Updated animation speed: {speed}%"))
        }
        ConfigUpdate::WeatherLocation(value) => {
            let value = clean_config_value("weather-location", value)?;
            // Match the weather address editor's character and length limits.
            if value.len() > 120
                || !value.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, ' ' | ',' | '.' | '-' | '\'' | '/' | '(' | ')')
                })
            {
                return Err("weather-location requires an English address, at most 120 characters (letters, digits, spaces and , . - ' / ( )); use auto to follow the timezone".into());
            }
            config.weather_location = (!value.eq_ignore_ascii_case("auto")).then_some(value);
            Ok("Updated weather location (timezone unchanged)".into())
        }
        ConfigUpdate::UpdateMode(value) => {
            config.linux_update_mode = match clean_config_value("update-mode", value)?.as_str() {
                "release" => storage::LinuxUpdateMode::Release,
                "beta" => storage::LinuxUpdateMode::Beta,
                _ => return Err("update-mode must be release or beta".into()),
            };
            Ok("Updated Linux update mode; no update has been started".into())
        }
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
    let root = ascii_assets::asset_root_for_recovery_from_env_or_current_exe()
        .map_err(|error| error.to_string())?;
    let catalog = i18n::LanguageCatalog::discover(&root)
        .unwrap_or_else(|_| i18n::LanguageCatalog::built_in());
    let code = i18n::canonicalize_locale(&value).unwrap_or_else(|_| value.clone());
    let option = catalog
        .options()
        .iter()
        .find(|option| option.code == code || option.native_name.eq_ignore_ascii_case(&value))
        .ok_or_else(|| {
            format!(
                "unsupported language {value:?}; available values: {}",
                catalog
                    .options()
                    .iter()
                    .map(|option| option.code.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    i18n::LanguageSnapshot::load(&root, &option.code, 1).map_err(|error| error.to_string())?;
    Ok(app::SetupLanguageOption {
        code: option.code.clone(),
        label: option.native_name.clone(),
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

fn save_config(
    storage: &StorageManager,
    config: &StorageConfig,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> i32 {
    match storage.save_config(config) {
        Ok(()) => {
            let _ = writeln!(
                stdout,
                "Saved. Restart TundraUX3 to apply settings to an already running UI."
            );
            0
        }
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not save config: {error}");
            1
        }
    }
}

fn reset_config_field(config: &mut StorageConfig, field: ConfigField) {
    let defaults = StorageConfig::default();
    match field {
        ConfigField::Theme => unreachable!("read-only field rejected by parser"),
        ConfigField::BorderShape => {
            config.appearance.border_shape = defaults.appearance.border_shape
        }
        ConfigField::BorderColor => {
            config.appearance.border_color = defaults.appearance.border_color
        }
        ConfigField::AccentColor => {
            config.appearance.accent_color = defaults.appearance.accent_color
        }
        ConfigField::Language => config.language = defaults.language,
        ConfigField::Timezone | ConfigField::Address => config.timezone = defaults.timezone,
        ConfigField::IconMode => {
            config.appearance.icon_display_mode = defaults.appearance.icon_display_mode
        }
        ConfigField::Motion => {
            config.appearance.motion_preference = defaults.appearance.motion_preference
        }
        ConfigField::AnimationSpeed => {
            config.appearance.animation_speed_percent = defaults.appearance.animation_speed_percent
        }
        ConfigField::WeatherLocation => config.weather_location = None,
        ConfigField::UpdateMode => config.linux_update_mode = defaults.linux_update_mode,
    }
}

fn write_config_options(output: &mut impl Write, field: Option<ConfigField>) {
    match field {
        Some(ConfigField::Language) => {
            let catalog = ascii_assets::asset_root_for_recovery_from_env_or_current_exe()
                .ok()
                .and_then(|root| i18n::LanguageCatalog::discover(&root).ok())
                .unwrap_or_else(i18n::LanguageCatalog::built_in);
            for option in catalog.options() {
                let _ = writeln!(output, "{}  {}", option.code, option.native_name);
            }
        }
        Some(ConfigField::Timezone | ConfigField::Address) => {
            for option in app::setup_timezone_options() {
                let _ = writeln!(output, "{}  {}", option.id, option.label);
            }
        }
        Some(field) => {
            let values = match field {
                ConfigField::BorderShape => "rounded, square".to_string(),
                ConfigField::BorderColor | ConfigField::AccentColor => {
                    format!("default, #RRGGBB, {}", BorderColor::NAMED_VALUES.join(", "))
                }
                ConfigField::IconMode => "ascii, image".into(),
                ConfigField::Motion => "full, reduced".into(),
                ConfigField::AnimationSpeed => {
                    "50, 75, 100, 125, 150, 175, 200 (percent; default 100)".into()
                }
                ConfigField::WeatherLocation => {
                    "English address (max 120 characters), auto (follow timezone location)".into()
                }
                ConfigField::UpdateMode => "release, beta (Linux only)".into(),
                ConfigField::Theme => {
                    "Read-only summary; use border-shape, border-color, accent-color or icon-mode"
                        .into()
                }
                ConfigField::Language | ConfigField::Timezone | ConfigField::Address => {
                    unreachable!()
                }
            };
            let _ = writeln!(output, "{values}");
        }
        None => {
            let _ = crate::help_text::write_config_help(output);
        }
    }
}
