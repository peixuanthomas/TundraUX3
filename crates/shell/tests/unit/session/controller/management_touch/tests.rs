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
    session.screen_stack.push(ShellScreen::Management);
    session.management_state.kind = Some(ManagementKind::Services);
    while session.notification_dismiss_active_modal_without_response() {}
    session
}

#[test]
fn choice_navigation_keeps_the_selected_item_visible_in_short_windows() {
    for size in [(40, 10), (60, 18), (120, 40)] {
        let mut session = session(size);
        session.management_state.form = Some(ManagementEditor {
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
                layout
                    .choice_rows
                    .iter()
                    .any(|(index, _)| *index == session.management_state.choice_selected),
                "{size:?}: selection {} is outside {:?}",
                session.management_state.choice_selected,
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
            let before = session.management_state.choice_selected;
            let page = ui::management_layout(
                session.management_main(),
                &session.to_management_view_model(),
            )
            .choice_rows
            .len();
            session.handle_management_choice_key(&KeyInput::new(key.clone()));
            if key == InputKey::PageDown {
                assert_eq!(
                    session.management_state.choice_selected,
                    before.saturating_add(page).min(19)
                );
            } else if key == InputKey::PageUp {
                assert_eq!(
                    session.management_state.choice_selected,
                    before.saturating_sub(page)
                );
            }
            assert_selected_visible(&session);
        }
        session.handle_management_choice_key(&KeyInput::new(InputKey::Escape));
        assert!(session.management_state.choice_field.is_none());
        assert!(
            session.management_state.form.is_some(),
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
                session.screen_stack.push(ShellScreen::Editor);
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
            session.management_state.form = Some(ManagementEditor {
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
            let button = ui::management_button_regions(main, &session.to_management_view_model())
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
            session.management_state.terminal_mode = true;
            assert!(
                !session.management_overlay_contains(point),
                "hidden forms do not own the terminal"
            );
        }
    }
}
