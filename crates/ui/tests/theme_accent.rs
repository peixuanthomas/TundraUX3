use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::components::{Button, List, ListItem, NavRail, NavRailItem, Surface};
use ui::{MotionFrame, RenderCapabilities, RenderContext, TundraTheme};

const ANSI_ACCENTS: [(Color, Color); 16] = [
    (Color::Black, Color::Black),
    (Color::Red, Color::LightRed),
    (Color::Green, Color::LightGreen),
    (Color::Yellow, Color::LightYellow),
    (Color::Blue, Color::LightBlue),
    (Color::Magenta, Color::LightMagenta),
    (Color::Cyan, Color::LightCyan),
    (Color::Gray, Color::Gray),
    (Color::DarkGray, Color::DarkGray),
    (Color::LightRed, Color::LightRed),
    (Color::LightGreen, Color::LightGreen),
    (Color::LightYellow, Color::LightYellow),
    (Color::LightBlue, Color::LightBlue),
    (Color::LightMagenta, Color::LightMagenta),
    (Color::LightCyan, Color::LightCyan),
    (Color::White, Color::White),
];

#[test]
fn named_accents_survive_terminal_capability_and_legacy_theme_resolution() {
    for capabilities in [RenderCapabilities::ansi(), RenderCapabilities::default()] {
        for (accent, focus) in ANSI_ACCENTS {
            let theme = TundraTheme::default().with_accent_color(accent);
            let context = RenderContext::from_theme(&theme, MotionFrame::default(), capabilities);
            assert_eq!(context.theme.accent, accent, "{capabilities:?}");
            assert_eq!(context.theme.focus, focus, "accent={accent:?}");
            let legacy = context.compatibility_theme();
            assert_eq!(legacy.accent_color, accent);
            assert_eq!(legacy.tokens().focus, focus);
            let resolved_again =
                RenderContext::from_theme(&legacy, MotionFrame::default(), capabilities);
            assert_eq!(resolved_again.theme.accent, accent);
            assert_eq!(resolved_again.theme.focus, focus);
        }
    }
}

#[test]
fn ansi_components_render_user_accent_in_titles_selections_and_focus() {
    for (accent, focus) in ANSI_ACCENTS {
        let theme = TundraTheme::default()
            .with_border_color(Color::White)
            .with_accent_color(accent);
        let context =
            RenderContext::from_theme(&theme, MotionFrame::default(), RenderCapabilities::ansi());
        let area = Rect::new(0, 0, 24, 4);
        let mut buffer = Buffer::empty(area);

        Surface::new()
            .titled("Preview")
            .bordered(true)
            .render(area, &mut buffer, &context);
        assert_eq!(buffer[(1, 0)].symbol(), "P");
        assert_eq!(buffer[(1, 0)].fg, accent);

        let mut list = List::new("sections", vec![ListItem::new("appearance", "Appearance")])
            .titled("Sections");
        list.set_focused(true);
        list.render_with_context(area, &mut buffer, &context);
        assert_eq!(buffer[(1, 0)].symbol(), "S");
        assert_eq!(buffer[(1, 0)].fg, accent);
        assert_eq!(buffer[(1, 1)].symbol(), ">");
        assert_eq!(buffer[(1, 1)].fg, accent);

        let mut rail = NavRail::new("nav", vec![NavRailItem::new("settings", "Settings")]);
        for focused in [false, true] {
            rail.state.focused = focused;
            rail.render(area, &mut buffer, &context);
            assert_eq!(buffer[(2, 0)].symbol(), "S");
            assert_eq!(buffer[(2, 0)].fg, if focused { focus } else { accent });
        }

        let mut button = Button::new("save", "Save");
        button.state.selected = true;
        button.render_surface(area, &mut buffer, &context.compatibility_theme());
        assert_eq!(buffer[(0, 1)].fg, focus);
    }
}
