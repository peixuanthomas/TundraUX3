use super::*;

fn assets() -> ui::RuntimeAsciiAssets {
    let asset_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    let store =
        ui::AsciiAssetStore::load_with_root(asset_root, "default").expect("canonical ASCII assets");
    ui::RuntimeAsciiAssets::from_store(store)
}

#[test]
fn zero_timing_sequence_still_renders_logo_and_clears_screen() {
    let assets = assets();
    let first_visible_line = assets
        .banner_lines(BANNER_ASSET_KEY)
        .unwrap()
        .iter()
        .find(|line| !line.trim().is_empty())
        .unwrap();
    let mut output = Vec::new();

    display_startup_banner_with_timing_and_size_check(
        &mut output,
        &assets,
        StartupBannerTiming::ZERO,
        Color::White,
        || Ok((120, 40)),
    )
    .expect("zero-duration frost sequence");

    let output = String::from_utf8(output).unwrap();
    assert!(output.contains(first_visible_line.trim_end()));
    assert!(output.ends_with(CLEAR_SCREEN));
}

#[test]
fn startup_animation_stops_when_terminal_size_check_fails() {
    let assets = assets();
    let mut output = Vec::new();
    let mut checks = 0;

    let error = display_startup_banner_with_timing_and_size_check(
        &mut output,
        &assets,
        StartupBannerTiming::ZERO,
        Color::White,
        || {
            checks += 1;
            if checks >= 4 {
                Err(io::Error::other("terminal became too small"))
            } else {
                Ok((120, 40))
            }
        },
    )
    .expect_err("failed size check stops the frost sequence");

    let output = String::from_utf8(output).unwrap();
    assert!(error.to_string().contains("too small"));
    assert!(output.contains("ooooooooooooo"));
    assert!(!output.ends_with(CLEAR_SCREEN));
}
