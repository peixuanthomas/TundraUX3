use super::*;

#[test]
fn maximum_clock_width_includes_composed_glyphs_and_both_spacing_kinds() {
    let glyphs = "0123456789: APM"
        .chars()
        .map(|glyph| (glyph, vec![glyph.to_string()]))
        .collect();
    let font = ClockFontAsset {
        height: 1,
        spacing: 2,
        separator_spacing: 3,
        glyphs,
    };

    assert_eq!(font.max_rendered_clock_width(), 24);
}
