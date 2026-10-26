use super::{fit_cell, text_width, truncate_status_text};
use ratatui::text::Line;

#[test]
fn status_message_uses_shared_hover_pressed_and_keyboard_colors() {
    use crate::components::{ButtonFrame, ButtonRegion};
    use crate::{
        HomeDisplayMode, RenderContext, ShellChromeViewModel, ShellFrameLayout, StatusViewModel,
        TundraTheme,
    };
    use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
    let mut theme = TundraTheme::default_dark();
    theme.accent_color = Color::Rgb(30, 120, 200);
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let layout = ShellFrameLayout::new(Rect::new(0, 0, 80, 24), Some("12:00"), &context);
    let message = layout.status_message.unwrap();
    let region = ButtonRegion {
        id: "shell.status".into(),
        area: message,
        disabled: false,
    };
    let chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".into(),
        build_mode: "debug".into(),
        display_mode: HomeDisplayMode::User,
        terminal_size: (80, 24),
        screen_stack: vec![],
        back_button_hovered: false,
        help_available: false,
        help_button_hovered: false,
        help_button_focused: false,
        tasks: None,
        back_shortcut: "Esc",
        status: StatusViewModel {
            status: "Complete status message".into(),
            toast: None,
            error: None,
            alert_tone: crate::NotificationTone::Info,
            time_button_label: Some("12:00".into()),
            time_button_selected: false,
        },
    };
    for (hovered, pressed, keyboard, focused, expected) in [
        (true, false, false, false, theme.accent_color),
        (true, true, false, false, theme.button_pressed_color()),
        (false, false, true, true, theme.accent_color),
        (false, false, false, false, theme.foreground),
    ] {
        let mut buttons = ButtonFrame::new(
            hovered.then(|| region.clone()),
            pressed.then(|| region.clone()),
            &theme,
        );
        buttons.keyboard_focus_visible = keyboard;
        let mut context = context.clone();
        context.buttons = Some(buttons.clone());
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                super::render_shell_chrome_with_status_focus(
                    frame, &layout, &chrome, &context, focused,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(message.x + 1, message.y + 1)].fg, expected);
        assert_eq!(
            buffer[(message.x, message.y + 1)].fg,
            if expected == theme.foreground {
                theme.border_color
            } else {
                expected
            }
        );
        assert!(buttons.regions().contains(&region));
        assert_ne!(layout.time_button.unwrap(), message);
    }
    let mut chrome = chrome;
    chrome.status.status = "Ready".into();
    for visibility in [100, 500] {
        let layout = layout.with_input_keyboard(Rect::new(0, 24, 80, 0), visibility);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| super::render_shell_chrome(frame, &layout, &chrome, &context))
            .unwrap();
        for (area, expected) in [
            (layout.status_message.unwrap(), "Ready"),
            (layout.time_button.unwrap(), "12:00"),
        ] {
            let row = (area.x..area.right())
                .map(|x| terminal.backend().buffer()[(x, area.y)].symbol())
                .collect::<String>();
            assert_eq!(row.trim(), expected);
        }
    }
}

#[test]
fn help_button_sits_left_of_back_and_hides_in_compact() {
    use crate::{RenderContext, ShellFrameLayout};
    use ratatui::layout::Rect;
    let context = RenderContext::default();
    let layout = ShellFrameLayout::new(Rect::new(0, 0, 80, 24), Some("12:00"), &context)
        .with_back_shortcut("Esc")
        .with_help_button(true);
    let back = layout.back_button.unwrap();
    let help = layout.help_button.unwrap();
    assert_eq!(help.right() + 1, back.x);
    assert_eq!(help.y, back.y);
    let compact =
        ShellFrameLayout::new(Rect::new(0, 0, 40, 10), None, &context).with_help_button(true);
    assert!(compact.help_button.is_none());
}

#[test]
fn cell_fitting_and_status_truncation_use_terminal_display_width() {
    assert_eq!(text_width("界面"), 4);
    assert_eq!(fit_cell("界面", 5), "界面 ");
    assert_eq!(fit_cell("界面", 3), "界…");
    assert_eq!(Line::from(fit_cell("界面", 3)).width(), 3);
    assert_eq!(truncate_status_text("界面状态", 5), "界...");
    assert_eq!(Line::from(truncate_status_text("界面状态", 5)).width(), 5);
}
