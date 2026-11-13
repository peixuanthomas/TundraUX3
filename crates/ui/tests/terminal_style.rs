use std::time::Duration;

use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
    text::Line,
    widgets::{Paragraph, Widget},
};
use ui::{
    BusyIndicator, MotionFrame, RenderContext, TundraTheme,
    automation::FrameCapture,
    components::{
        Button, ButtonEmphasis, ButtonFrame, ComponentEvent, ComponentTone, DataTable, InputEvent,
        List, ListItem, MouseButton, NavRail, NavRailItem, SidebarWorkspaceLayout, Surface,
    },
};

fn text(buffer: &Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn dark_blue_palette_keeps_user_accent_and_border_preferences() {
    let theme = TundraTheme::default()
        .with_accent_color(Color::Magenta)
        .with_border_color(Color::Blue);
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    assert_eq!(context.theme.canvas, Color::Rgb(7, 17, 22));
    assert_eq!(context.theme.surface, Color::Rgb(13, 27, 34));
    assert_eq!(context.theme.alternate_row, Color::Rgb(22, 40, 50));
    assert_eq!(context.theme.raised, Color::Rgb(25, 44, 54));
    assert_eq!(context.theme.text, Color::Rgb(230, 241, 244));
    assert_eq!(context.theme.muted, Color::Rgb(142, 167, 176));
    assert_eq!(context.theme.accent, Color::Magenta);
    assert_eq!(context.theme.border, Color::Blue);
    assert_eq!(theme.title_style().fg, Some(theme.foreground));
    assert!(theme.title_style().add_modifier.contains(Modifier::BOLD));
}

#[test]
fn borderless_surface_reserves_title_before_content_and_clips_wide_text() {
    let context = RenderContext::default();
    for height in [0, 1, 3] {
        let area = Rect::new(2, 1, 5, height);
        let surface = Surface::new().titled("中文标题");
        let inner = surface.inner(area);
        assert_eq!(inner.height, height.saturating_sub(1));
        if height == 0 {
            continue;
        }
        let mut terminal = Terminal::new(TestBackend::new(10, 6)).unwrap();
        terminal
            .draw(|frame| {
                surface.render_frame(frame, area, &context);
                frame.render_widget(Paragraph::new("body"), inner);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(area.x, area.y)].symbol(), "中");
        assert_eq!(buffer[(area.x + 2, area.y)].symbol(), "文");
        assert_eq!(buffer[(area.x, area.y)].fg, context.theme.text);
        if height > 1 {
            assert!(text(buffer, inner).starts_with("body"));
        }
    }
}

#[test]
fn scrolling_a_borderless_group_does_not_repeat_its_clipped_title() {
    let context = RenderContext::default();
    let viewport = Rect::new(1, 2, 16, 3);
    let mut terminal = Terminal::new(TestBackend::new(20, 7)).unwrap();
    terminal
        .draw(|frame| {
            Surface::new()
                .titled("Heading")
                .render_scrolled_frame(frame, viewport, 1, 5, &context);
            frame.render_widget(Paragraph::new("continued"), Rect::new(1, 2, 16, 1));
        })
        .unwrap();
    let rendered = text(terminal.backend().buffer(), viewport);
    assert!(rendered.starts_with("continued"));
    assert!(!rendered.contains("Heading"));
}

#[test]
fn borderless_rich_button_shares_capture_hit_and_content_style() {
    let area = Rect::new(2, 1, 10, 2);
    let bounds = Rect::new(0, 0, 16, 5);
    let mut theme = TundraTheme::default();
    let buttons = ButtonFrame::new(None, None, &theme);
    theme.buttons = Some(buttons.clone());
    let mut button = Button::new("app", "App")
        .with_surface_border(false)
        .with_current(true);
    button.set_focused(true);
    let style = button.surface_style(area, &theme);
    assert!(buttons.regions().is_empty());
    let capture = FrameCapture::begin(bounds, "test");
    capture.set_group("page", "page", bounds, false);
    let mut buffer = Buffer::empty(bounds);
    button.render_surface(area, &mut buffer, &theme);
    assert_eq!(buffer[(area.x, area.y)].symbol(), " ");
    assert_eq!(buffer[(area.x, area.y)].bg, style.bg.unwrap());
    assert!(
        !buffer[(area.x, area.y)]
            .modifier
            .contains(Modifier::UNDERLINED)
    );
    assert!(style.add_modifier.contains(Modifier::UNDERLINED));
    assert_eq!(buttons.regions().len(), 1);
    assert_eq!(buttons.regions()[0].area, area);
    let snapshot = capture.finish(&buffer);
    let node = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("/app"))
        .unwrap();
    assert_eq!(
        (node.rect.x, node.rect.y, node.rect.width, node.rect.height),
        (2, 1, 10, 2)
    );
    assert!(node.selected && node.focused);
    assert!(matches!(
        button.handle_event(
            InputEvent::mouse(ui::MouseEvent::down(11, 2, MouseButton::Left)),
            area
        ),
        ComponentEvent::FocusRequested(_)
    ));
    assert_eq!(
        button.handle_event(
            InputEvent::mouse(ui::MouseEvent::up(11, 2, MouseButton::Left)),
            area
        ),
        ComponentEvent::Activated("app".into())
    );
}

#[test]
fn navigation_keeps_current_fill_when_keyboard_focus_hides() {
    let area = Rect::new(0, 0, 18, 3);
    let mut rail = NavRail::new(
        "nav",
        vec![
            NavRailItem::new("one", "One"),
            NavRailItem::new("two", "Two"),
        ],
    );
    rail.state.focused = true;
    for keyboard in [true, false, true] {
        let theme = TundraTheme::default();
        let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut buttons = ButtonFrame::new(None, None, &theme);
        buttons.keyboard_focus_visible = keyboard;
        context.buttons = Some(buttons);
        let mut buffer = Buffer::empty(area);
        rail.render(area, &mut buffer, &context);
        assert_eq!(buffer[(0, 0)].bg, context.theme.raised);
        assert_eq!(buffer[(0, 0)].symbol(), if keyboard { ">" } else { " " });
        assert_eq!(rail.selected, Some(0));
        assert!(rail.state.focused);
    }
}

#[test]
fn sidebar_rows_are_compact_full_width_targets_with_separate_focus() {
    let area = Rect::new(1, 1, 110, 12);
    let labels = ["First", "Second"].map(str::to_owned);
    let layout = SidebarWorkspaceLayout::new(area, &labels);
    let theme = TundraTheme::default();
    let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let registry = ButtonFrame::new(None, None, &theme);
    context.buttons = Some(registry.clone());
    let mut buttons = labels
        .iter()
        .enumerate()
        .map(|(i, label)| Button::new(format!("nav-{i}"), label))
        .collect::<Vec<_>>();
    buttons[1].set_focused(true);
    let mut terminal = Terminal::new(TestBackend::new(120, 16)).unwrap();
    terminal
        .draw(|frame| layout.render_navigation(frame, &buttons, 0, &context))
        .unwrap();
    assert!(layout.navigation.iter().all(|row| row.height == 1));
    assert_eq!(layout.navigation[1].y, layout.navigation[0].y + 1);
    for (row, region) in layout.navigation.iter().zip(registry.regions()) {
        assert_eq!(*row, region.area);
    }
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(layout.navigation[0].x, layout.navigation[0].y)].bg,
        context.theme.raised
    );
    assert_eq!(
        buffer[(layout.navigation[1].x, layout.navigation[1].y)].symbol(),
        ">"
    );
}

#[test]
fn titled_borderless_list_and_table_capture_the_rows_that_receive_input() {
    let bounds = Rect::new(0, 0, 20, 8);
    let context = RenderContext::default();
    let capture = FrameCapture::begin(bounds, "test");
    capture.set_group("page", "page", bounds, false);
    let mut buffer = Buffer::empty(bounds);
    let mut list = List::new(
        "list",
        vec![
            ListItem::new("first", "First"),
            ListItem::new("second", "Second"),
        ],
    )
    .titled("Choices");
    let list_area = Rect::new(0, 0, 20, 3);
    list.render_borderless(list_area, &mut buffer, &context.compatibility_theme());
    assert_eq!(
        list.handle_event_borderless(
            InputEvent::mouse(ui::MouseEvent::down(19, 2, MouseButton::Left)),
            list_area
        ),
        ComponentEvent::Selected("list".into(), 1)
    );
    let mut table = DataTable::new("table", ["Name"], [["First"], ["Second"]]).titled("Data");
    let table_area = Rect::new(0, 4, 20, 4);
    table.render(table_area, &mut buffer, &context);
    assert_eq!(
        table.handle_event(
            InputEvent::mouse(ui::MouseEvent::down(19, 7, MouseButton::Left)),
            table_area
        ),
        ComponentEvent::Selected("table".into(), 1)
    );
    let snapshot = capture.finish(&buffer);
    for (suffix, y) in [("/list/second", 2), ("/table/row.1/select", 7)] {
        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.id.ends_with(suffix))
            .unwrap();
        assert_eq!(node.rect.y, y);
        assert_eq!(node.rect.width, 20);
    }
    assert!(text(&buffer, list_area).starts_with("Choices"));
    assert!(text(&buffer, table_area).starts_with("Data"));
}

#[test]
fn button_emphasis_and_success_stay_semantically_distinct() {
    let theme = TundraTheme::default();
    let area = Rect::new(0, 0, 12, 1);
    let mut buffer = Buffer::empty(area);
    Button::new("save", "Save")
        .with_emphasis(ButtonEmphasis::Primary)
        .render_borderless(area, &mut buffer, &theme);
    assert_eq!(buffer[(0, 0)].bg, theme.accent_color);
    assert_eq!(
        ui::components::CommandStatus::Succeeded
            .span(&theme)
            .style
            .fg,
        Some(theme.tokens().success)
    );
    let mut danger = Button::new("delete", "Delete").with_emphasis(ButtonEmphasis::Danger);
    danger.state.active = true;
    danger.render_borderless(area, &mut buffer, &theme);
    assert_eq!(buffer[(0, 0)].fg, theme.error);
}

#[test]
fn busy_indicator_uses_one_cell_eighty_millisecond_frames_and_static_reduced_motion() {
    let first = BusyIndicator::new(Duration::ZERO).symbol();
    assert_eq!(
        BusyIndicator::new(Duration::from_millis(79)).symbol(),
        first
    );
    assert_ne!(
        BusyIndicator::new(Duration::from_millis(80)).symbol(),
        first
    );
    assert_eq!(
        BusyIndicator::new(Duration::from_millis(800)).symbol(),
        first
    );
    for elapsed in [0, 80, 700] {
        assert_eq!(
            BusyIndicator::from_motion(MotionFrame::reduced(Duration::from_millis(elapsed)))
                .symbol(),
            "○"
        );
    }
    let area = Rect::new(0, 0, 4, 2);
    let mut buffer = Buffer::with_lines(["keep", "next"]);
    BusyIndicator::new(Duration::from_millis(80)).render(
        area,
        &mut buffer,
        &RenderContext::default(),
    );
    assert_eq!(text(&buffer, area), "⠙eep\nnext");
}

#[test]
fn tea_preview_at_minimum_size_keeps_single_line_actions_and_input_boundary() {
    use ui::{
        components::{Dialog, TextInput},
        style_preview::{PreviewView, UiStyleVersion, preview_layout, render_preview},
    };
    let bounds = Rect::new(0, 0, 108, 20);
    let layout = preview_layout(bounds, UiStyleVersion::Tea);
    assert!(layout.usable);
    assert_eq!(layout.replay.height, 1);
    assert_eq!(layout.inspect.height, 1);
    assert_eq!(layout.input.height, 3);
    assert!(!layout.progress.intersects(layout.replay));
    let list = List::new("items", vec![ListItem::new("first", "First")]);
    let input = TextInput::new("query");
    let replay = Button::new("replay", "Replay");
    let inspect = Button::new("inspect", "Inspect");
    let dialog = Dialog::new("dialog", "Details", "Body", Vec::new());
    let context = RenderContext::default();
    let capture = FrameCapture::begin(bounds, "preview");
    capture.set_group("page", "page", bounds, false);
    let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
    terminal
        .draw(|frame| {
            render_preview(
                frame,
                &PreviewView {
                    version: UiStyleVersion::Tea,
                    list: &list,
                    input: &input,
                    replay: &replay,
                    inspect: &inspect,
                    dialog: &dialog,
                    displayed: 0.5,
                    target: 1.0,
                    running: true,
                },
                &context,
            )
        })
        .unwrap();
    let snapshot = capture.finish(terminal.backend().buffer());
    let field = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("/query"))
        .unwrap();
    assert_eq!(field.rect.height, 3);
    assert_eq!(
        terminal.backend().buffer()[(layout.replay.x, layout.replay.y)].bg,
        context.theme.accent
    );
    let title = terminal.backend().buffer()[(layout.header.x, layout.header.y)].clone();
    assert_eq!(title.fg, context.theme.text);
    assert!(title.modifier.contains(Modifier::BOLD));
}

#[test]
fn neutral_titles_do_not_remove_accent_from_the_active_text_field_or_cursor() {
    let theme = TundraTheme::default().with_accent_color(Color::Magenta);
    let area = Rect::new(0, 0, 12, 3);
    let mut input = ui::components::TextInput::new("name");
    input.set_value("value");
    input.set_focused(true);
    let mut buffer = Buffer::empty(area);
    input.render(area, &mut buffer, &theme);
    let text_cell = buffer
        .content()
        .iter()
        .find(|cell| cell.symbol() == "v")
        .unwrap();
    let cursor = buffer
        .content()
        .iter()
        .find(|cell| cell.symbol() == "_")
        .unwrap();
    assert_eq!(text_cell.fg, theme.accent_color);
    assert_eq!(cursor.fg, theme.accent_color);
    assert!(cursor.modifier.contains(Modifier::SLOW_BLINK));
    assert_eq!(theme.title_style().fg, Some(theme.foreground));
}

#[test]
fn focused_bordered_buttons_keep_the_underline_on_text_without_underlining_the_frame() {
    let theme = TundraTheme::default();
    let area = Rect::new(0, 0, 12, 3);
    for emphasis in [
        ButtonEmphasis::Normal,
        ButtonEmphasis::Primary,
        ButtonEmphasis::Danger,
    ] {
        let mut button = Button::new("save", "Save").with_emphasis(emphasis);
        button.set_focused(true);
        for rich_content in [false, true] {
            let mut buffer = Buffer::empty(area);
            if rich_content {
                button.render_surface(area, &mut buffer, &theme);
                Paragraph::new(Line::styled("Save", button.surface_style(area, &theme)))
                    .render(Rect::new(1, 1, 10, 1), &mut buffer);
            } else {
                button.render(area, &mut buffer, &theme);
            }
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    if x == area.x || x + 1 == area.right() || y == area.y || y + 1 == area.bottom()
                    {
                        assert!(
                            !buffer[(x, y)]
                                .modifier
                                .intersects(Modifier::BOLD | Modifier::UNDERLINED),
                            "frame at ({x}, {y}): {emphasis:?}, rich={rich_content}"
                        );
                    } else if buffer[(x, y)].symbol() == " " {
                        assert!(!buffer[(x, y)].modifier.contains(Modifier::UNDERLINED));
                    }
                }
            }
            let label = buffer
                .content()
                .iter()
                .find(|cell| cell.symbol() == "S")
                .unwrap();
            assert!(label.modifier.contains(Modifier::UNDERLINED));
        }
    }
}

#[test]
fn selected_status_rows_keep_semantic_colors_while_focus_and_input_mode_change() {
    let area = Rect::new(0, 0, 18, 3);
    let theme = TundraTheme::default();
    let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let mut table = DataTable::new("states", ["State"], [["Ready"], ["Attention"], ["Failed"]])
        .show_header(false)
        .with_row_tones(vec![
            ComponentTone::Success,
            ComponentTone::Warning,
            ComponentTone::Danger,
        ])
        .with_selected_rows(vec![0, 1, 2]);
    table.state.focused = true;
    for current in 0..3 {
        table.selected = Some(current);
        for keyboard in [true, false, true] {
            let mut buttons = ButtonFrame::new(None, None, &theme);
            buttons.keyboard_focus_visible = keyboard;
            context.buttons = Some(buttons);
            let mut buffer = Buffer::empty(area);
            table.render(area, &mut buffer, &context);
            for (index, color) in [
                context.theme.success,
                context.theme.warning,
                context.theme.danger,
            ]
            .into_iter()
            .enumerate()
            {
                let cell = &buffer[(0, index as u16)];
                assert_eq!(cell.fg, color);
                assert_eq!(cell.bg, context.theme.accent_soft);
                assert_eq!(
                    cell.modifier.contains(Modifier::UNDERLINED),
                    keyboard && current == index
                );
            }
            assert_eq!(table.selected, Some(current));
        }
    }
    table.state.disabled = true;
    let mut buffer = Buffer::empty(area);
    table.render(area, &mut buffer, &context);
    for y in 0..area.height {
        assert_eq!(buffer[(0, y)].fg, Color::DarkGray);
    }
}
