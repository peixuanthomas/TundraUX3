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
    assert!(embedded.contains(&("launcher_icons/editor.png", "launcher_icons/editor.png")));
    assert_eq!(embedded.len(), required.len() + 11);
    assert_eq!(
        EMBEDDED_DEFAULT_THEME_FILES
            .iter()
            .filter(|asset| Path::new(asset.relative_path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png")))
            .count(),
        11
    );
    assert!(
        EMBEDDED_DEFAULT_THEME_FILES
            .iter()
            .all(|asset| !asset.contents.is_empty())
    );
}
