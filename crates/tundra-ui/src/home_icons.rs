use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeIcon {
    pub key: &'static str,
    pub width: u16,
    pub height: u16,
    pub lines: &'static [&'static str],
}

const HOME_ICONS_TOML: &str = include_str!("../assets/home_icons.toml");
const EXPECTED_KEYS: &[&str] = &[
    "explorer",
    "launcher",
    "editor",
    "settings",
    "diagnostics",
    "user_management",
    "user_profile",
    "default",
];

static HOME_ICONS: OnceLock<Vec<HomeIcon>> = OnceLock::new();

pub fn home_icon_for_label(label: &str) -> HomeIcon {
    let key = icon_key_for_label(label);
    let icons = HOME_ICONS.get_or_init(|| parse_home_icons(HOME_ICONS_TOML));
    icons
        .iter()
        .find(|icon| icon.key == key)
        .or_else(|| icons.iter().find(|icon| icon.key == "default"))
        .copied()
        .expect("home icon asset must define a default icon")
}

fn icon_key_for_label(label: &str) -> &'static str {
    match label {
        "Explorer" => "explorer",
        "Launcher" => "launcher",
        "Editor" => "editor",
        "Settings" => "settings",
        "Diagnostics" => "diagnostics",
        "User Management" => "user_management",
        "User Profile" => "user_profile",
        _ => "default",
    }
}

fn parse_home_icons(asset: &'static str) -> Vec<HomeIcon> {
    let mut icons = Vec::new();
    let mut current_key: Option<&'static str> = None;
    let mut current_width = 0;
    let mut current_height = 0;
    let mut current_lines: Vec<&'static str> = Vec::new();
    let mut in_lines = false;

    for raw_line in asset.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("[icons.") && line.ends_with(']') {
            push_icon(
                &mut icons,
                current_key.take(),
                current_width,
                current_height,
                &mut current_lines,
            );
            current_key = Some(&line[7..line.len() - 1]);
            current_width = 0;
            current_height = 0;
            in_lines = false;
            continue;
        }

        if in_lines {
            if line.starts_with(']') {
                in_lines = false;
            } else if let Some(value) = literal_array_value(line) {
                current_lines.push(value);
            }
            continue;
        }

        if let Some(value) = line.strip_prefix("width = ") {
            current_width = value.parse().expect("home icon width must be a u16");
        } else if let Some(value) = line.strip_prefix("height = ") {
            current_height = value.parse().expect("home icon height must be a u16");
        } else if line == "lines = [" {
            in_lines = true;
        }
    }

    push_icon(
        &mut icons,
        current_key,
        current_width,
        current_height,
        &mut current_lines,
    );
    validate_icons(&icons);
    icons
}

fn push_icon(
    icons: &mut Vec<HomeIcon>,
    key: Option<&'static str>,
    width: u16,
    height: u16,
    lines: &mut Vec<&'static str>,
) {
    let Some(key) = key else {
        return;
    };
    let leaked_lines = std::mem::take(lines).into_boxed_slice();
    icons.push(HomeIcon {
        key,
        width,
        height,
        lines: Box::leak(leaked_lines),
    });
}

fn literal_array_value(line: &'static str) -> Option<&'static str> {
    let start = line.find('\'')?;
    let end = line.rfind('\'')?;
    (end > start).then_some(&line[start + 1..end])
}

fn validate_icons(icons: &[HomeIcon]) {
    for key in EXPECTED_KEYS {
        let icon = icons
            .iter()
            .find(|icon| icon.key == *key)
            .unwrap_or_else(|| panic!("home icon asset missing icon: {key}"));
        assert!(icon.width > 0, "home icon width must be positive: {key}");
        assert!(icon.height > 0, "home icon height must be positive: {key}");
        assert!(
            icon.lines.len() == usize::from(icon.height),
            "home icon line count must match height: {key}"
        );
    }
}
