use super::*;

#[test]
fn bundled_assets_are_complete_and_self_contained() {
    let assets = WeatherAsciiAssets::bundled().expect("bundled font is valid");

    assert_eq!(assets.animation().clouds.len(), 4);
    assert_eq!(assets.animation().sun_frames.len(), 2);
    assert_eq!(assets.animation().moon_phases.len(), 8);
    assert_eq!(assets.clock_font().height(), 7);
    assert!(assets.max_dimensions().0 >= 64);
    assert!(assets.max_dimensions().1 >= 10);
}
