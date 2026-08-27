use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::schema::{SCHEMA_VERSION, VersionedDocument};

pub const SUPPORTED_LANGUAGE: &str = "en-US";
pub const DEFAULT_ANIMATION_SPEED_PERCENT: u16 = 100;
pub const MIN_ANIMATION_SPEED_PERCENT: u16 = 50;
pub const MAX_ANIMATION_SPEED_PERCENT: u16 = 200;
pub const ANIMATION_SPEED_STEP_PERCENT: u16 = 25;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageConfig {
    pub schema_version: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default)]
    pub time_sync: TimeSyncConfig,
    /// Optional English address text used only by Weathr.
    /// `None` keeps weather tied to the configured timezone location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weather_location: Option<String>,
    #[serde(default)]
    pub shortcuts: BTreeMap<String, String>,
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub explorer: ExplorerConfig,
    #[serde(default)]
    pub editor: EditorConfig,
    #[serde(default)]
    pub launcher: LauncherConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub system_status: SystemStatusConfig,
}

impl StorageConfig {
    pub(crate) fn normalize(&mut self) -> bool {
        let legacy_schema = self.schema_version < 2;
        let mut changed = self.schema_version != SCHEMA_VERSION;
        if legacy_schema && self.appearance == legacy_appearance_default() {
            self.appearance = AppearanceConfig::default();
            changed = true;
        }
        self.schema_version = SCHEMA_VERSION;
        changed |= self.launcher.migrate_legacy_pinned_apps();
        changed |= self.editor.normalize();
        changed |= self.time_sync.normalize();
        changed |= self.system_status.normalize();
        if self.language != SUPPORTED_LANGUAGE {
            self.language = SUPPORTED_LANGUAGE.to_string();
            changed = true;
        }
        changed
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            theme: default_theme(),
            language: default_language(),
            timezone: default_timezone(),
            time_sync: TimeSyncConfig::default(),
            weather_location: None,
            shortcuts: BTreeMap::new(),
            appearance: AppearanceConfig::default(),
            explorer: ExplorerConfig::default(),
            editor: EditorConfig::default(),
            launcher: LauncherConfig::default(),
            security: SecurityConfig::default(),
            system_status: SystemStatusConfig::default(),
        }
    }
}

pub const SYSTEM_STATUS_MIN_AVAILABLE_GIB: u16 = 1;
pub const SYSTEM_STATUS_MAX_AVAILABLE_GIB: u16 = 1024;
pub const SYSTEM_STATUS_MIN_PERCENTAGE: u8 = 1;
pub const SYSTEM_STATUS_MAX_PERCENTAGE: u8 = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SystemStatusConfig {
    pub low_available_gib: u16,
    pub low_percentage: u8,
    pub critical_available_gib: u16,
    pub critical_percentage: u8,
}

impl Default for SystemStatusConfig {
    fn default() -> Self {
        Self {
            low_available_gib: 5,
            low_percentage: 10,
            critical_available_gib: 1,
            critical_percentage: 5,
        }
    }
}

impl SystemStatusConfig {
    pub fn normalize(&mut self) -> bool {
        let original = self.clone();
        self.low_available_gib = self.low_available_gib.clamp(
            SYSTEM_STATUS_MIN_AVAILABLE_GIB,
            SYSTEM_STATUS_MAX_AVAILABLE_GIB,
        );
        self.critical_available_gib = self.critical_available_gib.clamp(
            SYSTEM_STATUS_MIN_AVAILABLE_GIB,
            SYSTEM_STATUS_MAX_AVAILABLE_GIB,
        );
        self.low_percentage = self
            .low_percentage
            .clamp(SYSTEM_STATUS_MIN_PERCENTAGE, SYSTEM_STATUS_MAX_PERCENTAGE);
        self.critical_percentage = self
            .critical_percentage
            .clamp(SYSTEM_STATUS_MIN_PERCENTAGE, SYSTEM_STATUS_MAX_PERCENTAGE);
        self.critical_available_gib = self.critical_available_gib.min(self.low_available_gib);
        self.critical_percentage = self.critical_percentage.min(self.low_percentage);
        *self != original
    }
}

impl VersionedDocument for StorageConfig {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn upgrade_schema(&mut self) {
        self.normalize();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TimeSyncConfig {
    pub source: TimeSyncSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
}

impl Default for TimeSyncConfig {
    fn default() -> Self {
        Self {
            source: TimeSyncSource::NetworkServer,
            server_url: None,
        }
    }
}

impl TimeSyncConfig {
    fn normalize(&mut self) -> bool {
        let normalized = self
            .server_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if normalized == self.server_url {
            false
        } else {
            self.server_url = normalized;
            true
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimeSyncSource {
    #[default]
    NetworkServer,
    OperatingSystem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppearanceConfig {
    pub border_shape: BorderShape,
    pub border_color: BorderColor,
    #[serde(deserialize_with = "deserialize_accent_color")]
    pub accent_color: AccentColor,
    pub icon_display_mode: IconDisplayMode,
    /// Controls terminal UI transitions.  It is deliberately per-user so an
    /// accessibility preference never changes another user's session.
    #[serde(default)]
    pub motion_preference: MotionPreference,
    #[serde(
        default = "default_animation_speed_percent",
        deserialize_with = "deserialize_animation_speed_percent"
    )]
    pub animation_speed_percent: u16,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            border_shape: BorderShape::Rounded,
            border_color: BorderColor::Rgb(0x29, 0x43, 0x4e),
            accent_color: BorderColor::Rgb(0x63, 0xd3, 0xe5),
            icon_display_mode: IconDisplayMode::Image,
            motion_preference: MotionPreference::Full,
            animation_speed_percent: DEFAULT_ANIMATION_SPEED_PERCENT,
        }
    }
}

impl AppearanceConfig {
    /// The complete appearance written by pre-Glacier versions.  Migration
    /// intentionally treats only this exact tuple as the old default; any
    /// custom border, accent, or icon choice stays untouched.
    pub(crate) const fn is_legacy_default(&self) -> bool {
        matches!(self.border_shape, BorderShape::Rounded)
            && matches!(self.border_color, BorderColor::White)
            && matches!(self.accent_color, BorderColor::Cyan)
            && matches!(self.icon_display_mode, IconDisplayMode::Image)
            && matches!(self.motion_preference, MotionPreference::Full)
            && self.animation_speed_percent == DEFAULT_ANIMATION_SPEED_PERCENT
    }

    pub fn normalized_animation_speed_percent(&self) -> u16 {
        self.animation_speed_percent
            .clamp(MIN_ANIMATION_SPEED_PERCENT, MAX_ANIMATION_SPEED_PERCENT)
    }
}

const fn legacy_appearance_default() -> AppearanceConfig {
    AppearanceConfig {
        border_shape: BorderShape::Rounded,
        border_color: BorderColor::White,
        accent_color: BorderColor::Cyan,
        icon_display_mode: IconDisplayMode::Image,
        motion_preference: MotionPreference::Full,
        animation_speed_percent: DEFAULT_ANIMATION_SPEED_PERCENT,
    }
}

const fn default_animation_speed_percent() -> u16 {
    DEFAULT_ANIMATION_SPEED_PERCENT
}

fn deserialize_animation_speed_percent<'de, DeserializerType>(
    deserializer: DeserializerType,
) -> Result<u16, DeserializerType::Error>
where
    DeserializerType: Deserializer<'de>,
{
    Ok(u16::deserialize(deserializer)?
        .clamp(MIN_ANIMATION_SPEED_PERCENT, MAX_ANIMATION_SPEED_PERCENT))
}

/// Accessibility preference for Frost Motion.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MotionPreference {
    #[default]
    Full,
    Reduced,
}

impl MotionPreference {
    pub const fn reduced(self) -> bool {
        matches!(self, Self::Reduced)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IconDisplayMode {
    Ascii,
    #[default]
    Image,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BorderShape {
    #[default]
    Rounded,
    Square,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ExplorerConfig {
    pub show_hidden: bool,
    pub show_system: bool,
    pub show_extensions: bool,
    pub folders_first: bool,
    pub case_sensitive_sort: bool,
    pub size_format: ExplorerSizeFormat,
    pub date_zone: ExplorerDateZone,
    pub confirm_delete: bool,
    pub confirm_name_conflicts: bool,
    pub show_sidebar: bool,
    pub sort_field: ExplorerSortField,
    pub sort_direction: ExplorerSortDirection,
}

impl Default for ExplorerConfig {
    fn default() -> Self {
        Self {
            show_hidden: false,
            show_system: false,
            show_extensions: true,
            folders_first: true,
            case_sensitive_sort: false,
            size_format: ExplorerSizeFormat::HumanBinary,
            date_zone: ExplorerDateZone::ConfiguredTimezone,
            confirm_delete: true,
            confirm_name_conflicts: true,
            show_sidebar: true,
            sort_field: ExplorerSortField::Name,
            sort_direction: ExplorerSortDirection::Ascending,
        }
    }
}

pub const DEFAULT_EDITOR_EXPLORER_OPEN_EXTENSIONS: &[&str] =
    &["md", "markdown", "mdown", "mkd", "txt", "log"];
pub const MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS: usize = 64;
pub const MAX_EDITOR_EXPLORER_OPEN_EXTENSION_LEN: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct EditorConfig {
    /// Filename suffixes that Explorer routes to the built-in editor.
    ///
    /// Values omit the leading dot, are matched case-insensitively, and may
    /// contain multiple components such as `d.ts`.
    pub explorer_open_extensions: Vec<String>,
    pub cursor_acceleration_enabled: bool,
    pub cursor_acceleration_delay_ms: u32,
    pub cursor_acceleration_ramp_ms: u32,
    pub cursor_horizontal_max_step: u8,
    pub cursor_vertical_max_step: u8,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            explorer_open_extensions: DEFAULT_EDITOR_EXPLORER_OPEN_EXTENSIONS
                .iter()
                .map(|extension| (*extension).to_string())
                .collect(),
            cursor_acceleration_enabled: true,
            cursor_acceleration_delay_ms: 2_000,
            cursor_acceleration_ramp_ms: 3_000,
            cursor_horizontal_max_step: 8,
            cursor_vertical_max_step: 3,
        }
    }
}

impl EditorConfig {
    fn normalize(&mut self) -> bool {
        let normalized = self
            .explorer_open_extensions
            .iter()
            .filter_map(|extension| normalize_editor_explorer_open_extension(extension))
            .fold(Vec::new(), |mut extensions, extension| {
                if extensions.len() < MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS
                    && !extensions.contains(&extension)
                {
                    extensions.push(extension);
                }
                extensions
            });
        if normalized == self.explorer_open_extensions {
            false
        } else {
            self.explorer_open_extensions = normalized;
            true
        }
    }
}

/// Normalizes one configurable Explorer suffix. The leading dot is optional;
/// path separators and empty compound-extension components are rejected.
pub fn normalize_editor_explorer_open_extension(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches('.').to_ascii_lowercase();
    if value.is_empty()
        || value.len() > MAX_EDITOR_EXPLORER_OPEN_EXTENSION_LEN
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '+' | '.')
        })
    {
        return None;
    }
    Some(value)
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorerSizeFormat {
    #[default]
    HumanBinary,
    Bytes,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorerDateZone {
    #[default]
    ConfiguredTimezone,
    Utc,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorerSortField {
    #[default]
    Name,
    Type,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorerSortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LauncherConfig {
    pub entries: Vec<LauncherEntryRecord>,
    /// Legacy input retained only so schema-1 configurations can be read. Values are moved into
    /// `entries` during normalization and are never emitted again.
    #[serde(skip_serializing)]
    pub pinned_apps: Vec<String>,
    /// Legacy directory pins remain readable for backwards compatibility, but Launcher does not
    /// treat directories as executable entries.
    pub pinned_dirs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    #[default]
    White,
    Rgb(u8, u8, u8),
}

/// A semantic alias for colors used to emphasize selected and focused UI elements.
///
/// Accent and border colors intentionally share the same serialized color vocabulary.
pub type AccentColor = BorderColor;

/// The legacy UI visual accent: cyan.
/// Legacy serialized `"default"` accent sentinel. New defaults are explicit
/// Glacier RGB values; old files keep parsing this token as cyan.
pub const DEFAULT_ACCENT_COLOR: AccentColor = AccentColor::Cyan;

fn deserialize_accent_color<'de, DeserializerType>(
    deserializer: DeserializerType,
) -> Result<AccentColor, DeserializerType::Error>
where
    DeserializerType: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().eq_ignore_ascii_case("default") {
        Ok(DEFAULT_ACCENT_COLOR)
    } else {
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl BorderColor {
    pub const NAMED_VALUES: &'static [&'static str] = &[
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "gray",
        "dark-gray",
        "light-red",
        "light-green",
        "light-yellow",
        "light-blue",
        "light-magenta",
        "light-cyan",
        "white",
    ];

    pub const fn rgb(self) -> Option<(u8, u8, u8)> {
        match self {
            Self::Rgb(red, green, blue) => Some((red, green, blue)),
            _ => None,
        }
    }
}

impl fmt::Display for BorderColor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Black => "black",
            Self::Red => "red",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Blue => "blue",
            Self::Magenta => "magenta",
            Self::Cyan => "cyan",
            Self::Gray => "gray",
            Self::DarkGray => "dark-gray",
            Self::LightRed => "light-red",
            Self::LightGreen => "light-green",
            Self::LightYellow => "light-yellow",
            Self::LightBlue => "light-blue",
            Self::LightMagenta => "light-magenta",
            Self::LightCyan => "light-cyan",
            Self::White => "white",
            Self::Rgb(red, green, blue) => {
                return write!(formatter, "#{red:02X}{green:02X}{blue:02X}");
            }
        };
        formatter.write_str(name)
    }
}

impl FromStr for BorderColor {
    type Err = BorderColorParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let normalized = value.to_ascii_lowercase();
        let color = match normalized.as_str() {
            "black" => Self::Black,
            "red" => Self::Red,
            "green" => Self::Green,
            "yellow" => Self::Yellow,
            "blue" => Self::Blue,
            "magenta" => Self::Magenta,
            "cyan" => Self::Cyan,
            "gray" => Self::Gray,
            "dark-gray" => Self::DarkGray,
            "light-red" => Self::LightRed,
            "light-green" => Self::LightGreen,
            "light-yellow" => Self::LightYellow,
            "light-blue" => Self::LightBlue,
            "light-magenta" => Self::LightMagenta,
            "light-cyan" => Self::LightCyan,
            "white" | "default" => Self::White,
            _ => parse_rgb(value)?,
        };
        Ok(color)
    }
}

impl Serialize for BorderColor {
    fn serialize<SerializerType>(
        &self,
        serializer: SerializerType,
    ) -> Result<SerializerType::Ok, SerializerType::Error>
    where
        SerializerType: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for BorderColor {
    fn deserialize<DeserializerType>(
        deserializer: DeserializerType,
    ) -> Result<Self, DeserializerType::Error>
    where
        DeserializerType: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderColorParseError {
    value: String,
}

impl fmt::Display for BorderColorParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unsupported border color {:?}; use #RRGGBB or one of: {}",
            self.value,
            BorderColor::NAMED_VALUES.join(", ")
        )
    }
}

impl std::error::Error for BorderColorParseError {}

fn parse_rgb(value: &str) -> Result<BorderColor, BorderColorParseError> {
    let invalid = || BorderColorParseError {
        value: value.to_string(),
    };
    let hex = value
        .strip_prefix('#')
        .filter(|hex| hex.len() == 6 && hex.is_ascii())
        .ok_or_else(invalid)?;
    let red = u8::from_str_radix(&hex[0..2], 16).map_err(|_| invalid())?;
    let green = u8::from_str_radix(&hex[2..4], 16).map_err(|_| invalid())?;
    let blue = u8::from_str_radix(&hex[4..6], 16).map_err(|_| invalid())?;
    Ok(BorderColor::Rgb(red, green, blue))
}

impl LauncherConfig {
    fn migrate_legacy_pinned_apps(&mut self) -> bool {
        if self.pinned_apps.is_empty() {
            return false;
        }

        for path in &self.pinned_apps {
            if self.entries.iter().any(|entry| entry.path == *path) {
                continue;
            }
            self.entries.push(LauncherEntryRecord {
                id: legacy_launcher_entry_id(path),
                path: path.clone(),
                executable_kind: None,
                fingerprint: None,
                added_by_user_id: "legacy".to_string(),
                added_at_epoch_ms: 0,
            });
        }
        self.pinned_apps.clear();
        true
    }
}

/// A globally-managed application approved for Launcher execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LauncherEntryRecord {
    pub id: String,
    /// A canonical, absolute path recorded by the admin approval workflow.
    pub path: String,
    /// Missing only for entries migrated from the obsolete `pinned_apps` setting; such entries
    /// require fresh admin approval before they can be launched.
    pub executable_kind: Option<LauncherExecutableKind>,
    pub fingerprint: Option<LauncherFingerprint>,
    pub added_by_user_id: String,
    pub added_at_epoch_ms: i64,
}

/// The executable classification persisted with a Launcher approval.
///
/// This mirrors `platform::ExecutableKind` without making storage serialization depend on
/// a platform-facing enum. Application code converts between the two at its boundary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LauncherExecutableKind {
    NativeBinary,
    Installer,
    Script,
    Shortcut,
    ApplicationBundle,
}

/// Content identity captured when an administrator approves a Launcher entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LauncherFingerprint {
    pub sha256: String,
    pub byte_len: u64,
    pub modified_at_epoch_ms: Option<i64>,
}

fn legacy_launcher_entry_id(path: &str) -> String {
    // FNV-1a makes the migration deterministic without introducing a hashing dependency. This
    // ID identifies a record only; it is never used as an integrity check.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("legacy-{hash:016x}")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityConfig {
    /// Retained for backward-compatible config parsing. Release builds never enable debug mode.
    pub allow_release_debug: bool,
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_language() -> String {
    SUPPORTED_LANGUAGE.to_string()
}

fn default_timezone() -> String {
    "UTC".to_string()
}

#[cfg(test)]
mod glacier_migration_tests {
    use super::*;

    fn legacy() -> AppearanceConfig {
        legacy_appearance_default()
    }

    #[test]
    fn glacier_is_the_new_and_reset_appearance_default() {
        assert_eq!(
            AppearanceConfig::default(),
            AppearanceConfig {
                border_shape: BorderShape::Rounded,
                border_color: BorderColor::Rgb(0x29, 0x43, 0x4e),
                accent_color: BorderColor::Rgb(0x63, 0xd3, 0xe5),
                icon_display_mode: IconDisplayMode::Image,
                motion_preference: MotionPreference::Full,
                animation_speed_percent: DEFAULT_ANIMATION_SPEED_PERCENT,
            }
        );
        let appearance = AppearanceConfig::default();
        assert_eq!(appearance, StorageConfig::default().appearance);
    }

    #[test]
    fn animation_speed_defaults_and_clamps_when_loaded() {
        let missing: AppearanceConfig = serde_json::from_str("{}").expect("default appearance");
        let below: AppearanceConfig = serde_json::from_str(r#"{"animation_speed_percent":0}"#)
            .expect("lower bound appearance");
        let above: AppearanceConfig = serde_json::from_str(r#"{"animation_speed_percent":999}"#)
            .expect("upper bound appearance");

        assert_eq!(
            missing.animation_speed_percent,
            DEFAULT_ANIMATION_SPEED_PERCENT
        );
        assert_eq!(below.animation_speed_percent, MIN_ANIMATION_SPEED_PERCENT);
        assert_eq!(above.animation_speed_percent, MAX_ANIMATION_SPEED_PERCENT);
    }

    #[test]
    fn exact_legacy_config_default_migrates_to_glacier() {
        let mut config = StorageConfig {
            schema_version: 1,
            appearance: legacy(),
            ..StorageConfig::default()
        };
        assert!(config.normalize());
        assert_eq!(config.appearance, AppearanceConfig::default());
    }

    #[test]
    fn every_single_legacy_appearance_override_is_preserved() {
        let variants = [
            AppearanceConfig {
                border_shape: BorderShape::Square,
                ..legacy()
            },
            AppearanceConfig {
                border_color: BorderColor::LightBlue,
                ..legacy()
            },
            AppearanceConfig {
                accent_color: BorderColor::LightMagenta,
                ..legacy()
            },
            AppearanceConfig {
                icon_display_mode: IconDisplayMode::Ascii,
                ..legacy()
            },
            AppearanceConfig {
                motion_preference: MotionPreference::Reduced,
                ..legacy()
            },
            AppearanceConfig {
                animation_speed_percent: 125,
                ..legacy()
            },
        ];
        for expected in variants {
            let mut config = StorageConfig {
                schema_version: 1,
                appearance: expected.clone(),
                ..StorageConfig::default()
            };
            config.normalize();
            assert_eq!(config.appearance, expected);
        }
    }

    #[test]
    fn system_status_defaults_and_normalization_are_stable() {
        assert_eq!(
            SystemStatusConfig::default(),
            SystemStatusConfig {
                low_available_gib: 5,
                low_percentage: 10,
                critical_available_gib: 1,
                critical_percentage: 5,
            }
        );

        let mut config = SystemStatusConfig {
            low_available_gib: 0,
            low_percentage: 101,
            critical_available_gib: u16::MAX,
            critical_percentage: 0,
        };
        assert!(config.normalize());
        assert_eq!(config.low_available_gib, SYSTEM_STATUS_MIN_AVAILABLE_GIB);
        assert_eq!(config.low_percentage, SYSTEM_STATUS_MAX_PERCENTAGE);
        assert_eq!(config.critical_available_gib, config.low_available_gib);
        assert_eq!(config.critical_percentage, SYSTEM_STATUS_MIN_PERCENTAGE);

        let mut inverted = SystemStatusConfig {
            low_available_gib: 20,
            low_percentage: 30,
            critical_available_gib: 21,
            critical_percentage: 31,
        };
        assert!(inverted.normalize());
        assert_eq!(inverted.critical_available_gib, 20);
        assert_eq!(inverted.critical_percentage, 30);
        assert!(!inverted.normalize());
    }
}
