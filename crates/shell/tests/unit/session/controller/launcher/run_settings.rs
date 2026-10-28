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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = platform::cleanup_temp_path(&self.root);
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
