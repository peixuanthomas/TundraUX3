#[derive(Debug)]
pub(crate) struct EmbeddedDefaultThemeFile {
    pub key: &'static str,
    pub relative_path: &'static str,
    pub contents: &'static [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultThemeFile {
    pub key: &'static str,
    pub relative_path: &'static str,
}

macro_rules! embedded_default_theme_file {
    ($key:literal, $relative_path:literal) => {
        EmbeddedDefaultThemeFile {
            key: $key,
            relative_path: $relative_path,
            contents: include_bytes!(concat!("../assets/themes/default/", $relative_path)),
        }
    };
}

pub(crate) const EMBEDDED_DEFAULT_THEME_FILES: &[EmbeddedDefaultThemeFile] = &[
    embedded_default_theme_file!("banner", "banner.toml"),
    embedded_default_theme_file!("explorer_icons", "explorer_icons.toml"),
    embedded_default_theme_file!("home_icons", "home_icons.toml"),
    embedded_default_theme_file!("launcher_icons", "launcher_icons.toml"),
    embedded_default_theme_file!("home_icons/default.png", "home_icons/default.png"),
    embedded_default_theme_file!("home_icons/diagnostics.png", "home_icons/diagnostics.png"),
    embedded_default_theme_file!("home_icons/editor.png", "home_icons/editor.png"),
    embedded_default_theme_file!("home_icons/explorer.png", "home_icons/explorer.png"),
    embedded_default_theme_file!("home_icons/launcher.png", "home_icons/launcher.png"),
    embedded_default_theme_file!("home_icons/settings.png", "home_icons/settings.png"),
    embedded_default_theme_file!(
        "home_icons/system_status.png",
        "home_icons/system_status.png"
    ),
    embedded_default_theme_file!(
        "home_icons/user_management.png",
        "home_icons/user_management.png"
    ),
    embedded_default_theme_file!("home_icons/user_profile.png", "home_icons/user_profile.png"),
    embedded_default_theme_file!(
        "launcher_icons/command_line.png",
        "launcher_icons/command_line.png"
    ),
    embedded_default_theme_file!("weathr/render/clock_font", "weathr/render/clock_font.toml"),
    embedded_default_theme_file!("weathr/animation/airplane", "weathr/animation/airplane.txt"),
    embedded_default_theme_file!("weathr/animation/cloud_0", "weathr/animation/cloud_0.txt"),
    embedded_default_theme_file!("weathr/animation/cloud_1", "weathr/animation/cloud_1.txt"),
    embedded_default_theme_file!("weathr/animation/cloud_2", "weathr/animation/cloud_2.txt"),
    embedded_default_theme_file!("weathr/animation/cloud_3", "weathr/animation/cloud_3.txt"),
    embedded_default_theme_file!("weathr/animation/sun_0", "weathr/animation/sun_0.txt"),
    embedded_default_theme_file!("weathr/animation/sun_1", "weathr/animation/sun_1.txt"),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_0",
        "weathr/animation/moon/phase_0.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_1",
        "weathr/animation/moon/phase_1.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_2",
        "weathr/animation/moon/phase_2.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_3",
        "weathr/animation/moon/phase_3.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_4",
        "weathr/animation/moon/phase_4.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_5",
        "weathr/animation/moon/phase_5.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_6",
        "weathr/animation/moon/phase_6.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_7",
        "weathr/animation/moon/phase_7.txt"
    ),
    embedded_default_theme_file!("weathr/world/fence", "weathr/world/fence.txt"),
    embedded_default_theme_file!("weathr/world/house", "weathr/world/house.txt"),
    embedded_default_theme_file!("weathr/world/mailbox", "weathr/world/mailbox.txt"),
    embedded_default_theme_file!("weathr/world/pine_tree", "weathr/world/pine_tree.txt"),
    embedded_default_theme_file!("weathr/world/tree", "weathr/world/tree.txt"),
];

pub(crate) fn embedded_default_theme_file(key: &str) -> Option<&'static EmbeddedDefaultThemeFile> {
    EMBEDDED_DEFAULT_THEME_FILES
        .iter()
        .find(|asset| asset.key == key)
}

pub fn default_theme_files() -> Vec<DefaultThemeFile> {
    EMBEDDED_DEFAULT_THEME_FILES
        .iter()
        .map(|file| DefaultThemeFile {
            key: file.key,
            relative_path: file.relative_path,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::required_assets;
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn embedded_default_theme_covers_required_ascii_assets_and_images() {
        let embedded = EMBEDDED_DEFAULT_THEME_FILES
            .iter()
            .map(|asset| (asset.key, asset.relative_path))
            .collect::<HashSet<_>>();
        let required = required_assets()
            .into_iter()
            .map(|asset| (asset.key, asset.relative_path))
            .collect::<HashSet<_>>();

        assert_eq!(embedded.len(), EMBEDDED_DEFAULT_THEME_FILES.len());
        assert!(required.is_subset(&embedded));
        assert!(embedded.contains(&(
            "home_icons/system_status.png",
            "home_icons/system_status.png"
        )));
        assert_eq!(embedded.len(), required.len() + 10);
        assert_eq!(
            EMBEDDED_DEFAULT_THEME_FILES
                .iter()
                .filter(|asset| Path::new(asset.relative_path)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png")))
                .count(),
            10
        );
        assert!(
            EMBEDDED_DEFAULT_THEME_FILES
                .iter()
                .all(|asset| !asset.contents.is_empty())
        );
    }
}
