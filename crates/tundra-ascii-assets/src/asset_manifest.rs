pub const ENV_ASSETS_DIR: &str = "TUNDRA_ASCII_ASSETS_DIR";
pub const DEFAULT_THEME_ID: &str = "default";
pub const CANONICAL_ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

pub(crate) const REQUIRED_TEXT_ARTS: &[(&str, &str)] = &[
    ("weathr/animation/airplane", "weathr/animation/airplane.txt"),
    ("weathr/animation/cloud_0", "weathr/animation/cloud_0.txt"),
    ("weathr/animation/cloud_1", "weathr/animation/cloud_1.txt"),
    ("weathr/animation/cloud_2", "weathr/animation/cloud_2.txt"),
    ("weathr/animation/cloud_3", "weathr/animation/cloud_3.txt"),
    ("weathr/animation/sun_0", "weathr/animation/sun_0.txt"),
    ("weathr/animation/sun_1", "weathr/animation/sun_1.txt"),
    (
        "weathr/animation/moon/phase_0",
        "weathr/animation/moon/phase_0.txt",
    ),
    (
        "weathr/animation/moon/phase_1",
        "weathr/animation/moon/phase_1.txt",
    ),
    (
        "weathr/animation/moon/phase_2",
        "weathr/animation/moon/phase_2.txt",
    ),
    (
        "weathr/animation/moon/phase_3",
        "weathr/animation/moon/phase_3.txt",
    ),
    (
        "weathr/animation/moon/phase_4",
        "weathr/animation/moon/phase_4.txt",
    ),
    (
        "weathr/animation/moon/phase_5",
        "weathr/animation/moon/phase_5.txt",
    ),
    (
        "weathr/animation/moon/phase_6",
        "weathr/animation/moon/phase_6.txt",
    ),
    (
        "weathr/animation/moon/phase_7",
        "weathr/animation/moon/phase_7.txt",
    ),
    ("weathr/world/fence", "weathr/world/fence.txt"),
    ("weathr/world/house", "weathr/world/house.txt"),
    ("weathr/world/mailbox", "weathr/world/mailbox.txt"),
    ("weathr/world/pine_tree", "weathr/world/pine_tree.txt"),
    ("weathr/world/tree", "weathr/world/tree.txt"),
];

const REQUIRED_TOML_ASSETS: &[(&str, &str, AssetKind)] = &[
    ("banner", "banner.toml", AssetKind::ArtSet),
    ("explorer_icons", "explorer_icons.toml", AssetKind::ArtSet),
    ("home_icons", "home_icons.toml", AssetKind::ArtSet),
    (
        "weathr/render/clock_font",
        "weathr/render/clock_font.toml",
        AssetKind::Font,
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Text,
    ArtSet,
    Font,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredAsset {
    pub key: &'static str,
    pub relative_path: &'static str,
    pub kind: AssetKind,
}

pub fn required_assets() -> Vec<RequiredAsset> {
    let mut assets = REQUIRED_TOML_ASSETS
        .iter()
        .map(|(key, relative_path, kind)| RequiredAsset {
            key,
            relative_path,
            kind: *kind,
        })
        .collect::<Vec<_>>();
    assets.extend(
        REQUIRED_TEXT_ARTS
            .iter()
            .map(|(key, relative_path)| RequiredAsset {
                key,
                relative_path,
                kind: AssetKind::Text,
            }),
    );
    assets
}
