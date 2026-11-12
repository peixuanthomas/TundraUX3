use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::components::{
    Button, ButtonEmphasis, ButtonFrame, ButtonRegion, List, ListItem, NavRail, NavRailItem,
    Surface,
};
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
fn reported_palette_drives_accents_pressed_color_and_default_colors() {
    let mut colors = ui::TerminalColors::default();
    colors.palette[1] = Some([240, 245, 250]);
    colors.foreground = Some([200, 210, 220]);
    colors.background = Some([10, 20, 30]);
    let theme = TundraTheme {
        accent_color: Color::Red,
        foreground: Color::Reset,
        background: Color::Reset,
        ..Default::default()
    };
    let context = RenderContext::from_theme(
        &theme,
        MotionFrame::default(),
        RenderCapabilities {
            terminal_colors: colors,
            ..Default::default()
        },
    );
    assert_eq!(context.theme.accent, Color::Rgb(240, 245, 250));
    assert_eq!(context.theme.canvas, Color::Rgb(10, 20, 30));
    assert_eq!(context.theme.text, Color::Rgb(200, 210, 220));
    assert_eq!(
        context.theme.filled_style(context.theme.accent).fg,
        Some(Color::Black)
    );
    assert_eq!(
        context.compatibility_theme().button_pressed_color(),
        Color::Rgb(245, 248, 251)
    );
    let area = Rect::new(0, 0, 12, 3);
    let mut buffer = Buffer::empty(area);
    let mut button = Button::new("save", "Save");
    button.state.selected = true;
    button.render_surface(area, &mut buffer, &context.compatibility_theme());
    assert_eq!(buffer[(0, 1)].fg, Color::Rgb(240, 245, 250));
    // Without RGB output support, the renderer still emits named ANSI colors.
    let ansi = RenderContext::from_theme(
        &theme,
        MotionFrame::default(),
        RenderCapabilities {
            terminal_colors: colors,
            ..RenderCapabilities::ansi()
        },
    );
    assert_eq!(ansi.theme.accent, Color::Red);
}

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
fn compatibility_tokens_retain_resolved_surfaces_and_semantic_colors() {
    let theme = TundraTheme::default()
        .with_accent_color(Color::Magenta)
        .with_border_color(Color::Blue);
    for capabilities in [RenderCapabilities::ansi(), RenderCapabilities::default()] {
        let context = RenderContext::from_theme(&theme, MotionFrame::default(), capabilities);
        let tokens = context.compatibility_theme().tokens();
        assert_eq!(tokens, context.theme);
        assert_eq!(tokens.accent, Color::Magenta);
        assert_eq!(tokens.border, Color::Blue);
        if capabilities == RenderCapabilities::ansi() {
            assert_eq!(tokens.surface, Color::Black);
            assert_eq!(tokens.raised, Color::Black);
            assert_eq!(tokens.success, Color::Green);
            assert_eq!(tokens.danger, Color::LightRed);
        } else {
            assert_eq!(tokens.surface, Color::Rgb(24, 24, 24));
            assert_eq!(tokens.raised, Color::Rgb(36, 36, 36));
            assert_eq!(tokens.success, Color::Rgb(121, 214, 155));
            assert_eq!(tokens.danger, Color::Rgb(242, 125, 137));
        }
    }
}

#[test]
fn shared_button_frames_keep_terminal_colors_in_every_interaction_state() {
    let area = Rect::new(0, 0, 12, 3);
    for capabilities in [RenderCapabilities::ansi(), RenderCapabilities::default()] {
        for state in [
            "normal", "hovered", "pressed", "focused", "current", "disabled",
        ] {
            for emphasis in [
                ButtonEmphasis::Normal,
                ButtonEmphasis::Primary,
                ButtonEmphasis::Danger,
            ] {
                let mut context = RenderContext::from_theme(
                    &TundraTheme::default(),
                    MotionFrame::default(),
                    capabilities,
                );
                let region = ButtonRegion {
                    id: "save".into(),
                    area,
                    disabled: state == "disabled",
                };
                let hovered = matches!(state, "hovered" | "pressed" | "disabled");
                let pressed = matches!(state, "pressed" | "disabled");
                let mut buttons = ButtonFrame::new(
                    hovered.then(|| region.clone()),
                    pressed.then_some(region),
                    &context.compatibility_theme(),
                );
                buttons.keyboard_focus_visible = state == "focused";
                context.buttons = Some(buttons.clone());
                let theme = context.compatibility_theme();
                let mut button = Button::new("save", "Save")
                    .with_current(state == "current")
                    .with_emphasis(emphasis);
                button.set_focused(matches!(state, "focused" | "disabled"));
                button.set_disabled(state == "disabled");
                for variant in 0..3 {
                    let mut buffer = Buffer::empty(area);
                    match variant {
                        0 => button.render(area, &mut buffer, &theme),
                        1 => button.render_borderless(area, &mut buffer, &theme),
                        _ => button.render_surface(area, &mut buffer, &theme),
                    }
                    if capabilities == RenderCapabilities::ansi() {
                        for cell in buffer.content() {
                            assert!(
                                !matches!(cell.fg, Color::Rgb(..))
                                    && !matches!(cell.bg, Color::Rgb(..))
                                    && !matches!(cell.underline_color, Color::Rgb(..)),
                                "{state}, {emphasis:?}, variant={variant}: {cell:?}"
                            );
                        }
                    }
                    if emphasis == ButtonEmphasis::Normal && variant == 2 {
                        let expected = match state {
                            "normal" => context.theme.surface,
                            "current" => context.theme.raised,
                            "disabled" => context.theme.canvas,
                            _ => context.theme.accent_soft,
                        };
                        assert_eq!(buffer[(1, 1)].bg, expected, "{state}, {capabilities:?}");
                    }
                    if emphasis == ButtonEmphasis::Normal && variant == 1 {
                        let expected = match state {
                            "hovered" | "focused" => context.theme.accent,
                            "pressed" if capabilities == RenderCapabilities::ansi() => {
                                Color::LightCyan
                            }
                            "pressed" => Color::Rgb(153, 226, 238),
                            "disabled" => Color::DarkGray,
                            _ => context.theme.text,
                        };
                        assert_eq!(buffer[(1, 1)].fg, expected, "{state}, {capabilities:?}");
                    }
                }
                assert_eq!(buttons.regions().len(), 3);
            }
        }
    }
}

#[test]
fn ansi_components_keep_titles_neutral_and_use_user_accent_for_focus() {
    for (accent, _) in ANSI_ACCENTS {
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
        assert_eq!(buffer[(1, 0)].fg, context.theme.text);

        let mut list = List::new("sections", vec![ListItem::new("appearance", "Appearance")])
            .titled("Sections");
        list.set_focused(true);
        list.render_with_context(area, &mut buffer, &context);
        assert_eq!(buffer[(1, 0)].symbol(), "S");
        assert_eq!(buffer[(1, 0)].fg, context.theme.text);
        assert_eq!(buffer[(1, 1)].symbol(), ">");
        assert_eq!(buffer[(1, 1)].fg, accent);

        let mut rail = NavRail::new("nav", vec![NavRailItem::new("settings", "Settings")]);
        for focused in [false, true] {
            rail.state.focused = focused;
            rail.render(area, &mut buffer, &context);
            assert_eq!(buffer[(2, 0)].symbol(), "S");
            assert_eq!(
                buffer[(2, 0)].fg,
                if focused { accent } else { context.theme.text }
            );
        }

        let mut button = Button::new("save", "Save");
        button.state.selected = true;
        button.render_surface(area, &mut buffer, &context.compatibility_theme());
        assert_eq!(buffer[(0, 1)].fg, accent);
    }
}
