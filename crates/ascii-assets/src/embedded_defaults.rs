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

    #[test]
    fn embedded_default_theme_covers_required_ascii_assets() {
        let embedded = EMBEDDED_DEFAULT_THEME_FILES
            .iter()
            .map(|asset| (asset.key, asset.relative_path))
            .collect::<HashSet<_>>();
        let required = required_assets()
            .into_iter()
            .map(|asset| (asset.key, asset.relative_path))
            .collect::<HashSet<_>>();

        assert_eq!(embedded.len(), EMBEDDED_DEFAULT_THEME_FILES.len());
        assert_eq!(embedded, required);
        assert!(
            EMBEDDED_DEFAULT_THEME_FILES
                .iter()
                .all(|asset| !asset.contents.is_empty())
        );
    }
}
