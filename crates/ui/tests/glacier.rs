use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::components::{
    ComponentEvent, ComponentTone, DataTable, EmptyState, InputEvent, List, ListItem, MouseButton,
    NavRail, NavRailItem, Panel, Picker, Scrollbar, Skeleton, Surface, Toast, ToastTone,
};
use ui::{
    BorderShape, ColorCapability, FrostMotion, MotionDirection, MotionFrame, MotionIdentity,
    MotionOverlayIdentity, MotionOverlayKind, MotionTimings, MotionTransitionKind, MouseEvent,
    RenderCapabilities, RenderContext, ThemeTokens, TundraTheme, schedule_motion,
    schedule_motion_range,
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
        overlay: Some(MotionOverlayIdentity {
            kind: MotionOverlayKind::Dialog,
            id: "dialog",
        }),
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
    let dialog_start = start.transitions.overlay.expect("dialog transition");
    assert_eq!(dialog_start.kind, MotionTransitionKind::Dialog);
    assert_eq!(dialog_start.direction, MotionDirection::Entering);
    assert_eq!(dialog_start.progress, 0);
    let dialog_mid = schedule_motion(home, settings, Duration::from_millis(10), frame(100))
        .transitions
        .overlay
        .expect("mid dialog transition");
    assert!(dialog_mid.active && dialog_mid.progress > 0 && dialog_mid.progress < 1_000);
    let dialog_final = schedule_motion(home, settings, Duration::from_millis(10), frame(190))
        .transitions
        .overlay
        .expect("final dialog transition");
    assert!(!dialog_final.active);
    assert_eq!(dialog_final.progress, 1_000);
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

    let popover = MotionIdentity {
        overlay: Some(MotionOverlayIdentity {
            kind: MotionOverlayKind::Popover,
            id: "menu",
        }),
        ..home
    };
    let popover_start = schedule_motion(home, popover, Duration::ZERO, frame(0))
        .transitions
        .overlay
        .expect("popover enter");
    assert_eq!(popover_start.kind, MotionTransitionKind::Popover);
    assert!(popover_start.active);
    assert!(
        !schedule_motion(home, popover, Duration::ZERO, frame(160))
            .transitions
            .overlay
            .expect("popover final")
            .active
    );

    let reversed = schedule_motion(popover, home, Duration::from_millis(80), frame(80))
        .transitions
        .overlay
        .expect("popover reversal");
    assert_eq!(reversed.direction, MotionDirection::Exiting);
    assert_eq!(reversed.progress, 1_000);
}

#[test]
fn render_context_exposes_observable_page_and_overlay_frames() {
    let home = MotionIdentity {
        screen: Some("home"),
        ..MotionIdentity::default()
    };
    let settings = MotionIdentity {
        screen: Some("settings"),
        overlay: Some(MotionOverlayIdentity {
            kind: MotionOverlayKind::Dialog,
            id: "dialog",
        }),
        ..MotionIdentity::default()
    };
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
    };
    let theme = TundraTheme::default();
    let area = Rect::new(0, 0, 80, 24);

    let start_frame = frame(0);
    let start = RenderContext::from_theme_with_transitions(
        &theme,
        start_frame,
        schedule_motion(home, settings, Duration::ZERO, start_frame).transitions,
        RenderCapabilities::default(),
    );
    assert_eq!(start.page_area(area), Rect::new(0, 1, 80, 23));
    assert_eq!(start.theme.border, start.theme.canvas);
    assert!(!start.overlay_interaction_ready());

    let mid_frame = frame(90);
    let mid = RenderContext::from_theme_with_transitions(
        &theme,
        mid_frame,
        schedule_motion(home, settings, Duration::ZERO, mid_frame).transitions,
        RenderCapabilities::default(),
    );
    assert_eq!(mid.page_area(area), area);
    assert_ne!(mid.theme.border, mid.theme.canvas);
    assert!(mid.overlay_interaction_ready());

    let final_frame = frame(220);
    let final_context = RenderContext::from_theme_with_transitions(
        &theme,
        final_frame,
        schedule_motion(home, settings, Duration::ZERO, final_frame).transitions,
        RenderCapabilities::default(),
    );
    assert_eq!(final_context.page_area(area), area);
    assert_eq!(final_context.theme.border, theme.tokens().border);

    let exit_frame = frame(0);
    let exit_context = RenderContext::from_theme_with_transitions(
        &theme,
        exit_frame,
        schedule_motion(settings, home, Duration::ZERO, exit_frame).transitions,
        RenderCapabilities::default(),
    );
    assert_eq!(
        exit_context.theme.border,
        theme.tokens().border,
        "an absent exiting overlay must not fade the page and jump at completion"
    );
}

#[test]
fn ranged_motion_is_continuous_and_overlay_exit_projection_is_endpoint_neutral() {
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
    };
    for kind in [
        MotionTransitionKind::Page,
        MotionTransitionKind::Focus,
        MotionTransitionKind::Dialog,
        MotionTransitionKind::Popover,
    ] {
        let entering = schedule_motion_range(
            kind,
            MotionDirection::Entering,
            0,
            1_000,
            Duration::ZERO,
            frame(60),
        );
        let reversed = schedule_motion_range(
            kind,
            MotionDirection::Exiting,
            entering.progress,
            0,
            Duration::from_millis(60),
            frame(60),
        );
        assert_eq!(reversed.progress, entering.progress, "{kind:?} jumped");
        assert_eq!(reversed.phase_progress, 0);
        let settled = schedule_motion_range(
            kind,
            MotionDirection::Exiting,
            entering.progress,
            0,
            Duration::from_millis(60),
            frame(500),
        );
        assert_eq!(settled.progress, 0);
        assert_eq!(settled.phase_progress, 1_000);
        assert!(!settled.active);
        let reduced = schedule_motion_range(
            kind,
            MotionDirection::Exiting,
            entering.progress,
            0,
            Duration::from_millis(60),
            MotionFrame::reduced(Duration::from_millis(60)),
        );
        assert_eq!((reduced.progress, reduced.phase_progress), (0, 1_000));
        assert!(!reduced.active);
    }

    let theme = TundraTheme::default();
    let area = Rect::new(0, 0, 80, 24);
    for kind in [MotionTransitionKind::Dialog, MotionTransitionKind::Popover] {
        let context = |millis| {
            let motion = schedule_motion_range(
                kind,
                MotionDirection::Exiting,
                1_000,
                0,
                Duration::ZERO,
                frame(millis),
            );
            RenderContext::from_theme_with_transitions(
                &theme,
                frame(millis),
                ui::MotionTransitions {
                    overlay: Some(motion),
                    ..ui::MotionTransitions::default()
                },
                RenderCapabilities::default(),
            )
        };
        assert_eq!(context(0).page_area(area), area);
        assert_eq!(context(120).page_area(area), Rect::new(0, 1, 80, 23));
        assert_eq!(context(500).page_area(area), area);
    }
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
fn toast_enter_and_exit_progress_remain_self_scheduled_behind_other_overlays() {
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
    };
    let mut toast = Toast::new("Saved", ToastTone::Success, frame(0));
    assert_eq!(toast.visible_progress(frame(0)), 0);
    assert!(toast.requests_redraw(frame(0)));
    assert!(toast.visible_progress(frame(100)) > 0);
    let interrupted = toast.visible_progress(frame(75));
    toast.dismiss(frame(75));
    assert_eq!(toast.visible_progress(frame(75)), interrupted);
    assert_eq!(toast.visible_progress(frame(500)), 0);

    let mut toast = Toast::new("Saved", ToastTone::Success, frame(0));
    assert_eq!(toast.visible_progress(frame(200)), 1_000);
    assert!(!toast.requests_redraw(frame(200)));

    toast.dismiss(frame(200));
    assert_eq!(toast.visible_progress(frame(200)), 1_000);
    assert!(toast.requests_redraw(frame(200)));
    let exit_mid = toast.visible_progress(frame(275));
    assert!(exit_mid > 0 && exit_mid < 1_000);
    assert_eq!(toast.visible_progress(frame(350)), 0);
    assert!(!toast.requests_redraw(frame(350)));
    assert!(!toast.is_visible(frame(350)));
}

#[test]
fn toast_resume_reverses_from_the_visible_progress_in_full_and_reduced_motion() {
    let frame = |millis| MotionFrame {
        now: Duration::from_millis(millis),
        delta: Duration::ZERO,
        reduced_motion: false,
    };
    let mut toast = Toast::new("Saved", ToastTone::Info, frame(0));
    toast.dismiss(frame(200));
    let exiting = toast.visible_progress(frame(250));
    toast.resume(frame(250));
    assert_eq!(toast.visible_progress(frame(250)), exiting);
    assert_eq!(toast.visible_progress(frame(500)), 1_000);

    let mut reduced = Toast::new(
        "Saved",
        ToastTone::Info,
        MotionFrame::reduced(Duration::ZERO),
    );
    reduced.dismiss(MotionFrame::reduced(Duration::from_millis(1)));
    reduced.resume(MotionFrame::reduced(Duration::from_millis(2)));
    assert_eq!(
        reduced.visible_progress(MotionFrame::reduced(Duration::from_millis(2))),
        1_000
    );
    assert!(!reduced.requests_redraw(MotionFrame::reduced(Duration::from_millis(2))));
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
