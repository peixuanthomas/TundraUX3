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
