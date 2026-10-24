use super::*;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

fn session() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.set_navigation_path(vec![ShellScreen::Home]);
    state.refresh_hit_map();
    state
}

fn home_frame(state: &ShellSession, millis: u64, reduced: bool) -> PreparedFrame {
    PreparedFrame {
        page: ScreenViewModel::Home(Box::new(state.to_home_view_model())),
        chrome: state.to_shell_chrome_view_model(),
        context: ui::RenderContext::from_theme(
            &ui::TundraTheme::default_dark(),
            ui::MotionFrame {
                now: Duration::from_millis(millis),
                delta: Duration::from_millis(50),
                reduced_motion: reduced,
                animation_speed_percent: 100,
            },
            Default::default(),
        ),
        notification: None,
        help: None,
        time_sync: None,
        progress_running: false,
    }
}

fn draw(
    compositor: &mut ScreenCompositor,
    terminal: &mut Terminal<TestBackend>,
    state: &mut ShellSession,
    frame: &PreparedFrame,
) -> bool {
    let mut running = false;
    terminal
        .draw(|target| running = compositor.render(target, state, frame, None))
        .unwrap();
    running
}

fn text(buffer: &Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            let mut row = String::new();
            let mut x = area.x;
            // TestBackend can retain old symbols in wide-character continuation cells.
            while x < area.right() {
                let symbol = buffer[(x, y)].symbol();
                row.push_str(symbol);
                x += ratatui::text::Span::raw(symbol).width().max(1) as u16;
            }
            row
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn vim_buttons_keep_pointer_presses_during_document_synchronization() {
    let platform = platform::mock::UnsupportedPlatform;
    for installing in [false, true] {
        let mut state = session();
        state.open_editor();
        assert!(state.set_editor_vim_enabled(true));
        if installing {
            state.editor_vim_install = Some((
                state.content_page_id(),
                ui::EditorVimInstallViewModel {
                    progress: Default::default(),
                    cancelling: false,
                },
            ));
        } else {
            state.editor_vim_terminal =
                Some(Arc::new(ui::CommandLineTerminalSnapshot::blank(116, 30)));
        }
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut prepared = home_frame(&state, 0, true);
        prepared.page = ScreenViewModel::Editor(Box::new(state.to_editor_view_model()));
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let id = if installing {
            "editor.vim-install.cancel"
        } else {
            "editor.vim-toggle"
        };
        let area = state
            .button_regions
            .iter()
            .find(|region| region.id.as_str() == id)
            .unwrap()
            .area;
        let point = (area.x + area.width / 2, area.y + area.height / 2);
        let now = Instant::now();
        assert!(
            compositor
                .prepare_input(
                    &mut state,
                    InputEvent::mouse_down(PointerButton::Left, point),
                    &platform,
                    now,
                )
                .is_none()
        );
        if !installing {
            state.activate_editor_document_page(state.content_page_id());
        }
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let released = now + Duration::from_millis(16);
        let input = compositor
            .prepare_input(
                &mut state,
                InputEvent::mouse_up(PointerButton::Left, point),
                &platform,
                released,
            )
            .expect("matching release activates the button");
        compositor.dispatch_input(&mut state, input, &platform, released);
        if installing {
            assert!(state.current_vim_install().unwrap().cancelling);
        } else {
            assert!(state.editor_vim_toggle_requested);
        }
    }
}

fn animation_settings_session() -> ShellSession {
    let mut state = session();
    state.app.dispatch_at(
        app::AppCommand::SetActiveAppearance(Some(Default::default())),
        Instant::now(),
    );
    state.enter_screen(ShellScreen::Settings);
    state.pages.settings.settings_state = Some(SettingsState {
        application: Default::default(),
        system_time: Default::default(),
        animation_editor: Some(SettingsAnimationEditorState {
            selected_index: 0,
            scroll_offset: 0,
        }),
        number_editor: None,
        category: ui::SettingsCategory::Appearance,
        selected_field: ui::SettingsField::AnimationAdvanced,
        status: String::new().into(),
        scroll_offset: 0,
        picker: None,
        color_editor: None,
        weather_location_editor: None,
        file_extensions_editor: None,
        time_sync_server_editor: None,
        time_sync_validation_request_id: None,
    });
    state
}

#[test]
fn settings_number_dialog_captures_only_its_buttons_and_waits_for_release() {
    let platform = platform::mock::UnsupportedPlatform;
    let mut state = animation_settings_session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.page = ScreenViewModel::Settings(Box::new(state.to_settings_view_model().unwrap()));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let covered = state
        .button_regions
        .iter()
        .find(|region| region.id.as_str() == "settings.field.pagecruise")
        .unwrap()
        .area;
    let point = (covered.right() - 1, covered.y);
    state.handle_settings_key(&KeyInput::new(InputKey::Enter), &platform);
    let layout = state.current_settings_layout().unwrap();
    let number = layout.number_editor.unwrap();
    assert!(!rect_contains(number.dialog, point));
    assert!(state.button_at(point).is_none());
    prepared.page = ScreenViewModel::Settings(Box::new(state.to_settings_view_model().unwrap()));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert!(state.button_at(point).is_none());
    let point = (number.increase.x, number.increase.y);
    let now = Instant::now();
    assert!(
        compositor
            .prepare_input(
                &mut state,
                InputEvent::mouse_down(PointerButton::Left, point),
                &platform,
                now
            )
            .is_none()
    );
    assert_eq!(
        state
            .to_settings_view_model()
            .unwrap()
            .number_editor
            .unwrap()
            .input
            .value(),
        "55"
    );
    let released = now + Duration::from_millis(16);
    let input = compositor
        .prepare_input(
            &mut state,
            InputEvent::mouse_up(PointerButton::Left, point),
            &platform,
            released,
        )
        .unwrap();
    compositor.dispatch_input(&mut state, input, &platform, released);
    assert_eq!(
        state
            .to_settings_view_model()
            .unwrap()
            .number_editor
            .unwrap()
            .input
            .value(),
        "56"
    );
    state.handle_settings_key(&KeyInput::new(InputKey::Escape), &platform);
    let model = state.to_settings_view_model().unwrap();
    assert!(model.number_editor.is_none());
    assert_eq!(model.animation_editor.unwrap().selected_index, 0);
    assert_eq!(
        state
            .app
            .active_appearance()
            .unwrap()
            .animation_timings
            .page[0],
        55
    );
}

#[test]
fn settings_nested_dialogs_keep_parents_visible_when_opening_and_closing() {
    for deferred in [false, true] {
        let platform = platform::mock::UnsupportedPlatform;
        let mut state = animation_settings_session();
        state.interaction.pointer_motion = true;
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut millis = 0;
        let mut render = |compositor: &mut ScreenCompositor,
                          terminal: &mut Terminal<TestBackend>,
                          state: &mut ShellSession,
                          delta| {
            millis += delta;
            let mut prepared = home_frame(state, millis, false);
            prepared.context.motion.delta = Duration::from_millis(delta);
            prepared.page =
                ScreenViewModel::Settings(Box::new(state.to_settings_view_model().unwrap()));
            prepared.help = state.help_open.then(|| state.to_page_help_view_model());
            draw(compositor, terminal, state, &prepared)
        };
        render(&mut compositor, &mut terminal, &mut state, 0);
        assert!(!render(&mut compositor, &mut terminal, &mut state, 180));
        let parent = state
            .current_settings_layout()
            .unwrap()
            .animation_editor
            .unwrap()
            .dialog;
        let corner = (parent.x, parent.y);
        let border = terminal.backend().buffer()[corner].symbol().to_owned();
        state.handle_settings_key(&KeyInput::new(InputKey::Enter), &platform);
        assert_eq!(
            state
                .interactive_overlays()
                .iter()
                .map(|overlay| overlay.id.as_str())
                .collect::<Vec<_>>(),
            ["settings-editor:animation", "settings-editor:number"]
        );
        assert!(render(&mut compositor, &mut terminal, &mut state, 0));
        assert!(render(&mut compositor, &mut terminal, &mut state, 90));
        assert_eq!(terminal.backend().buffer()[corner].symbol(), border);
        assert!(
            !render(&mut compositor, &mut terminal, &mut state, 90),
            "only the number dialog enters"
        );

        state.help_open = true;
        state.refresh_hit_map();
        assert_eq!(state.interactive_overlays().len(), 3);
        render(&mut compositor, &mut terminal, &mut state, 0);
        assert!(!render(&mut compositor, &mut terminal, &mut state, 180));
        state.help_open = false;
        state.refresh_hit_map();
        render(&mut compositor, &mut terminal, &mut state, 0);
        assert!(
            !render(&mut compositor, &mut terminal, &mut state, 180),
            "closing Help does not reenter the number dialog"
        );

        if deferred {
            let close = state
                .clone()
                .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
            assert_eq!(
                compositor.motion.intercept_input(&close),
                MotionInputDisposition::Defer
            );
            render(&mut compositor, &mut terminal, &mut state, 0);
            render(&mut compositor, &mut terminal, &mut state, 180);
            assert!(compositor.motion.take_deferred_close(&state).is_some());
        }
        state.handle_settings_key(&KeyInput::new(InputKey::Escape), &platform);
        render(&mut compositor, &mut terminal, &mut state, 0);
        assert_eq!(terminal.backend().buffer()[corner].symbol(), border);
        assert!(
            !render(&mut compositor, &mut terminal, &mut state, 180),
            "the existing parent does not enter again"
        );
        let model = state.to_settings_view_model().unwrap();
        assert!(model.number_editor.is_none());
        assert_eq!(model.animation_editor.unwrap().selected_index, 0);
    }
}

#[test]
fn physical_keyboard_finishes_pointer_motion_and_never_waits_to_open_or_close() {
    let platform = platform::mock::UnsupportedPlatform;
    for (pointer, aa) in [(false, false), (true, false), (true, true)] {
        let mut state = session();
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let frame = home_frame(&state, 0, false);
        draw(&mut compositor, &mut terminal, &mut state, &frame);
        let send = |state: &mut ShellSession, compositor: &mut ScreenCompositor, input| {
            let now = Instant::now();
            if let Some(input) = compositor.prepare_input(state, input, &platform, now) {
                let (_, blocked) = compositor.dispatch_input(state, input, &platform, now);
                assert!(!blocked);
            }
        };
        if aa {
            state.apply_input(InputEvent::mouse_moved((0, 0)));
            let (tx, _rx) = mpsc::channel();
            state
                .begin_auto_admin("Keyboard interruption".into(), false, tx)
                .unwrap();
        } else if pointer {
            let area = state.frame_layout.unwrap().back_button.unwrap();
            let point = (area.x + 1, area.y + 1);
            send(
                &mut state,
                &mut compositor,
                InputEvent::mouse_down(PointerButton::Left, point),
            );
            send(
                &mut state,
                &mut compositor,
                InputEvent::mouse_up(PointerButton::Left, point),
            );
        } else {
            send(
                &mut state,
                &mut compositor,
                InputEvent::key(InputKey::Escape),
            );
        }
        if aa {
            assert!(state.auto_admin_visible());
        } else {
            assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        }
        let mut frame = home_frame(&state, 1, false);
        frame.notification = state.to_notification_view_model();
        assert_eq!(
            draw(&mut compositor, &mut terminal, &mut state, &frame),
            pointer
        );
        send(&mut state, &mut compositor, InputEvent::key(InputKey::Tab));
        send(&mut state, &mut compositor, InputEvent::mouse_moved((1, 0)));
        let mut frame = home_frame(&state, 2, false);
        frame.notification = state.to_notification_view_model();
        assert!(!draw(&mut compositor, &mut terminal, &mut state, &frame));
        assert!(!compositor.motion.is_running());
        send(
            &mut state,
            &mut compositor,
            InputEvent::key(if aa {
                InputKey::F(12)
            } else {
                InputKey::Escape
            }),
        );
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert!(!state.auto_admin_visible());
        assert_eq!(state.notification_has_active_modal(), aa);
        assert!(!compositor.motion.is_running());
    }
}

#[test]
fn aa_keyboard_keeps_background_layout_and_places_one_global_launcher_on_every_page() {
    let host = CommandLineHost::new(default_editor_watchdog().unwrap());
    for screen in [
        ShellScreen::Home,
        ShellScreen::FirstRunSetup,
        ShellScreen::Login,
        ShellScreen::BootstrapAdmin,
        ShellScreen::UserManagement,
        ShellScreen::Explorer,
        ShellScreen::Launcher,
        ShellScreen::CommandLine,
        ShellScreen::Editor,
        ShellScreen::Settings,
        ShellScreen::Logs,
        ShellScreen::Management,
        ShellScreen::Diagnostics,
        ShellScreen::SystemStatus,
        ShellScreen::Clock,
    ] {
        for (width, height) in [(120, 40), (108, 20), (80, 24), (60, 23), (49, 11)] {
            let mut state = session();
            state.terminal_size = (width, height);
            state.app.dispatch_at(
                app::AppCommand::SetAuthSession(Some(AuthSession {
                    source: identity::IdentitySource::LocalAccount,
                    session_id: "layout".into(),
                    user_id: "layout".into(),
                    username: "layout".into(),
                    role: UserRole::Admin,
                    started_at_epoch_ms: 1,
                })),
                Instant::now(),
            );
            state.enter_screen(screen);
            state.app.dispatch_at(
                app::AppCommand::SetActiveAppearance(Some(Default::default())),
                Instant::now(),
            );
            if screen == ShellScreen::Settings {
                state.pages.settings.settings_state = Some(SettingsState {
                    application: Default::default(),
                    system_time: Default::default(),
                    animation_editor: None,
                    number_editor: None,
                    category: ui::SettingsCategory::Appearance,
                    selected_field: ui::SettingsField::Theme,
                    status: i18n::msg!("settings-ready").into(),
                    scroll_offset: 0,
                    picker: None,
                    color_editor: None,
                    weather_location_editor: None,
                    file_extensions_editor: None,
                    time_sync_server_editor: None,
                    time_sync_validation_request_id: None,
                });
            }
            let (tx, _rx) = mpsc::channel();
            state
                .begin_auto_admin("Keyboard layout".into(), false, tx)
                .unwrap();
            let mut prepared = home_frame(&state, 0, true);
            prepared.page =
                ScreenViewModel::from_session(&state, &host, Instant::now(), Default::default());
            let mut compositor = ScreenCompositor::default();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
            let original = state.frame_layout.unwrap();
            let launcher = state
                .keyboard_launcher_area()
                .expect("AA shares the global launcher");
            assert_eq!(launcher.right(), width);
            assert_eq!(
                launcher.bottom(),
                original.status_message.map_or(height, |r| r.y)
            );
            assert!(
                !state
                    .button_regions
                    .iter()
                    .any(|r| r.id.as_str() == "aa.keyboard")
            );
            state.toggle_input_keyboard();
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
            let layout = state.frame_layout.unwrap();
            let keyboard = state.input_keyboard_layout();
            let launcher = state.keyboard_launcher_area().unwrap();
            assert_eq!(layout, original);
            assert_eq!(launcher.bottom(), height);
            let aa = ui::auto_admin_layout_in(
                state.auto_admin_available_area(),
                &state.auto_admin_view().unwrap(),
            );
            assert!(aa.dialog.intersection(keyboard.keyboard).is_empty());
            assert!(aa.dialog.intersection(launcher).is_empty());
            state.toggle_input_keyboard();
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
            assert_eq!(state.frame_layout.unwrap(), original);
        }
    }
}

#[test]
fn page_keyboard_animates_chrome_and_content_together_and_restores_them() {
    for screen in [ShellScreen::CommandLine, ShellScreen::Management] {
        let mut state = session();
        state.enter_screen(screen);
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let prepared = home_frame(&state, 0, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let original = state.frame_layout.unwrap();
        let original_launcher = state.keyboard_launcher_area().unwrap();
        assert_eq!(original_launcher.right(), 120);
        assert_eq!(
            original_launcher.bottom(),
            original.status_message.unwrap().y
        );
        state.toggle_input_keyboard();
        let mut previous_top = 3;
        for millis in [0, 55, 110, 165, 220] {
            let prepared = home_frame(&state, millis, false);
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
            let layout = state.frame_layout.unwrap();
            let ui::ShellLayout::Full { top, status, main } = layout.shell else {
                panic!("full layout");
            };
            assert_eq!(main, layout.main);
            assert!(top.height <= previous_top);
            previous_top = top.height;
            let launcher = state.keyboard_launcher_area().unwrap();
            if let Some(hide) = state
                .input_keyboard_layout()
                .buttons
                .iter()
                .find(|button| button.action == ui::ScreenKeyboardAction::ToggleKeyboard)
            {
                assert_eq!(launcher, hide.area);
            } else {
                assert_eq!(launcher.height, 1);
            }
            assert!(main.bottom() <= state.input_keyboard_layout().keyboard.y);
            assert!(launcher.bottom() <= status.y);
        }
        let layout = state.frame_layout.unwrap();
        assert_eq!(layout.back_button.unwrap().height, 1);
        assert!(layout.status_message.is_none() && layout.time_button.is_none());
        assert_eq!(layout.main.y, 1);
        assert_eq!(state.keyboard_launcher_area().unwrap().bottom(), 40);
        assert!(text(terminal.backend().buffer(), Rect::new(0, 0, 120, 1)).contains("TundraUX"));
        state.toggle_input_keyboard();
        let prepared = home_frame(&state, 500, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        assert_eq!(state.frame_layout.unwrap(), original);
    }
}

#[test]
fn status_details_capture_the_drawn_debug_text_before_click_diagnostics_change() {
    let mut state = session();
    state.home_mode = ShellHomeMode::Debug;
    state.last_key_event = Some("original key".into());
    state.notify_status("Status with 中文 and 👩‍💻\noriginal second line".repeat(20));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut compositor = ScreenCompositor::default();
    let prepared = home_frame(&state, 0, true);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let expected = state.displayed_status.clone().unwrap();
    let area = state.frame_layout.unwrap().status_message.unwrap();
    let point = (area.x + 1, area.y + 1);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(
        state.to_notification_view_model().unwrap().message,
        expected
    );
    assert!(expected.contains("Mouse position: none"));
    assert!(
        state
            .to_shell_chrome_view_model()
            .status
            .full_message()
            .contains(&format!("Mouse position: {},{}", point.0, point.1))
    );
}

#[test]
fn status_details_keep_a_toast_visible_during_its_exit_animation() {
    let mut state = session();
    state.interaction.pointer_motion = true;
    state.notify_toast("Read this disappearing toast 中文");
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut compositor = ScreenCompositor::default();
    let prepared = home_frame(&state, 0, false);
    compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let mut expired = home_frame(&state, 100, false);
    expired.chrome.status.toast = None;
    compositor.synchronize_toast(&expired.chrome, expired.context.motion);
    draw(&mut compositor, &mut terminal, &mut state, &expired);
    state.open_status_details();
    assert_eq!(
        state.to_notification_view_model().unwrap().message,
        "Read this disappearing toast 中文"
    );
}

#[test]
fn auto_admin_keeps_shell_title_and_status_visible_after_resize() {
    for (width, height) in [(180, 55), (80, 24), (120, 40)] {
        let mut state = session();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut compositor = ScreenCompositor::default();
        let prepared = home_frame(&state, 0, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let ui::ShellLayout::Full { top, main, status } =
            ui::compute_shell_layout(Rect::new(0, 0, width, height))
        else {
            panic!("full shell layout expected");
        };
        let title_before = text(terminal.backend().buffer(), top);
        let status_before = text(terminal.backend().buffer(), status);
        let (responses, _inputs) = mpsc::channel();
        state
            .begin_auto_admin("Remove neofetch".into(), false, responses)
            .unwrap();
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        assert_eq!(text(terminal.backend().buffer(), top), title_before);
        assert_eq!(text(terminal.backend().buffer(), status), status_before);
        assert!(text(terminal.backend().buffer(), main).contains("AutoAdmin (AA)"));
    }
}

#[test]
fn live_aa_keyboard_keeps_dialog_buttons_visible_and_uses_one_layout_for_input() {
    for (width, height) in [(120, 40), (108, 24), (80, 24), (60, 23), (49, 11), (20, 8)] {
        let mut state = session();
        state.terminal_size = (width, height);
        let (responses, inputs) = mpsc::channel();
        state
            .begin_auto_admin("Live input".into(), false, responses)
            .unwrap();
        state.toggle_input_keyboard();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut compositor = ScreenCompositor::default();
        let prepared = home_frame(&state, 0, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let model = state.auto_admin_view().unwrap();
        let aa = ui::auto_admin_layout_in(state.auto_admin_available_area(), &model);
        let keyboard = state.input_keyboard_layout();
        let bounds = Rect::new(0, 0, width, height);
        assert_eq!(aa.dialog.intersection(bounds), aa.dialog);
        assert!(aa.dialog.intersection(keyboard.keyboard).is_empty());
        assert!(aa.buttons[0].is_empty());
        assert!(
            state
                .button_regions
                .iter()
                .any(|button| Some(button.area) == state.keyboard_launcher_area()
                    && state.button_at((button.area.x, button.area.y)).as_ref() == Some(button))
        );
        if keyboard.usable {
            assert!(text(terminal.backend().buffer(), keyboard.keyboard).contains("CapsLock"));
            assert!(text(terminal.backend().buffer(), aa.dialog).contains("AutoAdmin"));
            let region = keyboard
                .buttons
                .iter()
                .find(|b| b.action == ui::ScreenKeyboardAction::Letter('y'))
                .unwrap()
                .region();
            let point = (region.area.x, region.area.y);
            state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
            let theme = prepared.context.compatibility_theme();
            for y in region.area.y..region.area.bottom() {
                for x in region.area.x..region.area.right() {
                    assert_eq!(terminal.backend().buffer()[(x, y)].bg, theme.accent_color);
                }
            }
            state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
            assert!(inputs.try_iter().any(|input| matches!(input, platform::management::OperationInput::Terminal { bytes } if bytes == b"y")));
        } else {
            assert!(keyboard.keyboard.is_empty());
        }
    }
}

#[test]
fn command_line_keyboard_render_and_pty_viewport_agree_and_restore() {
    let mut state = session();
    state.enter_screen(ShellScreen::CommandLine);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut compositor = ScreenCompositor::default();
    let mut prepared = home_frame(&state, 0, true);
    prepared.page = ScreenViewModel::CommandLine(Box::new(ui::CommandLineViewModel::new(
        ui::CommandLineTerminalSnapshot::blank(120, 40),
    )));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let original = state.command_line_input_area().unwrap();
    assert!(
        text(
            terminal.backend().buffer(),
            terminal.backend().buffer().area
        )
        .contains(&i18n::tr!("screen-keyboard-show"))
    );
    state.toggle_input_keyboard();
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(
        ui::command_line_terminal_area_in(&state.frame_layout.unwrap()),
        state.command_line_input_area()
    );
    assert!(state.command_line_input_area().unwrap().height < original.height);
    assert!(
        text(
            terminal.backend().buffer(),
            state.input_keyboard_layout().keyboard
        )
        .contains("CapsLock")
    );
    state.toggle_input_keyboard();
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(state.command_line_input_area(), Some(original));
    assert!(
        !text(
            terminal.backend().buffer(),
            terminal.backend().buffer().area
        )
        .contains("CapsLock")
    );
}

#[test]
fn live_keyboard_animation_keeps_aa_and_cli_content_above_visible_keys() {
    for aa in [false, true] {
        let mut state = session();
        state.interaction.pointer_motion = true;
        if aa {
            let (tx, _rx) = mpsc::channel();
            state
                .begin_auto_admin("Keyboard animation".into(), false, tx)
                .unwrap();
        } else {
            state.enter_screen(ShellScreen::CommandLine);
        }
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut compositor = ScreenCompositor::default();
        let original = state.command_line_input_area();
        state.toggle_input_keyboard();
        for millis in [0, 1, 55, 110, 220, 300, 355, 410, 510, 520] {
            if millis == 300 {
                state.toggle_input_keyboard();
            }
            let mut prepared = home_frame(&state, millis, false);
            if !aa {
                prepared.page = ScreenViewModel::CommandLine(Box::new(
                    ui::CommandLineViewModel::new(ui::CommandLineTerminalSnapshot::blank(120, 40)),
                ));
            }
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
            let keyboard = state.input_keyboard_layout();
            if aa {
                let shell_main = ui::ShellFrameLayout::new(
                    terminal.backend().buffer().area,
                    None,
                    &prepared.context,
                )
                .main;
                assert!(state.auto_admin_available_area().bottom() <= shell_main.bottom());
                let dialog = ui::auto_admin_layout_in(
                    state.auto_admin_available_area(),
                    &state.auto_admin_view().unwrap(),
                )
                .dialog;
                assert_eq!(
                    dialog.intersection(terminal.backend().buffer().area),
                    dialog
                );
                assert!(dialog.intersection(keyboard.keyboard).is_empty());
            } else {
                assert!(
                    state.command_line_input_area().unwrap().height <= original.unwrap().height
                );
                assert_eq!(
                    ui::command_line_terminal_area_in(&state.frame_layout.unwrap()),
                    state.command_line_input_area()
                );
                if !keyboard.keyboard.is_empty() {
                    assert!(
                        state.command_line_input_area().unwrap().bottom() <= keyboard.keyboard.y
                    );
                }
            }
            for key in &keyboard.buttons {
                assert_eq!(key.area.intersection(keyboard.keyboard), key.area);
                assert!(state.button_regions.contains(&key.region()));
            }
            if millis == 355 {
                assert!(!state.input_keyboard_open());
                assert!(
                    state.screen_keyboard.visibility > 0
                        && state.screen_keyboard.visibility < 1_000
                );
                assert!(
                    !keyboard.keyboard.is_empty(),
                    "closing panel must remain visible"
                );
            }
        }
        assert!(state.input_keyboard_layout().keyboard.is_empty());
        assert_eq!(state.command_line_input_area(), original);
    }
}

#[test]
fn auto_admin_pointer_entry_honors_speed_and_keyboard_close_is_immediate() {
    for (speed, reduced) in [(50, false), (100, false), (200, false), (100, true)] {
        let mut state = session();
        state.interaction.pointer_motion = true;
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut prepared = home_frame(&state, 0, reduced);
        prepared.context.motion.animation_speed_percent = speed;
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let base = terminal.backend().buffer().clone();
        let main = state.frame_layout.unwrap().main;
        let (responses, _inputs) = mpsc::channel();
        state
            .begin_auto_admin("Review target".into(), true, responses)
            .unwrap();
        let dialog =
            ui::auto_admin_layout(Rect::new(0, 0, 120, 40), &state.auto_admin_view().unwrap())
                .dialog;
        assert_eq!(
            draw(&mut compositor, &mut terminal, &mut state, &prepared),
            !reduced
        );
        let entering = terminal.backend().buffer().clone();
        if !reduced {
            let position = state.interaction.pointer_position;
            compositor.prepare_input(
                &mut state,
                InputEvent::mouse_down(PointerButton::Left, (dialog.x + 1, dialog.y + 1)),
                &platform::mock::UnsupportedPlatform,
                Instant::now(),
            );
            assert_eq!(state.interaction.pointer_position, position);
        }
        let mut frames = 0;
        while compositor.motion.is_running() {
            frames += 1;
            assert!(frames <= 10, "AA animation must stop requesting redraws");
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
        }
        if reduced {
            assert_eq!(frames, 0);
        } else {
            assert_eq!(frames, (18000_u32).div_ceil(50 * u32::from(speed)));
            assert_ne!(
                entering[(dialog.x, dialog.y)],
                terminal.backend().buffer()[(dialog.x, dialog.y)]
            );
        }
        for position in base.area.positions().filter(|p| !main.contains(*p)) {
            assert_eq!(
                entering[position], base[position],
                "AA motion changed Shell chrome"
            );
        }
        // Ordinary repainting does not replay the entry effect or delay input.
        assert!(!draw(&mut compositor, &mut terminal, &mut state, &prepared));
        state.apply_input(InputEvent::key(InputKey::F(12)));
        assert!(!state.auto_admin_visible());
        assert!(!draw(&mut compositor, &mut terminal, &mut state, &prepared));
        assert!(!compositor.motion.is_running());
        assert_eq!(
            terminal.backend().buffer(),
            &base,
            "AA exit must restore the page"
        );
    }
}

#[test]
fn pointer_input_hides_home_focus_but_keeps_launcher_selection() {
    for mode in 0..3 {
        let launcher = mode != 0;
        let mut state = session();
        if launcher {
            state.enter_screen(ShellScreen::Launcher);
            if mode == 2 {
                state.pages.launcher.launcher_view_mode = ui::LauncherViewMode::Details;
            }
            state.refresh_hit_map();
        }
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let render = |state: &mut ShellSession,
                      compositor: &mut ScreenCompositor,
                      terminal: &mut Terminal<TestBackend>| {
            let mut prepared = home_frame(state, 0, true);
            if launcher {
                prepared.page = ScreenViewModel::Launcher(Box::new(state.to_launcher_view_model()));
            }
            draw(compositor, terminal, state, &prepared);
        };
        render(&mut state, &mut compositor, &mut terminal);
        assert!(state.interaction.keyboard_focus_visible());
        state.apply_input(InputEvent::key(InputKey::Home));
        assert!(state.interaction.keyboard_focus_visible());
        render(&mut state, &mut compositor, &mut terminal);
        let id = if launcher {
            format!("launcher.item.{}", app::EDITOR_APPLICATION.id)
        } else {
            "home.entry.0".to_string()
        };
        let area = if mode == 2 {
            let ui::ShellLayout::Full { main, .. } =
                state.shell_layout_for(Rect::new(0, 0, 120, 40))
            else {
                panic!("full layout required");
            };
            ui::launcher_layout(main, &state.to_launcher_view_model()).items[0].area
        } else {
            state
                .button_regions
                .iter()
                .find(|region| region.id.as_str() == id)
                .unwrap()
                .area
        };
        let border = (area.x, area.y);
        let focused_color = terminal.backend().buffer()[border].fg;
        let selection = if launcher {
            state.pages.launcher.launcher_selected_index
        } else {
            state.pages.home.selected_home_entry_index
        };
        for pointer in [
            InputEvent::mouse_moved((0, 0)),
            InputEvent::mouse_down(PointerButton::Left, (0, 0)),
        ] {
            state.apply_input(pointer);
            assert!(!state.interaction.keyboard_focus_visible());
            render(&mut state, &mut compositor, &mut terminal);
            if launcher {
                assert_eq!(terminal.backend().buffer()[border].fg, focused_color);
            } else {
                assert_ne!(terminal.backend().buffer()[border].fg, focused_color);
            }
            assert_eq!(
                if launcher {
                    state.pages.launcher.launcher_selected_index
                } else {
                    state.pages.home.selected_home_entry_index
                },
                selection
            );
            for ignored in [
                InputEvent::Key(KeyInput::new(InputKey::Home).released()),
                InputEvent::key(InputKey::Char('z')),
                InputEvent::Tick,
                InputEvent::FocusGained,
            ] {
                state.apply_input(ignored);
                assert!(!state.interaction.keyboard_focus_visible());
            }
            state.apply_input(InputEvent::key(InputKey::Home));
            render(&mut state, &mut compositor, &mut terminal);
            assert_eq!(terminal.backend().buffer()[border].fg, focused_color);
        }
        state.apply_input(InputEvent::FocusLost);
        assert!(!state.interaction.keyboard_focus_visible());
    }
}

#[test]
fn pointer_back_button_does_not_restore_keyboard_focus() {
    let mut state = session();
    state.apply_input(InputEvent::key(InputKey::Home));
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let prepared = home_frame(&state, 0, true);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let area = state
        .button_regions
        .iter()
        .find(|region| region.id.as_str() == "shell.back")
        .unwrap()
        .area;
    let point = (area.x, area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(!state.interaction.keyboard_focus_visible());
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert!(!state.interaction.keyboard_focus_visible());
}

#[test]
fn command_line_back_button_has_no_keyboard_shortcut_and_routes_to_confirmation() {
    let mut state = session();
    assert_eq!(state.to_shell_chrome_view_model().back_shortcut, "Esc");
    state.enter_screen(ShellScreen::CommandLine);
    state.refresh_hit_map();
    assert_eq!(state.to_shell_chrome_view_model().back_shortcut, "");
    let back = state.frame_layout.unwrap().back_button.unwrap();
    assert!(back.width < 18);
    let input = InputEvent::mouse_down(PointerButton::Left, (back.x, back.y));
    assert_eq!(state.normalize_shell_navigation_input(input.clone()), input);
    assert_eq!(
        state.route_input_at(input, Instant::now()).command,
        ShellCommand::RequestCommandLineClose
    );
}

#[test]
fn compact_escape_button_closes_pages_and_modals_only_on_matching_release() {
    for (width, height) in [(49, 11), (30, 8)] {
        let mut state = session();
        state.apply_input(InputEvent::Resize { width, height });
        state.enter_screen(ShellScreen::Clock);
        state.refresh_hit_map();
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut prepared = home_frame(&state, 0, true);
        prepared.page = ScreenViewModel::Clock(Box::new(
            state.to_clock_view_model_at(&state.app.snapshot().clock, Instant::now()),
        ));
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let back = state.frame_layout.unwrap().back_button.unwrap();
        assert!(text(terminal.backend().buffer(), back).contains("[◀ Esc]"));
        assert_eq!(
            back.intersection(state.frame_layout.unwrap().main).area(),
            0
        );
        let point = (back.x + back.width / 2, back.y);
        let now = Instant::now();
        state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
        assert_eq!(state.active_screen(), ShellScreen::Clock);
        state.apply_input_at(
            InputEvent::mouse_up(PointerButton::Left, point),
            now + Duration::from_millis(10),
        );
        assert_eq!(state.active_screen(), ShellScreen::Home);

        // Home still reaches the exit dialog through the same global button.
        let prepared = home_frame(&state, 0, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
        assert_eq!(state.active_screen(), ShellScreen::Home);
        state.apply_input_at(
            InputEvent::mouse_up(PointerButton::Left, point),
            now + Duration::from_millis(10),
        );
        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        let mut prepared = home_frame(&state, 0, true);
        prepared.notification = state.to_notification_view_model();
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        assert!(text(terminal.backend().buffer(), back).contains("[◀ Esc]"));
        assert_eq!(
            state.hit_map().target_at(point),
            Some(ShellComponent::BackButton)
        );
        if let ui::NotificationLayout::Dialog(dialog) = ui::notification_layout(
            state.frame_layout.unwrap().modal_area(),
            prepared.notification.as_ref().unwrap(),
        ) {
            assert_eq!(dialog.dialog.intersection(back).area(), 0);
            for action in &dialog.actions {
                assert_eq!(
                    state.notification_action_index_at((action.area.x, action.area.y)),
                    Some(action.index)
                );
            }
        } else {
            assert_eq!((width, height), (30, 8));
        }
        state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        state.apply_input_at(
            InputEvent::mouse_up(PointerButton::Left, point),
            now + Duration::from_millis(10),
        );
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert!(!state.shutdown_requested());
    }
}

#[test]
fn alert_preempts_an_exiting_toast_and_reduced_motion_has_no_animation_demand() {
    let mut state = session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, false);
    prepared.chrome.status.toast = Some("Saved changes".into());
    compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    prepared.context.motion.now = Duration::from_millis(100);
    prepared.chrome.status.toast = None;
    compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
    assert!(compositor.toast.is_some());
    prepared.chrome.status.error = Some("Disk is full".into());
    prepared.context.motion = ui::MotionFrame::reduced(Duration::from_millis(150));
    compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
    assert!(compositor.toast.is_none());
    assert!(!draw(&mut compositor, &mut terminal, &mut state, &prepared));
    let output = text(
        terminal.backend().buffer(),
        state.frame_layout.unwrap().status_message.unwrap(),
    );
    assert!(output.contains("Disk is full"));
    assert!(!output.contains("Saved changes"));
}

#[test]
fn compositor_resize_uses_actual_frame_geometry_for_chrome_and_hit_map() {
    let mut state = session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let prepared = home_frame(&state, 0, true);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    terminal.backend_mut().resize(90, 25);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let layout = state.frame_layout.unwrap();
    assert_eq!(layout.bounds, Rect::new(0, 0, 90, 25));
    assert_eq!(state.hit_map().terminal_size(), (90, 25));
    assert!(text(terminal.backend().buffer(), Rect::new(0, 0, 90, 3)).contains("90x25"));
    for region in state.hit_map().regions() {
        assert_eq!(region.area.intersection(layout.bounds), region.area);
    }
    terminal.backend_mut().resize(49, 11);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert!(state.frame_layout.unwrap().is_compact());
    assert!(state.hit_map().regions().iter().all(|region| !matches!(
        region.component,
        ShellComponent::TopBar | ShellComponent::StatusBar | ShellComponent::ClockButton
    )));
}

#[test]
fn global_notification_is_the_only_shell_modal_visual() {
    let mut state = session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.notification = Some(ui::NotificationViewModel::new(
        "test.modal",
        ui::NotificationLevel::Modal,
        ui::NotificationTone::Warning,
        "Confirm operation",
        "Keep this dialog visible",
        vec![],
    ));
    prepared.time_sync = Some(ui::TimeSyncDialogViewModel::new());
    prepared.chrome.status.toast = Some("Background toast".into());
    compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let output = text(terminal.backend().buffer(), Rect::new(0, 0, 120, 40));
    assert!(output.contains("Keep this dialog visible"));
    assert!(!output.contains(&ui::TimeSyncDialogViewModel::new().message()));
}

#[test]
fn exit_dialog_round_trip_with_keyboard_mouse_or_touch_alone() {
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    // Touch uses the terminal's left-button stream with no hover event.
    for input_mode in ["keyboard", "mouse", "touch"] {
        let mut state = session();
        let original_page = state.active_page_id();
        let original_focus = state.focused_component();
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let prepared = home_frame(&state, 0, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let tap = |state: &mut ShellSession, area: Rect| {
            let point = (area.x + area.width / 2, area.y + area.height / 2);
            let kinds = if input_mode == "mouse" {
                vec![
                    MouseEventKind::Moved,
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ]
            } else {
                vec![
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ]
            };
            let at = Instant::now();
            for (index, kind) in kinds.into_iter().enumerate() {
                state.apply_input_at(
                    crossterm_event_to_input(Event::Mouse(MouseEvent {
                        kind,
                        column: point.0,
                        row: point.1,
                        modifiers: KeyModifiers::NONE,
                    })),
                    at + Duration::from_millis(index as u64 * 10),
                );
            }
        };
        if input_mode == "keyboard" {
            state.apply_input(crossterm_event_to_input(Event::Key(KeyEvent::new(
                KeyCode::Esc,
                KeyModifiers::NONE,
            ))));
        } else {
            let back = state.frame_layout.unwrap().back_button.unwrap();
            tap(&mut state, back);
        }
        assert!(state.notification_has_active_modal(), "{input_mode}");
        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        let mut prepared = home_frame(&state, 50, true);
        prepared.notification = state.to_notification_view_model();
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        if input_mode == "keyboard" {
            state.apply_input(crossterm_event_to_input(Event::Key(KeyEvent::new(
                KeyCode::Esc,
                KeyModifiers::NONE,
            ))));
        } else {
            let cancel = state
                .button_regions
                .iter()
                .find(|region| region.id.as_str().ends_with(".action.cancel"))
                .unwrap()
                .area;
            tap(&mut state, cancel);
        }
        assert!(!state.notification_has_active_modal(), "{input_mode}");
        assert!(!state.shutdown_requested());
        assert_eq!(state.active_page_id(), original_page);
        assert_eq!(state.focused_component(), original_focus);
        assert_eq!(
            state.interaction.keyboard_focus_visible(),
            input_mode == "keyboard"
        );
        assert!(state.interaction.button_capture.is_none());
    }
}

#[test]
fn rendered_chrome_buttons_hover_press_and_activate_only_on_release() {
    let mut state = session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.chrome.status.time_button_label = Some("12:00".into());
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let back = state
        .button_regions
        .iter()
        .find(|r| r.id.as_str() == "shell.back")
        .unwrap()
        .clone();
    let point = (back.area.x + 3, back.area.y + 1);
    let theme = prepared.context.compatibility_theme();
    state.apply_input(InputEvent::mouse_moved(point));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(
        terminal.backend().buffer()[point].fg,
        theme.button_hover_color()
    );
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::Home);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(
        terminal.backend().buffer()[point].fg,
        theme.button_pressed_color()
    );
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert!(state.interaction.button_capture.is_none());
}

#[test]
fn button_capture_cancels_outside_on_focus_loss_resize_and_page_change() {
    for cancel in 0..5 {
        let mut state = session();
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let prepared = home_frame(&state, 0, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let back = state
            .button_regions
            .iter()
            .find(|r| r.id.as_str() == "shell.back")
            .unwrap()
            .area;
        let point = (back.x + 3, back.y + 1);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        match cancel {
            0 => {
                state.apply_input(InputEvent::mouse_up(PointerButton::Left, (0, 0)));
            }
            1 => {
                state.apply_input(InputEvent::FocusLost);
            }
            2 => {
                state.apply_input(InputEvent::Resize {
                    width: 130,
                    height: 42,
                });
            }
            3 => {
                state.enter_screen(ShellScreen::Clock);
            }
            _ => {
                state.apply_input(InputEvent::mouse_drag(PointerButton::Left, point));
            }
        }
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        assert_ne!(state.active_screen(), ShellScreen::ExitConfirm);
        assert!(state.interaction.button_capture.is_none());
    }
}

#[test]
fn button_click_allows_short_presses_but_rejects_holds_over_500_ms() {
    for kind in 0..4 {
        for millis in [0, 1, 499, 500, 501, 2_000] {
            let mut state = session();
            if kind == 1 || kind == 2 {
                state.enter_screen(ShellScreen::Launcher);
                if kind == 2 {
                    state.pages.launcher.launcher_view_mode = ui::LauncherViewMode::Details;
                }
                state.refresh_hit_map();
            } else if kind == 3 {
                state.apply_input(InputEvent::key(InputKey::Escape));
            }
            let mut compositor = ScreenCompositor::default();
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let mut prepared = home_frame(&state, 0, true);
            let point = if kind == 1 || kind == 2 {
                let model = state.to_launcher_view_model();
                let ui::ShellLayout::Full { main, .. } =
                    state.shell_layout_for(Rect::new(0, 0, 120, 40))
                else {
                    panic!("full layout required");
                };
                let layout = ui::launcher_layout(main, &model);
                let editor = model
                    .items
                    .iter()
                    .position(|item| item.id == app::EDITOR_APPLICATION.id)
                    .unwrap();
                let area = layout
                    .items
                    .iter()
                    .find(|item| item.index == editor)
                    .unwrap()
                    .area;
                prepared.page = ScreenViewModel::Launcher(Box::new(model));
                draw(&mut compositor, &mut terminal, &mut state, &prepared);
                (area.x + 1, area.y)
            } else {
                prepared.notification = state.to_notification_view_model();
                draw(&mut compositor, &mut terminal, &mut state, &prepared);
                let area = state
                    .button_regions
                    .iter()
                    .find(|region| {
                        if kind == 0 {
                            region.id.as_str() == "shell.back"
                        } else {
                            region.id.as_str().starts_with("notification.")
                        }
                    })
                    .unwrap()
                    .area;
                (area.x, area.y)
            };
            let now = Instant::now();
            let before = state.active_screen();
            let modal_before = state.notification_active_modal_id();
            state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
            assert_eq!(state.active_screen(), before);
            state.apply_input_at(
                InputEvent::mouse_up(PointerButton::Left, point),
                now + Duration::from_millis(millis),
            );
            let activated = match kind {
                0 => state.active_screen() == ShellScreen::ExitConfirm,
                1 | 2 => state.active_screen() == ShellScreen::Editor,
                _ => state.notification_active_modal_id() != modal_before,
            };
            assert_eq!(activated, millis <= 500, "kind={kind}, millis={millis}");
            assert!(state.interaction.button_capture.is_none());
            if millis > 500 {
                // A late extra release must not resurrect a cancelled click.
                state.apply_input_at(
                    InputEvent::mouse_up(PointerButton::Left, point),
                    now + Duration::from_millis(millis + 1),
                );
                assert_eq!(state.active_screen(), before);
                assert!(!state.shutdown_requested());
            }
        }
    }
}

#[test]
fn clock_create_cancel_closes_on_release_with_or_without_hover() {
    for hover in [false, true] {
        let mut state = session();
        state.enter_screen(ShellScreen::Clock);
        state.restore_clock_profile(Default::default());
        state.open_clock_create_dialog();
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut prepared = home_frame(&state, 0, true);
        prepared.page = ScreenViewModel::Clock(Box::new(state.to_clock_view_model()));
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let cancel = state
            .button_regions
            .iter()
            .find(|region| region.id.as_str() == "clock.create-cancel")
            .unwrap()
            .area;
        let point = (cancel.x + cancel.width / 2, cancel.y);
        if hover {
            state.apply_input(InputEvent::mouse_moved(point));
        }
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        assert!(state.pages.clock.clock_create_state.is_some());
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, (0, 0)));
        assert!(state.pages.clock.clock_create_state.is_some());
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        assert!(state.pages.clock.clock_create_state.is_none());
    }
}

#[test]
fn launcher_card_single_click_waits_for_release() {
    let mut state = session();
    state.enter_screen(ShellScreen::Launcher);
    state.refresh_hit_map();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.page = ScreenViewModel::Launcher(Box::new(state.to_launcher_view_model()));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let id = format!("launcher.item.{}", app::EDITOR_APPLICATION.id);
    let area = state
        .button_regions
        .iter()
        .find(|r| r.id.as_str() == id)
        .unwrap()
        .area;
    let point = (area.x + 1, area.y + 1);
    let now = Instant::now();
    state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    state.apply_input_at(
        InputEvent::mouse_up(PointerButton::Left, point),
        now + Duration::from_millis(30),
    );
    assert_eq!(state.active_screen(), ShellScreen::Editor);
}

#[test]
fn launcher_click_launches_only_after_matching_release_in_both_views() {
    for view in [
        ui::LauncherViewMode::LargeIcons,
        ui::LauncherViewMode::Details,
    ] {
        for cancel in 0..5 {
            let mut state = session();
            state.enter_screen(ShellScreen::Launcher);
            state.pages.launcher.launcher_view_mode = view;
            state.refresh_hit_map();
            let mut compositor = ScreenCompositor::default();
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let mut prepared = home_frame(&state, 0, true);
            let model = state.to_launcher_view_model();
            let ui::ShellLayout::Full { main, .. } =
                state.shell_layout_for(Rect::new(0, 0, 120, 40))
            else {
                panic!("full layout required");
            };
            let layout = ui::launcher_layout(main, &model);
            let editor = model
                .items
                .iter()
                .position(|item| item.id == app::EDITOR_APPLICATION.id)
                .unwrap();
            let area = layout
                .items
                .iter()
                .find(|item| item.index == editor)
                .unwrap()
                .area;
            let point = (area.x + 1, area.y);
            prepared.page = ScreenViewModel::Launcher(Box::new(model));
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
            state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
            assert_eq!(state.active_screen(), ShellScreen::Launcher);
            match cancel {
                1 => {
                    state.apply_input(InputEvent::mouse_up(PointerButton::Left, (0, 0)));
                }
                2 => {
                    state.apply_input(InputEvent::FocusLost);
                }
                3 => {
                    state.apply_input(InputEvent::mouse_drag(PointerButton::Left, point));
                }
                4 => {
                    state.apply_input(InputEvent::Resize {
                        width: 130,
                        height: 42,
                    });
                }
                _ => {}
            }
            state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
            assert_eq!(
                state.active_screen(),
                if cancel == 0 {
                    ShellScreen::Editor
                } else {
                    ShellScreen::Launcher
                },
                "view={view:?}, cancel={cancel}"
            );
        }
    }
}

#[test]
fn home_entry_single_click_expands_from_its_border_and_returns_to_it() {
    struct StorageFixtureGuard(PathBuf);
    impl Drop for StorageFixtureGuard {
        fn drop(&mut self) {
            let _ = platform::cleanup_temp_path(&self.0);
        }
    }

    let root = platform::create_temp_dir(
        &std::env::temp_dir().join("TundraUX3-shell-tests"),
        "home-launcher-click",
    )
    .expect("create private Launcher storage fixture");
    let _storage_guard = StorageFixtureGuard(root.clone());
    let paths = platform::build_linux_app_paths(
        root.join("Config"),
        root.join("Data"),
        root.join("Cache"),
        root.join("State"),
        root.join("Temp"),
    )
    .unwrap();
    let manager = StorageManager::open(paths.clone()).unwrap().manager;
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("UserData"),
    )
    .unwrap();
    let platform = platform::mock::MockPlatform::new(user_dirs, paths);
    let launcher_session = || {
        let mut state = session();
        state.storage_manager = Some(manager.clone());
        state.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(AuthSession {
                source: identity::IdentitySource::LocalAccount,
                session_id: "click-session".into(),
                user_id: "click-user".into(),
                username: "click-user".into(),
                role: UserRole::User,
                started_at_epoch_ms: 1,
            })),
            Instant::now(),
        );
        state.refresh_hit_map();
        state
    };
    // Check the storage and authorization prerequisites through the real
    // keyboard path before testing pointer activation on an independent session.
    let mut keyboard_state = launcher_session();
    let launcher_index = keyboard_state
        .user_home_entries()
        .iter()
        .position(|entry| entry.icon_identity() == "launcher")
        .unwrap();
    keyboard_state.select_home_entry(launcher_index);
    keyboard_state.apply_input_with_platform(InputEvent::key(InputKey::Enter), &platform);
    assert_eq!(keyboard_state.active_screen(), ShellScreen::Launcher);
    assert!(keyboard_state.pages.common.error_message.is_none());

    for hover in [false, true] {
        let mut state = launcher_session();
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let prepared = home_frame(&state, 0, false);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let launcher_id = format!("home.entry.{launcher_index}");
        let area = state
            .button_regions
            .iter()
            .find(|r| r.id.as_str() == launcher_id)
            .unwrap()
            .area;
        let point = (area.x + 1, area.y + 1);
        assert_eq!(state.home_entry_index_at(point), Some(launcher_index));
        if hover {
            state.apply_input_with_platform(InputEvent::mouse_moved(point), &platform);
        }
        let pressed_at = Instant::now();
        state.apply_input_with_platform_at(
            InputEvent::mouse_down(PointerButton::Left, point),
            &platform,
            pressed_at,
        );
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(
            state
                .interaction
                .button_capture
                .as_ref()
                .map(|capture| capture.region.id.as_str()),
            Some(launcher_id.as_str())
        );
        state.apply_input_with_platform_at(
            InputEvent::mouse_up(PointerButton::Left, point),
            &platform,
            pressed_at + Duration::from_millis(10),
        );
        assert_eq!(state.active_screen(), ShellScreen::Launcher);
        assert!(state.pages.common.error_message.is_none());
        assert_eq!(state.content_opening_button(), Some(area));
        let mut prepared = home_frame(&state, 1, false);
        prepared.page = ScreenViewModel::Launcher(Box::new(state.to_launcher_view_model()));
        assert!(draw(&mut compositor, &mut terminal, &mut state, &prepared));
        assert_eq!(terminal.backend().buffer()[(area.x, area.y)].symbol(), "╭");
        prepared.context.motion.delta = ui::MotionTimings::PAGE;
        assert!(!draw(&mut compositor, &mut terminal, &mut state, &prepared));
        let main = state.frame_layout.unwrap().main;
        let back = state.frame_layout.unwrap().back_button.unwrap();
        let point = (back.x + 1, back.y + 1);
        state.apply_input_with_platform(
            InputEvent::mouse_down(PointerButton::Left, point),
            &platform,
        );
        state
            .apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, point), &platform);
        assert_eq!(state.active_screen(), ShellScreen::Home);
        let mut prepared = home_frame(&state, 2, false);
        assert!(draw(&mut compositor, &mut terminal, &mut state, &prepared));
        prepared.context.motion.delta = ui::MotionTimings::PAGE / 2;
        assert!(draw(&mut compositor, &mut terminal, &mut state, &prepared));
        let current = ui::reveal_rect(main, 500, ui::RevealOrigin::Button(area));
        assert_eq!(
            terminal.backend().buffer()[(current.x, current.y)].symbol(),
            "╭"
        );
        assert!(!draw(&mut compositor, &mut terminal, &mut state, &prepared));
        assert_eq!(terminal.backend().buffer()[(area.x, area.y)].symbol(), "╭");
    }
}

#[test]
fn exit_buttons_activate_on_first_release_across_ticks_and_redraws() {
    let root = std::env::temp_dir().join("tundra-exit-click-test");
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("AppData"),
    )
    .unwrap();
    let paths = platform::build_windows_app_paths(
        root.join("Roaming"),
        root.join("Local"),
        root.join("Temp"),
    )
    .unwrap();
    let platform = platform::mock::MockPlatform::new(user_dirs, paths);
    for (id, expected, restart) in [
        ("restore-terminal", ShellAction::Exit, false),
        ("restart", ShellAction::Exit, true),
        ("reboot", ShellAction::Reboot, false),
        ("poweroff", ShellAction::PowerOff, false),
        ("cancel", ShellAction::Redraw, false),
    ] {
        let mut state = session();
        state.apply_input_with_platform(InputEvent::key(InputKey::Escape), &platform);
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut prepared = home_frame(&state, 0, true);
        prepared.notification = state.to_notification_view_model();
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let area = state
            .button_regions
            .iter()
            .find(|region| region.id.as_str().ends_with(&format!(".action.{id}")))
            .unwrap()
            .area;
        let point = (area.x, area.y);
        let pressed_at = Instant::now();
        state.apply_input_with_platform_at(
            InputEvent::mouse_down(PointerButton::Left, point),
            &platform,
            pressed_at,
        );
        assert!(!state.shutdown_requested());
        for millis in [50, 100] {
            state.apply_input_with_platform_at(
                InputEvent::Tick,
                &platform,
                pressed_at + Duration::from_millis(millis),
            );
            prepared.notification = state.to_notification_view_model();
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
        }
        assert_eq!(
            state.apply_input_with_platform_at(
                InputEvent::mouse_up(PointerButton::Left, point),
                &platform,
                pressed_at + Duration::from_millis(150),
            ),
            expected,
            "first click on {id}",
        );
        assert_eq!(state.restart_requested, restart, "{id}");
        assert!(!state.notification_has_active_modal(), "{id}");
    }
}

#[test]
fn notification_button_release_after_focus_loss_does_not_activate() {
    let mut state = session();
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.notification = state.to_notification_view_model();
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let area = state
        .button_regions
        .iter()
        .find(|region| region.id.as_str().starts_with("notification."))
        .unwrap()
        .area;
    let point = (area.x, area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(state.notification_pointer_capture.is_some());
    state.apply_input(InputEvent::FocusLost);
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert!(!state.shutdown_requested());
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
}

#[test]
fn scrolling_or_leaving_a_pressed_modal_button_cancels_the_click() {
    for wheel in [false, true] {
        let mut state = session();
        state.apply_input(InputEvent::key(InputKey::Escape));
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut prepared = home_frame(&state, 0, true);
        prepared.notification = state.to_notification_view_model();
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let area = state
            .button_regions
            .iter()
            .find(|region| region.id.as_str().ends_with(".action.restore-terminal"))
            .unwrap()
            .area;
        let point = (area.x, area.y);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        assert!(state.interaction.button_capture.is_some());
        state.apply_input(if wheel {
            InputEvent::mouse_scroll(ScrollDirection::Down, point)
        } else {
            InputEvent::mouse_moved((0, 0))
        });
        state.apply_input(InputEvent::mouse_moved(point));
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        assert!(!state.shutdown_requested());
        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        assert!(state.interaction.button_capture.is_none());
    }
}
