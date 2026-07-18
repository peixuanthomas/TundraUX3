use std::collections::BTreeSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use tundra_ui::{
    HomeDisplayMode, NotificationTone, SetupField, SetupPasswordRequirementViewModel, SetupStep,
    SetupTimezoneOption, SetupViewModel, ShellChromeViewModel, ShellLayout, StatusViewModel,
    TundraTheme, compute_shell_layout, render_setup, setup_admin_field_area,
    setup_language_options, setup_timezone_options,
};

const WIDE_SETUP_WIDTH: u16 = 120;
const WIDE_SETUP_HEIGHT: u16 = 34;
const SETUP_CONTROLS_WIDTH: u16 = 48;
const LEGACY_LONGITUDE_BAND_MIN_CELLS: usize = 6;

#[test]
fn setup_catalog_exposes_only_english_and_required_timezones() {
    let languages = setup_language_options();
    let language_labels = languages
        .iter()
        .map(|language| format!("{} ({})", language.label, language.code))
        .collect::<Vec<_>>()
        .join(" ");
    let timezones = setup_timezone_options();
    let timezone_ids: Vec<&str> = timezones
        .iter()
        .map(|timezone| timezone.id.as_str())
        .collect();

    assert_eq!(languages.len(), 1);
    assert!(language_labels.contains("English (en-US)"));
    assert!(!language_labels.contains("zh-Hans"));
    assert!(timezone_ids.contains(&"UTC"));
    assert!(timezone_ids.contains(&"America/Los_Angeles"));
    assert!(timezone_ids.contains(&"Pacific/Auckland"));
}

#[test]
fn setup_language_page_is_step_specific() {
    let model = sample_model(SetupStep::Language, None);
    let terminal = render_terminal(&model, 120, 34, TundraTheme::default_dark());
    let output = terminal_output(&terminal);

    assert!(output.contains("Step: Language"));
    assert!(output.contains("English (en-US)"));
    assert!(output.contains("Selected language: en-US"));
    assert!(output.contains("continue"));
    assert!(output.contains("help"));
    assert!(!output.contains("Timezone"));
    assert!(!output.contains("Timezone Map"));
    assert!(!output.contains("Admin username"));
    assert!(!output.contains("Admin password"));
    assert!(!output.contains("Shanghai - China Standard Time"));
    assert!(!output.contains("Tokyo - Japan Standard Time"));
}

#[test]
fn setup_timezone_page_is_step_specific() {
    let model = sample_model(SetupStep::Timezone, None);
    let terminal = render_terminal(&model, 120, 34, TundraTheme::default_dark());
    let output = terminal_output(&terminal);

    assert!(output.contains("Step: Timezone"));
    assert!(output.contains("Selected timezone: Asia/Tokyo"));
    assert!(output.contains("Tokyo - Japan Standard Time"));
    assert!(output.contains("Timezone Map"));
    assert!(!output.contains("Language"));
    assert!(!output.contains("English (en-US)"));
    assert!(!output.contains("Admin username"));
    assert!(!output.contains("Admin password"));
}

#[test]
fn setup_admin_page_is_step_specific_and_masks_password() {
    let model = sample_model(SetupStep::Admin, None);
    let terminal = render_terminal(&model, 120, 34, TundraTheme::default_dark());
    let output = terminal_output(&terminal);

    assert!(output.contains("Step: Admin"));
    assert!(output.contains("Create the first administrator account."));
    assert!(output.contains("Admin username"));
    assert!(output.contains("AdminUser"));
    assert!(output.contains("Admin password"));
    assert!(output.contains("Re-enter password"));
    assert!(output.contains("*************"));
    assert!(!output.contains("ActualPlaintext"));
    assert!(output.contains("Password hint"));
    assert!(output.contains("Stored in 1Password"));
    assert!(output.contains("Password checklist"));
    assert!(output.contains("[x] At least 10 characters"));
    assert!(output.contains("[x] Different from username"));
    assert!(output.contains("[x] Passwords match"));
    assert!(output.contains("Submit: ready"));
    assert!(!output.contains("Admin username:"));
    assert!(!output.contains("Admin password:"));
    assert!(!output.contains("Timezone Map"));
    assert!(!output.contains("Selected timezone"));
    assert!(!output.contains("Los Angeles"));
    assert!(!output.contains("Shanghai - China Standard Time"));
    assert!(!output.contains("Tokyo - Japan Standard Time"));
    assert!(!output.contains("English (en-US)"));
    assert!(!output.contains("简体中文"));
}

#[test]
fn setup_admin_page_draws_empty_field_placeholders() {
    let model = empty_admin_model();
    let terminal = render_terminal(&model, 120, 34, TundraTheme::default_dark());
    let output = terminal_output(&terminal);

    assert!(output.contains("Enter admin username"));
    assert!(output.contains("Enter admin password"));
    assert!(output.contains("Re-enter admin password"));
    assert!(output.contains("Optional recovery hint, not the password"));
    assert!(output.contains("[ ] At least 10 characters"));
    assert!(output.contains("[x] At most 256 characters"));
    assert!(output.contains("[ ] Not blank"));
    assert!(output.contains("[ ] Passwords match"));
    assert!(output.contains("Submit: incomplete"));
}

#[test]
fn setup_admin_page_highlights_focused_text_box() {
    let theme = TundraTheme::default_dark();
    let model = sample_model(SetupStep::Admin, None);
    let terminal = render_terminal(&model, 120, 34, theme);
    let main = setup_main_rect(120, 34);
    let password_area = setup_admin_field_area(main, SetupField::AdminPassword);

    assert!(
        region_has_fg(&terminal, password_area, theme.accent),
        "focused admin password box should use the accent style"
    );
}

#[test]
fn setup_renderer_shows_errors_with_error_style() {
    let model = sample_model(
        SetupStep::Timezone,
        Some("Timezone service unavailable".to_string()),
    );
    let terminal = render_terminal(&model, 120, 34, TundraTheme::default_dark());
    let output = terminal_output(&terminal);

    assert!(output.contains("Error: Timezone service unavailable"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == Color::Red && cell.symbol() != " ")
    );
}

#[test]
fn setup_renderer_uses_no_map_fallback_on_narrow_full_layout() {
    let model = sample_model(SetupStep::Timezone, None);
    let terminal = render_terminal(&model, 70, 24, TundraTheme::default_dark());
    let output = terminal_output(&terminal);

    assert!(output.contains("First Run Setup"));
    assert!(!output.contains("Timezone Map"));
}

#[test]
fn setup_renderer_compact_layout_does_not_panic() {
    let model = sample_model(SetupStep::Timezone, None);
    let terminal = render_terminal(&model, 48, 10, TundraTheme::default_dark());
    let output = terminal_output(&terminal);

    assert!(output.contains("needs at least 50x12"));
}

#[test]
fn setup_renderer_draws_timezone_map_layers() {
    let theme = map_test_theme();
    let model = sample_model_with_timezone(SetupStep::Timezone, "Asia/Shanghai", None);
    let terminal = render_terminal(&model, WIDE_SETUP_WIDTH, WIDE_SETUP_HEIGHT, theme);

    let gray_map_cells = map_cells_with_fg(&terminal, theme.muted);
    let selected_timezone_cells = map_cells_with_fg(&terminal, Color::White);
    let marker_cells = map_cells_with_fg(&terminal, theme.accent);

    assert!(
        !gray_map_cells.is_empty(),
        "map should draw gray unselected cells"
    );
    assert!(
        !selected_timezone_cells.is_empty(),
        "map should draw white selected timezone cells"
    );
    assert!(
        !marker_cells.is_empty(),
        "map should draw an accent city marker"
    );
}

#[test]
fn setup_renderer_draws_fine_world_map_art_not_full_blocks() {
    let theme = map_test_theme();
    let model = sample_model_with_timezone(SetupStep::Timezone, "Asia/Shanghai", None);
    let terminal = render_terminal(&model, WIDE_SETUP_WIDTH, WIDE_SETUP_HEIGHT, theme);
    let symbols = map_symbols(&terminal);
    let gray_map_cells = map_cells_with_fg(&terminal, theme.muted);
    let min_gray_x = gray_map_cells.iter().map(|(x, _)| *x).min().unwrap_or(0);
    let max_gray_x = gray_map_cells.iter().map(|(x, _)| *x).max().unwrap_or(0);

    assert!(
        symbols.iter().any(|symbol| is_braille_symbol(symbol)),
        "map should render fine-grained terminal art"
    );
    assert!(
        !symbols
            .iter()
            .any(|symbol| matches!(symbol.as_str(), "█" | "▀" | "▄")),
        "map should not use legacy full-block map cells"
    );
    assert!(
        max_gray_x.saturating_sub(min_gray_x) >= 40,
        "unselected world map should span the panel instead of only drawing the selected city"
    );
}

#[test]
fn setup_renderer_updates_selected_timezone_cells_between_shanghai_and_tokyo() {
    let theme = map_test_theme();
    let shanghai = sample_model_with_timezone(SetupStep::Timezone, "Asia/Shanghai", None);
    let tokyo = sample_model_with_timezone(SetupStep::Timezone, "Asia/Tokyo", None);
    let shanghai_terminal = render_terminal(&shanghai, WIDE_SETUP_WIDTH, WIDE_SETUP_HEIGHT, theme);
    let tokyo_terminal = render_terminal(&tokyo, WIDE_SETUP_WIDTH, WIDE_SETUP_HEIGHT, theme);

    let shanghai_selected_cells = map_cells_with_fg(&shanghai_terminal, Color::White)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let tokyo_selected_cells = map_cells_with_fg(&tokyo_terminal, Color::White)
        .into_iter()
        .collect::<BTreeSet<_>>();

    assert!(
        !shanghai_selected_cells.is_empty(),
        "Shanghai should highlight selected timezone map cells"
    );
    assert!(
        !tokyo_selected_cells.is_empty(),
        "Tokyo should highlight selected timezone map cells"
    );
    assert_ne!(
        shanghai_selected_cells, tokyo_selected_cells,
        "switching from Shanghai to Tokyo should move or change selected map cells"
    );
}

#[test]
fn setup_renderer_does_not_draw_legacy_cyan_longitude_band() {
    let model = sample_model_with_timezone(SetupStep::Timezone, "Asia/Tokyo", None);
    let terminal = render_terminal(
        &model,
        WIDE_SETUP_WIDTH,
        WIDE_SETUP_HEIGHT,
        TundraTheme::default_dark(),
    );
    let cyan_cells = map_cells_with_fg(&terminal, Color::Cyan);

    assert!(
        !has_vertical_band(&cyan_cells, LEGACY_LONGITUDE_BAND_MIN_CELLS),
        "legacy cyan longitude band should be absent"
    );
}

#[test]
fn setup_renderer_handles_utc_and_utc_alias_timezone_map_without_panic() {
    let utc = sample_model_with_timezone(SetupStep::Timezone, "UTC", None);
    let utc_terminal = render_terminal(
        &utc,
        WIDE_SETUP_WIDTH,
        WIDE_SETUP_HEIGHT,
        TundraTheme::default_dark(),
    );
    let utc_output = terminal_output(&utc_terminal);
    assert!(utc_output.contains("Selected timezone: UTC"));
    assert!(utc_output.contains("Timezone Map"));

    let alias = sample_model_with_timezone_option(
        SetupStep::Timezone,
        SetupTimezoneOption {
            id: "Etc/UTC".to_string(),
            label: "UTC".to_string(),
            description: "Coordinated Universal Time".to_string(),
            longitude: 0.0,
            latitude: 0.0,
        },
        None,
    );
    let alias_terminal = render_terminal(
        &alias,
        WIDE_SETUP_WIDTH,
        WIDE_SETUP_HEIGHT,
        TundraTheme::default_dark(),
    );
    let alias_output = terminal_output(&alias_terminal);
    assert!(alias_output.contains("Selected timezone: Etc/UTC"));
    assert!(alias_output.contains("Timezone Map"));
}

#[test]
fn setup_renderer_shows_timezone_scroll_indicators_when_window_is_partial() {
    let model = sample_model(SetupStep::Timezone, None);
    let terminal = render_terminal(&model, 70, 19, TundraTheme::default_dark());
    let output = terminal_output(&terminal);

    assert!(output.contains("^ more timezones"));
    assert!(output.contains("v more timezones"));
}

#[test]
fn compact_setup_shows_highest_priority_notification() {
    let model = sample_model(SetupStep::Language, None);
    let mut chrome = chrome_for("Setup", 49, 11);
    chrome.status = StatusViewModel {
        status: "Compact status".to_string(),
        toast: Some("Compact toast".to_string()),
        error: Some("Setup alert".to_string()),
        alert_tone: NotificationTone::Warning,
        time_button_label: None,
        time_button_selected: false,
    };
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_setup(
                frame,
                frame.area(),
                &chrome,
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render compact setup");

    let output = terminal_output(&terminal);
    assert!(output.contains("[WARN] Setup alert"));
    assert!(!output.contains("Compact toast"));
    assert!(!output.contains("Compact status"));
}

fn sample_model(step: SetupStep, error: Option<String>) -> SetupViewModel {
    sample_model_with_timezone(step, "Asia/Tokyo", error)
}

fn empty_admin_model() -> SetupViewModel {
    let mut model = sample_model(SetupStep::Admin, None);
    model.admin_username.clear();
    model.admin_password_len = 0;
    model.admin_password_confirm_len = 0;
    model.password_requirements = sample_password_requirements(false);
    model.password_hint.clear();
    model.focused_field = SetupField::AdminUsername;
    model.can_submit = false;
    model
}

fn sample_password_requirements(valid: bool) -> Vec<SetupPasswordRequirementViewModel> {
    vec![
        SetupPasswordRequirementViewModel::new("At least 10 characters", valid),
        SetupPasswordRequirementViewModel::new("At most 256 characters", true),
        SetupPasswordRequirementViewModel::new("Not blank", valid),
        SetupPasswordRequirementViewModel::new("Different from username", valid),
        SetupPasswordRequirementViewModel::new("Passwords match", valid),
    ]
}

fn sample_model_with_timezone(
    step: SetupStep,
    timezone_id: &str,
    error: Option<String>,
) -> SetupViewModel {
    let languages = setup_language_options();
    let timezones = setup_timezone_options();
    let selected_timezone_index = timezones
        .iter()
        .position(|timezone| timezone.id == timezone_id)
        .unwrap_or_else(|| panic!("{timezone_id} in setup catalog"));

    SetupViewModel {
        step,
        languages,
        timezones,
        selected_language_index: 0,
        selected_timezone_index,
        timezone_window_start: selected_timezone_index.saturating_sub(2),
        admin_username: "AdminUser".to_string(),
        admin_password_len: 13,
        admin_password_confirm_len: 13,
        password_requirements: sample_password_requirements(true),
        password_hint: "Stored in 1Password".to_string(),
        focused_field: SetupField::AdminPassword,
        can_submit: true,
        error,
    }
}

fn sample_model_with_timezone_option(
    step: SetupStep,
    timezone: SetupTimezoneOption,
    error: Option<String>,
) -> SetupViewModel {
    let mut model = sample_model(step, error);
    let selected_timezone_index = model
        .timezones
        .iter()
        .position(|candidate| candidate.id == timezone.id)
        .unwrap_or(model.timezones.len());

    if selected_timezone_index == model.timezones.len() {
        model.timezones.push(timezone);
    } else {
        model.timezones[selected_timezone_index] = timezone;
    }

    model.selected_timezone_index = selected_timezone_index;
    model.timezone_window_start = selected_timezone_index.saturating_sub(2);
    model
}

fn chrome_for(screen: &str, width: u16, height: u16) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (width, height),
        screen_stack: vec![screen.to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: tundra_ui::NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn render_terminal(
    model: &SetupViewModel,
    width: u16,
    height: u16,
    theme: TundraTheme,
) -> Terminal<TestBackend> {
    let chrome = chrome_for("Setup", width, height);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_setup(frame, frame.area(), &chrome, model, &theme);
        })
        .expect("render setup");
    terminal
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

fn setup_main_rect(width: u16, height: u16) -> Rect {
    match compute_shell_layout(Rect::new(0, 0, width, height)) {
        ShellLayout::Full { main, .. } => main,
        ShellLayout::Compact(_) => panic!("setup render tests expect a full shell layout"),
    }
}

fn region_has_fg(terminal: &Terminal<TestBackend>, area: Rect, fg: Color) -> bool {
    let buffer = terminal.backend().buffer();
    (area.y..area.y.saturating_add(area.height)).any(|y| {
        (area.x..area.x.saturating_add(area.width)).any(|x| {
            buffer
                .cell((x, y))
                .is_some_and(|cell| cell.fg == fg && cell.symbol() != " ")
        })
    })
}

fn map_test_theme() -> TundraTheme {
    TundraTheme {
        background: Color::Black,
        foreground: Color::Blue,
        accent: Color::LightMagenta,
        muted: Color::Gray,
        error: Color::Red,
        border_shape: tundra_ui::BorderShape::Rounded,
    }
}

fn map_cells_with_fg(terminal: &Terminal<TestBackend>, fg: Color) -> Vec<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    let mut cells = Vec::new();
    let map_x = SETUP_CONTROLS_WIDTH + 1;
    let map_y = 4;
    let map_right = WIDE_SETUP_WIDTH - 1;
    let map_bottom = WIDE_SETUP_HEIGHT - 5;

    for y in map_y..map_bottom {
        for x in map_x..map_right {
            if buffer
                .cell((x, y))
                .is_some_and(|cell| cell.fg == fg && cell.symbol() != " ")
            {
                cells.push((x, y));
            }
        }
    }

    cells
}

fn map_symbols(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    let mut symbols = Vec::new();
    let map_x = SETUP_CONTROLS_WIDTH + 1;
    let map_y = 4;
    let map_right = WIDE_SETUP_WIDTH - 1;
    let map_bottom = WIDE_SETUP_HEIGHT - 5;

    for y in map_y..map_bottom {
        for x in map_x..map_right {
            if let Some(cell) = buffer.cell((x, y)) {
                symbols.push(cell.symbol().to_string());
            }
        }
    }

    symbols
}

fn is_braille_symbol(symbol: &str) -> bool {
    symbol
        .chars()
        .any(|character| ('\u{2801}'..='\u{28ff}').contains(&character))
}

fn has_vertical_band(cells: &[(u16, u16)], min_cells_in_column: usize) -> bool {
    let map_x = SETUP_CONTROLS_WIDTH + 1;
    let map_right = WIDE_SETUP_WIDTH - 1;

    (map_x..map_right)
        .any(|x| cells.iter().filter(|(cell_x, _)| *cell_x == x).count() >= min_cells_in_column)
}
