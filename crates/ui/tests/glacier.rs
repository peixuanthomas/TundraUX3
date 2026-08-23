use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::components::{
    ComponentEvent, ComponentTone, DataTable, EmptyState, InputEvent, List, ListItem, MouseButton,
    NavRail, NavRailItem, Panel, Picker, Scrollbar, Skeleton, Surface, Toast, ToastTone,
};
use ui::{
    BorderShape, ColorCapability, FrostMotion, MotionFrame, MotionIdentity, MotionTimings,
    MouseEvent, RenderCapabilities, RenderContext, ThemeTokens, TundraTheme, schedule_motion,
};

#[test]
fn render_context_and_surface_preserve_or_explicitly_override_border_shape() {
    let theme = TundraTheme::default().with_border_shape(BorderShape::Square);
    let context =
        RenderContext::from_theme(&theme, MotionFrame::default(), RenderCapabilities::ansi());
    assert_eq!(context.theme.border_shape, BorderShape::Square);
    assert_eq!(
        context.compatibility_theme().border_shape,
        BorderShape::Square
    );

    let area = Rect::new(0, 0, 8, 3);
    let mut inherited = Buffer::empty(area);
    Surface::new()
        .bordered(true)
        .render(area, &mut inherited, &context);
    assert_eq!(inherited[(0, 0)].symbol(), "┌");

    let mut overridden = Buffer::empty(area);
    Surface::new()
        .bordered(true)
        .border_shape(BorderShape::Rounded)
        .render(area, &mut overridden, &context);
    assert_eq!(overridden[(0, 0)].symbol(), "╭");
}

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
fn shared_collections_preserve_explicit_geometry_viewport_and_semantic_tones() {
    let context = RenderContext::default();
    let area = Rect::new(0, 0, 12, 3);
    let mut table = DataTable::new(
        "table",
        ["A", "B"],
        [["zero", "0"], ["warn", "1"], ["ok", "2"]],
    )
    .bordered(false)
    .with_column_widths(vec![8, 4])
    .with_viewport_start(1)
    .with_row_tones(vec![
        ComponentTone::Default,
        ComponentTone::Warning,
        ComponentTone::Success,
    ]);
    table.selected = Some(2);
    let mut buffer = Buffer::empty(area);
    table.render(area, &mut buffer, &context);
    assert_eq!(buffer[(0, 1)].symbol(), "w");
    assert_eq!(buffer[(8, 1)].symbol(), "1");
    assert_eq!(buffer[(0, 1)].fg, context.theme.warning);
    assert_eq!(
        table.handle_event(
            InputEvent::mouse(MouseEvent::down(1, 2, MouseButton::Left)),
            area
        ),
        ComponentEvent::Consumed
    );

    let list = List::new(
        "list",
        vec![
            ListItem::new("zero", "zero"),
            ListItem::new("danger", "danger").tone(ComponentTone::Danger),
        ],
    )
    .with_viewport_start(1);
    let mut list_buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
    list.render_borderless(
        Rect::new(0, 0, 10, 1),
        &mut list_buffer,
        &context.compatibility_theme(),
    );
    assert_eq!(list_buffer[(2, 0)].symbol(), "d");
    assert_eq!(list_buffer[(2, 0)].fg, context.theme.danger);
}

#[test]
fn pure_motion_schedule_covers_start_mid_end_interruption_reversal_and_idle() {
    let home = MotionIdentity {
        screen: Some("home"),
        focus: Some("a"),
        overlay: None,
    };
    let settings = MotionIdentity {
        screen: Some("settings"),
        focus: Some("b"),
        overlay: Some("dialog"),
    };
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
    };
    let start = schedule_motion(home, settings, Duration::from_millis(10), frame(10));
    assert!(
        start.active
            && start.screen_transition
            && start.focus_transition
            && start.overlay_transition
    );
    assert!(start.next_redraw_in <= Duration::from_nanos(16_666_667));
    assert!(schedule_motion(home, settings, Duration::from_millis(10), frame(100)).active);
    assert!(!schedule_motion(home, settings, Duration::from_millis(10), frame(231)).active);
    assert!(
        schedule_motion(settings, home, Duration::from_millis(120), frame(120)).active,
        "interruption/reversal restarts from its supplied change time"
    );
    assert!(!schedule_motion(home, home, Duration::ZERO, frame(1)).active);
    assert_eq!(
        schedule_motion(
            home,
            settings,
            Duration::ZERO,
            MotionFrame::reduced(Duration::ZERO)
        ),
        Default::default()
    );
}

#[test]
fn dismissed_toast_is_immediately_invisible_in_reduced_motion() {
    let shown = MotionFrame::reduced(Duration::from_millis(10));
    let mut toast = Toast::new("Saved", ToastTone::Success, shown);
    toast.dismiss(MotionFrame::reduced(Duration::from_millis(20)));
    let dismissed = MotionFrame::reduced(Duration::from_millis(20));
    assert!(!toast.is_visible(dismissed));
    let area = Rect::new(0, 0, 12, 3);
    let mut buffer = Buffer::empty(area);
    let context = RenderContext {
        motion: dismissed,
        ..RenderContext::default()
    };
    toast.render(area, &mut buffer, &context);
    assert!(buffer.content().iter().all(|cell| cell.symbol() == " "));
}

#[test]
fn data_table_render_and_hit_rows_agree_with_or_without_title() {
    for titled in [false, true] {
        let area = Rect::new(0, 0, 14, 5);
        let mut table = DataTable::new("table", ["Name"], [["First"], ["Second"]]);
        if titled {
            table = table.titled("Data");
        }
        let mut buffer = Buffer::empty(area);
        table.render(area, &mut buffer, &RenderContext::default());
        assert_eq!(buffer[(1, 2)].symbol(), "F");
        assert_eq!(
            table.handle_event(
                InputEvent::mouse(MouseEvent::down(1, 3, MouseButton::Left)),
                area
            ),
            ComponentEvent::Selected("table".into(), 1)
        );
    }
}

#[test]
fn nav_rail_render_and_hit_rows_agree_with_or_without_title() {
    for titled in [false, true] {
        let area = Rect::new(0, 0, 14, 3);
        let mut rail = NavRail::new(
            "nav",
            vec![
                NavRailItem::new("one", "One"),
                NavRailItem::new("two", "Two"),
            ],
        );
        if titled {
            rail = rail.titled("Nav");
        }
        let mut buffer = Buffer::empty(area);
        rail.render(area, &mut buffer, &RenderContext::default());
        let row_offset = u16::from(titled);
        let rendered_row = (0..area.width)
            .map(|x| buffer[(x, row_offset)].symbol())
            .collect::<String>();
        assert!(
            rendered_row.contains("One"),
            "titled={titled}: {rendered_row:?}"
        );
        assert_eq!(
            rail.handle_event(
                InputEvent::mouse(MouseEvent::down(1, row_offset + 1, MouseButton::Left)),
                area
            ),
            ComponentEvent::Selected("nav".into(), 1)
        );
    }
}

#[test]
fn data_table_padding_uses_terminal_cell_width_for_cjk_and_emoji() {
    let area = Rect::new(0, 0, 12, 4);
    let table = DataTable::new("table", ["甲", "B"], [["中", "🙂"]]);
    let mut buffer = Buffer::empty(area);
    table.render(area, &mut buffer, &RenderContext::default());
    assert_eq!(buffer[(6, 2)].symbol(), "🙂");
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
