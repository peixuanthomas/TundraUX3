use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Borders, Clear, Paragraph};

use super::layout::{ClockEntryKind, ClockPageLayout, clock_page_layout};
use super::model::{ClockCreateDialogFocus, ClockEntryViewModel, ClockViewModel};
use crate::screens::shell::{render_compact_home, render_status, render_top};
use crate::{ClockFontAsset, ShellChromeViewModel, ShellLayout, TundraTheme, compute_shell_layout};

const LARGE_CLOCK_NUMERAL_MIN_WIDTH: usize = 64;
const LARGE_CLOCK_NUMERAL_MIN_HEIGHT: usize = 21;
const LARGE_CLOCK_NUMERAL_CENTER_CLEARANCE: usize = 24;
const LARGE_CLOCK_NUMERAL_VERTICAL_CLEARANCE: usize = 5;
const CLOCK_OUTLINE_MIN_SAMPLES: usize = 720;
const CLOCK_OUTLINE_MAX_SAMPLE_STEP: f64 = 0.5;
pub fn render_clock_placeholder(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ClockViewModel,
    theme: &TundraTheme,
) {
    render_clock(frame, area, chrome, model, theme);
}

pub fn render_clock(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ClockViewModel,
    theme: &TundraTheme,
) {
    let main = match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => {
            render_compact_home(frame, compact, chrome, theme);
            return;
        }
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_status(frame, status, chrome, theme);
            main
        }
    };

    let layout = clock_page_layout(main, model);
    render_clock_face(frame, &layout, model, theme);
    render_clock_panel(frame, &layout, model, theme);
    if let (Some(dialog_model), Some(dialog_layout)) = (&model.create_dialog, layout.create_dialog)
    {
        render_clock_create_dialog(frame, dialog_layout, dialog_model, theme);
    }
}

fn render_clock_face(
    frame: &mut Frame<'_>,
    layout: &ClockPageLayout,
    model: &ClockViewModel,
    theme: &TundraTheme,
) {
    if layout.clock.width == 0 || layout.clock.height == 0 {
        return;
    }
    frame.render_widget(
        theme
            .block()
            .title("Clock")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.clock,
    );

    if let Some(analog) = layout
        .analog
        .filter(|area| area.width > 0 && area.height > 0)
    {
        frame.render_widget(
            Paragraph::new(ascii_clock_lines(analog, model))
                .alignment(Alignment::Left)
                .style(theme.body_style()),
            analog,
        );
    }

    if layout.digital.width == 0 || layout.digital.height == 0 {
        return;
    }
    let lines = if layout.digital.height == 1 || model.date.is_empty() {
        vec![Line::styled(
            model.digital_time.clone(),
            theme.title_style(),
        )]
    } else {
        vec![
            Line::from(model.date.clone()),
            Line::styled(model.digital_time.clone(), theme.title_style()),
        ]
    };
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(theme.body_style()),
        layout.digital,
    );
}

fn render_clock_panel(
    frame: &mut Frame<'_>,
    layout: &ClockPageLayout,
    model: &ClockViewModel,
    theme: &TundraTheme,
) {
    if layout.panel.width == 0 || layout.panel.height == 0 {
        return;
    }
    frame.render_widget(
        theme
            .block()
            .title("Alarms & Timers")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.panel,
    );

    render_clock_line(
        frame,
        layout.new_button,
        "[ + New ]".to_string(),
        if model.selected_entry_id.is_none() && model.create_dialog.is_none() {
            theme.title_style()
        } else {
            theme.body_style()
        },
        Alignment::Center,
    );
    render_clock_line(
        frame,
        layout.alarms_heading,
        if model.alarms.is_empty() {
            "ALARMS (none)".to_string()
        } else {
            "ALARMS".to_string()
        },
        theme.title_style(),
        Alignment::Left,
    );
    render_clock_line(
        frame,
        layout.countdowns_heading,
        if model.countdowns.is_empty() {
            "COUNTDOWNS (none)".to_string()
        } else {
            "COUNTDOWNS".to_string()
        },
        theme.title_style(),
        Alignment::Left,
    );

    for row in &layout.entry_rows {
        let Some(entry) = clock_entry(model, row.kind, row.id) else {
            continue;
        };
        let selected = model.selected_entry_id == Some(entry.id);
        let marker = if selected { "> " } else { "  " };
        let kind = match row.kind {
            ClockEntryKind::Alarm => "[A]",
            ClockEntryKind::Countdown => "[T]",
        };
        let strong = if entry.strong { " !" } else { "" };
        render_clock_line(
            frame,
            row.area,
            format!("{marker}{kind} {}{strong}", entry.label),
            if selected {
                theme.title_style()
            } else {
                theme.body_style()
            },
            Alignment::Left,
        );
    }
}

fn render_clock_create_dialog(
    frame: &mut Frame<'_>,
    layout: crate::ClockCreateDialogLayout,
    model: &crate::ClockCreateDialogViewModel,
    theme: &TundraTheme,
) {
    if layout.dialog.width == 0 || layout.dialog.height == 0 {
        return;
    }
    frame.render_widget(Clear, layout.dialog);
    frame.render_widget(
        theme
            .block()
            .title("New Alarm or Countdown")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.dialog,
    );

    let prompt = Rect::new(
        layout.dialog.x.saturating_add(1),
        layout.dialog.y.saturating_add(1),
        layout.dialog.width.saturating_sub(2),
        u16::from(layout.dialog.height > 2),
    );
    render_clock_line(
        frame,
        prompt,
        "Enter time (hh mm ss)".to_string(),
        theme.body_style(),
        Alignment::Left,
    );

    let input_is_empty = model.input.is_empty();
    render_clock_line(
        frame,
        layout.input,
        format!(
            "[ {} ]",
            if input_is_empty {
                "hh mm ss"
            } else {
                model.input.as_str()
            }
        ),
        if model.focus == ClockCreateDialogFocus::Input {
            theme.title_style()
        } else if input_is_empty {
            theme.muted_style()
        } else {
            theme.body_style()
        },
        Alignment::Left,
    );
    if let Some(error) = &model.error {
        render_clock_line(
            frame,
            layout.error,
            error.clone(),
            theme.error_style(),
            Alignment::Left,
        );
    }
    render_clock_line(
        frame,
        layout.create_alarm,
        "[ Create Alarm ]".to_string(),
        if model.focus == ClockCreateDialogFocus::CreateAlarm {
            theme.title_style()
        } else {
            theme.body_style()
        },
        Alignment::Center,
    );
    render_clock_line(
        frame,
        layout.create_countdown,
        "[ Create Countdown ]".to_string(),
        if model.focus == ClockCreateDialogFocus::CreateCountdown {
            theme.title_style()
        } else {
            theme.body_style()
        },
        Alignment::Center,
    );
}

pub(crate) fn render_clock_line(
    frame: &mut Frame<'_>,
    area: Rect,
    text: String,
    style: Style,
    alignment: Alignment,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::styled(text, style))
            .alignment(alignment)
            .style(style),
        area,
    );
}

fn clock_entry(
    model: &ClockViewModel,
    kind: ClockEntryKind,
    id: u64,
) -> Option<&ClockEntryViewModel> {
    let entries = match kind {
        ClockEntryKind::Alarm => &model.alarms,
        ClockEntryKind::Countdown => &model.countdowns,
    };
    entries.iter().find(|entry| entry.id == id)
}

fn ascii_clock_lines(area: Rect, model: &ClockViewModel) -> Vec<Line<'static>> {
    let width = usize::from(area.width);
    let height = usize::from(area.height);
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let mut cells = vec![vec![' '; width]; height];
    if width >= 7 && height >= 5 {
        let center_x = (width.saturating_sub(1) as f64) / 2.0;
        let center_y = (height.saturating_sub(1) as f64) / 2.0;
        let (large_numerals, radius_x, radius_y) = clock_face_geometry(width, height, model);

        let outline_samples = clock_outline_sample_count(radius_x, radius_y);
        for sample in 0..outline_samples {
            let angle = (sample as f64) * std::f64::consts::TAU / (outline_samples as f64);
            let x = (center_x + angle.sin() * radius_x).round() as isize;
            let y = (center_y - angle.cos() * radius_y).round() as isize;
            let outline = if angle.sin().abs() < 0.35 {
                '-'
            } else if angle.cos().abs() < 0.35 {
                '|'
            } else {
                '.'
            };
            put_clock_char(&mut cells, x, y, outline);
        }

        if large_numerals.is_none() {
            put_clock_text_centered(&mut cells, center_x, center_y - radius_y, "12");
            put_clock_text_centered(&mut cells, center_x + radius_x, center_y, "3");
            put_clock_text_centered(&mut cells, center_x, center_y + radius_y, "6");
            put_clock_text_centered(&mut cells, center_x - radius_x, center_y, "9");
        }

        draw_clock_hands(&mut cells, center_x, center_y, radius_x, radius_y, model);
        if let Some(numerals) = &large_numerals {
            draw_large_clock_numerals(&mut cells, numerals, center_x, center_y, radius_x, radius_y);
        }
        put_clock_char(
            &mut cells,
            center_x.round() as isize,
            center_y.round() as isize,
            '@',
        );
    }

    cells
        .into_iter()
        .map(|line| Line::from(line.into_iter().collect::<String>()))
        .collect()
}

#[derive(Debug)]
struct LargeClockNumerals {
    twelve: Vec<String>,
    three: Vec<String>,
    six: Vec<String>,
    nine: Vec<String>,
}

fn clock_face_geometry(
    width: usize,
    height: usize,
    model: &ClockViewModel,
) -> (Option<LargeClockNumerals>, f64, f64) {
    let cell_height_to_width = model.terminal_cell_aspect_ratio().height_to_width();
    if let Some(numerals) = large_clock_numerals(width, height, model)
        && let Some((radius_x, radius_y)) =
            clock_face_radii(width, height, Some(&numerals), cell_height_to_width)
    {
        return (Some(numerals), radius_x, radius_y);
    }

    let (radius_x, radius_y) = clock_face_radii(width, height, None, cell_height_to_width)
        .expect("a drawable clock canvas should have positive radii");
    (None, radius_x, radius_y)
}

fn large_clock_numerals(
    width: usize,
    height: usize,
    model: &ClockViewModel,
) -> Option<LargeClockNumerals> {
    if width < LARGE_CLOCK_NUMERAL_MIN_WIDTH || height < LARGE_CLOCK_NUMERAL_MIN_HEIGHT {
        return None;
    }
    let font = model.clock_font()?;
    let numerals = LargeClockNumerals {
        twelve: clock_font_lines("12", font)?,
        three: clock_font_lines("3", font)?,
        six: clock_font_lines("6", font)?,
        nine: clock_font_lines("9", font)?,
    };
    let side_width = clock_art_width(&numerals.nine)
        .saturating_add(clock_art_width(&numerals.three))
        .saturating_add(LARGE_CLOCK_NUMERAL_CENTER_CLEARANCE);
    let vertical_height = numerals
        .twelve
        .len()
        .saturating_add(numerals.six.len())
        .saturating_add(LARGE_CLOCK_NUMERAL_VERTICAL_CLEARANCE);
    let widest_centered = clock_art_width(&numerals.twelve).max(clock_art_width(&numerals.six));
    if width < side_width.max(widest_centered) || height < vertical_height {
        return None;
    }
    Some(numerals)
}

fn clock_face_radii(
    width: usize,
    height: usize,
    numerals: Option<&LargeClockNumerals>,
    cell_height_to_width: f64,
) -> Option<(f64, f64)> {
    let center_x = (width.saturating_sub(1) as f64) / 2.0;
    let center_y = (height.saturating_sub(1) as f64) / 2.0;
    let (max_radius_x, max_radius_y, min_radius_x) =
        numerals.map_or((center_x, center_y, 0.0), |numerals| {
            let side_width = clock_art_width(&numerals.three).max(clock_art_width(&numerals.nine));
            let centered_width =
                clock_art_width(&numerals.twelve).max(clock_art_width(&numerals.six));
            let vertical_height = numerals
                .twelve
                .len()
                .max(numerals.six.len())
                .max(numerals.three.len())
                .max(numerals.nine.len());
            let side_half_width = side_width.saturating_sub(1) as f64 / 2.0;
            let centered_half_width = centered_width.saturating_sub(1) as f64 / 2.0;
            let numeral_half_height = vertical_height.saturating_sub(1) as f64 / 2.0;
            (
                center_x - side_half_width,
                center_y - numeral_half_height,
                centered_half_width + side_half_width + 1.0,
            )
        });
    if max_radius_x <= 0.0 || max_radius_y <= 0.0 {
        return None;
    }

    // Horizontal and vertical radii must be solved together. In physical
    // pixels, radius_x * cell_width must equal radius_y * cell_height;
    // clamping only one axis turns tall or narrow terminal areas into ellipses.
    let radius_y = max_radius_y.min(max_radius_x / cell_height_to_width);
    let radius_x = radius_y * cell_height_to_width;
    (radius_y >= 1.0 && radius_x + f64::EPSILON >= min_radius_x).then_some((radius_x, radius_y))
}

fn clock_outline_sample_count(radius_x: f64, radius_y: f64) -> usize {
    ((std::f64::consts::TAU * radius_x.max(radius_y) / CLOCK_OUTLINE_MAX_SAMPLE_STEP).ceil()
        as usize)
        .max(CLOCK_OUTLINE_MIN_SAMPLES)
}

fn clock_font_lines(text: &str, font: &ClockFontAsset) -> Option<Vec<String>> {
    if font.height == 0 {
        return None;
    }
    let mut lines = vec![String::new(); font.height];
    let characters = text.chars().collect::<Vec<_>>();
    for (index, character) in characters.iter().copied().enumerate() {
        if index > 0 {
            let gap = if character == ':' || characters[index - 1] == ':' {
                font.separator_spacing
            } else {
                font.spacing
            };
            for line in &mut lines {
                line.push_str(&" ".repeat(gap));
            }
        }
        let glyph = font.glyphs.get(&character)?;
        if glyph.len() != font.height {
            return None;
        }
        for (line, glyph_line) in lines.iter_mut().zip(glyph) {
            line.push_str(glyph_line);
        }
    }
    Some(lines)
}

fn draw_large_clock_numerals(
    cells: &mut [Vec<char>],
    numerals: &LargeClockNumerals,
    center_x: f64,
    center_y: f64,
    radius_x: f64,
    radius_y: f64,
) {
    let height = cells.len();
    let width = cells.first().map(Vec::len).unwrap_or_default();
    let twelve = centered_clock_art_origin(
        center_x,
        center_y - radius_y,
        &numerals.twelve,
        width,
        height,
    );
    let three = centered_clock_art_origin(
        center_x + radius_x,
        center_y,
        &numerals.three,
        width,
        height,
    );
    let six =
        centered_clock_art_origin(center_x, center_y + radius_y, &numerals.six, width, height);
    let nine =
        centered_clock_art_origin(center_x - radius_x, center_y, &numerals.nine, width, height);

    put_clock_art(cells, twelve.0, twelve.1, &numerals.twelve);
    put_clock_art(cells, three.0, three.1, &numerals.three);
    put_clock_art(cells, six.0, six.1, &numerals.six);
    put_clock_art(cells, nine.0, nine.1, &numerals.nine);
}

fn centered_clock_art_origin(
    target_x: f64,
    target_y: f64,
    lines: &[String],
    width: usize,
    height: usize,
) -> (usize, usize) {
    let art_width = clock_art_width(lines);
    let art_height = lines.len();
    let half_width = art_width.saturating_sub(1) as f64 / 2.0;
    let half_height = art_height.saturating_sub(1) as f64 / 2.0;
    let x = (target_x - half_width)
        .round()
        .clamp(0.0, width.saturating_sub(art_width) as f64) as usize;
    let y = (target_y - half_height)
        .round()
        .clamp(0.0, height.saturating_sub(art_height) as f64) as usize;
    (x, y)
}

fn clock_art_width(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or_default()
}

fn put_clock_art(cells: &mut [Vec<char>], x: usize, y: usize, lines: &[String]) {
    for (row, line) in lines.iter().enumerate() {
        for (column, character) in line.chars().enumerate() {
            put_clock_char(
                cells,
                x.saturating_add(column) as isize,
                y.saturating_add(row) as isize,
                character,
            );
        }
    }
}

fn draw_clock_hands(
    cells: &mut [Vec<char>],
    center_x: f64,
    center_y: f64,
    radius_x: f64,
    radius_y: f64,
    model: &ClockViewModel,
) {
    let hour_angle = ((f64::from(model.hour % 12)
        + f64::from(model.minute) / 60.0
        + f64::from(model.second) / 3600.0)
        * 30.0)
        .to_radians();
    let minute_angle =
        ((f64::from(model.minute) + f64::from(model.second) / 60.0) * 6.0).to_radians();
    let second_angle = (f64::from(model.second) * 6.0).to_radians();

    // Draw longest to shortest so aligned hands keep visible radial bands:
    // hour (#) inside minute (*) inside second (+).
    draw_clock_hand(
        cells,
        center_x,
        center_y,
        radius_x * 0.78,
        radius_y * 0.78,
        second_angle,
        '+',
    );
    draw_clock_hand(
        cells,
        center_x,
        center_y,
        radius_x * 0.62,
        radius_y * 0.62,
        minute_angle,
        '*',
    );
    draw_clock_hand(
        cells,
        center_x,
        center_y,
        radius_x * 0.42,
        radius_y * 0.42,
        hour_angle,
        '#',
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_clock_hand(
    cells: &mut [Vec<char>],
    center_x: f64,
    center_y: f64,
    radius_x: f64,
    radius_y: f64,
    angle: f64,
    character: char,
) {
    let end_x = center_x + angle.sin() * radius_x;
    let end_y = center_y - angle.cos() * radius_y;
    let steps = ((end_x - center_x).abs().max((end_y - center_y).abs()) * 2.0)
        .ceil()
        .max(1.0) as usize;
    for step in 0..=steps {
        let progress = (step as f64) / (steps as f64);
        put_clock_char(
            cells,
            (center_x + (end_x - center_x) * progress).round() as isize,
            (center_y + (end_y - center_y) * progress).round() as isize,
            character,
        );
    }
}

fn put_clock_text(cells: &mut [Vec<char>], x: isize, y: isize, text: &str) {
    for (offset, character) in text.chars().enumerate() {
        put_clock_char(cells, x.saturating_add(offset as isize), y, character);
    }
}

fn put_clock_text_centered(cells: &mut [Vec<char>], x: f64, y: f64, text: &str) {
    let half_width = text.chars().count().saturating_sub(1) as f64 / 2.0;
    put_clock_text(
        cells,
        (x - half_width).round() as isize,
        y.round() as isize,
        text,
    );
}

fn put_clock_char(cells: &mut [Vec<char>], x: isize, y: isize, character: char) {
    let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
        return;
    };
    if let Some(cell) = cells.get_mut(y).and_then(|row| row.get_mut(x)) {
        *cell = character;
    }
}

#[cfg(test)]
mod analog_clock_tests {
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
                .with_ascii_assets(
                    RuntimeAsciiAssets::load_default().expect("default ASCII assets"),
                )
                .with_terminal_cell_aspect_ratio(
                    TerminalCellAspectRatio::new(cell_height_to_width)
                        .expect("test ratio is valid"),
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
                    let maximum_step =
                        std::f64::consts::TAU * radius_x.max(radius_y) / samples as f64;
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
}
