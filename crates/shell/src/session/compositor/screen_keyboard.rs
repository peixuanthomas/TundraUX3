//! The debug keyboard uses the same final frame owner as normal Shell pages.
use super::ScreenCompositor;
use ratatui::{Frame, layout::Rect, widgets::Paragraph};
use ui::{RenderContext, ScreenKeyboardLayout, ScreenKeyboardViewModel};

impl ScreenCompositor {
    pub(crate) fn keyboard_demo_layout(
        bounds: Rect,
        collapsed: bool,
        visibility: u16,
        aa_test: bool,
        context: &RenderContext,
    ) -> ScreenKeyboardLayout {
        if aa_test {
            ui::screen_keyboard_aa_layout(bounds, collapsed, visibility, context)
        } else {
            ui::screen_keyboard_layout_with_visibility(bounds, collapsed, visibility)
        }
    }

    pub(crate) fn render_keyboard_demo(
        frame: &mut Frame<'_>,
        layout: &ScreenKeyboardLayout,
        model: &ScreenKeyboardViewModel<'_>,
        aa_test: bool,
        context: &RenderContext,
    ) {
        let bounds = frame.area();
        if !aa_test {
            ui::render_screen_keyboard(frame, bounds, layout, model, context);
            return;
        }
        let chrome = ui::ShellChromeViewModel {
            app_name: "TundraUX 3".into(),
            build_mode: i18n::tr!("screen-keyboard-test-aa"),
            display_mode: ui::HomeDisplayMode::Debug,
            terminal_size: (bounds.width, bounds.height),
            screen_stack: vec![i18n::tr!("screen-keyboard-test-aa")],
            back_button_hovered: false,
            back_shortcut: "Esc",
            status: ui::StatusViewModel {
                status: i18n::tr!("screen-keyboard-aa-safe"),
                toast: None,
                error: None,
                alert_tone: ui::NotificationTone::Info,
                time_button_label: Some(chrono::Local::now().format("%H:%M:%S").to_string()),
                time_button_selected: false,
            },
        };
        let shell =
            ui::ShellFrameLayout::new(bounds, chrome.status.time_button_label.as_deref(), context);
        ui::components::Surface::new().render_frame(frame, bounds, context);
        frame.render_widget(
            Paragraph::new(i18n::tr!("aa-preview-page-content"))
                .style(context.compatibility_theme().muted_style()),
            shell.main,
        );
        // Background chrome is deliberately inactive while the AA test is open.
        let mut background = context.clone();
        background.buttons = None;
        ui::render_shell_chrome(frame, &shell, &chrome, &background);
        ui::render_screen_keyboard_aa_test(frame, layout, model, context);
        // Paint last, over bottom status/time chrome. Layout has already moved
        // and bounded the dialog, so this panel cannot cover any part of it.
        ui::render_screen_keyboard_panel(frame, layout, model, context);
    }
}
