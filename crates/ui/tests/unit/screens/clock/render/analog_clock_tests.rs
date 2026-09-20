use super::*;
use crate::{RuntimeAsciiAssets, TerminalCellAspectRatio};

#[test]
fn aligned_hands_keep_hour_minute_and_second_bands_visible() {
    let mut cells = vec![vec![' '; 101]; 41];
    let model = ClockViewModel::at("2026-07-10", "00:00:00", 0, 0, 0);

    draw_clock_hands(&mut cells, 50.0, 20.0, 40.0, 20.0, &model);

    assert_eq!(cells[15][50], '#', "inner hour-hand band disappeared");
    assert_eq!(cells[10][50], '*', "middle minute-hand band disappeared");
    assert_eq!(cells[5][50], '+', "outer second-hand band disappeared");
}

#[test]
fn large_numeral_centers_land_on_the_cardinal_rim_points() {
    let width = LARGE_CLOCK_NUMERAL_MIN_WIDTH;
    let height = LARGE_CLOCK_NUMERAL_MIN_HEIGHT;
    let model = ClockViewModel::at("2026-07-10", "14:32:08", 14, 32, 8)
        .with_ascii_assets(RuntimeAsciiAssets::load_default().expect("default ASCII assets"));
    let (large_numerals, radius_x, radius_y) = clock_face_geometry(width, height, &model);
    let numerals = large_numerals.as_ref().expect("large numerals");
    let center_x = (width.saturating_sub(1) as f64) / 2.0;
    let center_y = (height.saturating_sub(1) as f64) / 2.0;

    assert_art_center(
        centered_clock_art_origin(
            center_x,
            center_y - radius_y,
            &numerals.twelve,
            width,
            height,
        ),
        &numerals.twelve,
        (center_x, center_y - radius_y),
    );
    assert_art_center(
        centered_clock_art_origin(
            center_x + radius_x,
            center_y,
            &numerals.three,
            width,
            height,
        ),
        &numerals.three,
        (center_x + radius_x, center_y),
    );
    assert_art_center(
        centered_clock_art_origin(center_x, center_y + radius_y, &numerals.six, width, height),
        &numerals.six,
        (center_x, center_y + radius_y),
    );
    assert_art_center(
        centered_clock_art_origin(center_x - radius_x, center_y, &numerals.nine, width, height),
        &numerals.nine,
        (center_x - radius_x, center_y),
    );
}

#[test]
fn drawable_clock_sizes_preserve_a_circular_physical_radius() {
    let mut saw_large_numerals = false;
    let mut saw_small_numerals = false;

    for cell_height_to_width in [1.5, 2.0, 2.5, 3.0] {
        let model = ClockViewModel::default()
            .with_ascii_assets(RuntimeAsciiAssets::load_default().expect("default ASCII assets"))
            .with_terminal_cell_aspect_ratio(
                TerminalCellAspectRatio::new(cell_height_to_width).expect("test ratio is valid"),
            );

        for width in 7..=200 {
            for height in 5..=80 {
                let (numerals, radius_x, radius_y) = clock_face_geometry(width, height, &model);
                let center_x = (width.saturating_sub(1) as f64) / 2.0;
                let center_y = (height.saturating_sub(1) as f64) / 2.0;

                assert!(radius_x <= center_x + f64::EPSILON);
                assert!(radius_y <= center_y + f64::EPSILON);
                assert!(
                    (radius_x - radius_y * cell_height_to_width).abs() <= f64::EPSILON,
                    "{width}x{height} at {cell_height_to_width}:1 distorted the clock face"
                );
                let samples = clock_outline_sample_count(radius_x, radius_y);
                let maximum_step = std::f64::consts::TAU * radius_x.max(radius_y) / samples as f64;
                assert!(maximum_step <= CLOCK_OUTLINE_MAX_SAMPLE_STEP);

                if let Some(numerals) = numerals.as_ref() {
                    saw_large_numerals = true;
                    assert_art_center(
                        centered_clock_art_origin(
                            center_x,
                            center_y - radius_y,
                            &numerals.twelve,
                            width,
                            height,
                        ),
                        &numerals.twelve,
                        (center_x, center_y - radius_y),
                    );
                    assert_art_center(
                        centered_clock_art_origin(
                            center_x + radius_x,
                            center_y,
                            &numerals.three,
                            width,
                            height,
                        ),
                        &numerals.three,
                        (center_x + radius_x, center_y),
                    );
                    assert_art_center(
                        centered_clock_art_origin(
                            center_x,
                            center_y + radius_y,
                            &numerals.six,
                            width,
                            height,
                        ),
                        &numerals.six,
                        (center_x, center_y + radius_y),
                    );
                    assert_art_center(
                        centered_clock_art_origin(
                            center_x - radius_x,
                            center_y,
                            &numerals.nine,
                            width,
                            height,
                        ),
                        &numerals.nine,
                        (center_x - radius_x, center_y),
                    );
                } else {
                    saw_small_numerals = true;
                }
            }
        }
    }

    assert!(saw_large_numerals);
    assert!(saw_small_numerals);
}

#[test]
fn terminal_metrics_keep_the_reported_vscode_face_physically_round() {
    let aspect = TerminalCellAspectRatio::from_window_size(155, 59, 902, 679);
    let model = ClockViewModel::default()
        .with_ascii_assets(RuntimeAsciiAssets::load_default().expect("default ASCII assets"))
        .with_terminal_cell_aspect_ratio(aspect);

    let (numerals, radius_x, radius_y) = clock_face_geometry(118, 48, &model);

    assert!(numerals.is_some());
    assert!((aspect.height_to_width() - 1.97762).abs() < 0.00001);
    assert!((radius_y - 20.5).abs() <= f64::EPSILON);
    assert!((radius_x - radius_y * aspect.height_to_width()).abs() <= f64::EPSILON);
    assert!(radius_x > 40.0 && radius_x < 41.0);
}

#[test]
fn missing_terminal_pixel_metrics_use_the_two_to_one_fallback() {
    for dimensions in [(155, 59, 0, 0), (0, 59, 902, 679), (155, 0, 902, 679)] {
        let aspect = TerminalCellAspectRatio::from_window_size(
            dimensions.0,
            dimensions.1,
            dimensions.2,
            dimensions.3,
        );
        assert_eq!(aspect, TerminalCellAspectRatio::FALLBACK);
        assert_eq!(aspect.height_to_width(), 2.0);
    }
}

#[test]
fn outline_sampling_scales_beyond_legacy_wide_terminals() {
    let radius_x = 500.0;
    let radius_y = 250.0;
    let samples = clock_outline_sample_count(radius_x, radius_y);

    assert!(samples > CLOCK_OUTLINE_MIN_SAMPLES);
    assert!(std::f64::consts::TAU * radius_x / samples as f64 <= CLOCK_OUTLINE_MAX_SAMPLE_STEP);
}

fn assert_art_center(origin: (usize, usize), lines: &[String], target: (f64, f64)) {
    let actual_x = origin.0 as f64 + clock_art_width(lines).saturating_sub(1) as f64 / 2.0;
    let actual_y = origin.1 as f64 + lines.len().saturating_sub(1) as f64 / 2.0;

    assert!(
        (actual_x - target.0).abs() <= 0.5,
        "numeral x center {actual_x} missed rim point {}",
        target.0
    );
    assert!(
        (actual_y - target.1).abs() <= 0.5,
        "numeral y center {actual_y} missed rim point {}",
        target.1
    );
}
