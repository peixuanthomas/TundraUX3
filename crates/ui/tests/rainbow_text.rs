use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Widget,
};
use std::time::Duration;
use ui::components::{Button, ComponentEvent, InputEvent, Key, MouseButton, RainbowText};
use ui::{MouseEvent, TundraTheme};

fn render(text: impl Into<Text<'static>>, elapsed: Duration) -> Buffer {
    let area = Rect::new(0, 0, 20, 3);
    let mut buffer = Buffer::empty(area);
    RainbowText::new(text, elapsed).render(area, &mut buffer);
    buffer
}

#[test]
fn rainbow_changes_colors_over_time_and_returns_to_the_start() {
    let first = render("Rainbow", Duration::ZERO);
    let later = render("Rainbow", Duration::from_secs(1));
    assert_ne!(first[(0, 0)].fg, first[(1, 0)].fg);
    assert_ne!(first[(0, 0)].fg, later[(0, 0)].fg);
    assert_eq!(first[(0, 0)].symbol(), later[(0, 0)].symbol());
    assert_eq!(first, render("Rainbow", Duration::from_secs(3)));
}

#[test]
fn rainbow_preserves_wide_characters_combining_marks_and_existing_styles() {
    let content = Text::from(Line::from(Span::styled(
        "中e\u{301}🙂Z",
        Style::new().bg(Color::Blue).add_modifier(Modifier::BOLD),
    )));
    let buffer = render(content, Duration::ZERO);
    for (column, symbol) in [(0, "中"), (2, "e\u{301}"), (3, "🙂"), (5, "Z")] {
        assert_eq!(buffer[(column, 0)].symbol(), symbol);
        assert_eq!(buffer[(column, 0)].bg, Color::Blue);
        assert!(buffer[(column, 0)].modifier.contains(Modifier::BOLD));
    }
    let narrow = render("abZ", Duration::ZERO);
    assert_eq!(buffer[(2, 0)].fg, narrow[(2, 0)].fg);
}

#[test]
fn rainbow_buttons_keep_cycling_while_hovered_pressed_and_disabled() {
    let area = Rect::new(0, 0, 20, 3);
    let theme = TundraTheme::default_dark();
    for borderless in [false, true] {
        let render_button = |button: &Button| {
            let mut buffer = Buffer::empty(area);
            if borderless {
                button.render_borderless(area, &mut buffer, &theme);
            } else {
                button.render(area, &mut buffer, &theme);
            }
            buffer
        };
        for (hovered, active, disabled) in [
            (false, false, false),
            (true, false, false),
            (true, true, false),
            (true, true, true),
        ] {
            let mut button = Button::new("rainbow", "中e\u{301}🙂Z")
                .with_bracketed_label(false)
                .with_pressed_fill(true)
                .with_focus_fill(Some(Color::Blue));
            button.state.hovered = hovered;
            button.state.active = active;
            button.state.disabled = disabled;
            let plain = render_button(&button);
            let first = render_button(&button.clone().with_rainbow_label(Duration::ZERO));
            let later = render_button(&button.with_rainbow_label(Duration::from_secs(1)));
            for ((expected, current), next) in plain
                .content()
                .iter()
                .zip(first.content())
                .zip(later.content())
            {
                assert_eq!(current.symbol(), expected.symbol());
                assert_eq!(next.symbol(), expected.symbol());
                assert_eq!(
                    (current.bg, current.modifier),
                    (expected.bg, expected.modifier)
                );
                assert_eq!((next.bg, next.modifier), (expected.bg, expected.modifier));
            }
            for symbol in ["中", "e\u{301}", "🙂", "Z"] {
                let index = first
                    .content()
                    .iter()
                    .position(|cell| cell.symbol() == symbol)
                    .unwrap();
                assert_ne!(first.content()[index].fg, later.content()[index].fg);
            }
            if !borderless {
                assert_eq!(first[(0, 0)].fg, plain[(0, 0)].fg);
                assert_eq!(later[(0, 0)].fg, plain[(0, 0)].fg);
            }
        }
    }
}

#[test]
fn disabling_a_rainbow_button_still_cancels_pointer_and_keyboard_activation() {
    let area = Rect::new(0, 0, 16, 3);
    let mut button = Button::new("rainbow", "Rainbow").with_rainbow_label(Duration::from_secs(1));
    button.handle_event(
        InputEvent::mouse(MouseEvent::down(2, 1, MouseButton::Left)),
        area,
    );
    button.set_disabled(true);
    for event in [
        InputEvent::key(Key::Enter),
        InputEvent::key(Key::Space),
        InputEvent::mouse(MouseEvent::up(2, 1, MouseButton::Left)),
        InputEvent::mouse(MouseEvent::click(2, 1, MouseButton::Left)),
        InputEvent::mouse(MouseEvent::down(2, 1, MouseButton::Left)),
    ] {
        assert_eq!(button.handle_event(event, area), ComponentEvent::None);
        assert!(!button.state.active && !button.state.hovered);
    }
    button.set_disabled(false);
    assert_eq!(
        button.handle_event(
            InputEvent::mouse(MouseEvent::up(2, 1, MouseButton::Left)),
            area
        ),
        ComponentEvent::None
    );
    assert_eq!(
        button.handle_event(InputEvent::key(Key::Enter), area),
        ComponentEvent::Activated("rainbow".into())
    );
}

#[test]
fn preview_uses_real_elapsed_time_with_reduced_motion_and_zero_animation_speed() {
    use ratatui::{Terminal, backend::TestBackend};
    use ui::{MotionFrame, RenderContext};

    let render_preview = |elapsed, motion| {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let context =
            RenderContext::from_theme(&TundraTheme::default_dark(), motion, Default::default());
        terminal
            .draw(|frame| {
                ui::rainbow_text_preview::render(frame, elapsed, &context);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    };
    for reduced_motion in [false, true] {
        let motion = MotionFrame {
            reduced_motion,
            animation_speed_percent: 0,
            ..Default::default()
        };
        let first = render_preview(Duration::ZERO, motion);
        let later = render_preview(Duration::from_secs(1), motion);
        assert!(
            first
                .content()
                .iter()
                .zip(later.content())
                .any(|(a, b)| a.fg != b.fg)
        );
        assert_eq!(
            first,
            render_preview(Duration::ZERO, MotionFrame::default())
        );
        assert_eq!(
            later,
            render_preview(Duration::from_secs(1), MotionFrame::default())
        );
    }
}
