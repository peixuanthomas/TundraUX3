use super::*;
use chrono::NaiveTime;

#[test]
fn formats_twenty_four_hour_time() {
    let time = NaiveTime::from_hms_opt(0, 5, 30).unwrap();
    let evening = NaiveTime::from_hms_opt(23, 59, 0).unwrap();

    assert_eq!(
        format_local_time(time, ClockFormat::TwentyFourHour),
        "00:05"
    );
    assert_eq!(
        format_local_time(evening, ClockFormat::TwentyFourHour),
        "23:59"
    );
}

#[test]
fn formats_twelve_hour_time() {
    let midnight = NaiveTime::from_hms_opt(0, 5, 0).unwrap();
    let noon = NaiveTime::from_hms_opt(12, 30, 0).unwrap();
    let evening = NaiveTime::from_hms_opt(23, 59, 0).unwrap();

    assert_eq!(
        format_local_time(midnight, ClockFormat::TwelveHour),
        "12:05 AM"
    );
    assert_eq!(format_local_time(noon, ClockFormat::TwelveHour), "12:30 PM");
    assert_eq!(
        format_local_time(evening, ClockFormat::TwelveHour),
        "11:59 PM"
    );
}

#[test]
fn parses_open_meteo_local_timestamp() {
    let parsed = parse_local_datetime("2026-07-08T15:45").unwrap();
    assert_eq!(parsed.time(), NaiveTime::from_hms_opt(15, 45, 0).unwrap());
}

fn test_glyphs() -> Vec<(char, Vec<String>)> {
    let mut glyphs = Vec::new();
    for ch in "0123456789".chars() {
        glyphs.push((ch, vec![format!("{ch}{ch}"), format!("{ch}{ch}")]));
    }
    glyphs.retain(|(glyph, _)| *glyph != '1');
    glyphs.push(('1', vec!["1".to_string(), "1".to_string()]));
    glyphs.push((':', vec!["##".to_string(), "##".to_string()]));
    glyphs.push((' ', vec![" ".to_string(), " ".to_string()]));
    glyphs.push(('A', vec!["AA".to_string(), "AA".to_string()]));
    glyphs.push(('P', vec!["PP".to_string(), "PP".to_string()]));
    glyphs.push(('M', vec!["MM".to_string(), "MM".to_string()]));
    glyphs
}

fn font_from_glyphs(glyphs: &[(char, Vec<String>)]) -> Result<ClockFont, ClockFontError> {
    let borrowed = glyphs
        .iter()
        .map(|(glyph, lines)| (*glyph, lines.iter().map(String::as_str).collect::<Vec<_>>()))
        .collect::<Vec<_>>();
    let definition = borrowed
        .iter()
        .map(|(glyph, lines)| (*glyph, lines.as_slice()))
        .collect::<Vec<_>>();
    ClockFont::from_static(2, 1, 5, &definition)
}

fn test_font() -> ClockFont {
    font_from_glyphs(&test_glyphs()).expect("test font definition is valid")
}

#[test]
fn adapts_clock_font_definition_and_pads_glyph_rows() {
    let mut glyphs = test_glyphs();
    glyphs
        .iter_mut()
        .find(|(glyph, _)| *glyph == '0')
        .expect("zero glyph exists")
        .1 = vec!["0".to_string(), "00".to_string()];

    let font = font_from_glyphs(&glyphs).expect("definition adapts");
    let zero = font.glyphs.get(&'0').unwrap();
    assert_eq!(zero, &vec!["0 ".to_string(), "00".to_string()]);
}

#[test]
fn separator_anchored_layout_keeps_colon_fixed() {
    let font = test_font();

    fn separator_col(text: &str, font: &ClockFont) -> u16 {
        let lines = ascii_lines(text, font);
        let layout = separator_anchored_layout(text, &lines, font, 100, 30);
        let anchor = separator_anchor_offset(text, font).unwrap();
        layout.col + anchor.offset as u16
    }

    let expected = {
        let anchor = separator_anchor_offset("12:00", &font).unwrap();
        100_u16.saturating_sub(anchor.width as u16) / 2
    };

    assert_eq!(separator_col("12:00", &font), separator_col("12:11", &font));
    assert_eq!(separator_col("09:59", &font), separator_col("10:00", &font));
    assert_eq!(separator_col("12:00", &font), expected);
}

#[test]
fn separator_anchor_takes_priority_over_right_edge_fit() {
    let font = test_font();
    let text = "12:05 AM";
    let lines = ascii_lines(text, &font);
    let layout = separator_anchored_layout(text, &lines, &font, 70, 30);
    let anchor = separator_anchor_offset(text, &font).unwrap();

    assert_eq!(
        layout.col + anchor.offset as u16,
        70_u16.saturating_sub(anchor.width as u16) / 2
    );
}

#[test]
fn layout_clamps_when_content_exceeds_area() {
    let font = test_font();
    assert_eq!(
        center_above_start(120, clock_height(&font) as u16, 80, 4),
        ClockLayout { col: 0, row: 0 }
    );
}

#[test]
fn clock_font_definition_requires_required_glyphs() {
    let mut glyphs = test_glyphs();
    glyphs.retain(|(glyph, _)| *glyph != 'A');

    let err = font_from_glyphs(&glyphs).unwrap_err();
    assert_eq!(err, ClockFontError::MissingGlyph('A'));
}

#[test]
fn clock_font_definition_rejects_wrong_glyph_height() {
    let mut glyphs = test_glyphs();
    glyphs
        .iter_mut()
        .find(|(glyph, _)| *glyph == '0')
        .expect("zero glyph exists")
        .1 = vec!["only one row".to_string()];

    let err = font_from_glyphs(&glyphs).unwrap_err();
    assert_eq!(
        err,
        ClockFontError::GlyphHeight {
            glyph: '0',
            actual: 1,
            expected: 2,
        }
    );
}
