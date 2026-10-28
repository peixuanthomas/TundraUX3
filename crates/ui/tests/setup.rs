#[path = "support/composition.rs"]
mod composition;
use composition as ui;
mod support;

use support::terminal_output;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::{
    HomeDisplayMode, SetupField, SetupPasswordRequirementViewModel, SetupStep, SetupViewModel,
    ShellChromeViewModel, StatusViewModel, TundraTheme, render_setup, setup_language_options,
    setup_timezone_options,
};

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
fn narrow_setup_navigation_keeps_previous_and_primary_shortcuts_visible() {
    let area = Rect::new(3, 2, 30, 10);
    let mut model = sample_model(SetupStep::Timezone, None);
    model.scroll_offset = u16::MAX;
    let viewport = ui::setup_viewport(area, &model);
    let [back, next] = ui::setup_navigation_areas(viewport.content);
    assert!(back.bottom() <= next.y);
    assert!(viewport.project(back).is_some() && viewport.project(next).is_some());
    let mut terminal = Terminal::new(TestBackend::new(40, 15)).unwrap();
    let context = ui::RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    terminal
        .draw(|frame| ui::render_setup_content(frame, area, &model, &context))
        .unwrap();
    let output = terminal_output(&terminal);
    assert!(output.contains("Alt+← Back"));
    assert!(output.contains("F10 Next"));
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
    assert!(output.contains("F10 Submit"));
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
        custom_color_editing: Default::default(),
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
        help_available: false,
        help_button_hovered: false,
        help_button_focused: false,
        back_shortcut: "Esc",
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
