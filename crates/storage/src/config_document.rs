use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::schema::{SCHEMA_VERSION, VersionedDocument};

pub const SUPPORTED_LANGUAGE: &str = "en-US";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageConfig {
    pub schema_version: u32,
    #[serde(default = "default_theme", skip_serializing)]
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
}

impl StorageConfig {
    pub(crate) fn normalize(&mut self) -> bool {
        let mut changed = self.launcher.migrate_legacy_pinned_apps();
        changed |= self.editor.normalize();
        changed |= self.time_sync.normalize();
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
        }
    }
}

impl VersionedDocument for StorageConfig {
    fn schema_version(&self) -> u32 {
        self.schema_version
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
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            border_shape: BorderShape::default(),
            border_color: BorderColor::default(),
            accent_color: default_accent_color(),
        }
    }
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
pub const DEFAULT_ACCENT_COLOR: AccentColor = AccentColor::Cyan;

fn default_accent_color() -> AccentColor {
    DEFAULT_ACCENT_COLOR
}

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

impl Default for LauncherEntryRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            path: String::new(),
            executable_kind: None,
            fingerprint: None,
            added_by_user_id: String::new(),
            added_at_epoch_ms: 0,
        }
    }
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
