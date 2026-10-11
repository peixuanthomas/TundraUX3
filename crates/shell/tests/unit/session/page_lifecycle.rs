use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

fn state() -> ShellSession {
    ShellSession::new_for_home_mode(ShellLaunchConfig::default(), (120, 40), ShellHomeMode::User)
}

#[test]
fn staying_in_explorer_preserves_its_input_purpose_model_and_alert() {
    let mut document = EditorState::new();
    let snapshot = match document
        .apply(app::editor::EditorCommand::RequestSave)
        .remove(0)
    {
        app::editor::EditorEffect::SaveFilePicker { snapshot, .. } => snapshot,
        _ => panic!("an untitled document should request a save picker"),
    };
    for purpose in [
        ExplorerPurpose::DiagnosticsLogs,
        ExplorerPurpose::EditorOpen,
        ExplorerPurpose::EditorSaveAs { snapshot },
    ] {
        let save_picker = matches!(purpose, ExplorerPurpose::EditorSaveAs { .. });
        let editor_picker = !matches!(purpose, ExplorerPurpose::DiagnosticsLogs);
        let mut state = state();
        state.open_editor();
        let owner = state.active_page_id();
        state.pages.editor.editor_close_after_save = true;
        state.pages.editor.editor_open_after_save = true;
        state.pages.editor.editor_discard_for_open = true;
        state.enter_new_page(ShellScreen::Explorer);
        let picker = state.active_page_id();
        state.replace_explorer_state(Some(ExplorerState::new(".", false)));
        state.pages.explorer.explorer_purpose = purpose;
        state.pages.explorer.editor_picker_owner = Some(owner);
        state.pages.explorer.explorer_input_mode = ExplorerInputMode::Address;
        state.pages.explorer.explorer_input = "unfinished address".into();
        state.pages.explorer.explorer_input_replace_all = true;
        state.pages.explorer.explorer_overlay_mode = Some(ExplorerOverlayMode::Options);
        state.notify_page_alert_with_key(
            EXPLORER_ALERT_KEY,
            "keep this error",
            ui::NotificationTone::Error,
        );
        let page_before = state.pages.explorer.get(picker).unwrap().clone();
        let model_before = state.app.explorer_state().cloned();
        state.register_page_task(
            TaskDomain::Explorer,
            703,
            TaskClass::Operation,
            TaskControl::new(|| {}, || false),
        );

        state.close_explorer();
        assert_eq!(
            state.app.notification_center().active_modal_action_count(),
            3
        );
        state.apply_input(InputEvent::from_key_label("Esc"));

        assert_eq!(state.active_page_id(), picker);
        assert!(!state.notification_has_active_modal());
        assert_eq!(state.pages.explorer.get(picker), Some(&page_before));
        assert_eq!(state.app.explorer_state(), model_before.as_ref());
        assert_eq!(
            state.page_alert_message_for_key(EXPLORER_ALERT_KEY),
            Some(&i18n::LocalizedText::from("keep this error"))
        );
        assert!(
            state
                .pages
                .editor
                .get(owner)
                .unwrap()
                .editor_close_after_save
        );
        assert!(
            state
                .pages
                .editor
                .get(owner)
                .unwrap()
                .editor_open_after_save
        );
        assert!(
            state
                .pages
                .editor
                .get(owner)
                .unwrap()
                .editor_discard_for_open
        );

        state.open_editor();
        let covering = state.active_page_id();
        state.pages.editor.editor_close_after_save = true;
        state.pages.editor.editor_open_after_save = true;
        state.pages.editor.editor_discard_for_open = true;
        let covering_before = state.pages.editor.get(covering).unwrap().clone();
        state.continue_page_tasks_in_background(picker);
        assert_eq!(state.active_page_id(), covering);
        assert!(state.page_screen(picker).is_none());
        assert_eq!(state.pages.editor.get(covering), Some(&covering_before));
        let original = state.pages.editor.get(owner).unwrap();
        assert_eq!(original.editor_discard_for_open, !editor_picker);
        assert_eq!(original.editor_close_after_save, !save_picker);
        assert_eq!(original.editor_open_after_save, !save_picker);
    }
}

#[test]
fn editor_close_rechecks_an_old_approval_before_discarding_dirty_text() {
    for (discard, edit_again) in [(true, true), (false, false)] {
        let mut state = state();
        state.open_editor();
        let page = state.active_page_id();
        state.app.dispatch_at(
            app::AppCommand::Editor(app::editor::EditorCommand::InsertText("first".into())),
            Instant::now(),
        );
        let revision = state.app.editor_state().unwrap().revision();
        state
            .approved_editor_closes
            .insert(page, (Some(revision), discard));
        if edit_again {
            state.app.dispatch_at(
                app::AppCommand::Editor(app::editor::EditorCommand::InsertText(" newer".into())),
                Instant::now(),
            );
        }

        state.request_page_close(page);

        assert_eq!(state.active_page_id(), page);
        assert!(state.app.editor_state().unwrap().is_dirty());
        assert!(!state.approved_editor_closes.contains_key(&page));
        assert!(state.notification_has_active_modal());
    }
}

#[test]
fn all_page_kinds_keep_separate_state_and_reopen_with_a_fresh_identity() {
    for screen in [
        ShellScreen::Home,
        ShellScreen::Clock,
        ShellScreen::Login,
        ShellScreen::BootstrapAdmin,
        ShellScreen::FirstRunSetup,
        ShellScreen::UserManagement,
        ShellScreen::Settings,
        ShellScreen::SystemStatus,
        ShellScreen::Launcher,
        ShellScreen::CommandLine,
        ShellScreen::Explorer,
        ShellScreen::Editor,
        ShellScreen::Diagnostics,
        ShellScreen::Logs,
        ShellScreen::Management,
        ShellScreen::ExitConfirm,
    ] {
        let mut state = state();
        state.enter_new_page(screen);
        let first = state.active_page_id();
        state.pages.common.error_message = Some("first instance".into());
        state.enter_new_page(screen);
        let second = state.active_page_id();
        assert_ne!(first, second);
        assert!(state.pages.common.error_message.is_none(), "{screen:?}");
        state.pages.common.error_message = Some("second instance".into());
        state.close_page_instance(second);
        assert_eq!(state.active_page_id(), first);
        assert_eq!(
            state.pages.common.error_message,
            Some("first instance".into())
        );
        state.close_page_instance(first);
        state.enter_new_page(screen);
        assert_ne!(state.active_page_id(), first);
        assert_ne!(state.active_page_id(), second);
        assert!(state.pages.common.error_message.is_none());
    }
}

#[test]
fn close_choices_preserve_page_or_transfer_task_or_wait_for_real_completion() {
    for choice in 0..3 {
        let mut state = state();
        state.enter_new_page(ShellScreen::Launcher);
        let page = state.active_page_id();
        let stopped = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let complete = stopped.clone();
        let cancel = cancelled.clone();
        state.register_page_task(
            TaskDomain::Explorer,
            1,
            TaskClass::Operation,
            TaskControl::new(
                move || {
                    cancel.store(true, Ordering::Release);
                },
                move || complete.load(Ordering::Acquire),
            ),
        );
        state.close_launcher();
        assert!(state.page_screen(page).is_some());
        assert_eq!(
            state.app.notification_center().active_modal_action_count(),
            3
        );
        match choice {
            0 => {
                state.continue_page_tasks_in_background(page);
                assert_eq!(state.page_screen(page), None);
                assert_eq!(
                    state.tasks.owner(TaskDomain::Explorer, 1),
                    Some(TaskOwner::Session)
                );
                assert!(!cancelled.load(Ordering::Acquire));
            }
            1 => {
                state.cancel_page_tasks_and_close(page);
                assert!(cancelled.load(Ordering::Acquire));
                assert!(state.page_screen(page).is_some());
                state.tasks.complete(TaskDomain::Explorer, 1);
                state.poll_page_closes();
                assert!(state.page_screen(page).is_some());
                stopped.store(true, Ordering::Release);
                state.poll_page_closes();
                assert_eq!(state.page_screen(page), None);
            }
            _ => {
                state.stay_on_page(page);
                assert_eq!(state.active_page_id(), page);
                assert!(!cancelled.load(Ordering::Acquire));
            }
        }
    }
}

#[test]
fn page_notifications_wait_for_their_owner_and_old_callbacks_are_rejected() {
    let mut state = state();
    state.enter_new_page(ShellScreen::Launcher);
    let first = state.active_page_id();
    state.notify_modal(
        "First",
        "First page only",
        ui::NotificationTone::Info,
        vec![
            ShellNotificationAction::new("close", "Close")
                .with_follow_up(ShellCommand::CloseLauncher),
        ],
    );
    state.enter_new_page(ShellScreen::Launcher);
    let second = state.active_page_id();
    assert!(!state.notification_has_active_modal());
    state.close_page_instance(second);
    assert!(state.notification_has_active_modal());
    state.close_page_instance(first);
    state.enter_new_page(ShellScreen::Launcher);
    let current = state.active_page_id();
    let platform = platform::native_platform();
    state.apply_routed_event(
        RoutedEvent {
            input: InputEvent::key(InputKey::Enter),
            target: RoutedTarget::Global,
            command: ShellCommand::PageAction(first, Box::new(ShellCommand::CloseLauncher)),
        },
        platform.as_ref(),
        Instant::now(),
    );
    assert_eq!(state.active_page_id(), current);
    assert!(!state.notification_has_active_modal());
}

#[test]
fn nested_background_delivery_restores_the_outer_owner_and_keeps_the_visible_page() {
    let mut state = state();
    state.enter_new_page(ShellScreen::Launcher);
    let first = state.active_page_id();
    state.enter_new_page(ShellScreen::Launcher);
    let second = state.active_page_id();
    state.enter_new_page(ShellScreen::Settings);
    let visible = state.active_page_id();
    state.with_page_context(first, |session| {
        session.pages.launcher.launcher_selected_index = 11;
        session.with_page_context(second, |session| {
            session.pages.launcher.launcher_selected_index = 22
        });
        assert_eq!(session.pages.launcher.selected_id(), Some(first));
        session.pages.common.error_message = Some("outer owner".into());
    });
    assert_eq!(state.active_page_id(), visible);
    assert_eq!(
        state
            .pages
            .launcher
            .get(first)
            .unwrap()
            .launcher_selected_index,
        11
    );
    assert_eq!(
        state
            .pages
            .launcher
            .get(second)
            .unwrap()
            .launcher_selected_index,
        22
    );
}
#[test]
fn background_modal_does_not_touch_visible_focus_hit_map_or_another_pages_modal() {
    let mut state = state();
    state.enter_new_page(ShellScreen::Editor);
    let first = state.active_page_id();
    state.enter_new_page(ShellScreen::Editor);
    let second = state.active_page_id();
    state.notify_modal_with_options(
        ShellNotification::modal(
            "Second",
            "Visible",
            ui::NotificationTone::Info,
            vec![ShellNotificationAction::new("ok", "OK")],
        )
        .with_key("same-key"),
    );
    let active = state.notification_active_modal_id();
    let generation = state.hit_map_generation();
    let visible = state.pages.common.get(second).unwrap().clone();
    state.with_page_context(first, |s| {
        s.notify_modal_with_options(
            ShellNotification::modal(
                "First",
                "Background",
                ui::NotificationTone::Info,
                vec![ShellNotificationAction::new("ok", "OK")],
            )
            .with_key("same-key"),
        );
        s.refresh_hit_map();
        s.notification_dismiss_modal_by_key("same-key");
    });
    assert_eq!(state.notification_active_modal_id(), active);
    assert_eq!(state.hit_map_generation(), generation);
    assert_eq!(state.pages.common.get(second).unwrap(), &visible);
    state.close_page_instance(second);
    assert!(!state.notification_has_active_modal());
}

#[test]
fn login_reset_waits_for_task_cleanup_and_rejects_old_results() {
    let mut state = state();
    state.enter_new_page(ShellScreen::Settings);
    let page = state.active_page_id();
    let done = Arc::new(AtomicBool::new(false));
    let stopped = done.clone();
    state.register_page_task(
        TaskDomain::Explorer,
        700,
        TaskClass::Operation,
        TaskControl::new(|| {}, move || stopped.load(Ordering::Acquire)),
    );
    state.return_to_login("Signed out");
    assert_eq!(state.active_page_id(), page);
    assert!(state.pending_login_reset.is_some());
    assert_eq!(state.tasks.owner(TaskDomain::Explorer, 700), None);
    done.store(true, Ordering::Release);
    state.apply_input(InputEvent::Tick);
    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert!(state.pending_login_reset.is_none());
    assert_eq!(state.page_screen(page), None);
}

#[test]
fn logout_to_lockscreen_waits_for_real_task_completion() {
    let mut state = state();
    state.enter_new_page(ShellScreen::Settings);
    let page = state.active_page_id();
    let done = Arc::new(AtomicBool::new(false));
    let stopped = done.clone();
    state.register_page_task(
        TaskDomain::Explorer,
        702,
        TaskClass::Operation,
        TaskControl::new(|| {}, move || stopped.load(Ordering::Acquire)),
    );

    assert!(!state.logout_to_lockscreen_at(Instant::now()));
    assert!(state.pending_login_reset.is_some());
    assert!(!state.return_to_lockscreen_requested());
    assert_eq!(state.active_page_id(), page);
    assert_eq!(state.apply_input(InputEvent::Tick), ShellAction::Redraw);
    assert!(!state.return_to_lockscreen_requested());

    done.store(true, Ordering::Release);
    assert_eq!(state.apply_input(InputEvent::Tick), ShellAction::Exit);
    assert!(state.pending_login_reset.is_none());
    assert!(state.return_to_lockscreen_requested());
    assert_eq!(state.page_screen(page), None);
}

#[test]
fn confirmed_exit_keeps_ticking_until_tasks_have_really_stopped() {
    let mut state = state();
    let done = Arc::new(AtomicBool::new(false));
    let stopped = done.clone();
    state.register_page_task(
        TaskDomain::Explorer,
        701,
        TaskClass::Operation,
        TaskControl::new(|| {}, move || stopped.load(Ordering::Acquire)),
    );
    state.apply_input(InputEvent::from_key_label("q"));
    assert_eq!(
        state.apply_input(InputEvent::from_key_label("Enter")),
        ShellAction::Redraw
    );
    assert!(state.pending_session_exit.is_some());
    assert!(!state.shutdown_requested());
    assert_eq!(state.apply_input(InputEvent::Tick), ShellAction::Redraw);
    done.store(true, Ordering::Release);
    assert_eq!(state.apply_input(InputEvent::Tick), ShellAction::Exit);
    assert!(state.pending_session_exit.is_none());
    assert!(state.shutdown_requested());
}
