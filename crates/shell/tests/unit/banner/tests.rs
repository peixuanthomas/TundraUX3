use super::*;

#[test]
fn animated_banner_stops_rendering_when_a_size_check_fails() {
    let asset_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    let store =
        ui::AsciiAssetStore::load_with_root(asset_root, "default").expect("canonical ASCII assets");
    let assets = ui::RuntimeAsciiAssets::from_store(store);
    let banner_lines = assets
        .banner_lines(BANNER_ASSET_KEY)
        .expect("default banner")
        .to_vec();
    let mut output = Vec::new();
    let mut checks = 0;

    let error = display_animated_banner_with_assets_and_size_check(
        &mut output,
        Duration::ZERO,
        &assets,
        Color::White,
        || {
            checks += 1;
            if checks >= 3 {
                Err(io::Error::other("terminal became too small"))
            } else {
                Ok(())
            }
        },
    )
    .expect_err("failed size check must stop the animation");

    let output = String::from_utf8(output).expect("banner output should be UTF-8");
    assert!(error.to_string().contains("too small"));
    assert!(output.contains(&banner_lines[0]));
    assert!(!output.contains(&banner_lines[1]));
}
