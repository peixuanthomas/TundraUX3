use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::widgets::BorderType;
use ui::components::{
    Button, CommandPalette, CommandPaletteCommand, ComponentEvent, ComponentId, ContextMenu,
    ContextMenuItem, Dialog, DialogAction, InputEvent, Key, List, ListItem, MouseButton, TabItem,
    Tabs, TextInput,
};
use ui::{BorderShape, InputPhase, KeyModifiers, MouseEvent, TundraTheme};

#[test]
fn button_keyboard_and_mouse_activate_the_same_component() {
    let mut button = Button::new("save", "Save");
    let area = Rect::new(0, 0, 12, 3);

    button.set_focused(true);
    assert_eq!(
        button.handle_event(InputEvent::key(Key::Enter), area),
        ComponentEvent::Activated(ComponentId::new("save"))
    );

    assert_eq!(
        button.handle_event(
            InputEvent::mouse(MouseEvent::down(2, 1, MouseButton::Left)),
            area
        ),
        ComponentEvent::FocusRequested(ComponentId::new("save"))
    );
    assert!(button.state.active);

    assert_eq!(
        button.handle_event(
            InputEvent::mouse(MouseEvent::up(2, 1, MouseButton::Left)),
            area
        ),
        ComponentEvent::Activated(ComponentId::new("save"))
    );
    assert!(!button.state.active);

    let mut buffer = Buffer::empty(area);
    button.render(area, &mut buffer, &TundraTheme::default());
    assert_regular_weight_vertical_border(&buffer, area);
}

#[test]
fn theme_defaults_to_rounded_borders_and_square_uses_ratatui_plain() {
    let area = Rect::new(0, 0, 12, 3);
    let button = Button::new("shape", "Shape");

    let rounded = TundraTheme::default();
    assert_eq!(rounded.border_shape, BorderShape::Rounded);
    assert_eq!(rounded.border_type(), BorderType::Rounded);
    let mut rounded_buffer = Buffer::empty(area);
    button.render(area, &mut rounded_buffer, &rounded);
    assert_eq!(rounded_buffer.cell((0, 0)).unwrap().symbol(), "╭");

    let square = rounded.with_border_shape(BorderShape::Square);
    assert_eq!(square.border_type(), BorderType::Plain);
    let mut square_buffer = Buffer::empty(area);
    button.render(area, &mut square_buffer, &square);
    assert_eq!(square_buffer.cell((0, 0)).unwrap().symbol(), "┌");
}

#[test]
fn themed_borders_keep_the_configured_color_when_controls_are_focused() {
    let area = Rect::new(0, 0, 16, 3);
    let theme = TundraTheme::default().with_border_color(Color::LightGreen);

    let mut button = Button::new("save", "Save");
    button.set_focused(true);
    let mut button_buffer = Buffer::empty(area);
    button.render(area, &mut button_buffer, &theme);
    assert_eq!(button_buffer.cell((0, 0)).unwrap().fg, Color::LightGreen);

    let mut input = TextInput::new("query");
    input.set_focused(true);
    let mut input_buffer = Buffer::empty(area);
    input.render(area, &mut input_buffer, &theme);
    assert_eq!(input_buffer.cell((0, 0)).unwrap().fg, Color::LightGreen);
}

#[test]
fn selected_controls_use_accent_borders_without_changing_regular_border_color() {
    let area = Rect::new(0, 0, 16, 3);
    let theme = TundraTheme::default()
        .with_border_color(Color::LightGreen)
        .with_accent_color(Color::LightMagenta);

    let mut button = Button::new("save", "Save");
    button.state.selected = true;
    let mut button_buffer = Buffer::empty(area);
    button.render(area, &mut button_buffer, &theme);
    assert_eq!(button_buffer.cell((0, 0)).unwrap().fg, Color::LightMagenta);

    let mut input = TextInput::new("query");
    input.state.selected = true;
    let mut input_buffer = Buffer::empty(area);
    input.render(area, &mut input_buffer, &theme);
    assert_eq!(input_buffer.cell((0, 0)).unwrap().fg, Color::LightMagenta);
}
#[test]
fn list_supports_keyboard_selection_mouse_selection_and_activation() {
    let mut list = List::new(
        "files",
        vec![
            ListItem::new("a", "Alpha"),
            ListItem::new("b", "Beta"),
            ListItem::new("c", "Gamma"),
        ],
    )
    .titled("Files");
    let area = Rect::new(0, 0, 20, 5);

    list.set_focused(true);
    assert_eq!(
        list.handle_event(InputEvent::key(Key::Down), area),
        ComponentEvent::Selected(ComponentId::new("files"), 1)
    );
    assert_eq!(list.selected_index(), Some(1));

    assert_eq!(
        list.handle_event(
            InputEvent::mouse(MouseEvent::click(1, 3, MouseButton::Left)),
            area
        ),
        ComponentEvent::Selected(ComponentId::new("files"), 2)
    );
    assert_eq!(list.selected_index(), Some(2));

    assert_eq!(
        list.handle_event(
            InputEvent::mouse(MouseEvent::double_click(1, 3, MouseButton::Left)),
            area
        ),
        ComponentEvent::Activated(ComponentId::new("c"))
    );

    let mut buffer = Buffer::empty(area);
    list.render(area, &mut buffer, &TundraTheme::default());
}

#[test]
fn text_input_edits_text_and_maps_mouse_clicks_to_cursor_positions() {
    let mut input = TextInput::new("query").with_placeholder("Search");
    let area = Rect::new(0, 0, 16, 3);

    input.set_focused(true);
    assert_eq!(
        input.handle_event(InputEvent::key(Key::Char('a')), area),
        ComponentEvent::Changed(ComponentId::new("query"))
    );
    input.handle_event(InputEvent::key(Key::Char('b')), area);
    input.handle_event(InputEvent::key(Key::Left), area);
    input.handle_event(InputEvent::key(Key::Char('x')), area);
    assert_eq!(input.value(), "axb");

    input.handle_event(InputEvent::key(Key::Backspace), area);
    assert_eq!(input.value(), "ab");

    assert_eq!(
        input.handle_event(
            InputEvent::mouse(MouseEvent::click(2, 1, MouseButton::Left)),
            area
        ),
        ComponentEvent::FocusRequested(ComponentId::new("query"))
    );
    assert_eq!(input.cursor(), 1);

    assert_eq!(
        input.handle_event(InputEvent::key(Key::Enter), area),
        ComponentEvent::Activated(ComponentId::new("query"))
    );

    let mut buffer = Buffer::empty(area);
    input.render(area, &mut buffer, &TundraTheme::default());
    assert_regular_weight_vertical_border(&buffer, area);
}

fn assert_regular_weight_vertical_border(buffer: &Buffer, area: Rect) {
    let right = area.right().saturating_sub(1);
    for y in area.y.saturating_add(1)..area.bottom().saturating_sub(1) {
        for x in [area.x, right] {
            let cell = buffer.cell((x, y)).expect("vertical border cell");
            assert_eq!(cell.symbol(), "│", "border at ({x}, {y}) must stay solid");
            assert!(
                !cell.modifier.contains(Modifier::BOLD),
                "border at ({x}, {y}) must use regular weight"
            );
        }
    }
}

#[test]
fn dialog_is_modal_and_activates_selected_actions() {
    let mut dialog = Dialog::new(
        "confirm",
        "Confirm",
        "Apply changes?",
        vec![
            DialogAction::new("ok", "OK"),
            DialogAction::new("cancel", "Cancel"),
        ],
    );
    let area = Rect::new(2, 2, 28, 7);

    dialog.open();
    assert_eq!(
        dialog.handle_event(
            InputEvent::mouse(MouseEvent::click(0, 0, MouseButton::Left)),
            area
        ),
        ComponentEvent::Consumed
    );
    assert!(dialog.open);

    assert_eq!(
        dialog.handle_event(InputEvent::key(Key::Tab), area),
        ComponentEvent::Selected(ComponentId::new("confirm"), 1)
    );
    assert_eq!(dialog.selected_action_index(), Some(1));
    assert_eq!(
        dialog.handle_event(InputEvent::key(Key::Enter), area),
        ComponentEvent::Activated(ComponentId::new("cancel"))
    );

    let mut buffer = Buffer::empty(Rect::new(0, 0, 32, 10));
    dialog.render(area, &mut buffer, &TundraTheme::default());
}

#[test]
fn context_menu_activates_items_and_dismisses_on_outside_click() {
    let mut menu = ContextMenu::new(
        "menu",
        vec![
            ContextMenuItem::new("open", "Open"),
            ContextMenuItem::new("rename", "Rename"),
        ],
    );
    let area = Rect::new(5, 5, 12, 4);

    menu.open();
    assert_eq!(
        menu.handle_event(InputEvent::key(Key::Down), area),
        ComponentEvent::Selected(ComponentId::new("menu"), 1)
    );
    assert_eq!(
        menu.handle_event(InputEvent::key(Key::Enter), area),
        ComponentEvent::Activated(ComponentId::new("rename"))
    );
    assert!(!menu.open);

    menu.open();
    assert_eq!(
        menu.handle_event(
            InputEvent::mouse(MouseEvent::click(0, 0, MouseButton::Left)),
            area
        ),
        ComponentEvent::Dismissed(ComponentId::new("menu"))
    );
    assert!(!menu.open);

    let render_area = menu.preferred_area(5, 5, Rect::new(0, 0, 80, 24));
    menu.open();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 24));
    menu.render(render_area, &mut buffer, &TundraTheme::default());
}

#[test]
fn tabs_support_keyboard_and_mouse_selection() {
    let mut tabs = Tabs::new(
        "sections",
        vec![
            TabItem::new("home", "Home"),
            TabItem::new("files", "Files"),
            TabItem::new("settings", "Settings"),
        ],
    );
    let area = Rect::new(0, 0, 32, 3);

    tabs.set_focused(true);
    assert_eq!(
        tabs.handle_event(InputEvent::key(Key::Right), area),
        ComponentEvent::Selected(ComponentId::new("sections"), 1)
    );
    assert_eq!(tabs.selected_index(), Some(1));

    assert_eq!(
        tabs.handle_event(
            InputEvent::mouse(MouseEvent::click(14, 1, MouseButton::Left)),
            area
        ),
        ComponentEvent::Selected(ComponentId::new("sections"), 2)
    );
    assert_eq!(tabs.selected_index(), Some(2));

    let mut buffer = Buffer::empty(area);
    tabs.render(area, &mut buffer, &TundraTheme::default());
}

#[test]
fn command_palette_filters_query_and_activates_selected_command() {
    let mut palette = CommandPalette::new(
        "commands",
        vec![
            CommandPaletteCommand::new("open", "Open File").with_keywords(["file", "explorer"]),
            CommandPaletteCommand::new("settings", "Open Settings")
                .with_hint("Configure TundraUX")
                .with_keywords(["preferences"]),
        ],
    );
    let area = Rect::new(1, 1, 40, 8);

    palette.open();
    palette.handle_event(InputEvent::key(Key::Char('s')), area);
    palette.handle_event(InputEvent::key(Key::Char('e')), area);
    palette.handle_event(InputEvent::key(Key::Char('t')), area);

    assert_eq!(palette.query(), "set");
    assert_eq!(
        palette.selected_command().map(|command| command.id.clone()),
        Some(ComponentId::new("settings"))
    );
    assert_eq!(
        palette.handle_event(InputEvent::key(Key::Enter), area),
        ComponentEvent::Activated(ComponentId::new("settings"))
    );
    assert!(!palette.open);

    palette.open();
    assert_eq!(
        palette.handle_event(
            InputEvent::mouse(MouseEvent::click(0, 0, MouseButton::Left)),
            area
        ),
        ComponentEvent::Dismissed(ComponentId::new("commands"))
    );

    palette.open();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 44, 10));
    palette.render(area, &mut buffer, &TundraTheme::default());
}

#[test]
fn components_ignore_key_release_events() {
    let mut button = Button::new("save", "Save");
    button.set_focused(true);

    assert_eq!(
        button.handle_event(
            InputEvent::key_with_phase(Key::Enter, KeyModifiers::NONE, InputPhase::Release),
            Rect::new(0, 0, 12, 3),
        ),
        ComponentEvent::None
    );
}
