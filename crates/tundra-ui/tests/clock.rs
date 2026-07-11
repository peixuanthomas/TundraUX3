use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use tundra_ui::{
    ClockCreateDialogFocus, ClockCreateDialogViewModel, ClockEntryKind, ClockEntryViewModel,
    ClockPageMode, ClockViewModel, HomeDisplayMode, NotificationTone, RuntimeAsciiAssets,
    ShellChromeViewModel, ShellLayout, StatusViewModel, TundraTheme, clock_page_layout,
    compute_shell_layout, render_clock, render_clock_placeholder,
};

#[test]
fn wide_layout_exposes_analog_panel_rows_and_dialog_hit_areas() {
    let mut model = clock_model();
    model.create_dialog = Some(ClockCreateDialogViewModel::default());
    let main = Rect::new(3, 5, 100, 24);

    let layout = clock_page_layout(main, &model);

    assert_eq!(layout.mode, ClockPageMode::Analog);
    assert!(layout.analog.is_some());
    assert_eq!(layout.clock.x, main.x);
    assert_eq!(layout.panel.right(), main.right());
    assert_eq!(layout.entry_rows.len(), 3);
    assert_eq!(layout.entry_rows[0].id, 10);
    assert_eq!(layout.entry_rows[0].kind, ClockEntryKind::Alarm);
    assert_eq!(layout.entry_rows[2].id, 30);
    assert_eq!(layout.entry_rows[2].kind, ClockEntryKind::Countdown);

    let dialog = layout.create_dialog.expect("dialog layout");
    for control in [
        dialog.input,
        dialog.error,
        dialog.create_alarm,
        dialog.create_countdown,
    ] {
        assert!(rect_contains(dialog.dialog, control));
        assert!(control.width > 0);
        assert!(control.height > 0);
    }
    assert!(dialog.create_alarm.right() <= dialog.create_countdown.x);
}

#[test]
fn entry_window_slices_alarms_then_countdowns_and_preserves_ids() {
    let mut model = ClockViewModel::at("2026-07-10", "14:32:08", 14, 32, 8);
    model.alarms = (1..=10)
        .map(|id| ClockEntryViewModel::new(id, format!("Alarm {id}"), false))
        .collect();
    model.countdowns = (11..=20)
        .map(|id| ClockEntryViewModel::new(id, format!("Timer {id}"), false))
        .collect();
    model.entry_window_start = 7;

    let layout = clock_page_layout(Rect::new(0, 0, 80, 18), &model);
    let ids = layout
        .entry_rows
        .iter()
        .map(|row| row.id)
        .collect::<Vec<_>>();

    assert_eq!(layout.entry_capacity, 12);
    assert_eq!(layout.entry_window_start, 7);
    assert_eq!(ids, (8..=19).collect::<Vec<_>>());
    assert_eq!(layout.entry_rows[0].kind, ClockEntryKind::Alarm);
    assert_eq!(layout.entry_rows[3].kind, ClockEntryKind::Countdown);
    assert!(layout.alarms_heading.y < layout.countdowns_heading.y);
}

#[test]
fn wide_renderer_draws_ascii_hands_digital_time_and_grouped_entries() {
    let model = clock_model();
    let (terminal, main) = render(100, 30, &model, false);
    let output = terminal_output(&terminal);
    let layout = clock_page_layout(main, &model);

    assert!(output.contains("2026-07-10"));
    assert!(output.contains("14:32:08"));
    assert!(output.contains("Alarms & Timers"));
    assert!(output.contains("[ + New ]"));
    assert!(output.contains("ALARMS"));
    assert!(output.contains("COUNTDOWNS"));
    assert!(output.contains("[A] 07:30:00 Daily"));
    assert!(output.contains("[T] 00:04:12 left !"));

    let analog = layout.analog.expect("wide page should have a face");
    let face = region_text(&terminal, analog);
    assert!(face.contains('#'), "hour hand should use #");
    assert!(face.contains('*'), "minute hand should use *");
    assert!(face.contains('+'), "second hand should use +");
    assert!(face.contains('@'), "center should use @");
    assert!(face.is_ascii());

    let selected_row = layout
        .entry_rows
        .iter()
        .find(|row| row.id == 20)
        .expect("selected row should be visible");
    assert!(region_has_fg(
        &terminal,
        selected_row.area,
        TundraTheme::default_dark().accent,
    ));
}

#[test]
fn analog_clock_falls_back_to_small_numerals_below_either_large_face_threshold() {
    let model = clock_model();
    for (width, height) in [(99, 30), (100, 29), (80, 24)] {
        let (terminal, main) = render(width, height, &model, false);
        let layout = clock_page_layout(main, &model);
        let face = region_text(
            &terminal,
            layout.analog.expect("test size should retain analog mode"),
        );

        assert!(face.contains("12"), "{width}x{height} lost the small 12");
        assert!(face.is_ascii());
    }
}

#[test]
fn upward_pointing_hands_do_not_overwrite_the_large_twelve() {
    let assets = RuntimeAsciiAssets::load_default().expect("default ASCII assets");
    let expected_twelve = (0..assets.clock_font().height)
        .map(|row| {
            format!(
                "{}{}{}",
                assets.clock_font().glyphs[&'1'][row],
                " ".repeat(assets.clock_font().spacing),
                assets.clock_font().glyphs[&'2'][row]
            )
        })
        .collect::<Vec<_>>();
    let model = ClockViewModel::at("2026-07-10", "00:00:00", 0, 0, 0).with_ascii_assets(assets);
    let (terminal, main) = render(100, 30, &model, false);
    let layout = clock_page_layout(main, &model);
    let face = region_text(
        &terminal,
        layout.analog.expect("wide page should have a face"),
    );

    for line in expected_twelve {
        assert!(
            face.contains(&line),
            "large 12 row was overwritten: {line:?}"
        );
    }
    assert!(face.contains('@'));
}

#[test]
fn create_dialog_renders_placeholder_error_and_both_focusable_actions() {
    let mut model = clock_model();
    model.create_dialog = Some(ClockCreateDialogViewModel {
        input: String::new(),
        error: Some("Use hh mm ss".to_string()),
        focus: ClockCreateDialogFocus::CreateCountdown,
    });

    let (terminal, main) = render(100, 30, &model, false);
    let output = terminal_output(&terminal);
    let dialog = clock_page_layout(main, &model)
        .create_dialog
        .expect("dialog layout");

    assert!(output.contains("New Alarm or Countdown"));
    assert!(output.contains("Enter time (hh mm ss)"));
    assert!(output.contains("[ hh mm ss ]"));
    assert!(output.contains("Use hh mm ss"));
    assert!(output.contains("[ Create Alarm ]"));
    assert!(output.contains("[ Create Countdown ]"));
    assert!(region_has_fg(
        &terminal,
        dialog.create_countdown,
        TundraTheme::default_dark().accent,
    ));
}

#[test]
fn narrow_layout_keeps_digital_time_and_operable_panel_without_panicking() {
    let model = clock_model();
    let (terminal, main) = render(60, 24, &model, false);
    let layout = clock_page_layout(main, &model);
    let output = terminal_output(&terminal);

    assert_eq!(layout.mode, ClockPageMode::DigitalOnly);
    assert!(layout.analog.is_none());
    assert!(layout.digital.height > 0);
    assert!(layout.new_button.height > 0);
    assert!(output.contains("14:32:08"));
    assert!(output.contains("[ + New ]"));
    assert!(output.contains("[A] 07:30:00 Daily"));
}

#[test]
fn minimum_full_shell_keeps_an_operable_entry_and_compacts_dialog_controls() {
    let mut model = clock_model();
    model.create_dialog = Some(ClockCreateDialogViewModel {
        input: "01 02 03".to_string(),
        error: Some("Example error".to_string()),
        focus: ClockCreateDialogFocus::Input,
    });

    let (_terminal, main) = render(50, 12, &model, false);
    let layout = clock_page_layout(main, &model);
    let dialog = layout.create_dialog.expect("dialog layout");

    assert_eq!(layout.mode, ClockPageMode::DigitalOnly);
    assert!(layout.new_button.width > 0);
    assert!(layout.new_button.height > 0);
    assert_eq!(layout.entry_capacity, 1);
    assert_eq!(layout.entry_rows.first().map(|row| row.id), Some(10));
    assert!(dialog.input.bottom() <= dialog.error.y);
    assert!(dialog.error.bottom() <= dialog.create_alarm.y);
    assert_eq!(dialog.create_alarm.y, dialog.create_countdown.y);
}

#[test]
fn read_only_clock_hides_new_control_and_ignores_create_dialog_model() {
    let mut model = clock_model().with_read_only(true);
    model.create_dialog = Some(ClockCreateDialogViewModel::default());

    let (terminal, main) = render(80, 24, &model, false);
    let layout = clock_page_layout(main, &model);
    let output = terminal_output(&terminal);

    assert!(model.is_read_only());
    assert_eq!(layout.new_button.width, 0);
    assert_eq!(layout.new_button.height, 0);
    assert!(layout.create_dialog.is_none());
    assert!(!output.contains("[ + New ]"));
    assert!(!output.contains("New Alarm or Countdown"));
    assert!(output.contains("ALARMS"));
    assert!(output.contains("COUNTDOWNS"));
    assert!(layout.entry_capacity > 0);
}

#[test]
fn legacy_clock_renderer_forwards_to_the_new_renderer() {
    let model = clock_model();
    let (new_terminal, _) = render(80, 24, &model, false);
    let (legacy_terminal, _) = render(80, 24, &model, true);

    assert_eq!(
        terminal_output(&new_terminal),
        terminal_output(&legacy_terminal)
    );
}

fn clock_model() -> ClockViewModel {
    let mut model = ClockViewModel::at("2026-07-10", "14:32:08", 14, 32, 8)
        .with_ascii_assets(RuntimeAsciiAssets::load_default().expect("default ASCII assets"));
    model.alarms = vec![
        ClockEntryViewModel::new(10, "07:30:00 Daily", false),
        ClockEntryViewModel::new(20, "18:15:45 Daily", true),
    ];
    model.countdowns = vec![ClockEntryViewModel::new(30, "00:04:12 left", true)];
    model.selected_entry_id = Some(20);
    model
}

fn render(
    width: u16,
    height: u16,
    model: &ClockViewModel,
    legacy: bool,
) -> (Terminal<TestBackend>, Rect) {
    let chrome = chrome(width, height);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| {
            if legacy {
                render_clock_placeholder(
                    frame,
                    frame.area(),
                    &chrome,
                    model,
                    &TundraTheme::default_dark(),
                );
            } else {
                render_clock(
                    frame,
                    frame.area(),
                    &chrome,
                    model,
                    &TundraTheme::default_dark(),
                );
            }
        })
        .expect("render clock");
    let main = match compute_shell_layout(Rect::new(0, 0, width, height)) {
        ShellLayout::Full { main, .. } => main,
        ShellLayout::Compact(_) => panic!("test dimensions should produce a full shell"),
    };
    (terminal, main)
}

fn chrome(width: u16, height: u16) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::User,
        terminal_size: (width, height),
        screen_stack: vec!["Clock".to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn terminal_output(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn region_text(terminal: &Terminal<TestBackend>, area: Rect) -> String {
    let buffer = terminal.backend().buffer();
    (area.y..area.bottom())
        .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
        .filter_map(|position| buffer.cell(position))
        .map(|cell| cell.symbol())
        .collect()
}

fn region_has_fg(terminal: &Terminal<TestBackend>, area: Rect, fg: Color) -> bool {
    let buffer = terminal.backend().buffer();
    (area.y..area.bottom()).any(|y| {
        (area.x..area.right()).any(|x| {
            buffer
                .cell((x, y))
                .is_some_and(|cell| cell.fg == fg && cell.symbol() != " ")
        })
    })
}

fn rect_contains(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}
