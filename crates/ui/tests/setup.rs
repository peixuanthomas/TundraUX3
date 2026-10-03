#[path = "support/composition.rs"]
mod composition;
use composition as ui;
mod support;

use std::collections::BTreeSet;
use support::terminal_output;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::{
    HomeDisplayMode, SetupField, SetupPasswordRequirementViewModel, SetupStep, SetupViewModel,
    ShellChromeViewModel, ShellLayout, StatusViewModel, TundraTheme, compute_shell_layout,
    render_setup, setup_appearance_palette_option_areas, setup_language_options,
    setup_standard_color_options, setup_timezone_list_area, setup_timezone_options,
};

const WIDE_SETUP_WIDTH: u16 = 120;
const WIDE_SETUP_HEIGHT: u16 = 34;
const SETUP_CONTROLS_WIDTH: u16 = 48;

#[test]
fn auth_pages_keep_their_submit_controls_without_duplicate_exit_buttons() {
    let login = ui::LoginViewModel::new(Vec::new(), 0, 0, 0, ui::LoginField::Password, None);
    let buttons = page_button_regions(|frame, area, context| {
        ui::render_login_content(frame, area, &login, context);
    });
    assert!(
        buttons
            .iter()
            .any(|button| button.id.as_str() == "login.submit")
    );
    assert!(
        buttons
            .iter()
            .all(|button| button.id.as_str() != "login.exit")
    );
    let layout = ui::login_layout(Rect::new(3, 2, 60, 36));
    assert_eq!(layout.submit.width, layout.selected_username.width);

    let bootstrap = ui::BootstrapAdminViewModel::new("", 0, ui::AuthField::Username, None);
    let buttons = page_button_regions(|frame, area, context| {
        ui::render_bootstrap_admin_content(frame, area, &bootstrap, context);
    });
    assert!(
        buttons
            .iter()
            .any(|button| button.id.as_str() == "bootstrap.submit")
    );
    assert!(
        buttons
            .iter()
            .all(|button| button.id.as_str() != "bootstrap.exit")
    );
    assert_eq!(ui::bootstrap_submit_area(Rect::new(3, 2, 60, 36)).width, 58);

    for step in [
        SetupStep::Language,
        SetupStep::Timezone,
        SetupStep::Admin,
        SetupStep::Appearance,
    ] {
        let model = sample_model(step, None);
        let buttons = page_button_regions(|frame, area, context| {
            ui::render_setup_content(frame, area, &model, context);
        });
        assert!(
            buttons
                .iter()
                .all(|button| button.id.as_str() != "setup.exit")
        );
        if matches!(step, SetupStep::Language | SetupStep::Timezone) {
            let previous = buttons
                .iter()
                .find(|button| button.id.as_str() == "setup.back")
                .expect("previous step remains available");
            assert_eq!(previous.disabled, step == SetupStep::Language);
            assert!(
                buttons
                    .iter()
                    .any(|button| button.id.as_str() == "setup.continue")
            );
        } else {
            let submit = if step == SetupStep::Admin {
                "setup.admin.submit"
            } else {
                "setup.appearance.finish"
            };
            assert!(buttons.iter().any(|button| button.id.as_str() == submit));
        }
    }
}

fn page_button_regions(
    mut paint: impl FnMut(&mut ratatui::Frame<'_>, Rect, &ui::RenderContext),
) -> Vec<ui::components::ButtonRegion> {
    let theme = TundraTheme::default_dark();
    let buttons = ui::components::ButtonFrame::new(None, None, &theme);
    let mut context = ui::RenderContext::from_theme(&theme, Default::default(), Default::default());
    context.buttons = Some(buttons.clone());
    let mut terminal = Terminal::new(TestBackend::new(70, 40)).unwrap();
    terminal
        .draw(|frame| paint(frame, Rect::new(3, 2, 60, 36), &context))
        .unwrap();
    buttons.regions()
}

#[test]
fn short_setup_projects_only_visible_buttons_to_the_scrolled_rows() {
    let area = Rect::new(3, 2, 40, 10);
    let mut model = sample_model(SetupStep::Admin, None);
    let theme = TundraTheme::default_dark();
    let buttons = ui::components::ButtonFrame::new(None, None, &theme);
    let mut context = ui::RenderContext::from_theme(&theme, Default::default(), Default::default());
    context.buttons = Some(buttons.clone());
    let mut terminal = Terminal::new(TestBackend::new(50, 15)).unwrap();
    terminal
        .draw(|frame| ui::render_setup_content(frame, area, &model, &context))
        .unwrap();
    assert!(
        !buttons
            .regions()
            .iter()
            .any(|button| button.id.as_str() == "setup.admin.submit")
    );
    model.scroll_offset = 15;
    terminal
        .draw(|frame| ui::render_setup_content(frame, area, &model, &context))
        .unwrap();
    let viewport = ui::setup_viewport(area, &model);
    let expected = viewport
        .project(ui::setup_admin_field_area(
            viewport.content,
            SetupField::Submit,
        ))
        .unwrap();
    assert!(
        buttons
            .regions()
            .iter()
            .any(|button| button.id.as_str() == "setup.admin.submit" && button.area == expected)
    );
    assert!(
        buttons
            .regions()
            .iter()
            .all(|button| button.id.as_str() != "setup.exit")
    );
    assert!(viewport.scrollbar.is_some());
    assert_eq!(
        viewport.content_point((expected.x, expected.y)),
        Some((expected.x - area.x, expected.y - area.y + viewport.offset))
    );
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
    assert!(output.contains("*************_"));
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
fn setup_appearance_disables_the_accent_option_matching_the_theme_color() {
    let theme = TundraTheme::default_dark();
    let mut model = sample_model(SetupStep::Appearance, None);
    model.focused_field = SetupField::AppearanceAccentColor;
    model.theme_color = Color::Cyan;
    model.theme_color_value = "cyan".to_string();
    model.accent_color = Color::Blue;
    model.accent_color_value = "blue".to_string();
    let terminal = render_terminal(&model, 120, 34, theme.clone());
    let output = terminal_output(&terminal);
    let cyan_index = setup_standard_color_options()
        .iter()
        .position(|option| option.value == "cyan")
        .expect("cyan is a standard setup color");
    let cyan_area = setup_appearance_palette_option_areas(
        setup_main_rect(120, 34),
        SetupField::AppearanceAccentColor,
    )
    .into_iter()
    .find_map(|(index, area)| (index == cyan_index).then_some(area))
    .expect("cyan accent option is visible");

    assert!(output.contains("Cyan"));
    assert!(!output.contains("[xCyan]"));
    assert!(region_has_fg(&terminal, cyan_area, theme.muted));
    assert!(!region_has_fg(&terminal, cyan_area, Color::Cyan));
}

#[test]
fn setup_renderer_updates_selected_timezone_cells_between_shanghai_and_tokyo() {
    let theme = map_test_theme();
    let shanghai = sample_model_with_timezone(SetupStep::Timezone, "Asia/Shanghai", None);
    let tokyo = sample_model_with_timezone(SetupStep::Timezone, "Asia/Tokyo", None);
    let shanghai_terminal = render_terminal(
        &shanghai,
        WIDE_SETUP_WIDTH,
        WIDE_SETUP_HEIGHT,
        theme.clone(),
    );
    let tokyo_terminal =
        render_terminal(&tokyo, WIDE_SETUP_WIDTH, WIDE_SETUP_HEIGHT, theme.clone());

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
fn setup_renderer_uses_glacier_timezone_scrollbar_when_window_is_partial() {
    let model = sample_model(SetupStep::Timezone, None);
    let terminal = render_terminal(&model, 70, 19, TundraTheme::default_dark());
    let output = terminal_output(&terminal);
    let viewport = ui::setup_viewport(setup_main_rect(70, 19), &model);
    let list_area = viewport
        .project(setup_timezone_list_area(viewport.content))
        .expect("timezone list is fully visible");
    let track = viewport
        .project(
            ui::setup_timezone_scrollbar(viewport.content, &model)
                .expect("partial timezone list needs a scrollbar"),
        )
        .expect("timezone scrollbar is fully visible");

    assert!(!output.contains("more timezones"));
    assert!(region_has_symbol(&terminal, list_area, "█"));
    assert!(region_has_symbol(&terminal, track, "█"));
}

fn sample_model(step: SetupStep, error: Option<String>) -> SetupViewModel {
    sample_model_with_timezone(step, "Asia/Tokyo", error)
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
        scroll_offset: 0,
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
        border_shape: ui::BorderShape::Rounded,
        theme_color: Color::White,
        theme_color_value: "white".to_string(),
        accent_color: Color::Cyan,
        accent_color_value: "cyan".to_string(),
        custom_color_target: None,
        custom_color_input: String::new(),
        custom_color_valid: false,
        custom_color_conflicts_with_theme: false,
        custom_color_error: None,
        error,
    }
}

fn chrome_for(screen: &str, width: u16, height: u16) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (width, height),
        back_button_hovered: false,
        screen_stack: vec![screen.to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: ui::NotificationTone::Info,
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

fn region_has_symbol(terminal: &Terminal<TestBackend>, area: Rect, symbol: &str) -> bool {
    let buffer = terminal.backend().buffer();
    (area.y..area.y.saturating_add(area.height)).any(|y| {
        (area.x..area.x.saturating_add(area.width)).any(|x| {
            buffer
                .cell((x, y))
                .is_some_and(|cell| cell.symbol() == symbol)
        })
    })
}

fn map_test_theme() -> TundraTheme {
    TundraTheme {
        color_capability: ui::ColorCapability::TrueColor,
        buttons: None,
        background: Color::Black,
        foreground: Color::Blue,
        accent_color: Color::LightMagenta,
        muted: Color::Gray,
        error: Color::Red,
        border_color: Color::White,
        border_shape: ui::BorderShape::Rounded,
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
