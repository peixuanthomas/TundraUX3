use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::common::focus_marker;
use super::{SetupCustomColorTarget, SetupField, SetupStep, SetupViewModel};
use crate::screens::shell::{
    ShellChromeViewModel, ShellLayout, centered_rect, compute_shell_layout, render_compact_home,
    render_status, render_top,
};
use crate::timezone_map::{TimezoneMapWidget, boundary_id_for_timezone};
use crate::{TundraTheme, setup_standard_color_options};

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
const SETUP_APPEARANCE_HEADER_HEIGHT: u16 = 3;
const SETUP_APPEARANCE_SHAPE_LINE: u16 = 3;
const SETUP_APPEARANCE_SHAPE_HEIGHT: u16 = 3;
const SETUP_APPEARANCE_THEME_LINE: u16 = 6;
const SETUP_APPEARANCE_PALETTE_HEIGHT: u16 = 4;
const SETUP_APPEARANCE_THEME_CUSTOM_LINE: u16 = 10;
const SETUP_APPEARANCE_ACCENT_LINE: u16 = 12;
const SETUP_APPEARANCE_ACCENT_CUSTOM_LINE: u16 = 16;
const SETUP_APPEARANCE_PREVIEW_LINE: u16 = 18;
const SETUP_APPEARANCE_PREVIEW_HEIGHT: u16 = 4;
const SETUP_APPEARANCE_SUBMIT_LINE: u16 = 23;
const SETUP_APPEARANCE_ERROR_LINE: u16 = 25;
const SETUP_APPEARANCE_BUTTON_GAP: u16 = 1;

pub fn render_setup(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let appearance_theme = if model.step == SetupStep::Appearance {
        theme
            .with_border_shape(model.border_shape)
            .with_border_color(model.theme_color)
            .with_accent_color(model.accent_color)
    } else {
        *theme
    };
    let theme = &appearance_theme;

    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_setup_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
        }
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
        SetupStep::Appearance => render_setup_appearance_page(frame, controls, model, theme),
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

fn render_setup_appearance_page(
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
        SETUP_APPEARANCE_HEADER_HEIGHT.min(content.height),
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                format!("Step: {}", setup_step_label(model.step)),
                theme.title_style(),
            ),
            Line::from("Choose the frame shape, theme color, and accent color."),
            Line::styled(
                "Tab / Up / Down: move    Left / Right: choose    Enter: activate",
                theme.muted_style(),
            ),
        ])
        .wrap(Wrap { trim: true }),
        header,
    );

    render_setup_shape_buttons(frame, area, model, theme);
    render_setup_color_palette(
        frame,
        area,
        model,
        SetupField::AppearanceThemeColor,
        "Theme color",
        &model.theme_color_value,
        theme,
    );
    render_setup_custom_color_button(
        frame,
        area,
        model,
        SetupField::AppearanceThemeCustom,
        "Use a custom theme color...",
        theme,
    );
    render_setup_color_palette(
        frame,
        area,
        model,
        SetupField::AppearanceAccentColor,
        "Accent color",
        &model.accent_color_value,
        theme,
    );
    render_setup_custom_color_button(
        frame,
        area,
        model,
        SetupField::AppearanceAccentCustom,
        "Use a custom accent color...",
        theme,
    );

    let preview = setup_appearance_preview_area(area);
    if preview.width > 0 && preview.height > 0 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Live preview", theme.title_style()),
                Line::from(format!(
                    "Theme: {}    Accent: {}",
                    model.theme_color_value, model.accent_color_value
                )),
            ])
            .block(
                theme
                    .block()
                    .title("Preview")
                    .title_style(theme.title_style())
                    .borders(Borders::ALL)
                    .style(theme.body_style()),
            ),
            preview,
        );
    }

    let submit_focused = model.focused_field == SetupField::AppearanceSubmit;
    let submit = Line::styled(
        format!("{}Finish setup", focus_marker(submit_focused)),
        if submit_focused {
            theme.title_style()
        } else {
            theme.body_style()
        },
    );
    frame.render_widget(
        Paragraph::new(submit),
        setup_appearance_field_area(area, SetupField::AppearanceSubmit),
    );

    if let Some(error) = &model.error {
        frame.render_widget(
            Paragraph::new(Line::styled(format!("Error: {error}"), theme.error_style()))
                .wrap(Wrap { trim: true }),
            setup_appearance_error_area(area),
        );
    }

    if model.custom_color_target.is_some() {
        render_setup_custom_color_dialog(frame, area, model, theme);
    }
}

fn render_setup_shape_buttons(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let field = SetupField::AppearanceShape;
    let outer = setup_appearance_field_area(area, field);
    if outer.width == 0 || outer.height == 0 {
        return;
    }

    let focused = model.focused_field == field;
    frame.render_widget(
        theme
            .block()
            .title("Frame shape")
            .title_style(if focused {
                theme.title_style()
            } else {
                theme.body_style()
            })
            .borders(Borders::ALL)
            .style(theme.body_style())
            .border_style(theme.selectable_border_style(focused)),
        outer,
    );

    for (shape, button_area) in setup_appearance_shape_option_areas(area) {
        let selected = model.border_shape == shape;
        let label = match shape {
            crate::BorderShape::Rounded => "Rounded",
            crate::BorderShape::Square => "Square",
        };
        let style = if selected {
            theme.title_style()
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(format!("[{} {label}]", if selected { "x" } else { " " })).style(style),
            button_area,
        );
    }
}

fn render_setup_color_palette(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    field: SetupField,
    title: &'static str,
    selected_value: &str,
    theme: &TundraTheme,
) {
    let outer = setup_appearance_field_area(area, field);
    if outer.width == 0 || outer.height == 0 {
        return;
    }

    let focused = model.focused_field == field;
    frame.render_widget(
        theme
            .block()
            .title(title)
            .title_style(if focused {
                theme.title_style()
            } else {
                theme.body_style()
            })
            .borders(Borders::ALL)
            .style(theme.body_style())
            .border_style(theme.selectable_border_style(focused)),
        outer,
    );

    for (index, button_area) in setup_appearance_palette_option_areas(area, field) {
        let option = setup_standard_color_options()[index];
        let selected = option.value.eq_ignore_ascii_case(selected_value);
        let disabled =
            field == SetupField::AppearanceAccentColor && option.color == model.theme_color;
        let style = if disabled {
            theme.muted_style()
        } else {
            Style::default()
                .fg(option.color)
                .bg(theme.background)
                .add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                })
        };
        frame.render_widget(
            Paragraph::new(format!(
                "[{}{}]",
                if disabled {
                    "x"
                } else if selected {
                    ">"
                } else {
                    " "
                },
                option.label
            ))
            .style(style),
            button_area,
        );
    }
}

fn render_setup_custom_color_button(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    field: SetupField,
    label: &'static str,
    theme: &TundraTheme,
) {
    let focused = model.focused_field == field;
    frame.render_widget(
        Paragraph::new(format!("{}[ {label} ]", focus_marker(focused))).style(if focused {
            theme.title_style()
        } else {
            theme.body_style()
        }),
        setup_appearance_field_area(area, field),
    );
}

fn render_setup_custom_color_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let dialog = setup_custom_color_dialog_area(area);
    if dialog.width == 0 || dialog.height == 0 {
        return;
    }
    let target_label = match model.custom_color_target {
        Some(SetupCustomColorTarget::Theme) => "theme",
        Some(SetupCustomColorTarget::Accent) => "accent",
        None => return,
    };

    frame.render_widget(Clear, dialog);
    frame.render_widget(
        theme
            .block()
            .title(format!("Custom {target_label} color"))
            .title_style(theme.title_style())
            .borders(Borders::ALL)
            .style(theme.body_style()),
        dialog,
    );

    let inner = setup_inner_area(dialog);
    let instruction = Rect::new(inner.x, inner.y, inner.width, 1.min(inner.height));
    frame.render_widget(
        Paragraph::new("Enter #RRGGBB or a supported color name.").style(theme.muted_style()),
        instruction,
    );

    let input_area = setup_custom_color_input_area(area);
    if input_area.width > 0 && input_area.height > 0 {
        let input_block = theme
            .block()
            .title("Color code")
            .title_style(theme.title_style())
            .borders(Borders::ALL)
            .style(theme.body_style())
            .border_style(theme.selectable_border_style(true));
        let input_inner = input_block.inner(input_area);
        frame.render_widget(input_block, input_area);
        frame.render_widget(
            Paragraph::new(if model.custom_color_input.is_empty() {
                "#38BDF8".to_string()
            } else {
                model.custom_color_input.clone()
            })
            .style(if model.custom_color_input.is_empty() {
                theme.muted_style()
            } else {
                theme.body_style()
            }),
            input_inner,
        );
    }

    let feedback = if let Some(error) = &model.custom_color_error {
        Line::styled(error.clone(), theme.error_style())
    } else if model.custom_color_conflicts_with_theme {
        Line::styled(
            "Accent color must differ from the theme color",
            theme.error_style(),
        )
    } else if model.custom_color_valid {
        Line::styled("Valid color - press Enter to apply", theme.title_style())
    } else {
        Line::styled("Enter a complete color code", theme.muted_style())
    };
    let feedback_area = Rect::new(
        inner.x,
        inner.y.saturating_add(5),
        inner.width,
        1.min(inner.height.saturating_sub(5)),
    );
    frame.render_widget(Paragraph::new(feedback), feedback_area);

    let actions_area = Rect::new(
        inner.x,
        inner.y.saturating_add(6),
        inner.width,
        1.min(inner.height.saturating_sub(6)),
    );
    frame.render_widget(
        Paragraph::new("Enter: apply    Esc: cancel").style(theme.muted_style()),
        actions_area,
    );
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
        .style(box_style)
        .border_style(theme.selectable_border_style(focused));
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
        SetupField::LanguageList
        | SetupField::TimezoneList
        | SetupField::AppearanceShape
        | SetupField::AppearanceThemeColor
        | SetupField::AppearanceThemeCustom
        | SetupField::AppearanceAccentColor
        | SetupField::AppearanceAccentCustom
        | SetupField::AppearanceSubmit => (SETUP_ADMIN_USERNAME_LINE, SETUP_ADMIN_FIELD_HEIGHT),
    };
    setup_admin_line_area(main, line, height)
}

pub fn setup_appearance_field_area(main: Rect, field: SetupField) -> Rect {
    let (line, height) = match field {
        SetupField::AppearanceShape => (SETUP_APPEARANCE_SHAPE_LINE, SETUP_APPEARANCE_SHAPE_HEIGHT),
        SetupField::AppearanceThemeColor => {
            (SETUP_APPEARANCE_THEME_LINE, SETUP_APPEARANCE_PALETTE_HEIGHT)
        }
        SetupField::AppearanceThemeCustom => (SETUP_APPEARANCE_THEME_CUSTOM_LINE, 1),
        SetupField::AppearanceAccentColor => (
            SETUP_APPEARANCE_ACCENT_LINE,
            SETUP_APPEARANCE_PALETTE_HEIGHT,
        ),
        SetupField::AppearanceAccentCustom => (SETUP_APPEARANCE_ACCENT_CUSTOM_LINE, 1),
        SetupField::AppearanceSubmit => (SETUP_APPEARANCE_SUBMIT_LINE, 1),
        _ => (SETUP_APPEARANCE_SHAPE_LINE, 0),
    };
    setup_line_area(main, line, height)
}

pub fn setup_appearance_shape_option_areas(main: Rect) -> [(crate::BorderShape, Rect); 2] {
    let outer = setup_appearance_field_area(main, SetupField::AppearanceShape);
    let inner = setup_inner_area(outer);
    let rounded_width = 13.min(inner.width);
    let square_x = inner
        .x
        .saturating_add(rounded_width)
        .saturating_add(SETUP_APPEARANCE_BUTTON_GAP);
    let square_width = 12.min(inner.x.saturating_add(inner.width).saturating_sub(square_x));
    [
        (
            crate::BorderShape::Rounded,
            Rect::new(inner.x, inner.y, rounded_width, inner.height.min(1)),
        ),
        (
            crate::BorderShape::Square,
            Rect::new(square_x, inner.y, square_width, inner.height.min(1)),
        ),
    ]
}

pub fn setup_appearance_palette_option_areas(main: Rect, field: SetupField) -> Vec<(usize, Rect)> {
    if !matches!(
        field,
        SetupField::AppearanceThemeColor | SetupField::AppearanceAccentColor
    ) {
        return Vec::new();
    }

    let outer = setup_appearance_field_area(main, field);
    let inner = setup_inner_area(outer);
    if inner.width == 0 || inner.height == 0 {
        return Vec::new();
    }

    let right = inner.x.saturating_add(inner.width);
    let bottom = inner.y.saturating_add(inner.height);
    let mut x = inner.x;
    let mut y = inner.y;
    let mut areas = Vec::new();
    for (index, option) in setup_standard_color_options().iter().enumerate() {
        let desired_width = u16::try_from(option.label.chars().count())
            .unwrap_or(u16::MAX)
            .saturating_add(3);
        if x > inner.x && x.saturating_add(desired_width) > right {
            x = inner.x;
            y = y.saturating_add(1);
        }
        if y >= bottom {
            break;
        }
        let width = desired_width.min(right.saturating_sub(x));
        if width == 0 {
            break;
        }
        areas.push((index, Rect::new(x, y, width, 1)));
        x = x
            .saturating_add(width)
            .saturating_add(SETUP_APPEARANCE_BUTTON_GAP);
    }
    areas
}

fn setup_appearance_preview_area(main: Rect) -> Rect {
    setup_line_area(
        main,
        SETUP_APPEARANCE_PREVIEW_LINE,
        SETUP_APPEARANCE_PREVIEW_HEIGHT,
    )
}

fn setup_appearance_error_area(main: Rect) -> Rect {
    setup_line_area(main, SETUP_APPEARANCE_ERROR_LINE, 2)
}

pub fn setup_custom_color_dialog_area(main: Rect) -> Rect {
    centered_rect(main, main.width.min(58), main.height.min(10))
}

pub fn setup_custom_color_input_area(main: Rect) -> Rect {
    let dialog = setup_custom_color_dialog_area(main);
    let inner = setup_inner_area(dialog);
    Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        3.min(inner.height.saturating_sub(2)),
    )
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
        SetupStep::Appearance => "Appearance",
    }
}

fn selection_marker(selected: bool) -> &'static str {
    if selected { "> " } else { "  " }
}
