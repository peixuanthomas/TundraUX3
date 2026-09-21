use super::*;

#[test]
fn candidates_cover_adjacent_and_test_profile_assets() {
    let candidates = asset_root_candidates(Path::new("/repo/target/debug/deps/tundra-shell-abc"))
        .expect("candidate paths");

    assert_eq!(
        candidates[0],
        PathBuf::from("/repo/target/debug/deps/assets")
    );
    assert!(candidates.contains(&PathBuf::from("/repo/target/debug/assets")));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_candidates_include_system_install_location() {
    let candidates =
        asset_root_candidates(Path::new("/usr/bin/tundra-shell")).expect("candidate paths");

    assert_eq!(
        candidates.last(),
        Some(&PathBuf::from("/usr/share/tundraux3/assets"))
    );
}
