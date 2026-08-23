use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::components::{
    DataTable, EmptyState, NavRail, NavRailItem, Panel, Picker, Scrollbar, Skeleton, Surface,
    Toast, ToastTone,
};
use ui::{
    ColorCapability, FrostMotion, MotionFrame, MotionTimings, RenderCapabilities, RenderContext,
    ThemeTokens, TundraTheme,
};

#[test]
fn glacier_night_palette_and_ansi_fallback_are_stable() {
    let tokens = ThemeTokens::glacier_night();
    assert_eq!(tokens.canvas, Color::Rgb(0x07, 0x11, 0x16));
    assert_eq!(tokens.surface, Color::Rgb(0x0D, 0x1B, 0x22));
    assert_eq!(tokens.accent, Color::Rgb(0x63, 0xD3, 0xE5));
    assert_eq!(tokens.danger, Color::Rgb(0xF2, 0x7D, 0x89));

    let ansi = tokens.for_capability(ColorCapability::Ansi);
    assert_eq!(ansi.canvas, Color::Black);
    assert_eq!(ansi.raised, Color::DarkGray);
    assert_eq!(ansi.text, Color::White);
    assert_eq!(ansi.muted, Color::Gray);
    assert_eq!(ansi.accent, Color::Cyan);
    assert_eq!(ansi.focus, Color::LightCyan);
    assert_eq!(ansi.success, Color::Green);
    assert_eq!(ansi.warning, Color::Yellow);
    assert_eq!(ansi.danger, Color::LightRed);
}

#[test]
fn custom_accent_derives_soft_strong_and_focus_tokens() {
    let tokens = ThemeTokens::glacier_night().with_accent(Color::Rgb(0xD8, 0x91, 0xFF));
    assert_eq!(tokens.accent, Color::Rgb(0xD8, 0x91, 0xFF));
    assert_ne!(tokens.accent_soft, tokens.accent);
    assert_ne!(tokens.accent_strong, tokens.accent);
    assert_ne!(tokens.focus, tokens.accent);
    assert_eq!(tokens.surface, ThemeTokens::glacier_night().surface);
}

#[test]
fn frost_motion_only_requests_redraw_while_active_and_respects_reduced_motion() {
    let start = MotionFrame {
        now: Duration::ZERO,
        delta: Duration::ZERO,
        reduced_motion: false,
    };
    let mut motion = FrostMotion::default();
    motion.begin(start, MotionTimings::FOCUS);
    assert!(motion.requests_redraw(start));
    assert!(motion.progress(start, true) < 20);

    let halfway = MotionFrame {
        now: Duration::from_millis(60),
        delta: Duration::from_millis(16),
        reduced_motion: false,
    };
    assert!(motion.requests_redraw(halfway));
    assert!(motion.progress(halfway, true) > 500);
    motion.begin(halfway, MotionTimings::DIALOG);
    assert!(
        motion.requests_redraw(halfway),
        "an interrupted transition restarts at the current frame"
    );

    let finished = MotionFrame {
        now: Duration::from_millis(241),
        delta: Duration::from_millis(16),
        reduced_motion: false,
    };
    assert!(!motion.requests_redraw(finished));
    let reduced = MotionFrame::reduced(Duration::from_millis(242));
    motion.begin(reduced, MotionTimings::PAGE);
    assert!(!motion.requests_redraw(reduced));
    assert_eq!(motion.progress(reduced, true), 1_000);

    let context = RenderContext::from_theme_with_motion_preference(
        &TundraTheme::default(),
        Duration::from_millis(300),
        true,
        RenderCapabilities::default(),
    );
    assert!(context.motion.reduced_motion);
    assert_eq!(
        MotionTimings::resolve(context.motion, MotionTimings::DIALOG),
        Duration::ZERO
    );
}

#[test]
fn glacier_components_render_at_minimum_standard_and_wide_sizes() {
    for (width, height) in [(1, 1), (80, 24), (120, 32)] {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        let context = RenderContext::from_theme(
            &TundraTheme::default(),
            MotionFrame::default(),
            RenderCapabilities::ansi(),
        );
        Surface::new()
            .bordered(true)
            .raised(true)
            .render(area, &mut buffer, &context);
        Panel::new("Panel").render(area, &mut buffer, &context);
        Picker::new("picker", ["One", "Two"])
            .titled("Picker")
            .render(area, &mut buffer, &context);
        NavRail::new(
            "nav",
            vec![
                NavRailItem::new("home", "Home"),
                NavRailItem::new("settings", "Settings"),
            ],
        )
        .render(area, &mut buffer, &context);
        DataTable::new("table", ["Name", "State"], [["Tundra", "Ready"]])
            .titled("Data")
            .render(area, &mut buffer, &context);
        Scrollbar::new(100, 10, 20).render(area, &mut buffer, &context);
        Toast::new("Saved", ToastTone::Success, context.motion).render(area, &mut buffer, &context);
        EmptyState::new("Nothing here")
            .detail("Try another folder")
            .render(area, &mut buffer, &context);
        Skeleton.render(area, &mut buffer, &context);
        assert_eq!(buffer.area, area);
    }
}
