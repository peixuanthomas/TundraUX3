use super::*;

fn session(size: (u16, u16)) -> ShellSession {
    let mut session =
        ShellSession::new_for_home_mode(ShellLaunchConfig::default(), size, ShellHomeMode::User);
    session.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LocalAccount,
            session_id: "choice-scroll-test".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            role: UserRole::User,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    session.enter_screen(ShellScreen::Management);
    session.pages.management.management_state.kind = Some(ManagementKind::Services);
    while session.notification_dismiss_active_modal_without_response() {}
    session
}

#[test]
fn physical_space_reaches_permissions_and_recursive_checkboxes() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    for (control, initial, changed) in [
        (FieldControl::Permissions, "000000000", "000010000"),
        (FieldControl::Checkbox, "false", "true"),
    ] {
        let mut session = session((108, 30));
        session.pages.management.management_state.kind = Some(ManagementKind::FilePermissions);
        session.pages.management.management_state.permission_cell = 4;
        session.pages.management.management_state.form = Some(ManagementEditor {
            instance: next_management_form_instance(),
            title: "Permissions".into(),
            message: String::new(),
            message_scroll: 0,
            fields: vec![ManagementField {
                id: if control == FieldControl::Checkbox {
                    "recursive"
                } else {
                    "files"
                }
                .into(),
                value: initial.into(),
                control,
                ..Default::default()
            }],
            selected: 0,
            purpose: FormPurpose::Action(ManagementAction::default(), None),
        });
        session.refresh_hit_map();
        let physical_space = crossterm_event_to_input(Event::Key(KeyEvent::new(
            KeyCode::Char(' '),
            KeyModifiers::NONE,
        )));
        let InputEvent::Key(key) = &physical_space else {
            panic!("expected key input")
        };
        assert_eq!(key.key, InputKey::Char(' '));
        assert_eq!(
            session.route_key_input(key).1,
            ShellCommand::ManagementKey(key.clone())
        );
        session.apply_input(physical_space);
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap()
                .fields[0]
                .value,
            changed
        );
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            session.apply_input(crossterm_event_to_input(Event::Key(KeyEvent {
                kind,
                ..KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)
            })));
            assert_eq!(
                session
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .unwrap()
                    .fields[0]
                    .value,
                changed
            );
        }
        session.apply_input(InputEvent::key(InputKey::Space));
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap()
                .fields[0]
                .value,
            initial
        );
        for (kind, expected) in [
            (KeyEventKind::Press, changed),
            (KeyEventKind::Repeat, changed),
            (KeyEventKind::Release, changed),
        ] {
            session.apply_input(crossterm_event_to_input(Event::Key(KeyEvent {
                kind,
                ..KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            })));
            let form = session
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap();
            assert_eq!(form.fields[0].value, expected);
            assert_eq!(form.selected, 0, "Enter toggles the box without submitting");
        }
    }
}

#[test]
fn physical_space_preserves_checkbox_modifiers_and_text_input() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    let mut session = session((108, 30));
    session.pages.management.management_state.kind = Some(ManagementKind::FilePermissions);
    session.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        title: "Permissions".into(),
        message: String::new(),
        message_scroll: 0,
        fields: vec![
            ManagementField {
                id: "files".into(),
                value: "000000000".into(),
                control: FieldControl::Permissions,
                ..Default::default()
            },
            ManagementField {
                id: "recursive".into(),
                value: "false".into(),
                control: FieldControl::Checkbox,
                ..Default::default()
            },
            ManagementField {
                id: "label".into(),
                value: "ab".into(),
                ..Default::default()
            },
        ],
        selected: 0,
        purpose: FormPurpose::Action(ManagementAction::default(), None),
    });
    session.refresh_hit_map();
    for (selected, initial) in [(0, "000000000"), (1, "false")] {
        session
            .pages
            .management
            .management_state
            .form
            .as_mut()
            .unwrap()
            .selected = selected;
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::SUPER,
            KeyModifiers::HYPER,
            KeyModifiers::META,
        ] {
            session.apply_input(crossterm_event_to_input(Event::Key(KeyEvent::new(
                KeyCode::Char(' '),
                modifiers,
            ))));
            assert_eq!(
                session
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .unwrap()
                    .fields[selected]
                    .value,
                initial
            );
        }
        session.apply_input(crossterm_event_to_input(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))));
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap()
                .fields[selected]
                .value,
            initial
        );
    }
    session
        .pages
        .management
        .management_state
        .form
        .as_mut()
        .unwrap()
        .selected = 2;
    for kind in [KeyEventKind::Press, KeyEventKind::Repeat] {
        session.apply_input(crossterm_event_to_input(Event::Key(KeyEvent {
            kind,
            ..KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)
        })));
    }
    let form = session
        .pages
        .management
        .management_state
        .form
        .as_ref()
        .unwrap();
    assert_eq!(form.fields[0].value, "000000000");
    assert_eq!(form.fields[1].value, "false");
    assert_eq!(form.fields[2].value, "ab  ");
    session.apply_input(crossterm_event_to_input(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    ))));
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .selected,
        3
    );
}

#[test]
fn permission_boxes_work_with_keyboard_mouse_and_touch() {
    for hover in [false, true] {
        let mut session = session((108, 30));
        session.pages.management.management_state.kind = Some(ManagementKind::FilePermissions);
        let field = ManagementField {
            id: "files".into(),
            value: "mmmmmmmmm".into(),
            control: FieldControl::Permissions,
            ..Default::default()
        };
        session.pages.management.management_state.form = Some(ManagementEditor {
            instance: next_management_form_instance(),
            title: "Permissions".into(),
            message: String::new(),
            message_scroll: 0,
            fields: vec![field.clone()],
            selected: 0,
            purpose: FormPurpose::Action(
                ManagementAction {
                    fields: vec![field],
                    ..Default::default()
                },
                None,
            ),
        });
        for cell in 0..9 {
            let region = ui::management_button_regions(
                session.management_main(),
                &session.to_management_view_model(),
            )
            .into_iter()
            .find(|region| region.id.as_str().ends_with(&format!(".cell.{cell}")))
            .unwrap();
            pointer_input(&mut session, (region.area.x, region.area.y), hover);
            assert_eq!(
                session
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .unwrap()
                    .fields[0]
                    .value
                    .as_bytes()[cell],
                b'1'
            );
            session.handle_management_key(&KeyInput::new(InputKey::Space));
            assert_eq!(
                session
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .unwrap()
                    .fields[0]
                    .value
                    .as_bytes()[cell],
                b'0'
            );
        }
        session.cancel_management_form();
        assert!(session.pages.management.management_state.form.is_some());
        assert!(session.notification_has_active_modal());
    }
}

#[test]
fn permissions_switched_back_still_apply_to_recursive_children() {
    let mut session = session((108, 30));
    session.pages.management.management_state.kind = Some(ManagementKind::FilePermissions);
    let field = ManagementField {
        id: "directories".into(),
        value: "111000000".into(),
        control: FieldControl::Permissions,
        ..Default::default()
    };
    session.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        title: "Permissions".into(),
        message: String::new(),
        message_scroll: 0,
        fields: vec![field.clone()],
        selected: 0,
        purpose: FormPurpose::Action(
            ManagementAction {
                fields: vec![field],
                values: BTreeMap::from([("directories_initial".into(), "111000000".into())]),
                ..Default::default()
            },
            None,
        ),
    });
    session.handle_management_key(&KeyInput::new(InputKey::Space));
    session.handle_management_key(&KeyInput::new(InputKey::Space));
    let form = session
        .pages
        .management
        .management_state
        .form
        .as_ref()
        .unwrap();
    assert_eq!(form.fields[0].value, "111000000");
    let FormPurpose::Action(action, _) = &form.purpose else {
        panic!()
    };
    assert_eq!(action.values["directories_initial"], "m11000000");
    session.cancel_management_form();
    assert!(session.notification_has_active_modal());
}

#[test]
fn searchable_account_choices_keep_the_keyboard_bound_to_the_same_form() {
    let mut session = session((108, 30));
    session.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        title: "Owner".into(),
        message: String::new(),
        message_scroll: 0,
        fields: vec![ManagementField {
            id: "owner".into(),
            value: "alice (1000)".into(),
            control: FieldControl::SearchableChoice,
            choices: vec![
                "alice (1000)".into(),
                "bob (1001)".into(),
                "root (0)".into(),
            ],
            ..Default::default()
        }],
        selected: 0,
        purpose: FormPurpose::Action(ManagementAction::default(), None),
    });
    session.open_management_choice_field(0);
    assert!(session.management_keyboard_identity().is_some());
    session.handle_management_choice_key(&KeyInput::new(InputKey::Char('b')));
    assert_eq!(session.pages.management.management_state.choice_selected, 1);
    session.handle_management_choice_key(&KeyInput::new(InputKey::Enter));
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "bob (1001)"
    );
}

#[test]
fn choice_navigation_keeps_the_selected_item_visible_in_short_windows() {
    for size in [(40, 10), (60, 18), (120, 40)] {
        let mut session = session(size);
        session.pages.management.management_state.form = Some(ManagementEditor {
            instance: next_management_form_instance(),
            title: "Select an option".into(),
            message: String::new(),
            message_scroll: 0,
            fields: vec![ManagementField {
                id: "option".into(),
                value: "Choice 16".into(),
                choices: (0..20).map(|i| format!("Choice {i}")).collect(),
                ..Default::default()
            }],
            selected: 0,
            purpose: FormPurpose::Action(ManagementAction::default(), None),
        });
        session.open_management_choice_field(0);
        let assert_selected_visible = |session: &ShellSession| {
            let layout = ui::management_layout(
                session.management_main(),
                &session.to_management_view_model(),
            );
            assert!(
                layout.choice_rows.iter().any(|(index, _)| *index
                    == session.pages.management.management_state.choice_selected),
                "{size:?}: selection {} is outside {:?}",
                session.pages.management.management_state.choice_selected,
                layout.choice_rows
            );
        };
        assert_selected_visible(&session);
        for key in [
            InputKey::End,
            InputKey::Up,
            InputKey::PageUp,
            InputKey::Home,
            InputKey::PageDown,
            InputKey::Down,
        ] {
            let before = session.pages.management.management_state.choice_selected;
            let page = ui::management_layout(
                session.management_main(),
                &session.to_management_view_model(),
            )
            .choice_rows
            .len();
            session.handle_management_choice_key(&KeyInput::new(key.clone()));
            if key == InputKey::PageDown {
                assert_eq!(
                    session.pages.management.management_state.choice_selected,
                    before.saturating_add(page).min(19)
                );
            } else if key == InputKey::PageUp {
                assert_eq!(
                    session.pages.management.management_state.choice_selected,
                    before.saturating_sub(page)
                );
            }
            assert_selected_visible(&session);
        }
        session.handle_management_choice_key(&KeyInput::new(InputKey::Escape));
        assert!(
            session
                .pages
                .management
                .management_state
                .choice_field
                .is_none()
        );
        assert!(
            session.pages.management.management_state.form.is_some(),
            "closing a field picker keeps its parent form"
        );
    }
}

#[test]
fn management_forms_block_buttons_from_the_previous_frame_registry() {
    for configuration in [false, true] {
        for choice in [false, true] {
            let mut session = session((120, 40));
            if configuration {
                session.enter_screen(ShellScreen::Editor);
            }
            let main = session.management_main();
            let background =
                ui::management_button_regions(main, &session.to_management_view_model())
                    .into_iter()
                    .find(|region| region.id.as_str().contains("control."))
                    .unwrap();
            let point = (background.area.x, background.area.y);
            // The menu has just opened, before its first draw replaces the registry.
            session.button_regions = vec![background];
            session.pages.management.management_state.form = Some(ManagementEditor {
                instance: next_management_form_instance(),
                title: "Review".into(),
                message: String::new(),
                message_scroll: 0,
                fields: vec![ManagementField {
                    id: "option".into(),
                    choices: vec!["One".into(), "Two".into()],
                    ..Default::default()
                }],
                selected: 0,
                purpose: if configuration {
                    FormPurpose::Configuration("preview".into())
                } else {
                    FormPurpose::Action(ManagementAction::default(), None)
                },
            });
            if choice {
                session.open_management_choice_field(0);
            }
            assert!(session.management_overlay_contains(point));
            assert!(session.management_button_at(point).is_none());
            assert!(
                session.button_at(point).is_none(),
                "covered buttons must not be recovered from the old registry"
            );
            let button = ui::management_button_regions(
                session.management_main(),
                &session.to_management_view_model(),
            )
            .into_iter()
            .find(|region| !region.disabled)
            .unwrap();
            assert_eq!(
                session.button_at((button.area.x, button.area.y)),
                Some(button)
            );
            assert!(
                !session.management_overlay_contains((0, 0)),
                "the form does not own shell chrome"
            );
            session.pages.management.management_state.terminal_mode = true;
            assert!(
                !session.management_overlay_contains(point),
                "hidden forms do not own the terminal"
            );
        }
    }
}

fn pointer_input(session: &mut ShellSession, point: CellPosition, hover: bool) {
    let platform = platform::native_platform();
    let events = if hover {
        vec![
            InputEvent::mouse_moved(point),
            InputEvent::mouse_down(PointerButton::Left, point),
            InputEvent::mouse_up(PointerButton::Left, point),
        ]
    } else {
        vec![
            InputEvent::mouse_down(PointerButton::Left, point),
            InputEvent::mouse_up(PointerButton::Left, point),
        ]
    };
    for event in events {
        if let Some(event) = session.prepare_input(event, platform.as_ref(), Instant::now()) {
            match event {
                InputEvent::Key(key) => session.handle_management_key(&key),
                InputEvent::Paste(text) => session.handle_management_paste(&text),
                InputEvent::Mouse(mouse) => session.handle_management_pointer(mouse),
                _ => {}
            }
        }
    }
}

fn keyboard_tap(session: &mut ShellSession, action: ui::ScreenKeyboardAction, hover: bool) {
    let area = session
        .input_keyboard_layout()
        .buttons
        .into_iter()
        .find(|button| button.action == action)
        .unwrap()
        .area;
    pointer_input(session, (area.x, area.y), hover);
}

#[test]
fn management_text_forms_and_search_accept_mouse_and_no_hover_touch_keyboard_input() {
    for hover in [false, true] {
        for size in [(108, 20), (120, 40)] {
            let mut session = session(size);
            session.pages.management.management_state.form = Some(ManagementEditor {
                instance: next_management_form_instance(),
                title: "Enter value".into(),
                message: String::new(),
                message_scroll: 0,
                fields: vec![ManagementField {
                    id: "value".into(),
                    required: true,
                    ..Default::default()
                }],
                selected: 0,
                purpose: FormPurpose::Answer("fixture".into()),
            });
            session.refresh_hit_map();
            session.sync_input_keyboard();
            let launcher = session.keyboard_launcher_area().unwrap();
            pointer_input(&mut session, (launcher.x, launcher.y), hover);
            session.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
            assert!(session.input_keyboard_open());
            let layout = ui::management_layout(
                session.management_main(),
                &session.to_management_view_model(),
            );
            assert!(!layout.submit.is_empty(), "{size:?}");
            assert!(layout.form.bottom() <= session.input_keyboard_layout().keyboard.y);
            keyboard_tap(&mut session, ui::ScreenKeyboardAction::Letter('a'), hover);
            keyboard_tap(&mut session, ui::ScreenKeyboardAction::Letter('b'), hover);
            keyboard_tap(&mut session, ui::ScreenKeyboardAction::Backspace, hover);
            assert_eq!(
                session
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .unwrap()
                    .fields[0]
                    .value,
                "a"
            );
            let submit = ui::management_layout(
                session.management_main(),
                &session.to_management_view_model(),
            )
            .submit;
            pointer_input(&mut session, (submit.x, submit.y), hover);
            assert!(session.pages.management.management_state.form.is_none());
            session.sync_input_keyboard();
            let launcher = session.keyboard_launcher_area().unwrap();
            pointer_input(&mut session, (launcher.x, launcher.y), hover);
            session.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
            keyboard_tap(&mut session, ui::ScreenKeyboardAction::Letter('x'), hover);
            assert_eq!(session.pages.management.management_state.filter_input, "x");
            assert!(session.pages.management.management_state.filtering);
            keyboard_tap(&mut session, ui::ScreenKeyboardAction::Escape, hover);
            assert!(!session.pages.management.management_state.filtering);
            session.sync_input_keyboard();
            assert!(!session.input_keyboard_open());
        }
    }
}

#[test]
fn management_keyboard_loses_old_press_on_form_choice_modal_and_page_changes() {
    let mut session = session((120, 40));
    session.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        title: "Form".into(),
        message: String::new(),
        message_scroll: 0,
        fields: vec![
            ManagementField {
                id: "value".into(),
                ..Default::default()
            },
            ManagementField {
                id: "choice".into(),
                value: "One".into(),
                choices: vec!["One".into(), "Two".into()],
                ..Default::default()
            },
        ],
        selected: 0,
        purpose: FormPurpose::Answer("fixture".into()),
    });
    session.toggle_input_keyboard();
    session.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    let key = session
        .input_keyboard_layout()
        .buttons
        .into_iter()
        .find(|button| button.action == ui::ScreenKeyboardAction::Letter('a'))
        .unwrap()
        .area;
    let platform = platform::native_platform();
    session.prepare_input(
        InputEvent::mouse_down(PointerButton::Left, (key.x, key.y)),
        platform.as_ref(),
        Instant::now(),
    );
    assert!(session.screen_keyboard.pressed.is_some());
    let mut replacement = session
        .pages
        .management
        .management_state
        .form
        .clone()
        .unwrap();
    replacement.instance = next_management_form_instance();
    session.pages.management.management_state.form = Some(replacement);
    session.sync_input_keyboard();
    assert!(!session.input_keyboard_open());
    assert!(session.screen_keyboard.pressed.is_none());
    session.toggle_input_keyboard();
    session.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    session.open_management_choice_field(1);
    session.sync_input_keyboard();
    assert!(!session.input_keyboard_open());
    assert!(session.screen_keyboard.pressed.is_none());
    session.pages.management.management_state.choice_field = None;
    session.toggle_input_keyboard();
    session.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    session.enter_screen(ShellScreen::Home);
    session.sync_input_keyboard();
    assert!(!session.input_keyboard_open());
    assert!(session.keyboard_launcher_area().is_none());
}

#[test]
fn editor_configuration_text_form_uses_the_same_keyboard_and_bounded_layout() {
    let mut session = session((108, 20));
    session.enter_screen(ShellScreen::Editor);
    session.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        title: "Configuration".into(),
        message: String::new(),
        message_scroll: 0,
        fields: vec![ManagementField {
            id: "value".into(),
            ..Default::default()
        }],
        selected: 0,
        purpose: FormPurpose::Configuration("edit-find".into()),
    });
    session.refresh_hit_map();
    session.sync_input_keyboard();
    let launcher = session.keyboard_launcher_area().unwrap();
    pointer_input(&mut session, (launcher.x, launcher.y), false);
    session.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    assert!(session.input_keyboard_open());
    keyboard_tap(&mut session, ui::ScreenKeyboardAction::Letter('a'), false);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "a"
    );
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    assert!(!layout.submit.is_empty());
    assert!(layout.form.bottom() <= session.input_keyboard_layout().keyboard.y);
}
