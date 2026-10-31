use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::components::{
    FormAction, FormButton, FormControl, FormDialog, FormEvent, FormField, InputEvent, Key,
    MouseButton,
};
use ui::{InputPhase, KeyInput, KeyModifiers, RenderContext};

fn example() -> FormDialog {
    FormDialog::new(
        "example",
        "Test",
        vec![
            FormField::text("name", "名称", ""),
            FormField::text("command", "启动命令", ""),
            FormField::toggle("keep", "退出后保留界面", false),
        ],
    )
}

#[test]
fn enter_advances_each_field_then_submits_without_changing_controls() {
    let area = Rect::new(0, 0, 80, 25);
    let mut form = example();
    for index in 1..3 {
        assert_eq!(
            form.handle_event(InputEvent::key(Key::Enter), area),
            FormEvent::Changed
        );
        assert_eq!(form.selected, index);
        let mut repeated = KeyInput::new(Key::Enter);
        repeated.phase = InputPhase::Repeat;
        assert_eq!(
            form.handle_event(InputEvent::Key(repeated), area),
            FormEvent::None
        );
        assert_eq!(form.selected, index);
    }
    assert_eq!(form.fields[2].value, "false");
    form.handle_event(InputEvent::key(Key::Space), area);
    assert_eq!(form.fields[2].value, "true");
    assert_eq!(
        form.handle_event(InputEvent::key(Key::Enter), area),
        FormEvent::Action(FormAction::Submit)
    );
    form.selected = 0;
    assert_eq!(
        form.handle_event(
            InputEvent::Key(KeyInput::with_modifiers(Key::Enter, KeyModifiers::CTRL)),
            area
        ),
        FormEvent::Action(FormAction::Submit)
    );
    for modifiers in [
        KeyModifiers {
            control: true,
            ..KeyModifiers::NONE
        },
        KeyModifiers {
            ctrl: true,
            ..KeyModifiers::NONE
        },
    ] {
        assert_eq!(
            form.handle_event(
                InputEvent::Key(KeyInput::with_modifiers(Key::Enter, modifiers)),
                area
            ),
            FormEvent::Action(FormAction::Submit)
        );
    }
}

#[test]
fn shared_text_input_keeps_unicode_selection_spaces_and_paste() {
    let area = Rect::new(0, 0, 80, 25);
    let mut form = example();
    form.handle_event(InputEvent::Paste("中文".into()), area);
    form.handle_event(InputEvent::key(Key::Space), area);
    form.handle_event(InputEvent::Paste("cmd".into()), area);
    assert_eq!(form.fields[0].value, "中文 cmd");
    form.handle_event(
        InputEvent::Key(KeyInput::with_modifiers(Key::Char('a'), KeyModifiers::CTRL)),
        area,
    );
    form.handle_event(InputEvent::Paste("新名字".into()), area);
    assert_eq!(form.fields[0].value, "新名字");
}

#[test]
fn configured_actions_and_disabled_submit_use_the_same_input_path() {
    let area = Rect::new(0, 0, 80, 25);
    let mut form = example();
    let mut submit = FormButton::new("save", "Save", FormAction::Submit);
    submit.disabled = true;
    form.buttons = vec![
        FormButton::new("reset", "Reset", FormAction::Custom("reset".into())),
        submit,
        FormButton::new("cancel", "Cancel", FormAction::Cancel),
    ];
    form.selected = 2;
    assert_eq!(
        form.handle_event(InputEvent::key(Key::Enter), area),
        FormEvent::None
    );
    form.selected = 3;
    assert_eq!(
        form.handle_event(InputEvent::key(Key::Enter), area),
        FormEvent::Action(FormAction::Custom("reset".into()))
    );
    assert_eq!(
        form.handle_event(InputEvent::key(Key::Escape), area),
        FormEvent::Action(FormAction::Cancel)
    );
}

#[test]
fn mouse_and_touch_activate_only_matching_presses_and_releases() {
    let area = Rect::new(0, 0, 80, 25);
    let mut form = example();
    let field = form
        .layout(area)
        .fields
        .iter()
        .find(|(index, _)| *index == 2)
        .unwrap()
        .1;
    let point = (field.x, field.y);
    let down = InputEvent::mouse_down(MouseButton::Left, point);
    let up = InputEvent::mouse_up(MouseButton::Left, point);
    form.handle_event(up.clone(), area);
    assert_eq!(form.fields[2].value, "false");
    form.handle_event(down.clone(), area);
    assert_eq!(form.fields[2].value, "false");
    form.handle_event(up.clone(), area);
    assert_eq!(form.fields[2].value, "true");
    form.handle_event(down, area);
    form.handle_event(InputEvent::FocusLost, area);
    form.handle_event(up, area);
    assert_eq!(form.fields[2].value, "true");
}

#[test]
fn moving_out_resizing_or_changing_the_form_cancels_the_pending_click() {
    let area = Rect::new(0, 0, 80, 25);
    for interruption in 0..3 {
        let mut form = example();
        let field = form
            .layout(area)
            .fields
            .iter()
            .find(|(index, _)| *index == 2)
            .unwrap()
            .1;
        let point = (field.x, field.y);
        form.handle_event(InputEvent::mouse_down(MouseButton::Left, point), area);
        match interruption {
            0 => {
                form.handle_event(InputEvent::mouse_moved((0, 0)), area);
            }
            1 => {
                form.handle_event(
                    InputEvent::Resize {
                        width: 70,
                        height: 20,
                    },
                    area,
                );
            }
            _ => {
                form.fields[2].id = "replacement".into();
            }
        }
        form.handle_event(InputEvent::mouse_up(MouseButton::Left, point), area);
        assert_eq!(form.fields[2].value, "false");
    }
}

#[test]
fn choosing_an_option_returns_to_the_form_before_enter_moves_on() {
    let area = Rect::new(0, 0, 40, 15);
    let mut field = FormField::text("role", "Role", "User");
    field.choices = vec!["User".into(), "Administrator".into()];
    let mut form = FormDialog::new(
        "choice",
        "Choice",
        vec![field, FormField::text("name", "Name", "")],
    );
    form.handle_event(InputEvent::key(Key::Space), area);
    form.handle_event(InputEvent::Key(KeyInput::new(Key::Down).repeated()), area);
    assert_eq!(form.choice.as_ref().unwrap().selected, 1);
    form.handle_event(InputEvent::Paste("admin".into()), area);
    assert_eq!(form.choice.as_ref().unwrap().values, ["Administrator"]);
    assert_eq!(
        form.handle_event(InputEvent::key(Key::Enter), area),
        FormEvent::Changed
    );
    assert_eq!(form.fields[0].value, "Administrator");
    assert!(form.choice.is_none());
    assert_eq!(form.selected, 0);
    form.handle_event(InputEvent::key(Key::Enter), area);
    assert_eq!(form.selected, 1);
    assert_eq!(
        form.handle_event(InputEvent::key(Key::Enter), area),
        FormEvent::Action(FormAction::Submit)
    );
}

#[test]
fn numeric_buttons_clamp_values_and_long_forms_keep_the_focused_field_visible() {
    let area = Rect::new(0, 0, 40, 15);
    let mut form = FormDialog::new(
        "numbers",
        "Numbers",
        (0..12)
            .map(|index| {
                let mut field = FormField::text(index.to_string(), "Long field label", "9");
                field.control = FormControl::Number {
                    minimum: 0,
                    maximum: 10,
                    step: 5,
                };
                field
            })
            .collect(),
    );
    form.handle_event(InputEvent::key(Key::Char('+')), area);
    assert_eq!(form.fields[0].value, "10");
    for index in 1..12 {
        form.handle_event(InputEvent::key(Key::Enter), area);
        assert_eq!(form.selected, index);
        assert!(
            form.layout(area)
                .fields
                .iter()
                .any(|(field, rect)| *field == index && !rect.is_empty())
        );
    }
    assert!(
        form.layout(area)
            .scrollbars
            .iter()
            .any(|bar| bar.target == ui::ManagementScrollTarget::FormFields)
    );
}

#[test]
fn viewport_and_drag_state_survive_rebuilding_the_dialog() {
    let area = Rect::new(0, 0, 40, 15);
    let fields = (0..16)
        .map(|index| FormField::text(index.to_string(), "Field", "value"))
        .collect::<Vec<_>>();
    let mut form = FormDialog::new("long", "Long", fields.clone());
    let bar = form
        .layout(area)
        .scrollbars
        .into_iter()
        .find(|bar| bar.target == ui::ManagementScrollTarget::FormFields)
        .unwrap();
    form.handle_event(
        InputEvent::mouse_down(MouseButton::Left, (bar.track.x - 1, bar.track.y)),
        area,
    );
    let mut form =
        FormDialog::new("long", "Long", fields.clone()).with_interaction(form.interaction_state());
    form.handle_event(
        InputEvent::mouse_drag(MouseButton::Left, (bar.track.x, bar.track.bottom() - 1)),
        area,
    );
    assert!(form.layout(area).field_start > 0);
    let offset = form.layout(area).field_start;
    let mut state = form.interaction_state();
    state.cancel_pointer();
    let form = FormDialog::new("long", "Long", fields).with_interaction(state);
    assert_eq!(form.layout(area).field_start, offset);
    assert!(!form.interaction.is_dragging());
}

#[test]
fn secrets_are_masked_by_the_shared_renderer() {
    let mut form = example();
    form.fields[0].secret = true;
    form.fields[0].value = "secret密碼".into();
    let mut terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    terminal
        .draw(|frame| form.render_frame(frame, frame.area(), &RenderContext::default()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!text.contains("secret"));
    assert!(text.contains("••••••••"));
}
