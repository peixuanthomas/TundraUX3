use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::layout::{
    ClockEntryKind, ClockPageLayout, ExplorerLayout, ExplorerOverlayControl, ExplorerOverlayLayout,
    UserManagementColumnMode, UserManagementFormLayout, UserManagementLayout, clock_page_layout,
    explorer_layout, notification_action_text, user_management_layout, wrap_notification_text,
};
use crate::theme::solid_border_style;
use crate::{
    AuthField, BootstrapAdminViewModel, ClockCreateDialogFocus, ClockEntryViewModel,
    ClockFontAsset, ClockViewModel, ExitConfirmViewModel, ExplorerDialogViewModel,
    ExplorerEntryViewModel, ExplorerOverlayViewModel, ExplorerSearchViewModel, ExplorerSortColumn,
    ExplorerToolbarAction, ExplorerViewModel, HomeDisplayMode, HomeViewModel, LoginField,
    LoginViewModel, NOTIFICATION_TOO_SMALL_MESSAGE, NotificationLayout, NotificationLevel,
    NotificationTone, NotificationViewModel, RuntimeAsciiAssets, SetupField, SetupStep,
    SetupViewModel, ShellChromeViewModel, ShellLayout, TimeSyncDialogViewModel, TundraTheme,
    UserManagementFeedbackTone, UserManagementField, UserManagementFocus, UserManagementFormKind,
    UserManagementFormViewModel, UserManagementUserViewModel, UserManagementViewModel,
    compute_shell_layout, notification_layout,
    timezone_map::{TimezoneMapWidget, boundary_id_for_timezone},
};

pub const EXPLORER_HELP_LINE: &str = "Enter: open    Backspace: parent    N: folder    T: text file    R: rename    X/Delete: delete    C: copy    V: paste    /: search    H: hidden    Esc: back";
const SETUP_WIDE_MAP_MIN_WIDTH: u16 = 90;
const SETUP_WIDE_MAP_MIN_HEIGHT: u16 = 14;
const SETUP_CONTROLS_WIDTH: u16 = 48;
const SETUP_TIMEZONE_HEADER_HEIGHT: u16 = 5;
const SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT: u16 = 1;
const SETUP_TIMEZONE_BOTTOM_INDICATOR_HEIGHT: u16 = 1;
const SETUP_TIMEZONE_FOOTER_HEIGHT: u16 = 3;
const SETUP_LANGUAGE_LIST_LINE: u16 = 4;
const SETUP_ADMIN_HEADER_HEIGHT: u16 = 3;
const SETUP_ADMIN_FIELD_HEIGHT: u16 = 3;
const LOGIN_USER_LIST_WIDTH: u16 = 30;
const LOGIN_USERNAME_FIELD_HEIGHT: u16 = 5;
const LOGIN_PASSWORD_FIELD_HEIGHT: u16 = 3;
const LOGIN_FORM_GAP: u16 = 1;
const LOGIN_PASSWORD_VISIBILITY_WIDTH: u16 = 6;
const LOGIN_CONTROL_GAP: u16 = 1;
const SETUP_ADMIN_CHECKLIST_HEIGHT: u16 = 7;
const SETUP_ADMIN_SIDE_CHECKLIST_MIN_WIDTH: u16 = 68;
const SETUP_ADMIN_CHECKLIST_WIDTH: u16 = 32;
const SETUP_ADMIN_COLUMN_GAP: u16 = 2;
const SETUP_ADMIN_USERNAME_LINE: u16 = 3;
const SETUP_ADMIN_PASSWORD_LINE: u16 = 7;
const SETUP_ADMIN_CONFIRM_PASSWORD_LINE: u16 = 11;
const SETUP_ADMIN_HINT_LINE: u16 = 15;
const SETUP_ADMIN_SUBMIT_LINE: u16 = 19;
const SETUP_ADMIN_ERROR_LINE: u16 = 21;
const SETUP_ADMIN_STACKED_CHECKLIST_LINE: u16 = 21;
const HOME_SUMMARY_HEIGHT: u16 = 1;
const HOME_CONTROLS_HEIGHT: u16 = 2;
const HOME_TILE_MAX_HEIGHT: u16 = 8;
const HOME_TILE_MIN_HEIGHT: u16 = 3;
const HOME_TILE_GAP: u16 = 1;
const STATUS_TIME_BUTTON_HORIZONTAL_CHROME: u16 = 4;
const STATUS_TIME_BUTTON_MIN_WIDTH: u16 = 3;
const STATUS_TIME_BUTTON_RESERVED_LEFT_WIDTH: u16 = 12;
const COMPACT_TERMINAL_MESSAGE: &str = "TundraUX 3 needs at least 50x12 terminal cells.";
const LARGE_CLOCK_NUMERAL_MIN_WIDTH: usize = 64;
const LARGE_CLOCK_NUMERAL_MIN_HEIGHT: usize = 19;
const LARGE_CLOCK_NUMERAL_CENTER_CLEARANCE: usize = 24;
const LARGE_CLOCK_NUMERAL_VERTICAL_CLEARANCE: usize = 5;
const CLOCK_CELL_HEIGHT_TO_WIDTH_RATIO: f64 = 2.5;
const CLOCK_OUTLINE_MIN_SAMPLES: usize = 720;
const CLOCK_OUTLINE_MAX_SAMPLE_STEP: f64 = 0.5;

/// Shared Login page geometry for rendering and input hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginLayout {
    pub user_list: Rect,
    pub selected_username: Rect,
    pub password: Rect,
    pub password_visibility: Rect,
    pub help: Rect,
}

pub fn render_home(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    home: &HomeViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_main(frame, main, home, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

pub fn render_exit_confirmation(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExitConfirmViewModel,
    theme: &TundraTheme,
) {
    let dialog = centered_rect(area, area.width.min(46), area.height.min(7));
    let lines = vec![
        Line::from(model.message.clone()),
        Line::from(""),
        Line::from(format!("{}    {}", model.confirm_label, model.cancel_label)),
    ];
    let dialog_widget = Paragraph::new(lines)
        .block(
            theme
                .block()
                .title(model.title.as_str())
                .borders(Borders::ALL)
                .style(theme.body_style()),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, dialog);
    frame.render_widget(dialog_widget, dialog);
}

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

fn render_clock_line(
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
    if let Some(numerals) = large_clock_numerals(width, height, model)
        && let Some((radius_x, radius_y)) = clock_face_radii(width, height, Some(&numerals))
    {
        return (Some(numerals), radius_x, radius_y);
    }

    let (radius_x, radius_y) = clock_face_radii(width, height, None)
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

    // Horizontal and vertical radii must be solved together. Clamping only one
    // axis turns tall or narrow terminal areas into ellipses.
    let radius_y = max_radius_y.min(max_radius_x / CLOCK_CELL_HEIGHT_TO_WIDTH_RATIO);
    let radius_x = radius_y * CLOCK_CELL_HEIGHT_TO_WIDTH_RATIO;
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

pub fn render_time_sync_failure_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &TimeSyncDialogViewModel,
    theme: &TundraTheme,
) {
    let dialog = centered_rect(area, area.width.min(34), area.height.min(5));
    let dialog_widget = Paragraph::new(Line::from(model.message()))
        .block(
            theme
                .block()
                .title("Time Sync")
                .borders(Borders::ALL)
                .style(theme.error_style()),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, dialog);
    frame.render_widget(dialog_widget, dialog);
}

pub fn render_notification_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &NotificationViewModel,
    theme: &TundraTheme,
) {
    if model.level != NotificationLevel::Modal {
        return;
    }

    let layout = match notification_layout(area, model) {
        NotificationLayout::Dialog(layout) => layout,
        NotificationLayout::TooSmall { .. } => {
            render_notification_too_small(frame, area, theme);
            return;
        }
    };

    frame.render_widget(Clear, layout.dialog);
    let tone_style = notification_tone_style(model.tone, theme);
    frame.render_widget(
        theme
            .block()
            .title(format!(
                "{} {}",
                notification_tone_prefix(model.tone),
                model.title
            ))
            .title_style(tone_style)
            .borders(Borders::ALL)
            .border_style(solid_border_style(tone_style))
            .style(tone_style),
        layout.dialog,
    );

    let message_lines = wrap_notification_text(&model.message, layout.message.width)
        .into_iter()
        .map(Line::from)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(message_lines)
            .style(theme.body_style())
            .alignment(Alignment::Center),
        layout.message,
    );

    for action_layout in layout.actions {
        let Some(action) = model.actions.get(action_layout.index) else {
            continue;
        };
        let action_text = notification_action_text(action);
        let action_lines = wrap_notification_text(&action_text, action_layout.area.width)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        let style = if action.selected {
            theme.title_style()
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(action_lines)
                .style(style)
                .alignment(Alignment::Center),
            action_layout.area,
        );
    }
}

fn render_notification_too_small(frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
    frame.render_widget(Clear, area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lines = wrap_notification_text(NOTIFICATION_TOO_SMALL_MESSAGE, area.width)
        .into_iter()
        .map(Line::from)
        .collect::<Vec<_>>();
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let prompt = centered_rect(area, area.width, height);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme.error_style())
            .alignment(Alignment::Center),
        prompt,
    );
}

pub fn render_login(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_login_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

pub fn render_bootstrap_admin(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &BootstrapAdminViewModel,
    theme: &TundraTheme,
) {
    render_auth_screen(
        frame,
        area,
        chrome,
        "Create Admin",
        bootstrap_lines(model),
        theme,
    );
}

pub fn render_setup(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_setup_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

pub fn render_user_management(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_user_management_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_user_management_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    let layout = user_management_layout(main, model);
    frame.render_widget(
        theme
            .block()
            .title("User Management")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.panel,
    );

    render_clock_line(
        frame,
        layout.summary,
        format!(
            "Signed in: {}    {} {}",
            model.current_user,
            model.users.len(),
            if model.users.len() == 1 {
                "user"
            } else {
                "users"
            }
        ),
        theme.body_style(),
        Alignment::Left,
    );
    render_user_management_header(frame, &layout, theme);
    render_user_management_rows(frame, &layout, model, theme);
    render_user_management_feedback(frame, &layout, model, theme);
    render_user_management_actions(frame, &layout, model, theme);
    render_clock_line(
        frame,
        layout.help,
        "↑↓ Select · Tab Actions · Enter Activate · Esc Back".to_string(),
        theme.muted_style(),
        Alignment::Left,
    );

    if let (Some(form_layout), Some(form)) = (layout.form.as_ref(), model.form.as_ref()) {
        render_user_management_form(frame, form_layout, form, theme);
    }
}

fn render_user_management_header(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    theme: &TundraTheme,
) {
    let (username, display_name) = match layout.column_mode {
        UserManagementColumnMode::Detailed => ("USERNAME", "DISPLAY NAME"),
        UserManagementColumnMode::Account => ("ACCOUNT", ""),
    };
    let text = user_management_table_line(
        layout.header.width,
        layout.column_mode,
        " ",
        username,
        display_name,
        "ROLE",
        "STATUS",
    );
    render_clock_line(
        frame,
        layout.header,
        text,
        theme.title_style(),
        Alignment::Left,
    );
}

fn render_user_management_rows(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    if layout.rows.is_empty() && model.users.is_empty() {
        let empty = Rect::new(
            layout.rows_area.x,
            layout.rows_area.y,
            layout.rows_area.width,
            u16::from(layout.rows_area.height > 0),
        );
        render_clock_line(
            frame,
            empty,
            "  No users available".to_string(),
            theme.muted_style(),
            Alignment::Left,
        );
        return;
    }

    for row in &layout.rows {
        let Some(user) = model.users.get(row.index) else {
            continue;
        };
        let selected = row.index == model.selected_index;
        let marker = if selected { ">" } else { " " };
        let status = user_management_status(user);
        let text = user_management_table_line(
            row.area.width,
            layout.column_mode,
            marker,
            &user.username,
            &user.display_name,
            &user.role,
            &status,
        );
        render_clock_line(
            frame,
            row.area,
            text,
            if selected {
                theme.title_style()
            } else {
                theme.body_style()
            },
            Alignment::Left,
        );
    }
}

fn render_user_management_feedback(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    let (text, style) = if let Some(message) = &model.message {
        let style = match model.feedback_tone {
            UserManagementFeedbackTone::Info => theme.body_style(),
            UserManagementFeedbackTone::Success => theme.title_style(),
            UserManagementFeedbackTone::Error => theme.error_style(),
        };
        (message.clone(), style)
    } else if let UserManagementFocus::Action(focused) = model.focus {
        let Some(reason) = model
            .actions
            .iter()
            .find(|action| action.action == focused && !action.enabled)
            .and_then(|action| action.disabled_reason.clone())
        else {
            return;
        };
        (reason, theme.muted_style())
    } else {
        return;
    };
    render_clock_line(frame, layout.feedback, text, style, Alignment::Left);
}

fn render_user_management_actions(
    frame: &mut Frame<'_>,
    layout: &UserManagementLayout,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    for action_layout in &layout.actions {
        let Some(action) = model
            .actions
            .iter()
            .find(|action| action.action == action_layout.action)
        else {
            continue;
        };
        let focused = model.focus == UserManagementFocus::Action(action.action);
        let style = if !action.enabled {
            theme.muted_style()
        } else if focused {
            theme.title_style()
        } else if action.dangerous {
            theme.error_style()
        } else {
            theme.body_style()
        };
        render_clock_line(
            frame,
            action_layout.area,
            fit_cell(
                &action.button_label(),
                usize::from(action_layout.area.width),
            ),
            style,
            Alignment::Center,
        );
    }
}

fn render_user_management_form(
    frame: &mut Frame<'_>,
    layout: &UserManagementFormLayout,
    form: &UserManagementFormViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(Clear, layout.dialog);
    if !layout.compact {
        frame.render_widget(
            theme
                .block()
                .title(form.title.clone())
                .borders(Borders::ALL)
                .style(theme.body_style()),
            layout.dialog,
        );
    }

    let prompt = match (layout.compact, form.kind) {
        (true, UserManagementFormKind::Create) => "Create user — User or Admin account".to_string(),
        (true, _) => form.title.clone(),
        (false, UserManagementFormKind::Create) => "Create a User or Admin account.".to_string(),
        (false, UserManagementFormKind::EditInfo) => format!("Editing: {}", form.username),
        (false, UserManagementFormKind::Password) => {
            format!("Set a new password for {}.", form.username)
        }
    };
    render_clock_line(
        frame,
        layout.prompt,
        prompt,
        theme.body_style(),
        Alignment::Left,
    );

    for field in &layout.fields {
        let value = match field.field {
            UserManagementField::Username => format!("Username: {}", form.username),
            UserManagementField::DisplayName => {
                format!("Display name: {}", form.display_name)
            }
            UserManagementField::Role => format!("Role: {}  ◀/▶", form.role),
            UserManagementField::Password => {
                format!("Password: {}", "*".repeat(form.password_len))
            }
            UserManagementField::Submit | UserManagementField::Cancel => continue,
        };
        render_clock_line(
            frame,
            field.area,
            format!("[ {value} ]"),
            if form.focused_field == field.field {
                theme.title_style()
            } else {
                theme.body_style()
            },
            Alignment::Left,
        );
    }
    if let Some(error) = &form.error {
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
        layout.submit,
        format!("[ {} ]", form.submit_label()),
        if form.focused_field == UserManagementField::Submit {
            theme.title_style()
        } else {
            theme.body_style()
        },
        Alignment::Center,
    );
    render_clock_line(
        frame,
        layout.cancel,
        "[ Cancel ]".to_string(),
        if form.focused_field == UserManagementField::Cancel {
            theme.title_style()
        } else {
            theme.body_style()
        },
        Alignment::Center,
    );
}

fn user_management_status(user: &UserManagementUserViewModel) -> String {
    let mut status = if !user.enabled {
        "Disabled".to_string()
    } else if user.locked {
        "Locked".to_string()
    } else {
        "Enabled".to_string()
    };
    if user.is_current {
        status.push_str(" · You");
    }
    status
}

fn user_management_table_line(
    width: u16,
    mode: UserManagementColumnMode,
    marker: &str,
    username: &str,
    display_name: &str,
    role: &str,
    status: &str,
) -> String {
    let width = usize::from(width);
    if width == 0 {
        return String::new();
    }
    let marker_width = 2_usize.min(width);
    let available = width.saturating_sub(marker_width);
    match mode {
        UserManagementColumnMode::Detailed => {
            let separators = 3_usize.min(available);
            let cells = available.saturating_sub(separators);
            let role_width = 10.min(cells / 4);
            let status_width = 18.min(cells.saturating_sub(role_width) / 2);
            let names_width = cells
                .saturating_sub(role_width)
                .saturating_sub(status_width);
            let username_width = names_width / 2;
            let display_width = names_width.saturating_sub(username_width);
            format!(
                "{}{} {} {} {}",
                fit_cell(marker, marker_width),
                fit_cell(username, username_width),
                fit_cell(display_name, display_width),
                fit_cell(role, role_width),
                fit_cell(status, status_width),
            )
        }
        UserManagementColumnMode::Account => {
            let separators = 2_usize.min(available);
            let cells = available.saturating_sub(separators);
            let role_width = 9.min(cells / 3);
            let status_width = 16.min(cells.saturating_sub(role_width) / 2);
            let account_width = cells
                .saturating_sub(role_width)
                .saturating_sub(status_width);
            let account = if display_name.is_empty() || display_name == username {
                username.to_string()
            } else {
                format!("{username} — {display_name}")
            };
            format!(
                "{}{} {} {}",
                fit_cell(marker, marker_width),
                fit_cell(&account, account_width),
                fit_cell(role, role_width),
                fit_cell(status, status_width),
            )
        }
    }
}

fn fit_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut characters = text.chars();
    let mut fitted = characters.by_ref().take(width).collect::<String>();
    if characters.next().is_some() && width > 1 {
        fitted.pop();
        fitted.push('…');
    }
    let used = fitted.chars().count();
    fitted.extend(std::iter::repeat_n(' ', width.saturating_sub(used)));
    fitted
}

pub fn render_explorer(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_explorer_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
            render_explorer_overlay(frame, main, model, theme);
        }
    }
}

fn render_login_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    let outer = theme
        .block()
        .title("Login")
        .borders(Borders::ALL)
        .style(theme.body_style());
    frame.render_widget(outer, main);

    let layout = login_layout(main);

    render_login_user_list(frame, layout.user_list, model, theme);
    render_login_username_field(frame, layout.selected_username, model, theme);
    render_login_password_field(frame, layout.password, model, theme);
    render_login_button(
        frame,
        layout.password_visibility,
        if model.password_is_visible() {
            "[Hide]"
        } else {
            "[Show]"
        },
        model.focused_field == LoginField::PasswordVisibility,
        theme,
    );
    if layout.help.height > 0 {
        let mut lines = vec![
            Line::from("Users: Up/Down/Home/End    Tab: password/show"),
            Line::from("Enter: activate    F2: show/hide    Esc: exit"),
        ];
        if let Some(error) = &model.error {
            lines.push(Line::from(""));
            lines.push(Line::styled(error.clone(), theme.error_style()));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(theme.muted_style())
                .wrap(Wrap { trim: true }),
            layout.help,
        );
    }
}

fn render_login_user_list(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let visible_rows = area.height.saturating_sub(2) as usize;
    let (start, end) = login_user_window_bounds(model, visible_rows);
    let items: Vec<ListItem<'static>> = if model.users.is_empty() {
        vec![ListItem::new(Line::from("(no local users)"))]
    } else {
        model.users[start..end]
            .iter()
            .map(|user| {
                let mut suffix = String::new();
                if !user.enabled {
                    suffix.push_str(" disabled");
                }
                if user.locked {
                    suffix.push_str(" locked");
                }
                let label = if suffix.is_empty() {
                    format!("{} ({})", user.username, user.role)
                } else {
                    format!("{} ({}) |{}", user.username, user.role, suffix)
                };
                ListItem::new(Line::from(label))
            })
            .collect()
    };

    let mut state = ListState::default();
    if model.selected_index >= start && model.selected_index < end {
        state.select(Some(model.selected_index - start));
    }

    let block_style = if model.focused_field == LoginField::UserList {
        theme.title_style()
    } else {
        theme.body_style()
    };
    let list = List::new(items)
        .block(
            theme
                .block()
                .title("Users")
                .title_style(block_style)
                .borders(Borders::ALL)
                .border_style(solid_border_style(block_style))
                .style(block_style),
        )
        .highlight_symbol("> ")
        .highlight_style(theme.title_style());
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_login_username_field(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let selected = model.selected_user();
    let username = selected
        .map(|user| user.username.clone())
        .unwrap_or_else(|| "No user selected".to_string());
    let display = selected
        .map(|user| user.display_name.clone())
        .unwrap_or_else(|| "Choose a local account".to_string());
    let role = selected
        .map(|user| user.role.clone())
        .unwrap_or_else(|| "Unavailable".to_string());

    let lines = vec![
        Line::styled(username, theme.title_style()),
        Line::from(display),
        Line::styled(role, theme.muted_style()),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                theme
                    .block()
                    .title("Selected User")
                    .borders(Borders::ALL)
                    .style(theme.body_style()),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_login_password_field(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let block_style = if model.focused_field == LoginField::Password {
        theme.title_style()
    } else {
        theme.body_style()
    };
    let password = if let Some(visible) = model.visible_password() {
        visible.to_string()
    } else if model.password_len == 0 {
        "Enter password".to_string()
    } else {
        "*".repeat(model.password_len)
    };
    let password_style = if model.password_len == 0 && !model.password_is_visible() {
        theme.muted_style()
    } else {
        theme.body_style()
    };
    frame.render_widget(
        Paragraph::new(password)
            .style(password_style)
            .block(
                theme
                    .block()
                    .title("Password")
                    .title_style(block_style)
                    .borders(Borders::ALL)
                    .border_style(solid_border_style(block_style))
                    .style(block_style),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_login_button(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &'static str,
    selected: bool,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let line = Rect::new(
        area.x,
        area.y.saturating_add(area.height / 2),
        area.width,
        1,
    );
    let style = if selected {
        theme.title_style()
    } else {
        theme.body_style()
    };
    frame.render_widget(
        Paragraph::new(Line::styled(label, style))
            .style(style)
            .alignment(Alignment::Center),
        line,
    );
}

pub fn login_layout(main: Rect) -> LoginLayout {
    let (user_list, form) = login_columns(main);
    let reserved_password_height = LOGIN_PASSWORD_FIELD_HEIGHT.saturating_add(LOGIN_FORM_GAP);
    let height = if form.height > reserved_password_height {
        form.height
            .saturating_sub(reserved_password_height)
            .min(LOGIN_USERNAME_FIELD_HEIGHT)
    } else {
        0
    };
    let selected_username = Rect::new(form.x, form.y, form.width, height);
    let gap = if selected_username.height > 0 {
        LOGIN_FORM_GAP.min(form.height.saturating_sub(selected_username.height))
    } else {
        0
    };
    let password_y = selected_username
        .y
        .saturating_add(selected_username.height)
        .saturating_add(gap);
    let max_height = form
        .y
        .saturating_add(form.height)
        .saturating_sub(password_y);
    let control_height = max_height.min(LOGIN_PASSWORD_FIELD_HEIGHT);

    let (password_width, control_gap, visibility_width) = if form.width
        >= 3_u16
            .saturating_add(LOGIN_CONTROL_GAP)
            .saturating_add(LOGIN_PASSWORD_VISIBILITY_WIDTH)
    {
        (
            form.width
                .saturating_sub(LOGIN_CONTROL_GAP.saturating_add(LOGIN_PASSWORD_VISIBILITY_WIDTH)),
            LOGIN_CONTROL_GAP,
            LOGIN_PASSWORD_VISIBILITY_WIDTH,
        )
    } else {
        let password_width = form.width.div_ceil(2);
        (password_width, 0, form.width.saturating_sub(password_width))
    };
    let password = Rect::new(form.x, password_y, password_width, control_height);
    let visibility_x = password
        .x
        .saturating_add(password.width)
        .saturating_add(control_gap);
    let password_visibility = Rect::new(visibility_x, password_y, visibility_width, control_height);

    let help_y = password_y
        .saturating_add(control_height)
        .saturating_add(LOGIN_FORM_GAP);
    let help_height = form.y.saturating_add(form.height).saturating_sub(help_y);
    let help = Rect::new(form.x, help_y, form.width, help_height);

    LoginLayout {
        user_list,
        selected_username,
        password,
        password_visibility,
        help,
    }
}

pub fn login_user_list_area(main: Rect) -> Rect {
    login_layout(main).user_list
}

pub fn login_selected_username_area(main: Rect) -> Rect {
    login_layout(main).selected_username
}

pub fn login_password_area(main: Rect) -> Rect {
    login_layout(main).password
}

pub fn login_password_visibility_area(main: Rect) -> Rect {
    login_layout(main).password_visibility
}

pub fn login_user_list_visible_rows(main: Rect) -> usize {
    login_user_list_area(main).height.saturating_sub(2) as usize
}

fn login_columns(main: Rect) -> (Rect, Rect) {
    let inner = login_inner_area(main);
    if inner.width <= LOGIN_USER_LIST_WIDTH.saturating_add(10) {
        let [list, form] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(inner);
        return (list, form);
    }

    let [list, form] = Layout::horizontal([
        Constraint::Length(LOGIN_USER_LIST_WIDTH),
        Constraint::Min(30),
    ])
    .areas(inner);
    (list, form)
}

fn login_inner_area(main: Rect) -> Rect {
    Block::default().borders(Borders::ALL).inner(main)
}

fn login_user_window_bounds(model: &LoginViewModel, visible_rows: usize) -> (usize, usize) {
    if model.users.is_empty() || visible_rows == 0 {
        return (0, 0);
    }

    let selected = model.selected_index.min(model.users.len() - 1);
    let max_start = model.users.len().saturating_sub(visible_rows);
    let mut start = model.user_window_start.min(max_start);
    if selected < start {
        start = selected;
    } else if selected >= start.saturating_add(visible_rows) {
        start = selected.saturating_add(1).saturating_sub(visible_rows);
    }
    start = start.min(max_start);

    let end = start.saturating_add(visible_rows).min(model.users.len());
    (start, end)
}

fn render_auth_screen(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    title: &'static str,
    lines: Vec<Line<'static>>,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            let widget = Paragraph::new(lines)
                .block(
                    theme
                        .block()
                        .title(title)
                        .borders(Borders::ALL)
                        .style(theme.body_style()),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(widget, main);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_compact_home(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (notification, style) = status_presentation(&chrome.status, theme);
    if area.width <= 2 || area.height <= 2 {
        let notification = truncate_status_text(&notification, area.width);
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(Line::styled(notification, style)), area);
        return;
    }

    let block = theme
        .block()
        .title("TundraUX 3")
        .borders(Borders::ALL)
        .style(theme.body_style());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let notification = truncate_status_text(&notification, inner.width);
    frame.render_widget(
        Paragraph::new(Line::styled(notification, style)).alignment(Alignment::Center),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height > 1 {
        let size_message = truncate_status_text(COMPACT_TERMINAL_MESSAGE, inner.width);
        frame.render_widget(
            Paragraph::new(size_message)
                .style(theme.muted_style())
                .alignment(Alignment::Center),
            Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1),
        );
    }
}

fn render_setup_main(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    if model.step == SetupStep::Timezone
        && area.width >= SETUP_WIDE_MAP_MIN_WIDTH
        && area.height >= SETUP_WIDE_MAP_MIN_HEIGHT
    {
        let [controls, map] = Layout::horizontal([
            Constraint::Length(SETUP_CONTROLS_WIDTH),
            Constraint::Min(30),
        ])
        .areas(area);
        render_setup_controls(frame, area, controls, model, theme);
        render_setup_timezone_map(frame, map, model, theme);
    } else {
        render_setup_controls(frame, area, area, model, theme);
    }
}

fn render_setup_controls(
    frame: &mut Frame<'_>,
    main: Rect,
    controls: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    match model.step {
        SetupStep::Language => render_setup_language_page(frame, controls, model, theme),
        SetupStep::Timezone => render_setup_timezone_page(frame, main, controls, model, theme),
        SetupStep::Admin => render_setup_admin_page(frame, controls, model, theme),
    }
}

fn render_setup_timezone_map(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let selected_timezone = model.selected_timezone();
    let selected_timezone_id = selected_timezone.map(|timezone| timezone.id.as_str());
    let selected_boundary_id = selected_timezone_id.map(boundary_id_for_timezone);
    let mut widget = TimezoneMapWidget::themed(&[], theme)
        .selected_timezone_id(selected_timezone_id)
        .selected_boundary_id(selected_boundary_id);

    if let Some(timezone) = selected_timezone {
        widget = widget.city(timezone.longitude, timezone.latitude);
    }

    frame.render_widget(widget, area);
}

fn render_setup_language_page(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let controls = Paragraph::new(setup_language_lines(model, theme))
        .block(setup_block(theme))
        .wrap(Wrap { trim: true });

    frame.render_widget(controls, area);
}

fn render_setup_timezone_page(
    frame: &mut Frame<'_>,
    main: Rect,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(setup_block(theme), area);

    let content = setup_inner_area(area);
    let list_area = setup_timezone_list_area(main);
    let visible_rows = setup_timezone_visible_rows(main);
    let (start, end) = setup_timezone_window_bounds(model, visible_rows);

    let header = Rect::new(
        content.x,
        content.y,
        content.width,
        SETUP_TIMEZONE_HEADER_HEIGHT.min(content.height),
    );
    let top_indicator = Rect::new(
        content.x,
        list_area
            .y
            .saturating_sub(SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT),
        content.width,
        SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT.min(content.height),
    );
    let bottom_indicator = Rect::new(
        content.x,
        list_area.y.saturating_add(list_area.height),
        content.width,
        SETUP_TIMEZONE_BOTTOM_INDICATOR_HEIGHT.min(content.height),
    );
    let footer = Rect::new(
        content.x,
        content
            .y
            .saturating_add(content.height.saturating_sub(SETUP_TIMEZONE_FOOTER_HEIGHT)),
        content.width,
        SETUP_TIMEZONE_FOOTER_HEIGHT.min(content.height),
    );

    frame.render_widget(
        Paragraph::new(setup_timezone_header_lines(model, theme)),
        header,
    );
    frame.render_widget(
        Paragraph::new(setup_timezone_indicator_line(
            start > 0,
            "^ more timezones",
            theme,
        )),
        top_indicator,
    );
    frame.render_widget(
        Paragraph::new(setup_timezone_window_lines(model, start, end, theme)),
        list_area,
    );
    frame.render_widget(
        Paragraph::new(setup_timezone_indicator_line(
            end < model.timezones.len(),
            "v more timezones",
            theme,
        )),
        bottom_indicator,
    );
    frame.render_widget(
        Paragraph::new(setup_timezone_footer_lines(model, theme)).wrap(Wrap { trim: true }),
        footer,
    );
}

fn render_setup_admin_page(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(setup_block(theme), area);

    let content = setup_inner_area(area);
    let header = Rect::new(
        content.x,
        content.y,
        content.width,
        SETUP_ADMIN_HEADER_HEIGHT.min(content.height),
    );
    frame.render_widget(
        Paragraph::new(setup_admin_header_lines(model, theme)).wrap(Wrap { trim: true }),
        header,
    );

    render_setup_admin_field(
        frame,
        area,
        model,
        SetupField::AdminUsername,
        "Admin username",
        model.admin_username.clone(),
        "Enter admin username",
        theme,
    );
    render_setup_admin_field(
        frame,
        area,
        model,
        SetupField::AdminPassword,
        "Admin password",
        "*".repeat(model.admin_password_len),
        "Enter admin password",
        theme,
    );
    render_setup_admin_field(
        frame,
        area,
        model,
        SetupField::AdminPasswordConfirm,
        "Re-enter password",
        "*".repeat(model.admin_password_confirm_len),
        "Re-enter admin password",
        theme,
    );
    render_setup_admin_field(
        frame,
        area,
        model,
        SetupField::PasswordHint,
        "Password hint",
        model.password_hint.clone(),
        "Optional recovery hint, not the password",
        theme,
    );

    render_setup_password_checklist(frame, area, model, theme);

    frame.render_widget(
        Paragraph::new(setup_submit_line(model, theme)),
        setup_admin_field_area(area, SetupField::Submit),
    );

    if let Some(error) = &model.error {
        frame.render_widget(
            Paragraph::new(Line::styled(format!("Error: {error}"), theme.error_style()))
                .wrap(Wrap { trim: true }),
            setup_admin_error_area(area),
        );
    }
}

fn setup_block(theme: &TundraTheme) -> Block<'static> {
    theme
        .block()
        .title("First Run Setup")
        .title_style(theme.title_style())
        .borders(Borders::ALL)
        .style(theme.body_style())
}

fn setup_language_lines(model: &SetupViewModel, theme: &TundraTheme) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(
            format!("Step: {}", setup_step_label(model.step)),
            theme.title_style(),
        ),
        Line::from("Choose a language, then continue."),
        Line::styled(
            "Enter / Space: continue    Up / Down: choose    F1: help",
            theme.muted_style(),
        ),
        Line::from(""),
    ];

    if model.languages.is_empty() {
        lines.push(Line::styled(
            "  No languages available",
            theme.muted_style(),
        ));
    } else {
        for (index, language) in model.languages.iter().enumerate() {
            let text = format!(
                "{}{} ({})",
                selection_marker(index == model.selected_language_index),
                language.label,
                language.code
            );
            if index == model.selected_language_index {
                lines.push(Line::styled(text, theme.title_style()));
            } else {
                lines.push(Line::from(text));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::styled(
        selected_language_summary(model),
        theme.muted_style(),
    ));
    append_setup_error(&mut lines, model, theme);

    lines
}

fn setup_timezone_header_lines(model: &SetupViewModel, theme: &TundraTheme) -> Vec<Line<'static>> {
    vec![
        Line::styled(
            format!("Step: {}", setup_step_label(model.step)),
            theme.title_style(),
        ),
        Line::from("Choose a city or IANA zone, then continue."),
        Line::styled(
            "Enter: continue    Up / Down: choose    PgUp / PgDn: jump    F1: help",
            theme.muted_style(),
        ),
        Line::from(selected_timezone_id_summary(model)),
        Line::styled(
            selected_timezone_description_summary(model),
            theme.muted_style(),
        ),
    ]
}

fn setup_timezone_footer_lines(model: &SetupViewModel, theme: &TundraTheme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(error) = &model.error {
        lines.push(Line::styled(format!("Error: {error}"), theme.error_style()));
    }
    lines
}

fn setup_admin_header_lines(model: &SetupViewModel, theme: &TundraTheme) -> Vec<Line<'static>> {
    vec![
        Line::styled(
            format!("Step: {}", setup_step_label(model.step)),
            theme.title_style(),
        ),
        Line::from("Create the first administrator account."),
        Line::styled(
            "Tab / Down / Enter: next    Shift+Tab / Up: previous    Enter on submit: finish",
            theme.muted_style(),
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn render_setup_admin_field(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    field: SetupField,
    title: &'static str,
    value: String,
    placeholder: &'static str,
    theme: &TundraTheme,
) {
    let field_area = setup_admin_field_area(area, field);
    if field_area.width == 0 || field_area.height == 0 {
        return;
    }

    let focused = model.focused_field == field;
    let box_style = if focused {
        theme.title_style()
    } else {
        theme.body_style()
    };
    let block = theme
        .block()
        .title(title)
        .title_style(box_style)
        .borders(Borders::ALL)
        .border_style(solid_border_style(box_style))
        .style(box_style);
    let inner = block.inner(field_area);
    frame.render_widget(block, field_area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let is_placeholder = value.is_empty();
    let display = if is_placeholder {
        placeholder.to_string()
    } else {
        value
    };
    let text_style = if is_placeholder {
        theme.muted_style()
    } else {
        theme.body_style()
    };
    frame.render_widget(
        Paragraph::new(display).style(text_style),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
}

fn render_setup_password_checklist(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let checklist_area = setup_admin_checklist_area(area);
    if checklist_area.width == 0 || checklist_area.height == 0 {
        return;
    }

    let block = theme
        .block()
        .title("Password checklist")
        .title_style(theme.title_style())
        .borders(Borders::ALL)
        .style(theme.body_style());
    let inner = block.inner(checklist_area);
    frame.render_widget(block, checklist_area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(setup_password_checklist_lines(model, theme)).wrap(Wrap { trim: true }),
        inner,
    );
}

fn append_setup_error(lines: &mut Vec<Line<'static>>, model: &SetupViewModel, theme: &TundraTheme) {
    if let Some(error) = &model.error {
        lines.push(Line::from(""));
        lines.push(Line::styled(format!("Error: {error}"), theme.error_style()));
    }
}

pub fn setup_timezone_list_area(main: Rect) -> Rect {
    let controls = setup_timezone_controls_area(main);
    let content = setup_inner_area(controls);
    let reserved_height = SETUP_TIMEZONE_HEADER_HEIGHT
        .saturating_add(SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT)
        .saturating_add(SETUP_TIMEZONE_BOTTOM_INDICATOR_HEIGHT)
        .saturating_add(SETUP_TIMEZONE_FOOTER_HEIGHT);
    Rect::new(
        content.x,
        content
            .y
            .saturating_add(SETUP_TIMEZONE_HEADER_HEIGHT)
            .saturating_add(SETUP_TIMEZONE_TOP_INDICATOR_HEIGHT),
        content.width,
        content.height.saturating_sub(reserved_height),
    )
}

pub fn setup_timezone_visible_rows(main: Rect) -> usize {
    usize::from(setup_timezone_list_area(main).height)
}

pub fn setup_language_list_area(main: Rect, language_count: usize) -> Rect {
    setup_line_area(
        main,
        SETUP_LANGUAGE_LIST_LINE,
        setup_rendered_row_count(language_count),
    )
}

pub fn setup_admin_field_area(main: Rect, field: SetupField) -> Rect {
    let (line, height) = match field {
        SetupField::AdminUsername => (SETUP_ADMIN_USERNAME_LINE, SETUP_ADMIN_FIELD_HEIGHT),
        SetupField::AdminPassword => (SETUP_ADMIN_PASSWORD_LINE, SETUP_ADMIN_FIELD_HEIGHT),
        SetupField::AdminPasswordConfirm => {
            (SETUP_ADMIN_CONFIRM_PASSWORD_LINE, SETUP_ADMIN_FIELD_HEIGHT)
        }
        SetupField::PasswordHint => (SETUP_ADMIN_HINT_LINE, SETUP_ADMIN_FIELD_HEIGHT),
        SetupField::Submit => (SETUP_ADMIN_SUBMIT_LINE, 1),
        SetupField::LanguageList | SetupField::TimezoneList => {
            (SETUP_ADMIN_USERNAME_LINE, SETUP_ADMIN_FIELD_HEIGHT)
        }
    };
    setup_admin_line_area(main, line, height)
}

fn setup_admin_form_area(area: Rect) -> Rect {
    let content = setup_inner_area(area);
    if content.width < SETUP_ADMIN_SIDE_CHECKLIST_MIN_WIDTH {
        return content;
    }

    let reserved_checklist_width =
        SETUP_ADMIN_CHECKLIST_WIDTH.saturating_add(SETUP_ADMIN_COLUMN_GAP);
    Rect::new(
        content.x,
        content.y,
        content.width.saturating_sub(reserved_checklist_width),
        content.height,
    )
}

fn setup_admin_checklist_area(area: Rect) -> Rect {
    let content = setup_inner_area(area);
    if content.width >= SETUP_ADMIN_SIDE_CHECKLIST_MIN_WIDTH {
        let form = setup_admin_form_area(area);
        let x = form
            .x
            .saturating_add(form.width)
            .saturating_add(SETUP_ADMIN_COLUMN_GAP);
        let width = content.x.saturating_add(content.width).saturating_sub(x);
        let line = SETUP_ADMIN_PASSWORD_LINE;
        return Rect::new(
            x,
            content.y.saturating_add(line),
            width,
            SETUP_ADMIN_CHECKLIST_HEIGHT.min(content.height.saturating_sub(line)),
        );
    }

    setup_line_area(
        area,
        SETUP_ADMIN_STACKED_CHECKLIST_LINE,
        SETUP_ADMIN_CHECKLIST_HEIGHT,
    )
}

fn setup_admin_error_area(area: Rect) -> Rect {
    let content = setup_inner_area(area);
    if content.width >= SETUP_ADMIN_SIDE_CHECKLIST_MIN_WIDTH {
        return setup_admin_line_area(area, SETUP_ADMIN_ERROR_LINE, 2);
    }

    setup_line_area(
        area,
        SETUP_ADMIN_STACKED_CHECKLIST_LINE.saturating_add(SETUP_ADMIN_CHECKLIST_HEIGHT),
        2,
    )
}

fn setup_admin_line_area(area: Rect, line: u16, desired_height: u16) -> Rect {
    let form = setup_admin_form_area(area);
    if line >= form.height || desired_height == 0 {
        return Rect::new(form.x, form.y.saturating_add(form.height), form.width, 0);
    }

    Rect::new(
        form.x,
        form.y.saturating_add(line),
        form.width,
        desired_height.min(form.height.saturating_sub(line)),
    )
}

fn setup_timezone_controls_area(main: Rect) -> Rect {
    if main.width >= SETUP_WIDE_MAP_MIN_WIDTH && main.height >= SETUP_WIDE_MAP_MIN_HEIGHT {
        Layout::horizontal([
            Constraint::Length(SETUP_CONTROLS_WIDTH),
            Constraint::Min(30),
        ])
        .split(main)[0]
    } else {
        main
    }
}

fn setup_inner_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

fn setup_line_area(area: Rect, line: u16, desired_height: u16) -> Rect {
    let content = setup_inner_area(area);
    if line >= content.height || desired_height == 0 {
        return Rect::new(
            content.x,
            content.y.saturating_add(content.height),
            content.width,
            0,
        );
    }

    Rect::new(
        content.x,
        content.y.saturating_add(line),
        content.width,
        desired_height.min(content.height.saturating_sub(line)),
    )
}

fn setup_rendered_row_count(count: usize) -> u16 {
    u16::try_from(count.max(1)).unwrap_or(u16::MAX)
}

fn setup_timezone_window_bounds(model: &SetupViewModel, visible_rows: usize) -> (usize, usize) {
    if model.timezones.is_empty() || visible_rows == 0 {
        return (0, 0);
    }

    let selected = model.selected_timezone_index.min(model.timezones.len() - 1);
    let max_start = model.timezones.len().saturating_sub(visible_rows);
    let mut start = model.timezone_window_start.min(max_start);

    if selected < start {
        start = selected;
    } else if selected >= start.saturating_add(visible_rows) {
        start = selected.saturating_add(1).saturating_sub(visible_rows);
    }
    start = start.min(max_start);

    let end = start
        .saturating_add(visible_rows)
        .min(model.timezones.len());
    (start, end)
}

fn setup_timezone_indicator_line(
    visible: bool,
    text: &'static str,
    theme: &TundraTheme,
) -> Line<'static> {
    if visible {
        Line::styled(text, theme.muted_style())
    } else {
        Line::from("")
    }
}

fn setup_timezone_window_lines(
    model: &SetupViewModel,
    start: usize,
    end: usize,
    theme: &TundraTheme,
) -> Vec<Line<'static>> {
    if model.timezones.is_empty() {
        return vec![Line::styled(
            "  No timezones available",
            theme.muted_style(),
        )];
    }

    if start >= end {
        return Vec::new();
    }

    model.timezones[start..end]
        .iter()
        .enumerate()
        .map(|(offset, timezone)| {
            let index = start + offset;
            let text = format!(
                "{}{} ({})",
                selection_marker(index == model.selected_timezone_index),
                timezone.label,
                timezone.id
            );
            if index == model.selected_timezone_index {
                Line::styled(text, theme.title_style())
            } else {
                Line::from(text)
            }
        })
        .collect()
}

fn setup_submit_line(model: &SetupViewModel, theme: &TundraTheme) -> Line<'static> {
    let label = if model.can_submit {
        "Submit: ready"
    } else {
        "Submit: incomplete"
    };
    let text = format!(
        "{}{}",
        focus_marker(model.focused_field == SetupField::Submit),
        label
    );

    if model.focused_field == SetupField::Submit {
        Line::styled(text, theme.title_style())
    } else if model.can_submit {
        Line::from(text)
    } else {
        Line::styled(text, theme.muted_style())
    }
}

fn setup_password_checklist_lines(
    model: &SetupViewModel,
    theme: &TundraTheme,
) -> Vec<Line<'static>> {
    if model.password_requirements.is_empty() {
        return vec![Line::styled(
            "No password rules available",
            theme.muted_style(),
        )];
    }

    model
        .password_requirements
        .iter()
        .map(|requirement| {
            let marker = if requirement.met { "[x]" } else { "[ ]" };
            let style = if requirement.met {
                theme.title_style()
            } else {
                theme.muted_style()
            };
            Line::styled(format!("{marker} {}", requirement.label), style)
        })
        .collect()
}

fn selected_language_summary(model: &SetupViewModel) -> String {
    model
        .selected_language()
        .map(|language| format!("Selected language: {}", language.code))
        .unwrap_or_else(|| "Selected language: none".to_string())
}

fn selected_timezone_id_summary(model: &SetupViewModel) -> String {
    model
        .selected_timezone()
        .map(|timezone| format!("Selected timezone: {}", timezone.id))
        .unwrap_or_else(|| "Selected timezone: none".to_string())
}

fn selected_timezone_description_summary(model: &SetupViewModel) -> String {
    model
        .selected_timezone()
        .map(|timezone| format!("{} - {}", timezone.label, timezone.description))
        .unwrap_or_else(|| "No timezone selected".to_string())
}

fn setup_step_label(step: SetupStep) -> &'static str {
    match step {
        SetupStep::Language => "Language",
        SetupStep::Timezone => "Timezone",
        SetupStep::Admin => "Admin",
    }
}

fn selection_marker(selected: bool) -> &'static str {
    if selected { "> " } else { "  " }
}

fn render_explorer_main(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(
        theme
            .block()
            .title("Explorer")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        area,
    );

    let layout = explorer_layout(area, model);
    let Some(assets) = model.ascii_assets.as_ref() else {
        frame.render_widget(
            Paragraph::new("Explorer ASCII assets are unavailable")
                .style(theme.error_style())
                .alignment(Alignment::Center),
            layout.table,
        );
        return;
    };

    render_explorer_toolbar(frame, &layout, model, assets, theme);
    render_explorer_path_bar(frame, &layout, model, theme);
    render_explorer_sidebar(frame, &layout, model, assets, theme);
    render_explorer_table(frame, &layout, model, assets, theme);
    render_explorer_footer(frame, &layout, model, assets, theme);
}

fn render_explorer_toolbar(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    theme: &TundraTheme,
) {
    for button_layout in &layout.toolbar_buttons {
        let Some(button) = model
            .toolbar
            .buttons
            .iter()
            .find(|button| button.action == button_layout.action)
        else {
            continue;
        };
        let icon_key = if button.action == ExplorerToolbarAction::Sort {
            model.sort_direction.icon_key()
        } else {
            button.icon_key.as_str()
        };
        let icon = explorer_icon_line(assets, icon_key);
        let text = if button_layout.show_label {
            format!("[{icon}] {}", button.label)
        } else {
            format!("[{icon}]")
        };
        let style = if !button.enabled {
            theme.muted_style()
        } else if button.active {
            theme.title_style()
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(fit_cell(&text, usize::from(button_layout.area.width))).style(style),
            button_layout.area,
        );
    }
}

fn render_explorer_path_bar(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    let address_style = if model.address_editing {
        theme.title_style()
    } else {
        theme.body_style()
    };
    frame.render_widget(
        Paragraph::new(fit_cell("[Edit]", usize::from(layout.address_button.width)))
            .style(address_style),
        layout.address_button,
    );
    if model.address_editing || model.breadcrumbs.is_empty() {
        let text = if model.address_editing {
            format!("> {}_", model.address_value)
        } else {
            model.address_value.clone()
        };
        frame.render_widget(
            Paragraph::new(fit_cell(&text, usize::from(layout.address_input.width)))
                .style(address_style),
            layout.address_input,
        );
    }
    for crumb_layout in &layout.breadcrumbs {
        let Some(crumb) = model.breadcrumbs.get(crumb_layout.index) else {
            continue;
        };
        let suffix = if crumb_layout.index + 1 < model.breadcrumbs.len() {
            " > "
        } else {
            ""
        };
        let style = if crumb.drop_target {
            theme.title_style()
        } else if crumb.enabled {
            theme.body_style()
        } else {
            theme.muted_style()
        };
        frame.render_widget(
            Paragraph::new(fit_cell(
                &format!("{}{suffix}", crumb.label),
                usize::from(crumb_layout.area.width),
            ))
            .style(style),
            crumb_layout.area,
        );
    }

    let search_text = model
        .search
        .as_ref()
        .map_or_else(|| "Search: /".to_string(), explorer_search_line);
    frame.render_widget(
        Paragraph::new(fit_cell(&search_text, usize::from(layout.search.width))).style(
            if model.search.as_ref().is_some_and(|search| search.active) {
                theme.title_style()
            } else {
                theme.muted_style()
            },
        ),
        layout.search,
    );
}

fn render_explorer_sidebar(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    theme: &TundraTheme,
) {
    if let Some(header) = layout.sidebar_header {
        frame.render_widget(
            Paragraph::new("Quick access").style(theme.title_style()),
            header,
        );
    }
    for location_layout in &layout.quick_locations {
        let Some(location) = model.quick_locations.get(location_layout.index) else {
            continue;
        };
        let icon = explorer_icon_line(assets, &location.icon_key);
        let text = format!("{icon} {}", location.label);
        let style = if location.current || location.drop_target {
            theme.title_style()
        } else if location.enabled {
            theme.body_style()
        } else {
            theme.muted_style()
        };
        frame.render_widget(
            Paragraph::new(fit_cell(&text, usize::from(location_layout.area.width))).style(style),
            location_layout.area,
        );
    }
}

fn render_explorer_table(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    theme: &TundraTheme,
) {
    for column in &layout.columns {
        let mut label = column.column.label().to_string();
        if model.sort_column == column.column {
            label.push(' ');
            label.push_str(&explorer_icon_line(assets, model.sort_direction.icon_key()));
        }
        frame.render_widget(
            Paragraph::new(explorer_table_cell(
                &label,
                column.area.width,
                column.column
                    != *layout
                        .columns
                        .last()
                        .map(|column| &column.column)
                        .unwrap_or(&column.column),
            ))
            .style(theme.title_style()),
            column.area,
        );
    }

    if model.entries.is_empty() && layout.table_body.height > 0 {
        frame.render_widget(
            Paragraph::new(if model.is_trash {
                "(Trash is empty)"
            } else {
                "(empty directory)"
            })
                .style(theme.muted_style())
                .alignment(Alignment::Center),
            layout.table_body,
        );
    }

    for row in &layout.rows {
        let Some(entry) = model.entries.get(row.index) else {
            continue;
        };
        let presentation = model.entry_presentation(row.index);
        let icon_key = presentation
            .map(|presentation| presentation.icon_key.as_str())
            .unwrap_or_else(|| legacy_explorer_icon_key(entry));
        let icon = explorer_icon_line(assets, icon_key);
        let selected = presentation
            .map(|presentation| presentation.selected)
            .unwrap_or(entry.selected);
        let focused = presentation
            .map(|presentation| presentation.focused)
            .unwrap_or(model.selected_index == Some(row.index));
        let cut = presentation.is_some_and(|presentation| presentation.cut);
        let drop_target = presentation.is_some_and(|presentation| presentation.drop_target);
        let marker = if selected { "* " } else { "  " };
        let name = format!("{marker}{icon} {}", entry.name);
        let values = [
            (ExplorerSortColumn::Name, name),
            (ExplorerSortColumn::Type, entry.kind.clone()),
            (
                ExplorerSortColumn::Size,
                entry.size.clone().unwrap_or_else(|| "--".to_string()),
            ),
            (
                ExplorerSortColumn::Modified,
                entry.modified.clone().unwrap_or_else(|| "--".to_string()),
            ),
        ];
        let style = if cut {
            theme.muted_style()
        } else if focused || drop_target {
            theme.title_style()
        } else {
            theme.body_style()
        };
        for (column_index, column) in layout.columns.iter().enumerate() {
            let value = values
                .iter()
                .find_map(|(candidate, value)| (*candidate == column.column).then_some(value))
                .map(String::as_str)
                .unwrap_or("");
            let area = Rect::new(column.area.x, row.area.y, column.area.width, 1);
            frame.render_widget(
                Paragraph::new(explorer_table_cell(
                    value,
                    area.width,
                    column_index + 1 < layout.columns.len(),
                ))
                .style(style),
                area,
            );
        }
    }

    if let Some(scrollbar) = layout.scrollbar {
        let total = model.entries.len().max(1);
        let track = usize::from(scrollbar.height);
        let thumb_height =
            (track.saturating_mul(layout.visible_capacity) / total).clamp(1, track.max(1));
        let travel = track.saturating_sub(thumb_height);
        let max_start = total.saturating_sub(layout.visible_capacity).max(1);
        let thumb_start = travel.saturating_mul(layout.visible_start) / max_start;
        let lines = (0..track)
            .map(|index| {
                if (thumb_start..thumb_start.saturating_add(thumb_height)).contains(&index) {
                    Line::styled("#", theme.title_style())
                } else {
                    Line::styled("|", theme.muted_style())
                }
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), scrollbar);
    }
}

fn render_explorer_footer(
    frame: &mut Frame<'_>,
    layout: &ExplorerLayout,
    model: &ExplorerViewModel,
    assets: &RuntimeAsciiAssets,
    theme: &TundraTheme,
) {
    if layout.footer.height == 0 {
        return;
    }
    let selected_names = selected_entry_names(model);
    let selected_summary = if selected_names.is_empty() {
        format!("{} selected", model.effective_selected_count())
    } else {
        format!("Selected: {}", selected_names.join(", "))
    };
    let mut lines = vec![Line::from(selected_summary)];
    if let Some(entry) = model.selected_entry() {
        lines.push(Line::from(format!(
            "Name: {} | Type: {} | Size: {}",
            entry.name,
            entry.kind,
            entry.size.as_deref().unwrap_or("-")
        )));
        lines.push(Line::from(format!(
            "Modified: {} | Attributes: {}",
            entry.modified.as_deref().unwrap_or("-"),
            format_attributes(&entry.attributes)
        )));
    } else {
        lines.push(Line::from("No entry selected"));
        lines.push(Line::from(""));
    }

    let feedback = if let Some(error) = &model.error {
        Line::styled(format!("Error: {error}"), theme.error_style())
    } else if let Some(operation) = &model.operation {
        let progress = operation.percent().map_or_else(
            || format!("{}: {} items", operation.label, operation.completed_items),
            |percent| format!("{}: {percent}%", operation.label),
        );
        Line::styled(progress, theme.title_style())
    } else if let Some(message) = &model.message {
        Line::styled(message.clone(), theme.muted_style())
    } else if model.listing_warning_count > 0 {
        Line::styled(
            format!("{} metadata warning(s)", model.listing_warning_count),
            theme.muted_style(),
        )
    } else {
        Line::from("")
    };
    lines.push(feedback);
    lines.push(Line::styled(
        format!(
            "Enter: open | Backspace: parent | /: search | Hidden files: {}",
            if model.show_hidden { "shown" } else { "hidden" }
        ),
        theme.muted_style(),
    ));
    lines.truncate(usize::from(layout.footer.height));
    frame.render_widget(Paragraph::new(lines), layout.footer);

    if let (Some(cancel), Some(operation)) = (layout.cancel_operation, model.operation.as_ref()) {
        let icon = explorer_icon_line(assets, "cancel");
        frame.render_widget(
            Paragraph::new(fit_cell(
                &format!("[{icon}] {}", operation.cancel_label),
                usize::from(cancel.width),
            ))
            .style(theme.title_style()),
            cancel,
        );
    }
}

fn render_explorer_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    let layout = explorer_layout(area, model);
    let Some(overlay_layout) = layout.overlay.as_ref() else {
        return;
    };
    let title = match model.overlay.as_ref() {
        Some(ExplorerOverlayViewModel::ContextMenu(menu)) => menu.title.as_str(),
        Some(ExplorerOverlayViewModel::Name(dialog)) => dialog.title.as_str(),
        Some(ExplorerOverlayViewModel::Options(options)) => options.title.as_str(),
        Some(ExplorerOverlayViewModel::Conflict(conflict)) => conflict.title.as_str(),
        Some(ExplorerOverlayViewModel::Properties(properties)) => properties.title.as_str(),
        None => model
            .pending_dialog
            .as_ref()
            .map(|dialog| dialog.title.as_str())
            .unwrap_or("Explorer"),
    };
    frame.render_widget(Clear, overlay_layout.area);
    frame.render_widget(
        theme
            .block()
            .title(title)
            .borders(Borders::ALL)
            .style(theme.body_style()),
        overlay_layout.area,
    );

    match model.overlay.as_ref() {
        Some(ExplorerOverlayViewModel::ContextMenu(menu)) => {
            for control in &overlay_layout.controls {
                let ExplorerOverlayControl::ContextItem(index) = control.control else {
                    continue;
                };
                let Some(item) = menu.items.get(index) else {
                    continue;
                };
                let shortcut = item
                    .shortcut
                    .as_ref()
                    .map(|shortcut| format!("  {shortcut}"))
                    .unwrap_or_default();
                let marker = if menu.selected_index == Some(index) {
                    "> "
                } else {
                    "  "
                };
                let text = format!("{marker}{}{shortcut}", item.label);
                let style = if !item.enabled {
                    theme.muted_style()
                } else if item.dangerous {
                    theme.error_style()
                } else if menu.selected_index == Some(index) {
                    theme.title_style()
                } else {
                    theme.body_style()
                };
                frame.render_widget(
                    Paragraph::new(fit_cell(&text, usize::from(control.area.width))).style(style),
                    control.area,
                );
            }
        }
        Some(ExplorerOverlayViewModel::Name(dialog)) => {
            render_explorer_name_dialog(frame, overlay_layout, dialog, theme);
        }
        Some(ExplorerOverlayViewModel::Options(options)) => {
            for control in &overlay_layout.controls {
                match control.control {
                    ExplorerOverlayControl::Option(index) => {
                        let Some(option) = options.options.get(index) else {
                            continue;
                        };
                        let marker = if option.focused {
                            ">"
                        } else if option.selected {
                            "*"
                        } else {
                            " "
                        };
                        let text = format!("{marker} {}: {}", option.label, option.value);
                        let style = if !option.enabled {
                            theme.muted_style()
                        } else if option.focused {
                            theme.title_style()
                        } else {
                            theme.body_style()
                        };
                        frame.render_widget(Paragraph::new(text).style(style), control.area);
                    }
                    ExplorerOverlayControl::OptionsClose => frame.render_widget(
                        Paragraph::new(format!("[{}]", options.close_label))
                            .style(theme.title_style())
                            .alignment(Alignment::Center),
                        control.area,
                    ),
                    _ => {}
                }
            }
        }
        Some(ExplorerOverlayViewModel::Conflict(conflict)) => {
            render_explorer_conflict_dialog(frame, overlay_layout, conflict, theme);
        }
        Some(ExplorerOverlayViewModel::Properties(properties)) => {
            for (index, property) in properties
                .properties
                .iter()
                .take(usize::from(overlay_layout.content.height.saturating_sub(1)))
                .enumerate()
            {
                let area = Rect::new(
                    overlay_layout.content.x,
                    overlay_layout
                        .content
                        .y
                        .saturating_add(u16::try_from(index).unwrap_or(u16::MAX)),
                    overlay_layout.content.width,
                    1,
                );
                frame.render_widget(
                    Paragraph::new(format!("{}: {}", property.label, property.value)),
                    area,
                );
            }
            if let Some(control) = overlay_layout.controls.first() {
                frame.render_widget(
                    Paragraph::new(format!("[{}]", properties.close_label))
                        .style(theme.title_style())
                        .alignment(Alignment::Center),
                    control.area,
                );
            }
        }
        None => {
            if let Some(dialog) = &model.pending_dialog {
                render_legacy_explorer_dialog(frame, overlay_layout, dialog, theme);
            }
        }
    }
}

fn render_explorer_name_dialog(
    frame: &mut Frame<'_>,
    layout: &ExplorerOverlayLayout,
    dialog: &crate::ExplorerNameDialogViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(
        Paragraph::new(dialog.prompt.clone()),
        Rect::new(layout.content.x, layout.content.y, layout.content.width, 1),
    );
    for control in &layout.controls {
        match control.control {
            ExplorerOverlayControl::NameInput => frame.render_widget(
                Paragraph::new(format!("> {}_", dialog.value))
                    .block(theme.block().borders(Borders::ALL))
                    .style(theme.title_style()),
                Rect::new(
                    control.area.x,
                    control.area.y.saturating_sub(1),
                    control.area.width,
                    3.min(layout.content.height),
                ),
            ),
            ExplorerOverlayControl::Confirm => frame.render_widget(
                Paragraph::new(format!("[{}]", dialog.confirm_label))
                    .alignment(Alignment::Center)
                    .style(theme.title_style()),
                control.area,
            ),
            ExplorerOverlayControl::Cancel => frame.render_widget(
                Paragraph::new(format!("[{}]", dialog.cancel_label)).alignment(Alignment::Center),
                control.area,
            ),
            _ => {}
        }
    }
    if let Some(error) = &dialog.error {
        let error_area = Rect::new(
            layout.content.x,
            layout.content.y.saturating_add(4),
            layout.content.width,
            u16::from(layout.content.height > 4),
        );
        frame.render_widget(
            Paragraph::new(error.clone()).style(theme.error_style()),
            error_area,
        );
    }
}

fn render_explorer_conflict_dialog(
    frame: &mut Frame<'_>,
    layout: &ExplorerOverlayLayout,
    conflict: &crate::ExplorerConflictViewModel,
    theme: &TundraTheme,
) {
    let lines = vec![
        Line::from(format!("Source: {}", conflict.source)),
        Line::from(format!("Destination: {}", conflict.destination)),
        Line::styled(
            "An item with this name already exists.",
            theme.muted_style(),
        ),
    ];
    frame.render_widget(Paragraph::new(lines), layout.content);
    for control in &layout.controls {
        match control.control {
            ExplorerOverlayControl::ConflictChoice(choice) => {
                let selected = conflict.selected_choice == choice;
                frame.render_widget(
                    Paragraph::new(if selected {
                        format!("[{}]", choice.label())
                    } else {
                        choice.label().to_string()
                    })
                    .alignment(Alignment::Center)
                    .style(if selected {
                        theme.title_style()
                    } else {
                        theme.body_style()
                    }),
                    control.area,
                );
            }
            ExplorerOverlayControl::ApplyToRemaining => frame.render_widget(
                Paragraph::new(format!(
                    "[{}] Apply to remaining items",
                    if conflict.apply_to_remaining {
                        "x"
                    } else {
                        " "
                    }
                )),
                control.area,
            ),
            _ => {}
        }
    }
}

fn render_legacy_explorer_dialog(
    frame: &mut Frame<'_>,
    layout: &ExplorerOverlayLayout,
    dialog: &ExplorerDialogViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(
        Paragraph::new(dialog.message.clone())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        layout.content,
    );
    for control in &layout.controls {
        let label = match control.control {
            ExplorerOverlayControl::Confirm => Some(dialog.confirm_label.as_str()),
            ExplorerOverlayControl::Cancel => Some(dialog.cancel_label.as_str()),
            _ => None,
        };
        if let Some(label) = label {
            frame.render_widget(
                Paragraph::new(label)
                    .alignment(Alignment::Center)
                    .style(theme.title_style()),
                control.area,
            );
        }
    }
}

fn explorer_icon_line(assets: &RuntimeAsciiAssets, key: &str) -> String {
    assets
        .explorer_icon(key)
        .unwrap_or_else(|error| panic!("required Explorer icon {key} is unavailable: {error}"))
        .lines()
        .first()
        .cloned()
        .expect("validated Explorer icon must contain one line")
}

fn explorer_table_cell(text: &str, width: u16, separator: bool) -> String {
    let separator_width = if separator { 3 } else { 0 };
    let content_width = usize::from(width.saturating_sub(separator_width));
    let mut cell = fit_cell(text, content_width);
    if separator && width >= 3 {
        cell.push_str(" | ");
    } else {
        cell = fit_cell(&cell, usize::from(width));
    }
    cell
}

fn legacy_explorer_icon_key(entry: &ExplorerEntryViewModel) -> &'static str {
    let kind = entry.kind.to_ascii_lowercase();
    if kind.contains("directory") || kind.contains("folder") {
        return "folder";
    }
    if entry
        .attributes
        .iter()
        .any(|attribute| attribute.eq_ignore_ascii_case("link"))
    {
        return "link";
    }
    if kind.contains("executable") {
        return "executable";
    }
    let extension = entry
        .name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("txt" | "md" | "rst" | "log") => "text",
        Some(
            "rs" | "c" | "h" | "cpp" | "hpp" | "go" | "py" | "rb" | "js" | "ts" | "tsx" | "jsx"
            | "java" | "kt" | "swift" | "toml" | "yaml" | "yml" | "json" | "xml" | "html" | "css"
            | "sh" | "ps1",
        ) => "code",
        Some("pdf" | "doc" | "docx" | "odt" | "rtf") => "document",
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico") => "image",
        Some("mp3" | "wav" | "flac" | "m4a" | "ogg" | "aac") => "audio",
        Some("mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v") => "video",
        Some("zip" | "7z" | "rar" | "tar" | "gz" | "bz2" | "xz") => "archive",
        Some(
            "exe" | "com" | "scr" | "cpl" | "msi" | "msp" | "appx" | "bat" | "cmd" | "vbs" | "jar"
            | "app" | "pkg" | "run" | "appimage",
        ) => "executable",
        Some(_) => "file",
        None => "other",
    }
}

fn render_top(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    theme: &TundraTheme,
) {
    let stack = if chrome.screen_stack.is_empty() {
        "Home".to_string()
    } else {
        chrome.screen_stack.join(" > ")
    };
    let lines = vec![
        Line::styled(chrome.app_name.clone(), theme.title_style()),
        Line::styled(
            format!(
                "{} | {:?} | {}x{} | {}",
                chrome.build_mode,
                chrome.display_mode,
                chrome.terminal_size.0,
                chrome.terminal_size.1,
                stack
            ),
            theme.muted_style(),
        ),
    ];
    let top = Paragraph::new(lines).block(
        theme
            .block()
            .borders(Borders::ALL)
            .style(theme.body_style()),
    );

    frame.render_widget(top, area);
}

fn render_main(frame: &mut Frame<'_>, area: Rect, home: &HomeViewModel, theme: &TundraTheme) {
    match home.display_mode() {
        HomeDisplayMode::Debug => {
            if home.logout_visible() {
                render_authenticated_debug_main(frame, area, home, theme);
                return;
            }
            let main = Paragraph::new(debug_lines(home))
                .block(
                    theme
                        .block()
                        .title("Home")
                        .borders(Borders::ALL)
                        .style(theme.body_style()),
                )
                .wrap(Wrap { trim: true });

            frame.render_widget(main, area);
        }
        HomeDisplayMode::User | HomeDisplayMode::Auth => render_user_main(frame, area, home, theme),
    }
}

fn render_user_main(frame: &mut Frame<'_>, area: Rect, home: &HomeViewModel, theme: &TundraTheme) {
    let outer = theme
        .block()
        .title("Home")
        .borders(Borders::ALL)
        .style(theme.body_style());
    frame.render_widget(outer, area);

    let content = home_content_area(area);
    if content.width == 0 || content.height == 0 {
        return;
    }

    let summary = home_summary_area(area);
    let controls = home_controls_area(area);
    render_home_account_summary(frame, area, summary, home, theme);

    for (index, (entry, tile)) in home
        .entries()
        .iter()
        .zip(home_entry_tile_areas(area, home.entries().len()))
        .enumerate()
    {
        let selected = index == home.selected_entry_index();
        let style = if selected {
            theme.title_style()
        } else {
            theme.body_style()
        };
        let content_width = usize::from(tile.width.saturating_sub(2));
        let mut lines: Vec<Line<'static>> = Vec::new();
        if let Some(icon) = home.home_icon_for_label(&entry.label) {
            lines.extend(
                icon.lines
                    .iter()
                    .map(|line| centered_home_tile_line(line, icon.width(), content_width)),
            );
        }
        lines.push(Line::styled(
            centered_home_tile_text(&entry.label, content_width),
            style,
        ));
        lines.push(Line::from(centered_home_tile_text(
            &entry.description,
            content_width,
        )));

        let tile_widget = Paragraph::new(lines)
            .block(
                theme
                    .block()
                    .borders(Borders::ALL)
                    .border_style(solid_border_style(style))
                    .style(style)
                    .title(if selected { "Selected" } else { "" })
                    .title_style(style),
            )
            .style(style);

        frame.render_widget(tile_widget, tile);
    }

    let controls_text = if home.logout_visible() && home.entries().is_empty() {
        "Tab: focus Logout / Clock    L: logout    Q / Esc: exit"
    } else if home.logout_visible() {
        "Arrows: select    Enter: open    E: explorer    U: users    L: logout    Q / Esc: exit"
    } else {
        "Arrows: select    Enter: open    E: explorer    U: users    Q / Esc: exit"
    };
    frame.render_widget(
        Paragraph::new(Line::from(controls_text))
            .style(theme.muted_style())
            .wrap(Wrap { trim: true }),
        controls,
    );
}

fn render_authenticated_debug_main(
    frame: &mut Frame<'_>,
    area: Rect,
    home: &HomeViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(
        theme
            .block()
            .title("Home")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        area,
    );
    let summary = home_summary_area(area);
    render_home_account_summary(frame, area, summary, home, theme);

    let content = home_content_area(area);
    let controls = home_controls_area(area);
    let diagnostics_y = summary.y.saturating_add(summary.height);
    let diagnostics = Rect::new(
        content.x,
        diagnostics_y,
        content.width,
        controls.y.saturating_sub(diagnostics_y),
    );
    frame.render_widget(
        Paragraph::new(debug_lines(home))
            .style(theme.body_style())
            .wrap(Wrap { trim: true }),
        diagnostics,
    );
    frame.render_widget(
        Paragraph::new(Line::from(
            "Tab: focus Logout    L: logout    Q / Esc: exit",
        ))
        .style(theme.muted_style())
        .wrap(Wrap { trim: true }),
        controls,
    );
}

fn render_home_account_summary(
    frame: &mut Frame<'_>,
    main: Rect,
    summary: Rect,
    home: &HomeViewModel,
    theme: &TundraTheme,
) {
    if summary.width == 0 || summary.height == 0 {
        return;
    }
    let logout = home_logout_area(main, home);
    let user_width = if logout.width > 0 {
        logout.x.saturating_sub(summary.x).saturating_sub(2)
    } else {
        summary.width
    };
    let user = home.current_user.as_deref().unwrap_or("Unknown user");
    frame.render_widget(
        Paragraph::new(Line::from(format!("User: {user}")))
            .style(theme.body_style())
            .wrap(Wrap { trim: true }),
        Rect::new(summary.x, summary.y, user_width, summary.height),
    );
    if logout.width > 0 {
        let style = if home.logout_selected() {
            theme.title_style()
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(Line::styled("[Logout]", style)).style(style),
            logout,
        );
    }
}

fn centered_home_tile_line(
    line: &str,
    measured_width: usize,
    content_width: usize,
) -> Line<'static> {
    Line::from(centered_home_tile_value(
        line,
        measured_width,
        content_width,
    ))
}

fn centered_home_tile_text(text: &str, content_width: usize) -> String {
    centered_home_tile_value(text, text.chars().count(), content_width)
}

fn centered_home_tile_value(text: &str, measured_width: usize, content_width: usize) -> String {
    let padding = " ".repeat(content_width.saturating_sub(measured_width) / 2);
    format!("{padding}{text}")
}

fn render_status(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let time_button = chrome
        .status
        .time_button_label
        .as_ref()
        .map(|label| status_time_button_area(area, label))
        .filter(|area| area.width > 0 && area.height > 0);

    frame.render_widget(
        theme
            .block()
            .title("Status")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        area,
    );

    let inner = theme.block().borders(Borders::ALL).inner(area);
    let left_width = match time_button {
        Some(button) if button.x > inner.x => button.x.saturating_sub(inner.x).saturating_sub(1),
        Some(_) => 0,
        None => inner.width,
    };
    let left_area = Rect::new(inner.x, inner.y, left_width.min(inner.width), inner.height);
    if left_area.width > 0 && left_area.height > 0 {
        let (notification, style) = status_presentation(&chrome.status, theme);
        let notification = truncate_status_text(&notification, left_area.width);
        frame.render_widget(
            Paragraph::new(Line::styled(notification, style)).style(theme.body_style()),
            left_area,
        );
    }

    if let (Some(label), Some(button_area)) = (&chrome.status.time_button_label, time_button) {
        render_status_time_button(
            frame,
            button_area,
            label,
            chrome.status.time_button_selected,
            theme,
        );
    }
}

pub fn status_time_button_area(status: Rect, label: &str) -> Rect {
    if status.width == 0 || status.height == 0 || label.is_empty() {
        return Rect::new(
            status.x.saturating_add(status.width),
            status.y,
            0,
            status.height,
        );
    }

    let label_width = u16::try_from(label.chars().count()).unwrap_or(u16::MAX);
    let desired_width = label_width.saturating_add(STATUS_TIME_BUTTON_HORIZONTAL_CHROME);
    let max_width = if status.width
        > STATUS_TIME_BUTTON_RESERVED_LEFT_WIDTH.saturating_add(STATUS_TIME_BUTTON_MIN_WIDTH)
    {
        status
            .width
            .saturating_sub(STATUS_TIME_BUTTON_RESERVED_LEFT_WIDTH)
    } else {
        status.width
    };
    let min_width = STATUS_TIME_BUTTON_MIN_WIDTH.min(max_width);
    let width = desired_width
        .min(max_width)
        .max(min_width)
        .min(status.width);

    Rect::new(
        status.x.saturating_add(status.width.saturating_sub(width)),
        status.y,
        width,
        status.height,
    )
}

fn status_presentation(status: &crate::StatusViewModel, theme: &TundraTheme) -> (String, Style) {
    if let Some(alert) = &status.error {
        return (
            format!("{} {alert}", notification_tone_prefix(status.alert_tone)),
            notification_tone_style(status.alert_tone, theme),
        );
    }
    if let Some(toast) = &status.toast {
        return (toast.clone(), theme.muted_style());
    }
    (status.status.clone(), theme.body_style())
}

fn notification_tone_prefix(tone: NotificationTone) -> &'static str {
    match tone {
        NotificationTone::Info => "[INFO]",
        NotificationTone::Success => "[SUCCESS]",
        NotificationTone::Warning => "[WARN]",
        NotificationTone::Error => "[ERROR]",
        NotificationTone::Critical => "[CRITICAL]",
    }
}

fn truncate_status_text(text: &str, width: u16) -> String {
    let text = text
        .chars()
        .map(|character| match character {
            '\r' | '\n' => ' ',
            character => character,
        })
        .collect::<String>();
    let width = usize::from(width);
    let length = text.chars().count();
    if length <= width {
        return text;
    }
    if width <= 3 {
        return text.chars().take(width).collect();
    }

    let visible = text.chars().take(width - 3).collect::<String>();
    format!("{visible}...")
}

fn notification_tone_style(tone: NotificationTone, theme: &TundraTheme) -> ratatui::style::Style {
    match tone {
        NotificationTone::Info => theme.body_style(),
        NotificationTone::Success => theme.title_style(),
        NotificationTone::Warning => theme.title_style(),
        NotificationTone::Error | NotificationTone::Critical => theme.error_style(),
    }
}

fn render_status_time_button(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    selected: bool,
    theme: &TundraTheme,
) {
    let style = if selected {
        theme.title_style()
    } else {
        theme.body_style()
    };
    let button = Paragraph::new(label.to_string())
        .style(style)
        .block(
            theme
                .block()
                .borders(Borders::ALL)
                .border_style(solid_border_style(style))
                .style(style),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, area);
    frame.render_widget(button, area);
}

fn debug_lines(home: &HomeViewModel) -> Vec<Line<'static>> {
    let Some(diagnostics) = home.diagnostics() else {
        return vec![Line::from("Diagnostics unavailable")];
    };

    vec![
        Line::from(format!("Tick: {}", diagnostics.tick_count)),
        Line::from(format!(
            "Last key: {}",
            optional_text(&diagnostics.last_key_event)
        )),
        Line::from(format!(
            "Last mouse: {}",
            optional_text(&diagnostics.last_mouse_event)
        )),
        Line::from(format!(
            "Last resize: {}",
            optional_text(&diagnostics.last_resize_event)
        )),
        Line::from(format!(
            "Mouse: {}",
            diagnostics
                .mouse_coordinates
                .map(|(x, y)| format!("{x},{y}"))
                .unwrap_or_else(|| "none".to_string())
        )),
        Line::from(format!(
            "Scroll: {}",
            optional_text(&diagnostics.scroll_direction)
        )),
        Line::from(format!(
            "Drag: {}",
            optional_text(&diagnostics.drag_direction)
        )),
        Line::from(format!(
            "Flags: {}",
            if diagnostics.terminal_flags.is_empty() {
                "none".to_string()
            } else {
                diagnostics.terminal_flags.join(", ")
            }
        )),
        Line::from(format!(
            "Platform capabilities: {}",
            diagnostics.platform_capability_summary
        )),
    ]
}

fn bootstrap_lines(model: &BootstrapAdminViewModel) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Tab / Down: password    Enter on password: create admin    Esc: exit"),
        Line::from(""),
        Line::from(format!(
            "{}Admin username: {}",
            focus_marker(model.focused_field == AuthField::Username),
            model.username
        )),
        Line::from(format!(
            "{}Admin password: {}",
            focus_marker(model.focused_field == AuthField::Password),
            "*".repeat(model.password_len)
        )),
    ];
    if let Some(error) = &model.error {
        lines.push(Line::from(""));
        lines.push(Line::from(error.clone()));
    }
    lines
}

fn focus_marker(focused: bool) -> &'static str {
    if focused { "> " } else { "  " }
}

pub fn explorer_first_entry_content_line(model: &ExplorerViewModel, content_width: u16) -> usize {
    let width = usize::from(content_width.max(1));
    let mut line = 0usize;
    line += wrapped_line_count(&format!("Path: {}", model.current_path), width);
    line += wrapped_line_count(
        &format!(
            "Hidden files: {}",
            if model.show_hidden { "shown" } else { "hidden" }
        ),
        width,
    );
    if let Some(search) = &model.search {
        line += wrapped_line_count(&explorer_search_line(search), width);
    }
    line += wrapped_line_count(EXPLORER_HELP_LINE, width);
    line += 1;
    line += wrapped_line_count("Entries", width);
    line
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    text.chars().count().max(1).div_ceil(width.max(1))
}

fn explorer_search_line(search: &ExplorerSearchViewModel) -> String {
    let query = if search.query.is_empty() {
        "<empty>"
    } else {
        search.query.as_str()
    };
    let mode = if search.active { "active" } else { "inactive" };
    match search.match_count {
        Some(1) => format!("Search: {query} (1 match, {mode})"),
        Some(count) => format!("Search: {query} ({count} matches, {mode})"),
        None => format!("Search: {query} ({mode})"),
    }
}

fn selected_entry_names(model: &ExplorerViewModel) -> Vec<String> {
    model
        .entries
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            model
                .entry_presentation(*index)
                .map(|presentation| presentation.selected)
                .unwrap_or(entry.selected)
        })
        .map(|(_, entry)| entry.name.clone())
        .collect()
}

fn format_attributes(attributes: &[String]) -> String {
    if attributes.is_empty() {
        "none".to_string()
    } else {
        attributes.join(", ")
    }
}

pub fn home_entry_tile_areas(main: Rect, entry_count: usize) -> Vec<Rect> {
    if entry_count == 0 {
        return Vec::new();
    }

    let grid = home_entry_grid_area(main);
    if grid.width == 0 || grid.height == 0 {
        return Vec::new();
    }

    let columns = home_entry_column_count(grid.width, entry_count);
    let rows = entry_count.div_ceil(columns);
    let horizontal_gap = if columns > 1 { HOME_TILE_GAP } else { 0 };
    let vertical_gap = if rows > 1 { HOME_TILE_GAP } else { 0 };
    let total_horizontal_gap = horizontal_gap.saturating_mul(columns.saturating_sub(1) as u16);
    let total_vertical_gap = vertical_gap.saturating_mul(rows.saturating_sub(1) as u16);
    let tile_width = grid
        .width
        .saturating_sub(total_horizontal_gap)
        .checked_div(columns as u16)
        .unwrap_or(0);
    let available_height = grid.height.saturating_sub(total_vertical_gap);
    let tile_height = available_height
        .checked_div(rows as u16)
        .unwrap_or(0)
        .min(HOME_TILE_MAX_HEIGHT)
        .max(HOME_TILE_MIN_HEIGHT.min(grid.height));

    let mut areas = Vec::with_capacity(entry_count);
    for index in 0..entry_count {
        let row = index / columns;
        let column = index % columns;
        let x = grid.x.saturating_add(
            (column as u16).saturating_mul(tile_width.saturating_add(horizontal_gap)),
        );
        let y = grid
            .y
            .saturating_add((row as u16).saturating_mul(tile_height.saturating_add(vertical_gap)));
        if x >= grid.x.saturating_add(grid.width) || y >= grid.y.saturating_add(grid.height) {
            break;
        }
        let width = tile_width.min(grid.x.saturating_add(grid.width).saturating_sub(x));
        let height = tile_height.min(grid.y.saturating_add(grid.height).saturating_sub(y));
        if width > 0 && height > 0 {
            areas.push(Rect::new(x, y, width, height));
        }
    }

    areas
}

pub fn home_entry_index_at(
    main: Rect,
    entry_count: usize,
    coordinates: (u16, u16),
) -> Option<usize> {
    home_entry_tile_areas(main, entry_count)
        .into_iter()
        .enumerate()
        .find_map(|(index, area)| rect_contains(area, coordinates).then_some(index))
}

/// Returns the exact Logout control rectangle used by Home rendering.
///
/// Homes without an authenticated account expose a zero-sized area so input
/// routing cannot accidentally make Logout interactive.
pub fn home_logout_area(main: Rect, home: &HomeViewModel) -> Rect {
    let summary = home_summary_area(main);
    if !home.logout_visible() || summary.width == 0 || summary.height == 0 {
        return Rect::new(summary.x.saturating_add(summary.width), summary.y, 0, 0);
    }

    const LOGOUT_LABEL_WIDTH: u16 = 8;
    const ACCOUNT_LOGOUT_GAP: u16 = 2;
    let width = LOGOUT_LABEL_WIDTH.min(summary.width);
    let user_width = home
        .current_user
        .as_deref()
        .unwrap_or("Unknown user")
        .chars()
        .count()
        .saturating_add("User: ".len());
    let desired_offset = u16::try_from(user_width)
        .unwrap_or(u16::MAX)
        .saturating_add(ACCOUNT_LOGOUT_GAP);
    let max_offset = summary.width.saturating_sub(width);
    Rect::new(
        summary.x.saturating_add(desired_offset.min(max_offset)),
        summary.y,
        width,
        1,
    )
}

fn home_content_area(main: Rect) -> Rect {
    Rect::new(
        main.x.saturating_add(1),
        main.y.saturating_add(1),
        main.width.saturating_sub(2),
        main.height.saturating_sub(2),
    )
}

fn home_summary_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    Rect::new(
        content.x,
        content.y,
        content.width,
        HOME_SUMMARY_HEIGHT.min(content.height),
    )
}

fn home_controls_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    let height = HOME_CONTROLS_HEIGHT.min(content.height);
    Rect::new(
        content.x,
        content
            .y
            .saturating_add(content.height.saturating_sub(height)),
        content.width,
        height,
    )
}

fn home_entry_grid_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    let reserved = HOME_SUMMARY_HEIGHT.saturating_add(HOME_CONTROLS_HEIGHT);
    let y = content
        .y
        .saturating_add(HOME_SUMMARY_HEIGHT.min(content.height));
    let bottom = content.y.saturating_add(
        content
            .height
            .saturating_sub(HOME_CONTROLS_HEIGHT.min(content.height)),
    );
    Rect::new(
        content.x,
        y,
        content.width,
        bottom
            .saturating_sub(y)
            .min(content.height.saturating_sub(reserved.min(content.height))),
    )
}

fn home_entry_column_count(width: u16, entry_count: usize) -> usize {
    let max_columns = if width >= 96 {
        4
    } else if width >= 72 {
        3
    } else if width >= 48 {
        2
    } else {
        1
    };

    max_columns.min(entry_count.max(1))
}

fn rect_contains(rect: Rect, coordinates: (u16, u16)) -> bool {
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);

    coordinates.0 >= rect.x
        && coordinates.0 < right
        && coordinates.1 >= rect.y
        && coordinates.1 < bottom
}

fn optional_text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("none")
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

#[cfg(test)]
mod analog_clock_tests {
    use super::*;
    use crate::RuntimeAsciiAssets;

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
        let model = ClockViewModel::default()
            .with_ascii_assets(RuntimeAsciiAssets::load_default().expect("default ASCII assets"));
        let mut saw_large_numerals = false;
        let mut saw_small_numerals = false;

        for width in 7..=200 {
            for height in 5..=80 {
                let (numerals, radius_x, radius_y) = clock_face_geometry(width, height, &model);
                let center_x = (width.saturating_sub(1) as f64) / 2.0;
                let center_y = (height.saturating_sub(1) as f64) / 2.0;

                assert!(radius_x <= center_x + f64::EPSILON);
                assert!(radius_y <= center_y + f64::EPSILON);
                assert!(
                    (radius_x / radius_y - CLOCK_CELL_HEIGHT_TO_WIDTH_RATIO).abs() <= f64::EPSILON,
                    "{width}x{height} distorted the clock face"
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

        assert!(saw_large_numerals);
        assert!(saw_small_numerals);
    }

    #[test]
    fn outline_sampling_scales_beyond_legacy_wide_terminals() {
        let radius_x = 500.0;
        let radius_y = radius_x / CLOCK_CELL_HEIGHT_TO_WIDTH_RATIO;
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
