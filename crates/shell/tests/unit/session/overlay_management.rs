use super::*;

fn state() -> ShellSession {
    ShellSession::new_for_home_mode(ShellLaunchConfig::default(), (120, 40), ShellHomeMode::User)
}

fn notice(state: &mut ShellSession) {
    state.notify_modal(
        "Notice",
        "Close this dialog",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("close", "Close").cancel()],
    );
}

#[test]
fn nested_aa_notification_and_page_dialog_restore_each_focus_without_navigation() {
    let mut state = state();
    state.enter_screen(ShellScreen::Clock);
    state.refresh_hit_map();
    state.clock_create_state = Some(ClockCreateState::default());
    state.refresh_hit_map();
    state.focus_component(ShellComponent::ClockCreateCountdownButton);
    notice(&mut state);
    let (tx, _rx) = mpsc::channel();
    state
        .begin_auto_admin("Example operation".into(), true, tx)
        .unwrap();
    assert!(state.auto_admin_visible());
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert!(!state.auto_admin_visible());
    assert!(state.notification_has_active_modal());
    assert_eq!(state.focused_component, ShellComponent::NotificationDialog);
    let mut release = KeyInput::new(InputKey::Escape);
    release.phase = InputPhase::Release;
    state.apply_input(InputEvent::Key(release));
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert!(!state.notification_has_active_modal());
    assert_eq!(
        state.focused_component,
        ShellComponent::ClockCreateCountdownButton
    );
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert!(state.clock_create_state.is_none());
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(state.focused_component, ShellComponent::ClockNewButton);
}

#[test]
fn context_popup_restores_origin_and_replacement_notice_does_not_resurrect_it() {
    for with_notice in [false, true] {
        let mut state = state();
        state.focus_component(ShellComponent::StatusBar);
        state.capture_modal_focus_context();
        state.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Home),
            anchor: (12, 8),
        });
        state.refresh_hit_map();
        assert_eq!(state.focused_component, ShellComponent::ContextMenu);
        if with_notice {
            notice(&mut state);
        }
        state.apply_input(InputEvent::key(InputKey::Escape));
        assert!(state.active_popup.is_none());
        assert!(!state.notification_has_active_modal());
        assert_eq!(state.focused_component, ShellComponent::StatusBar);
        assert_eq!(state.active_screen(), ShellScreen::Home);
    }
}

#[test]
fn covered_pages_do_not_claim_input_or_focus() {
    let mut state = state();
    state.enter_screen(ShellScreen::Editor);
    state.editor_open_menu = Some(ui::EditorMenu::File);
    state.refresh_hit_map();
    state.enter_screen(ShellScreen::Home);
    state.refresh_hit_map();
    assert!(state.interactive_overlays().is_empty());
    assert_eq!(state.focused_component, ShellComponent::Home);
    state.apply_input(InputEvent::key(InputKey::Tab));
    assert_ne!(state.focused_component, ShellComponent::Editor);
}

#[test]
fn page_change_closes_the_previous_context_popup() {
    let mut state = state();
    state.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Home),
        anchor: (12, 8),
    });
    state.refresh_hit_map();
    state.enter_screen(ShellScreen::Clock);
    state.refresh_hit_map();
    assert!(state.active_popup.is_none());
    assert!(state.interactive_overlays().is_empty());
    assert_eq!(state.focused_component, ShellComponent::ClockNewButton);
}

#[test]
fn editor_menu_and_notification_capture_paste_without_mutating_document() {
    let mut state = state();
    state.enter_screen(ShellScreen::Editor);
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(EditorState::new())),
        Instant::now(),
    );
    state.editor_open_menu = Some(ui::EditorMenu::File);
    state.refresh_hit_map();
    for with_notice in [false, true] {
        if with_notice {
            notice(&mut state);
        }
        let before = state.app.editor_state().cloned();
        state.apply_input(InputEvent::Paste("must not enter the document".into()));
        assert_eq!(state.app.editor_state().cloned(), before);
    }
}

#[test]
fn editor_form_receives_paste_without_changing_the_document() {
    let mut state = state();
    state.enter_screen(ShellScreen::Editor);
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(EditorState::new())),
        Instant::now(),
    );
    state.open_editor_find();
    state.refresh_hit_map();
    let before = state.app.editor_state().cloned();
    state.apply_input(InputEvent::Paste("search text".into()));
    assert_eq!(
        state.to_management_view_model().form.unwrap().fields[0].value,
        "search text"
    );
    assert_eq!(state.app.editor_state().cloned(), before);
}

#[test]
fn modal_captures_global_shortcuts_and_cancels_background_pointer_state() {
    let mut state = state();
    state.scrollbar_drag = Some(ScrollbarDragState::Home { grab_offset: 0 });
    state.editor_drag_anchor = Some(app::editor::EditorPosition::Source(0));
    state.drag_tracker = Some(DragTracker {
        button: PointerButton::Left,
        origin_coordinates: (4, 8),
        last_coordinates: (4, 9),
    });
    state.button_regions.push(ui::components::ButtonRegion {
        id: "covered-page-button".into(),
        area: Rect::new(4, 8, 10, 3),
        disabled: false,
    });
    notice(&mut state);
    assert!(state.scrollbar_drag.is_none());
    assert!(state.editor_drag_anchor.is_none());
    assert!(state.drag_tracker.is_none());
    assert!(state.button_at((5, 9)).is_none());
    state.apply_input(InputEvent::from_key_label("Ctrl+C"));
    assert!(!state.shutdown_requested);
    assert!(state.notification_has_active_modal());
}
