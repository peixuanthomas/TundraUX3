use super::*;

#[test]
fn unavailable_source_reports_an_error_without_replacing_the_document() {
    let mut state = session();
    saved_script(&mut state);
    let before = state.app.editor_state().unwrap().source().into_owned();
    state.locate_script_source(std::path::Path::new("/"), 1);
    assert!(state.pages.editor.script.state.error.is_some());
    assert!(state.pages.editor.script.source_view.is_none());
    assert_eq!(state.app.editor_state().unwrap().source(), before);
}

#[test]
fn finished_output_and_readonly_source_views_do_not_edit_the_main_document() {
    let mut state = session();
    saved_script(&mut state);
    let platform = platform::mock::UnsupportedPlatform;
    let before = state.app.editor_state().unwrap().source().into_owned();
    state.pages.editor.script.state.status = ScriptStatus::Finished;
    state.pages.editor.script.focus = Pane::Output;
    state.handle_editor_key_at(
        KeyInput::new(InputKey::Char('x')),
        &platform,
        Instant::now(),
    );
    state.handle_editor_paste("invisible edit".into());
    assert_eq!(state.app.editor_state().unwrap().source(), before);
    state.pages.editor.script.focus = Pane::Source;
    state.pages.editor.script.source_view =
        Some(EditorState::open_read_only("/tmp/library.sh", b"one\ntwo\nthree\nfour\n").unwrap());
    state.handle_editor_paste("invisible library edit".into());
    assert_eq!(state.app.editor_state().unwrap().source(), before);
    let page = state.content_page_id();
    state.request_editor_close(&platform);
    assert!(state.pages.editor.script.source_view.is_none());
    assert_eq!(state.content_page_id(), page);
}

#[test]
fn sourced_file_breakpoints_follow_the_displayed_cursor_and_f8_returns_to_main() {
    let mut state = session();
    saved_script(&mut state);
    let mut source =
        EditorState::open_read_only("/tmp/library.sh", b"one\ntwo\nthree\nfour\n").unwrap();
    source.cursor.byte_offset = source.source_offset(3, 0).unwrap();
    state.pages.editor.script.source_view = Some(source);
    let platform = platform::mock::UnsupportedPlatform;
    state.handle_script_key(&KeyInput::from_label("F9"), &platform);
    assert!(
        state
            .pages
            .editor
            .script
            .state
            .breakpoints
            .iter()
            .any(|bp| bp.path == std::path::Path::new("/tmp/library.sh") && bp.line == 4)
    );
    assert!(!state.handle_script_key(&KeyInput::from_label("F8"), &platform));
    assert!(state.pages.editor.script.source_view.is_none());
}

#[test]
fn list_navigation_keeps_external_source_still_and_source_pages_and_wheel_move_it() {
    let mut state = session();
    saved_script(&mut state);
    let text = "line\n".repeat(100);
    let mut source = EditorState::open_read_only("/tmp/library.sh", text.as_bytes()).unwrap();
    source.cursor.byte_offset = source.source_offset(10, 0).unwrap();
    source.viewport.top_line = 10;
    state.pages.editor.script.source_view = Some(source);
    let platform = platform::mock::UnsupportedPlatform;
    for pane in [Pane::Variables, Pane::Stack, Pane::Breakpoints] {
        state.pages.editor.script.focus = pane;
        state.handle_script_key(&KeyInput::from_label("Down"), &platform);
        state.handle_script_key(&KeyInput::from_label("PageDown"), &platform);
        let source = state.pages.editor.script.source_view.as_ref().unwrap();
        assert_eq!(source.viewport.top_line, 10);
        assert_eq!(
            source.source_display_position(source.cursor.byte_offset),
            Some((10, 0))
        );
    }
    state.pages.editor.script.focus = Pane::Source;
    let step = state
        .script_layout()
        .editor
        .visible_capacity
        .saturating_sub(2)
        .max(1);
    state.handle_script_key(&KeyInput::from_label("PageDown"), &platform);
    let source = state.pages.editor.script.source_view.as_ref().unwrap();
    assert_eq!(
        source.source_display_position(source.cursor.byte_offset),
        Some((10 + step, 0))
    );
    state.handle_script_key(&KeyInput::from_label("PageUp"), &platform);
    let area = state.script_layout().editor.canvas;
    state.handle_script_pointer(
        MouseInput::scroll(area.x, area.y, ScrollDirection::Down),
        &platform,
    );
    assert_eq!(
        state
            .pages
            .editor
            .script
            .source_view
            .as_ref()
            .unwrap()
            .viewport
            .top_line,
        13
    );
    state.handle_script_pointer(
        MouseInput::scroll(area.x, area.y, ScrollDirection::Up),
        &platform,
    );
    assert_eq!(
        state
            .pages
            .editor
            .script
            .source_view
            .as_ref()
            .unwrap()
            .viewport
            .top_line,
        10
    );
}

#[test]
fn output_wheel_and_drag_use_real_history_and_zero_means_latest() {
    let mut state = session();
    saved_script(&mut state);
    let mut output = ui::CommandLineTerminalSnapshot::blank(40, 8);
    output.scrollback_rows = 73;
    state.pages.editor.script.output = Some(Arc::new(output));
    let platform = platform::mock::UnsupportedPlatform;
    let area = state.script_layout().output_area;
    for (direction, expected) in [(ScrollDirection::Up, 3), (ScrollDirection::Down, 0)] {
        state.handle_script_pointer(MouseInput::scroll(area.x, area.y, direction), &platform);
        assert_eq!(
            state.pages.editor.script.scrolls[Pane::Output.index()],
            expected
        );
    }
    state.pages.editor.script.scrolls[Pane::Output.index()] = 72;
    state.handle_script_pointer(
        MouseInput::scroll(area.x, area.y, ScrollDirection::Up),
        &platform,
    );
    assert_eq!(state.pages.editor.script.scrolls[Pane::Output.index()], 73);
    let (_, bar) = state
        .script_layout()
        .scrollbars
        .into_iter()
        .find(|(pane, _)| *pane == Pane::Output)
        .unwrap();
    state.handle_script_pointer(
        MouseInput::down(bar.thumb.x, bar.thumb.y, PointerButton::Left),
        &platform,
    );
    state.handle_script_pointer(
        MouseInput::drag(bar.track.x, bar.track.y, PointerButton::Left),
        &platform,
    );
    assert_eq!(state.pages.editor.script.scrolls[Pane::Output.index()], 73);
    state.handle_script_pointer(
        MouseInput::drag(bar.track.x, bar.track.bottom() - 1, PointerButton::Left),
        &platform,
    );
    assert_eq!(state.pages.editor.script.scrolls[Pane::Output.index()], 0);
}

#[test]
fn f10_reaches_the_editor_menu_from_a_detail_pane_and_settings_owns_paste() {
    let mut state = session();
    saved_script(&mut state);
    let platform = platform::mock::UnsupportedPlatform;
    state.pages.editor.script.focus = Pane::Variables;
    state.handle_editor_key_at(KeyInput::from_label("F10"), &platform, Instant::now());
    assert_eq!(state.pages.editor.editor_focus, ui::EditorFocus::MenuBar);
    state.handle_editor_key_at(KeyInput::from_label("Down"), &platform, Instant::now());
    assert!(state.pages.editor.editor_open_menu.is_some());
    state.handle_editor_key_at(KeyInput::new(InputKey::Escape), &platform, Instant::now());
    state.pages.editor.script.focus = Pane::Expression;
    state.pages.editor.script.expression.set_value("unchanged");
    for key in [
        KeyInput::new(InputKey::F(10)),
        KeyInput::with_modifiers(InputKey::Char('s'), InputModifiers::ALT),
        KeyInput::new(InputKey::End),
        KeyInput::new(InputKey::Enter),
    ] {
        state.handle_editor_key_at(key, &platform, Instant::now());
    }
    assert!(state.config_editor_form_visible());
    state.pages.editor.script.focus = Pane::Expression;
    state.handle_editor_paste("/bin/bash".into());
    assert_eq!(state.pages.editor.script.expression.value(), "unchanged");
    let form = state
        .to_editor_view_model()
        .config_form
        .unwrap()
        .form
        .unwrap();
    assert_eq!(form.fields[0].value, "/bin/bash");
}

#[test]
fn compact_list_navigation_keeps_selection_visible_and_ignores_key_release() {
    let mut state = session();
    saved_script(&mut state);
    state.terminal_size = (108, 24);
    state.pages.editor.script.focus = Pane::Variables;
    state.pages.editor.script.detail = Pane::Variables;
    state.pages.editor.script.state.variables = (0..10)
        .map(|index| app::script::ScriptVariable {
            name: format!("var{index}"),
            value: index.to_string(),
            children: vec![],
        })
        .collect();
    let platform = platform::mock::UnsupportedPlatform;
    for _ in 0..3 {
        state.handle_script_key(&KeyInput::from_label("Down"), &platform);
        let selected = state.pages.editor.script.selected_variable;
        assert!(
            state
                .script_layout()
                .rows
                .iter()
                .any(|(pane, index, _)| *pane == Pane::Variables && *index == selected)
        );
    }
    let selected = state.pages.editor.script.selected_variable;
    for key in [InputKey::Down, InputKey::F(6), InputKey::Tab] {
        state.handle_script_key(
            &KeyInput::with_phase(key, InputModifiers::none(), InputPhase::Release),
            &platform,
        );
    }
    assert_eq!(state.pages.editor.script.selected_variable, selected);
    assert_eq!(state.pages.editor.script.focus, Pane::Variables);
    assert_eq!(state.pages.editor.script.selected_action, None);
    assert!(!state.handle_script_key(&KeyInput::new(InputKey::Escape), &platform));
}

#[test]
fn expression_tab_cycles_its_controls_and_returns_to_input_while_disabled_input_stays_still() {
    let mut state = session();
    saved_script(&mut state);
    state.pages.editor.script.state.status = ScriptStatus::Paused;
    state.pages.editor.script.focus = Pane::Expression;
    state.pages.editor.script.detail = Pane::Expression;
    state.advance_script_details(ui::MotionFrame::default());
    state.pages.editor.script.expression.set_value("value");
    let platform = platform::mock::UnsupportedPlatform;
    for expected in [Some(Action::Examine), Some(Action::Evaluate), None] {
        state.handle_script_key(&KeyInput::from_label("Tab"), &platform);
        assert_eq!(state.pages.editor.script.selected_action, expected);
    }
    for expected in [Some(Action::Evaluate), Some(Action::Examine), None] {
        state.handle_script_key(&KeyInput::from_label("Shift+Tab"), &platform);
        assert_eq!(state.pages.editor.script.selected_action, expected);
    }
    state.handle_script_key(&KeyInput::new(InputKey::Char('x')), &platform);
    assert_eq!(state.pages.editor.script.expression.value(), "valuex");
    state.pages.editor.script.state.status = ScriptStatus::Finished;
    state.handle_script_key(&KeyInput::new(InputKey::Char('y')), &platform);
    assert!(state.script_paste("hidden paste".into()));
    assert_eq!(state.pages.editor.script.expression.value(), "valuex");
    assert!(state.script_requests.is_empty());
}

#[test]
fn wide_detail_tabs_open_and_close_without_hover_and_output_keeps_primary_keyboard_actions() {
    let mut state = session();
    saved_script(&mut state);
    let platform = platform::mock::UnsupportedPlatform;
    state.advance_script_details(ui::MotionFrame::default());
    let source_width = state.script_layout().editor_area.width;
    for _ in 0..2 {
        let (_, tab) = state
            .script_layout()
            .tabs
            .into_iter()
            .find(|(pane, _)| *pane == Pane::Variables)
            .unwrap();
        state.handle_script_pointer(
            MouseInput::down(tab.x, tab.y, PointerButton::Left),
            &platform,
        );
        state.handle_script_pointer(MouseInput::up(tab.x, tab.y, PointerButton::Left), &platform);
        state.advance_script_details(ui::MotionFrame::default());
        assert!(!state.script_layout().output_area.is_empty());
        if state.pages.editor.script.detail == Pane::Variables {
            assert!(state.script_layout().editor_area.width < source_width);
        } else {
            assert_eq!(state.pages.editor.script.detail, Pane::Output);
            assert_eq!(state.script_layout().editor_area.width, source_width);
        }
    }
    assert_eq!(state.pages.editor.script.focus, Pane::Output);
    for expected in [
        Some(Action::Run),
        Some(Action::Debug),
        Some(Action::Settings),
        None,
    ] {
        state.handle_script_key(&KeyInput::from_label("Tab"), &platform);
        assert_eq!(state.pages.editor.script.selected_action, expected);
    }
}

#[test]
fn wide_detail_motion_keeps_source_output_and_hits_on_the_same_frame() {
    let mut state = session();
    saved_script(&mut state);
    state.interaction.pointer_motion = true;
    let mut frame = ui::MotionFrame::default();
    state.advance_script_details(frame);
    let initial = state.script_layout().editor_area;
    state.pages.editor.script.detail = Pane::Variables;
    assert!(state.advance_script_details(frame));
    frame.now = Duration::from_millis(60);
    assert!(state.advance_script_details(frame));
    let middle = state.script_layout();
    assert!(middle.editor_area.width < initial.width);
    assert_eq!(
        middle.editor_area,
        state.editor_content_area(state.script_main_area())
    );
    assert!(!middle.output_area.is_empty());
    let pane = middle.pane(Pane::Variables);
    assert_eq!(
        middle.hit_test(pane.x, pane.y),
        Some(Hit::Pane(Pane::Variables))
    );
    let platform = platform::mock::UnsupportedPlatform;
    let (_, tab) = middle
        .tabs
        .iter()
        .find(|(pane, _)| *pane == Pane::Variables)
        .unwrap();
    state.handle_script_pointer(
        MouseInput::down(tab.x, tab.y, PointerButton::Left),
        &platform,
    );
    frame.now = Duration::from_millis(80);
    state.advance_script_details(frame);
    let before_reverse = state.script_layout().editor_area;
    state.handle_script_pointer(MouseInput::up(tab.x, tab.y, PointerButton::Left), &platform);
    assert_eq!(state.pages.editor.script.detail, Pane::Output);
    state.advance_script_details(frame);
    assert_eq!(state.script_layout().editor_area, before_reverse);
    assert!(state.script_layout().editor_area.width < initial.width);
    state.interaction.pointer_motion = false;
    assert!(!state.advance_script_details(frame));
    assert_eq!(state.script_layout().editor_area.width, initial.width);
    assert_eq!(state.pages.editor.script.closing_detail, None);
}

#[test]
fn deleting_last_selected_breakpoint_selects_the_remaining_row() {
    let mut state = session();
    saved_script(&mut state);
    let path = state
        .app
        .editor_state()
        .unwrap()
        .document
        .path
        .clone()
        .unwrap();
    state.pages.editor.script.state.toggle_breakpoint(&path, 2);
    state.pages.editor.script.state.toggle_breakpoint(&path, 3);
    state.pages.editor.script.selected_breakpoint = 1;
    let platform = platform::mock::UnsupportedPlatform;
    state.activate_script_action(Action::DeleteBreakpoint, &platform);
    assert_eq!(state.pages.editor.script.selected_breakpoint, 0);
    state.activate_script_action(Action::ToggleBreakpoint, &platform);
    assert!(!state.pages.editor.script.state.breakpoints[0].enabled);
    state.activate_script_action(Action::DeleteBreakpoint, &platform);
    assert!(state.pages.editor.script.state.breakpoints.is_empty());
    assert_eq!(state.pages.editor.script.selected_breakpoint, 0);
}

fn session() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (144, 44),
        ShellHomeMode::User,
    );
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LinuxCurrentProcess,
            session_id: "script-ui-tests".into(),
            user_id: "current".into(),
            username: "current".into(),
            role: UserRole::User,
            started_at_epoch_ms: 0,
        })),
        Instant::now(),
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.open_script_editor();
    state
}

fn saved_script(state: &mut ShellSession) {
    let editor = EditorState::open(
        "/tmp/tundra-shell-script-ui.sh",
        b"#!/usr/bin/env bash\nvalue=1\nprintf '%s\\n' \"$value\"\n",
    )
    .unwrap();
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(editor)),
        Instant::now(),
    );
    state.enable_script_workspace();
}

#[test]
fn launcher_entry_reuses_editor_and_f6_reaches_every_workspace_pane() {
    let mut state = session();
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert!(state.pages.editor.script.enabled);
    assert_eq!(
        state.app.editor_state().unwrap().source(),
        app::script::NEW_SCRIPT
    );
    assert_eq!(
        state.to_page_help_view_model(),
        ui::page_help(ui::PageHelpScreen::Script)
    );
    let platform = platform::mock::UnsupportedPlatform;
    for expected in [
        Pane::Output,
        Pane::Variables,
        Pane::Stack,
        Pane::Expression,
        Pane::Breakpoints,
        Pane::Source,
    ] {
        assert!(state.handle_script_key(&KeyInput::from_label("F6"), &platform));
        assert_eq!(state.pages.editor.script.focus, expected);
    }
    assert!(state.handle_script_key(
        &KeyInput::with_modifiers(InputKey::F(6), InputModifiers::SHIFT),
        &platform,
    ));
    assert_eq!(state.pages.editor.script.focus, Pane::Breakpoints);
    assert_eq!(
        state.app.editor_state().unwrap().source(),
        app::script::NEW_SCRIPT
    );
}

#[test]
fn launching_a_saved_script_queues_one_task_and_locks_source_until_failure_or_exit() {
    let mut state = session();
    saved_script(&mut state);
    let platform = platform::mock::UnsupportedPlatform;
    let source = state.app.editor_state().unwrap().source().into_owned();
    state.request_script_launch(false, &platform);
    assert_eq!(
        state.pages.editor.script.state.status,
        ScriptStatus::Starting
    );
    assert!(state.app.editor_state().unwrap().is_read_only());
    assert!(matches!(
        state.script_requests.front(),
        Some((_, RuntimeCommand::Start { debug: false, .. }))
    ));
    state.request_script_launch(false, &platform);
    assert_eq!(state.script_requests.len(), 1);
    state.app.dispatch_at(
        app::AppCommand::Editor(app::editor::EditorCommand::InsertText("unexpected".into())),
        Instant::now(),
    );
    assert_eq!(state.app.editor_state().unwrap().source(), source);
    state.fail_script(i18n::msg!("script-runtime-start-timeout").into());
    assert!(!state.app.editor_state().unwrap().is_read_only());
}

#[test]
fn unsaved_launch_waits_for_the_original_documents_save_picker() {
    let mut state = session();
    let owner = state.active_page_id();
    state.request_script_launch(true, &platform::mock::UnsupportedPlatform);
    assert!(state.script_requests.is_empty());
    let script = &state.pages.editor.get(owner).unwrap().script;
    assert_eq!(script.state.status, ScriptStatus::Saving);
    assert_eq!(script.pending_launch, Some(true));
    assert!(script.save_requested);
    assert!(state.app.editor_state_for_page(owner).unwrap().is_dirty());
}

#[test]
fn expression_is_sent_only_after_explicit_execution_and_tracks_current_page() {
    let mut state = session();
    saved_script(&mut state);
    let owner = state.active_page_id();
    state.pages.editor.script.state.status = ScriptStatus::Paused;
    state.pages.editor.script.focus = Pane::Expression;
    state.script_paste("value = 5".into());
    assert!(state.script_requests.is_empty());
    state.handle_script_key(
        &KeyInput::from_label("Enter"),
        &platform::mock::UnsupportedPlatform,
    );
    assert_eq!(
        state.script_requests.pop_front(),
        Some((
            owner,
            RuntimeCommand::Evaluate {
                input: "value = 5".into(),
                execute: false
            }
        ))
    );
    state.activate_script_action(Action::Evaluate, &platform::mock::UnsupportedPlatform);
    assert!(state.script_requests.is_empty());
    state.pages.editor.script.state.status = ScriptStatus::Paused;
    state.activate_script_action(Action::Evaluate, &platform::mock::UnsupportedPlatform);
    assert_eq!(
        state.script_requests.pop_front(),
        Some((
            owner,
            RuntimeCommand::Evaluate {
                input: "value = 5".into(),
                execute: true
            }
        ))
    );
}

#[test]
fn settings_button_waits_for_release_and_opens_the_existing_modal_form() {
    let mut state = session();
    let layout = state.script_layout();
    let (_, area) = layout
        .actions
        .iter()
        .find(|(action, _)| *action == Action::Settings)
        .unwrap();
    let point = (area.x, area.y);
    let platform = platform::mock::UnsupportedPlatform;
    state.handle_script_pointer(
        MouseInput::down(point.0, point.1, PointerButton::Left),
        &platform,
    );
    assert!(!state.config_editor_form_visible());
    state.handle_script_pointer(
        MouseInput::up(point.0, point.1, PointerButton::Left),
        &platform,
    );
    assert!(state.config_editor_form_visible());
    let model = state.to_editor_view_model();
    let form = model.config_form.unwrap().form.unwrap();
    assert_eq!(
        form.fields
            .iter()
            .map(|field| field.id.as_str())
            .collect::<Vec<_>>(),
        ["interpreter", "arguments", "cwd"]
    );
    assert!(state.script_requests.is_empty());
}

#[test]
fn modal_changes_cancel_pending_script_clicks_and_scrollbar_drags() {
    let mut state = session();
    state.pages.editor.script.pointer = Some(Hit::Action(Action::Run));
    state.pages.editor.script.drag = Some((Pane::Variables, 1));
    state.cancel_pointer_gestures_for_modal();
    assert!(state.pages.editor.script.pointer.is_none());
    assert!(state.pages.editor.script.drag.is_none());
}

#[test]
fn background_and_return_preserve_the_same_document_and_debugger_state() {
    let mut state = session();
    saved_script(&mut state);
    let owner = state.active_page_id();
    state.pages.editor.script.state.status = ScriptStatus::Paused;
    state.pages.editor.script.state.session_id = 7;
    state.pages.editor.script.expression.set_value("value + 2");
    let source = state.app.editor_state().unwrap().source().into_owned();
    state.continue_page_tasks_in_background(owner);
    assert!(!state.live_page_ids().contains(&owner));
    assert!(state.pages.editor.get(owner).is_some());
    while state.notification_dismiss_active_modal_without_response() {}
    state.restore_script_page(owner);
    assert_eq!(state.active_page_id(), owner);
    assert_eq!(state.pages.editor.script.state.session_id, 7);
    assert_eq!(state.pages.editor.script.state.status, ScriptStatus::Paused);
    assert_eq!(state.pages.editor.script.expression.value(), "value + 2");
    assert_eq!(state.app.editor_state().unwrap().source(), source);
}

#[test]
fn vim_launch_keeps_its_export_owner_when_another_document_becomes_visible() {
    let mut state = session();
    saved_script(&mut state);
    let owner = state.active_page_id();
    assert!(state.set_editor_vim_enabled(true));
    state.editor_vim_terminal = Some(Arc::new(ui::CommandLineTerminalSnapshot::blank(80, 20)));
    let platform = platform::mock::UnsupportedPlatform;
    state.request_script_launch(true, &platform);
    assert_eq!(state.editor_vim_script_export, Some(owner));
    assert!(!state.editor_vim_toggle_requested);
    assert!(state.pages.editor.script.vim_pending);
    state.new_edit_document();
    let other = state.active_page_id();
    state.editor_vim_terminal = None;
    state.with_page_context(owner, |state| state.poll_script_launch(&platform));
    assert_eq!(state.active_page_id(), other);
    assert_eq!(state.editor_vim_script_export, Some(owner));
    assert!(state.script_requests.is_empty());
    assert!(state.pages.editor.get(owner).unwrap().script.vim_pending);
    assert!(!state.pages.editor.get(other).unwrap().script.vim_pending);
    assert!(state.app.storage_config().editor.vim_mode_enabled);
}
