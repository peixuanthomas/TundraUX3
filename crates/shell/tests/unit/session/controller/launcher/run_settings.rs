use super::*;
use platform::{PlatformKind, mock::MockPlatform};
use ratatui::{Terminal, backend::TestBackend};

struct Fixture {
    state: ShellSession,
    platform: MockPlatform,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tundra-run-settings-{}-{}",
            std::process::id(),
            PageInstanceId::new().get()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let platform = MockPlatform::new(
            platform::UserDirs::new(
                root.join("Desktop"),
                root.join("Documents"),
                root.join("Downloads"),
                root.join("Pictures"),
                root.join("Videos"),
                root.join("Music"),
                root.join("Roaming"),
            )
            .unwrap(),
            platform::build_windows_app_paths(
                root.join("Roaming"),
                root.join("Local"),
                root.join("Temp"),
            )
            .unwrap(),
        )
        .with_kind(PlatformKind::Windows);
        let storage = StorageManager::open(platform.app_paths().unwrap())
            .unwrap()
            .manager;
        let mut config = storage.load_config().unwrap();
        config.launcher.entries.push(storage::LauncherEntryRecord {
            id: "tui".into(),
            path: root.join("program.exe").to_string_lossy().into_owned(),
            executable_kind: Some(LauncherExecutableKind::NativeBinary),
            ..Default::default()
        });
        storage.save_config(&config).unwrap();
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.storage_manager = Some(storage);
        state.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(AuthSession {
                source: identity::IdentitySource::LocalAccount,
                session_id: "run-settings".into(),
                user_id: "admin".into(),
                username: "admin".into(),
                role: UserRole::Admin,
                started_at_epoch_ms: 1,
            })),
            Instant::now(),
        );
        while state.notification_dismiss_active_modal_without_response() {}
        state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::Launcher]);
        state.app.dispatch_at(
            app::AppCommand::SetLauncherState(Some(LauncherState::from_config(&config.launcher))),
            Instant::now(),
        );
        state.pages.launcher.launcher_selected_index = state.built_in_launcher_count();
        state.pages.common.focused_component = ShellComponent::Launcher;
        state.refresh_hit_map();
        Self {
            state,
            platform,
            root,
        }
    }

    fn input(&mut self, input: InputEvent) {
        self.state.apply_input_with_platform(input, &self.platform);
    }

    fn draw(&mut self) -> ui::LauncherLayout {
        let mut terminal = Terminal::new(TestBackend::new(
            self.state.terminal_size.0,
            self.state.terminal_size.1,
        ))
        .unwrap();
        let theme = ui::TundraTheme::default_dark();
        let buttons = ui::components::ButtonFrame::new(None, None, &theme);
        let mut context =
            ui::RenderContext::from_theme(&theme, Default::default(), Default::default());
        context.buttons = Some(buttons.clone());
        let model = self.state.to_launcher_view_model();
        let ui::ShellLayout::Full { main, .. } = self.state.shell_layout_for(Rect::new(
            0,
            0,
            self.state.terminal_size.0,
            self.state.terminal_size.1,
        )) else {
            panic!("full layout");
        };
        terminal
            .draw(|frame| {
                ui::render_launcher_content(frame, main, &model, &context, None);
                ui::render_launcher_overlay(frame, main, &model, &context);
            })
            .unwrap();
        self.state.button_regions = buttons.regions();
        ui::launcher_layout(main, &model)
    }

    fn click(&mut self, area: Rect, hover: bool) {
        let point = (area.x, area.y);
        if hover {
            self.input(InputEvent::mouse_moved(point));
        }
        self.input(InputEvent::mouse_down(PointerButton::Left, point));
        self.input(InputEvent::mouse_up(PointerButton::Left, point));
    }

    fn main_area(&self) -> Rect {
        let ui::ShellLayout::Full { main, .. } = self.state.shell_layout_for(Rect::new(
            0,
            0,
            self.state.terminal_size.0,
            self.state.terminal_size.1,
        )) else {
            panic!("full layout");
        };
        main
    }

    fn choose_context_action(&mut self, index: usize, hover: bool) {
        let layout = self.draw();
        let selected = self.state.to_launcher_view_model().selected_index.unwrap();
        let area = layout
            .items
            .iter()
            .find(|item| item.index == selected)
            .unwrap()
            .area;
        self.input(InputEvent::mouse_down(
            PointerButton::Right,
            (area.x + 1, area.y),
        ));
        self.input(InputEvent::mouse_up(
            PointerButton::Right,
            (area.x + 1, area.y),
        ));
        let menu = self
            .state
            .pages
            .launcher
            .context_menu
            .as_ref()
            .unwrap()
            .menu
            .clone();
        let (_, rows) = ui::launcher_context_menu_layout(self.main_area(), &menu);
        self.draw();
        self.click(rows[index], hover);
    }
}

#[test]
fn context_menu_edits_and_deletes_user_entries_in_both_views() {
    for (mode, hover) in [
        (app::launcher::LauncherViewMode::LargeIcons, true),
        (app::launcher::LauncherViewMode::Details, false),
    ] {
        let mut fixture = Fixture::new();
        fixture.state.pages.launcher.launcher_view_mode = mode;
        let storage = fixture.state.storage_manager.clone().unwrap();
        let original_id = storage.load_config().unwrap().launcher.entries[0]
            .id
            .clone();
        fixture.choose_context_action(0, hover);
        fixture.input(InputEvent::key_with_modifiers(
            InputKey::Char('a'),
            InputModifiers::CTRL,
        ));
        fixture.input(InputEvent::Paste("中文 renamed".into()));
        fixture.input(InputEvent::from_key_label("Enter"));
        assert_eq!(
            storage.load_config().unwrap().launcher.entries[0]
                .name
                .as_deref(),
            Some("中文 renamed")
        );
        assert_eq!(
            fixture
                .state
                .to_launcher_view_model()
                .items
                .last()
                .unwrap()
                .name,
            "中文 renamed"
        );
        fixture.choose_context_action(0, hover);
        fixture.input(InputEvent::Paste("discarded".into()));
        fixture.input(InputEvent::from_key_label("Esc"));
        assert_eq!(
            storage.load_config().unwrap().launcher.entries[0]
                .name
                .as_deref(),
            Some("中文 renamed")
        );
        fixture.choose_context_action(1, hover);
        fixture.input(InputEvent::key_with_modifiers(
            InputKey::Char('a'),
            InputModifiers::CTRL,
        ));
        fixture.input(InputEvent::from_key_label("Backspace"));
        fixture.input(InputEvent::from_key_label("F10"));
        assert!(
            fixture
                .state
                .pages
                .launcher
                .entry_editor
                .as_ref()
                .unwrap()
                .error
                .is_some()
        );
        fixture.input(InputEvent::Paste("printf 'changed command'".into()));
        fixture.input(InputEvent::from_key_label("F10"));
        assert!(fixture.state.pages.launcher.entry_editor.is_none());
        let record = storage.load_config().unwrap().launcher.entries.remove(0);
        assert_eq!(record.id, original_id);
        assert_eq!(record.custom.unwrap().command, "printf 'changed command'");
        fixture.choose_context_action(2, hover);
        assert!(fixture.state.page_keyboard_identity().is_some());
        fixture.input(InputEvent::Paste("中文x\n★ e\u{301}".into()));
        let draft = fixture.state.pages.launcher.entry_editor.clone().unwrap();
        assert_eq!(draft.inputs[0].value(), "中文x");
        assert_eq!(draft.inputs[1].value(), "★ e\u{301}");
        for input in [
            InputEvent::from_key_label("z"),
            InputEvent::Paste("a\nb\nc".into()),
            InputEvent::Paste("\u{1b}[31m".into()),
        ] {
            fixture.input(input);
            let editor = fixture.state.pages.launcher.entry_editor.as_ref().unwrap();
            assert_eq!(editor.inputs, draft.inputs);
            assert!(editor.error.is_some());
        }
        fixture.input(InputEvent::from_key_label("F10"));
        let record = storage.load_config().unwrap().launcher.entries.remove(0);
        assert_eq!(record.custom.unwrap().icon, "中文x\n★ e\u{301}");
        assert_eq!(
            fixture
                .state
                .to_launcher_view_model()
                .items
                .last()
                .unwrap()
                .custom_icon
                .as_deref(),
            Some("中文x\n★ e\u{301}")
        );
        assert!(!fixture.state.pages.launcher.organizing);
        fixture.choose_context_action(3, hover);
        assert!(
            fixture
                .state
                .pages
                .launcher
                .launcher_pending_confirmation
                .is_some()
        );
        fixture.input(InputEvent::from_key_label("Esc"));
        assert_eq!(storage.load_config().unwrap().launcher.entries.len(), 1);
        fixture.choose_context_action(3, hover);
        fixture.input(InputEvent::from_key_label("Enter"));
        assert!(storage.load_config().unwrap().launcher.entries.is_empty());
        assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
    }
}

#[test]
fn context_menu_outside_clicks_and_canceled_presses_do_not_activate_the_page() {
    for hover in [false, true] {
        let mut fixture = Fixture::new();
        let layout = fixture.draw();
        let area = layout
            .items
            .iter()
            .find(|item| item.index == fixture.state.built_in_launcher_count())
            .unwrap()
            .area;
        let point = (area.x + 1, area.y);
        fixture.input(InputEvent::mouse_down(PointerButton::Right, point));
        let menu = fixture
            .state
            .pages
            .launcher
            .context_menu
            .as_ref()
            .unwrap()
            .menu
            .clone();
        let (menu_area, rows) = ui::launcher_context_menu_layout(fixture.main_area(), &menu);
        let outside = layout
            .toolbar_buttons
            .iter()
            .find(|button| !rect_contains(menu_area, (button.area.x, button.area.y)))
            .unwrap()
            .area;
        // Keep the old page registry to verify immediate modal ownership.
        fixture.click(outside, hover);
        assert!(fixture.state.pages.launcher.context_menu.is_none());
        assert!(!fixture.state.pages.launcher.organizing);
        assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
        fixture.input(InputEvent::mouse_down(PointerButton::Right, point));
        fixture.draw();
        fixture.input(InputEvent::mouse_down(
            PointerButton::Left,
            (rows[0].x, rows[0].y),
        ));
        fixture.input(InputEvent::mouse_moved((outside.x, outside.y)));
        fixture.input(InputEvent::mouse_up(
            PointerButton::Left,
            (outside.x, outside.y),
        ));
        assert!(fixture.state.pages.launcher.context_menu.is_some());
        assert!(fixture.state.pages.launcher.entry_editor.is_none());
        fixture.input(InputEvent::from_key_label("Esc"));
        assert!(fixture.state.pages.launcher.context_menu.is_none());
        assert_eq!(fixture.state.focused_component(), ShellComponent::Launcher);
        fixture.state.select_launcher_index(0);
        let layout = fixture.draw();
        let builtin = layout.items[0].area;
        fixture.input(InputEvent::mouse_down(
            PointerButton::Right,
            (builtin.x, builtin.y),
        ));
        assert!(fixture.state.pages.launcher.context_menu.is_none());
        assert!(fixture.state.pages.launcher.entry_editor.is_none());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = platform::cleanup_temp_path(&self.root);
    }
}

#[test]
fn custom_launcher_exit_choice_defaults_to_immediate_and_accepts_each_input_method() {
    for (hover, height) in [(true, 40), (false, 24)] {
        let mut fixture = Fixture::new();
        fixture.input(InputEvent::Resize { width: 120, height });
        fixture.input(InputEvent::from_key_label("F2"));
        fixture.input(InputEvent::from_key_label("Insert"));
        let form = fixture.state.launcher_run_settings_form().unwrap();
        assert_eq!(form.fields.len(), 3);
        assert_eq!(form.fields[2].id, "exit");
        assert_eq!(form.fields[2].value, form.fields[2].choices[0]);
        for _ in 0..2 {
            fixture.input(InputEvent::from_key_label("Tab"));
        }
        assert!(fixture.state.page_keyboard_identity().is_none());
        fixture.input(InputEvent::from_key_label("Right"));
        let form = fixture.state.launcher_run_settings_form().unwrap();
        assert_eq!(form.fields[2].value, form.fields[2].choices[1]);
        let layout = fixture.draw();
        let choice = layout
            .run_settings
            .as_ref()
            .unwrap()
            .fields
            .iter()
            .find(|(index, _)| *index == 2)
            .unwrap()
            .1;
        fixture.click(choice, hover);
        let form = fixture.state.launcher_run_settings_form().unwrap();
        assert_eq!(form.fields[2].value, form.fields[2].choices[0]);
        assert_eq!(form.selected, 2);
        for _ in 0..2 {
            fixture.input(InputEvent::from_key_label("Shift+Tab"));
        }
        for text in ["exit choice", "printf done"] {
            fixture.input(InputEvent::Paste(text.into()));
            fixture.input(InputEvent::from_key_label("Tab"));
        }
        fixture.input(InputEvent::from_key_label("Enter"));
        assert!(fixture.state.launcher_run_settings_form().is_some());
        fixture.input(InputEvent::from_key_label("F10"));
        let config = fixture
            .state
            .storage_manager
            .as_ref()
            .unwrap()
            .load_config()
            .unwrap();
        assert!(
            config
                .launcher
                .entries
                .last()
                .unwrap()
                .custom
                .as_ref()
                .unwrap()
                .wait_for_enter
        );
    }
}

#[test]
fn keyboard_saves_arguments_as_data_and_keeps_invalid_drafts_open() {
    let mut fixture = Fixture::new();
    fixture.input(InputEvent::from_key_label("F4"));
    fixture.input(InputEvent::from_key_label("Enter"));
    fixture.input(InputEvent::from_key_label("Tab"));
    assert!(fixture.state.page_keyboard_identity().is_some());
    fixture.input(InputEvent::Paste("\"unterminated".into()));
    fixture.input(InputEvent::from_key_label("F10"));
    assert!(
        fixture
            .state
            .pages
            .launcher
            .launcher_run_settings
            .as_ref()
            .unwrap()
            .error
            .is_some()
    );
    fixture.input(InputEvent::key_with_modifiers(
        InputKey::Char('a'),
        InputModifiers::CTRL,
    ));
    fixture.input(InputEvent::Paste(
        "--name 'a b' '$HOME' '$(printf nope)'".into(),
    ));
    fixture.input(InputEvent::from_key_label("Tab"));
    let directory = fixture.root.join("中文 working folder");
    std::fs::create_dir(&directory).unwrap();
    fixture.input(InputEvent::Paste(directory.to_string_lossy().into_owned()));
    fixture.input(InputEvent::from_key_label("F10"));
    assert!(fixture.state.pages.launcher.launcher_run_settings.is_none());
    let config = fixture
        .state
        .storage_manager
        .as_ref()
        .unwrap()
        .load_config()
        .unwrap();
    let settings = config.launcher.entries[0].terminal.as_ref().unwrap();
    assert_eq!(
        settings.arguments,
        ["--name", "a b", "$HOME", "$(printf nope)"]
    );
    assert_eq!(settings.working_directory.as_deref(), directory.to_str());
    assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
}

#[test]
fn mouse_and_touch_can_open_edit_and_cancel_without_background_activation() {
    for hover in [false, true] {
        let mut fixture = Fixture::new();
        let layout = fixture.draw();
        let button = layout
            .toolbar_buttons
            .iter()
            .find(|button| button.action == ui::LauncherToolbarAction::RunSettings)
            .unwrap()
            .area;
        fixture.click(button, hover);
        assert!(fixture.state.pages.launcher.launcher_run_settings.is_some());
        // The form must own clicks even while the registry still describes the old page.
        let layout = fixture.draw();
        fixture.click(layout.run_settings.as_ref().unwrap().fields[0].1, hover);
        let layout = fixture.draw();
        let form = layout.run_settings.unwrap();
        fixture.click(
            form.fields.iter().find(|(index, _)| *index == 1).unwrap().1,
            hover,
        );
        assert!(fixture.state.page_keyboard_identity().is_some());
        fixture.input(InputEvent::Paste("--preview '图片.png'".into()));
        fixture.click(form.cancel, hover);
        assert!(fixture.state.pages.launcher.launcher_run_settings.is_none());
        assert!(
            fixture.state.app.launcher_state().unwrap().items[0]
                .record
                .terminal
                .is_none()
        );
        assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
    }
}

#[test]
fn builtins_and_unprivileged_users_cannot_open_run_settings() {
    let mut fixture = Fixture::new();
    fixture.state.pages.launcher.launcher_selected_index = 0;
    fixture.input(InputEvent::from_key_label("F4"));
    assert!(fixture.state.pages.launcher.launcher_run_settings.is_none());
    let model = fixture.state.to_launcher_view_model();
    assert!(
        !model
            .toolbar
            .iter()
            .find(|button| button.action == ui::LauncherToolbarAction::RunSettings)
            .unwrap()
            .enabled
    );
    fixture.state.pages.launcher.launcher_selected_index = fixture.state.built_in_launcher_count();
    let mut session = fixture.state.app.auth_session().unwrap().clone();
    session.role = UserRole::User;
    fixture.state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(session)),
        Instant::now(),
    );
    fixture.input(InputEvent::from_key_label("F4"));
    assert!(fixture.state.pages.launcher.launcher_run_settings.is_none());
}

#[test]
fn launcher_wheel_pages_to_boundaries_in_both_views() {
    for mode in [
        app::launcher::LauncherViewMode::LargeIcons,
        app::launcher::LauncherViewMode::Details,
    ] {
        let mut fixture = Fixture::new();
        fixture.state.update_launcher_state(|state| {
            for n in 0..80 {
                state.items.push(app::launcher::LauncherItem {
                    record: storage::LauncherEntryRecord {
                        id: format!("extra-{n}"),
                        ..Default::default()
                    },
                    status: LauncherItemStatus::Ready,
                });
            }
        });
        fixture.state.pages.launcher.launcher_view_mode = mode;
        fixture.state.select_launcher_index(0);
        let layout = fixture.draw();
        let point = (layout.content.x + 1, layout.content.y + 1);
        fixture.input(InputEvent::mouse_scroll(ScrollDirection::Up, point));
        assert_eq!(fixture.draw().visible_start, 0);
        fixture.input(InputEvent::mouse_scroll(ScrollDirection::Down, point));
        assert_eq!(fixture.draw().visible_start, layout.visible_capacity);
        for _ in 0..100 {
            fixture.input(InputEvent::mouse_scroll(ScrollDirection::Down, point));
        }
        let tail = fixture.draw();
        assert_eq!(
            tail.visible_start,
            tail.scroll_content_len
                .saturating_sub(tail.visible_capacity)
        );
        fixture.input(InputEvent::mouse_scroll(ScrollDirection::Down, point));
        assert_eq!(fixture.draw().visible_start, tail.visible_start);
        for _ in 0..100 {
            fixture.input(InputEvent::mouse_scroll(ScrollDirection::Up, point));
        }
        assert_eq!(fixture.draw().visible_start, 0);
        assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
    }
}

#[test]
fn organizing_add_cancel_persist_and_delete_only_user_entries() {
    let mut fixture = Fixture::new();
    fixture.input(InputEvent::from_key_label("Insert"));
    fixture.input(InputEvent::from_key_label("Delete"));
    assert!(fixture.state.pages.launcher.launcher_run_settings.is_none());
    assert!(
        fixture
            .state
            .pages
            .launcher
            .launcher_pending_confirmation
            .is_none()
    );
    fixture.input(InputEvent::from_key_label("F2"));
    assert!(fixture.state.pages.launcher.organizing);
    fixture.input(InputEvent::from_key_label("Home"));
    fixture.input(InputEvent::from_key_label("Delete"));
    assert!(
        fixture
            .state
            .pages
            .launcher
            .launcher_pending_confirmation
            .is_none()
    );
    fixture.input(InputEvent::from_key_label("Insert"));
    assert!(fixture.state.page_keyboard_identity().is_some());
    fixture.input(InputEvent::Paste("discard".into()));
    fixture.input(InputEvent::from_key_label("Esc"));
    assert_eq!(fixture.state.app.launcher_state().unwrap().items.len(), 1);
    fixture.input(InputEvent::from_key_label("Insert"));
    fixture.input(InputEvent::from_key_label("F10"));
    assert!(
        fixture
            .state
            .pages
            .launcher
            .launcher_run_settings
            .as_ref()
            .unwrap()
            .error
            .is_some()
    );
    for (index, text) in ["中文 tool", "printf 'hello world'"].iter().enumerate() {
        if index > 0 {
            fixture.input(InputEvent::from_key_label("Tab"));
        }
        fixture.input(InputEvent::Paste((*text).into()));
    }
    fixture.input(InputEvent::from_key_label("F10"));
    assert!(fixture.state.pages.launcher.launcher_run_settings.is_none());
    let storage = fixture.state.storage_manager.clone().unwrap();
    let config = storage.load_config().unwrap();
    let custom = config
        .launcher
        .entries
        .last()
        .unwrap()
        .custom
        .as_ref()
        .unwrap();
    assert_eq!(custom.name, "中文 tool");
    assert_eq!(custom.command, "printf 'hello world'");
    assert!(custom.icon.is_empty());
    assert!(!custom.wait_for_enter);
    fixture.input(InputEvent::from_key_label("End"));
    fixture.input(InputEvent::from_key_label("Enter"));
    assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
    fixture.input(InputEvent::from_key_label("Delete"));
    assert!(
        fixture
            .state
            .to_launcher_view_model()
            .confirmation
            .unwrap()
            .message
            .contains("中文 tool")
    );
    fixture.input(InputEvent::from_key_label("Esc"));
    assert_eq!(storage.load_config().unwrap().launcher.entries.len(), 2);
    fixture.input(InputEvent::from_key_label("Delete"));
    fixture.input(InputEvent::from_key_label("Enter"));
    assert_eq!(storage.load_config().unwrap().launcher.entries.len(), 1);
    fixture.input(InputEvent::from_key_label("Esc"));
    assert!(!fixture.state.pages.launcher.organizing);
    assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
}

#[test]
fn organize_pointer_and_keyboard_reorder_builtin_icons_and_reopen() {
    for hover in [false, true] {
        let mut fixture = Fixture::new();
        fixture.state.select_launcher_index(0);
        let layout = fixture.draw();
        let button = layout
            .toolbar_buttons
            .iter()
            .find(|b| b.action == ui::LauncherToolbarAction::Organize)
            .unwrap()
            .area;
        fixture.click(button, hover);
        assert!(fixture.state.pages.launcher.organizing);
        let layout = fixture.draw();
        let before = fixture
            .state
            .to_launcher_view_model()
            .items
            .iter()
            .map(|i| i.id.clone())
            .collect::<Vec<_>>();
        let source = layout.items[0].area;
        let target = layout.items[1].area;
        let start = (source.x + 1, source.y + 1);
        let end = (target.right() - 1, target.y + 1);
        fixture.input(InputEvent::mouse_down(PointerButton::Left, start));
        fixture.input(InputEvent::mouse_drag(PointerButton::Left, end));
        fixture.input(InputEvent::from_key_label("Esc"));
        fixture.input(InputEvent::mouse_up(PointerButton::Left, end));
        assert!(fixture.state.pages.launcher.organizing);
        assert_eq!(
            fixture.state.to_launcher_view_model().items[0].id,
            before[0]
        );
        fixture.input(InputEvent::mouse_down(PointerButton::Left, start));
        fixture.input(InputEvent::mouse_drag(PointerButton::Left, end));
        fixture.input(InputEvent::mouse_up(PointerButton::Left, end));
        let after = fixture
            .state
            .to_launcher_view_model()
            .items
            .iter()
            .map(|i| i.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(after[0], before[1]);
        assert_eq!(after[1], before[0]);
        assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
        let storage = fixture.state.storage_manager.clone().unwrap();
        assert_eq!(storage.load_config().unwrap().launcher.order, after);
        fixture.input(InputEvent::key_with_modifiers(
            InputKey::Left,
            InputModifiers::SHIFT,
        ));
        assert_eq!(
            fixture.state.to_launcher_view_model().items[0].id,
            before[0]
        );
        fixture.state.close_launcher();
        fixture.state.open_launcher(&fixture.platform);
        assert!(!fixture.state.pages.launcher.organizing);
        assert_eq!(
            fixture.state.to_launcher_view_model().items[0].id,
            before[0]
        );
    }
}
