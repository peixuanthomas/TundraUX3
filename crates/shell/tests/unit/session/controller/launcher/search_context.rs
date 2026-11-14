use super::*;

#[test]
fn launcher_search_filters_real_targets_and_zero_results_cannot_launch_hidden_items() {
    let mut fixture = Fixture::new();
    fixture.input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('f'),
        InputModifiers::CTRL,
    )));
    fixture.input(InputEvent::Paste("PROGRAM.EXE".into()));
    let model = fixture.state.to_launcher_view_model();
    assert_eq!(model.items.len(), 1);
    assert_eq!(model.items[0].id, "tui");
    assert_eq!(fixture.state.selected_launcher_id().as_deref(), Some("tui"));
    fixture.input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('a'),
        InputModifiers::CTRL,
    )));
    fixture.input(InputEvent::Paste("nonexistent-app-123".into()));
    assert!(fixture.state.to_launcher_view_model().items.is_empty());
    fixture.input(InputEvent::key(InputKey::Enter));
    fixture.input(InputEvent::key(InputKey::Enter));
    fixture.input(InputEvent::key(InputKey::F(4)));
    assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
    assert!(fixture.state.pages.launcher.launcher_run_settings.is_none());
    assert!(fixture.state.selected_launcher_id().is_none());
    assert!(
        fixture
            .state
            .selected_built_in_launcher_application()
            .is_none()
    );

    fixture.input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('f'),
        InputModifiers::CTRL,
    )));
    fixture.input(InputEvent::key(InputKey::Escape));
    let editor_name = fixture
        .state
        .to_launcher_view_model()
        .items
        .iter()
        .find(|item| item.id == app::EDITOR_APPLICATION.id)
        .unwrap()
        .name
        .clone();
    fixture.input(InputEvent::Paste(editor_name));
    let filtered = fixture.state.to_launcher_view_model();
    let mut expected_ids = vec![app::EDITOR_APPLICATION.id];
    if cfg!(target_os = "linux") {
        expected_ids.push(app::SCRIPT_EDITOR_APPLICATION.id);
    }
    assert_eq!(
        filtered
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        expected_ids,
    );
    assert_eq!(
        fixture
            .state
            .selected_built_in_launcher_application()
            .map(|application| application.id),
        Some(app::EDITOR_APPLICATION.id)
    );
    assert!(fixture.state.selected_launcher_id().is_none());
    fixture.input(InputEvent::key(InputKey::Enter));
    assert_eq!(fixture.state.active_screen(), ShellScreen::Editor);
}

#[test]
fn launcher_search_reuses_unicode_cursor_selection_and_clipboard_without_shutdown() {
    let mut fixture = Fixture::new();
    fixture.input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('f'),
        InputModifiers {
            super_key: true,
            ..InputModifiers::none()
        },
    )));
    fixture.input(InputEvent::Paste("中🙂".into()));
    fixture.input(InputEvent::key(InputKey::Home));
    fixture.input(InputEvent::key(InputKey::Right));
    fixture.input(InputEvent::key(InputKey::Char('文')));
    assert_eq!(
        fixture.state.to_launcher_view_model().search.value(),
        "中文🙂"
    );
    fixture.input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('a'),
        InputModifiers::CTRL,
    )));
    fixture.input(InputEvent::from_key_label("Ctrl+C"));
    assert!(!fixture.state.shutdown_requested());
    assert_eq!(fixture.platform.read_clipboard_text().unwrap(), "中文🙂");
    fixture.input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('x'),
        InputModifiers::CTRL,
    )));
    assert_eq!(fixture.state.to_launcher_view_model().search.value(), "");
    fixture.input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('v'),
        InputModifiers::CTRL,
    )));
    assert_eq!(
        fixture.state.to_launcher_view_model().search.value(),
        "中文🙂"
    );
    fixture.input(InputEvent::key(InputKey::Tab));
    assert!(!fixture.state.to_launcher_view_model().search.state.focused);
    fixture.input(InputEvent::key(InputKey::BackTab));
    assert!(fixture.state.to_launcher_view_model().search.state.focused);
    fixture.input(InputEvent::key(InputKey::Escape));
    assert_eq!(fixture.state.active_screen(), ShellScreen::Launcher);
    assert_eq!(fixture.state.to_launcher_view_model().search.value(), "");
}

#[test]
fn launcher_filtered_organize_never_overwrites_the_full_order() {
    let mut fixture = Fixture::new();
    fixture.input(InputEvent::key(InputKey::F(2)));
    fixture.input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('f'),
        InputModifiers::CTRL,
    )));
    fixture.input(InputEvent::Paste("program.exe".into()));
    fixture.input(InputEvent::key(InputKey::Down));
    let before = fixture.state.pages.launcher.order.clone();
    fixture.input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Right,
        InputModifiers::SHIFT,
    )));
    assert_eq!(fixture.state.pages.launcher.order, before);
    assert!(
        !fixture.state.to_launcher_view_model().items[0]
            .capabilities
            .reorderable
    );
}

#[test]
fn launcher_context_move_selects_the_clicked_icon_in_organize_mode() {
    let mut fixture = Fixture::new();
    fixture.state.select_launcher_index(0);
    let id = fixture.state.to_launcher_view_model().items[0].id.clone();
    fixture.choose_context_action(ui::LauncherEntryAction::Move, false);
    assert!(fixture.state.pages.launcher.organizing);
    fixture.input(InputEvent::key_with_modifiers(
        InputKey::Right,
        InputModifiers::SHIFT,
    ));
    assert_eq!(fixture.state.to_launcher_view_model().items[1].id, id);
    let mut config = fixture
        .state
        .storage_manager
        .as_ref()
        .unwrap()
        .load_config()
        .unwrap();
    assert_eq!(config.launcher.order[1], id);
    let previous = config.launcher.clone();
    config.launcher.entries.clear();
    fixture.state.replace_storage_config(config);
    fixture.state.sync_launcher_entries(&previous);
    assert!(
        fixture
            .state
            .to_launcher_view_model()
            .items
            .iter()
            .all(|item| item.is_builtin())
    );
}

#[test]
fn launcher_builtin_context_opens_the_clicked_item_and_blank_context_adds_directly() {
    let mut fixture = Fixture::new();
    let index = fixture
        .state
        .to_launcher_view_model()
        .items
        .iter()
        .position(|item| item.id == app::EDITOR_APPLICATION.id)
        .unwrap();
    fixture.state.select_launcher_index(index);
    let menu = fixture.open_context_menu();
    assert!(!menu.editable);
    assert_eq!(
        menu.actions().collect::<Vec<_>>(),
        vec![
            ui::LauncherEntryAction::Toolbar(ui::LauncherToolbarAction::Open),
            ui::LauncherEntryAction::NewSession,
            ui::LauncherEntryAction::Move,
        ],
    );
    let open = menu
        .actions()
        .position(|action| {
            action == ui::LauncherEntryAction::Toolbar(ui::LauncherToolbarAction::Open)
        })
        .unwrap();
    let (_, rows) = ui::launcher_context_menu_layout(fixture.main_area(), &menu);
    fixture.draw();
    fixture.click(rows[open], false);
    assert_eq!(fixture.state.active_screen(), ShellScreen::Editor);

    let mut fixture = Fixture::new();
    let layout = fixture.draw();
    let point = (layout.content.right() - 2, layout.content.bottom() - 1);
    assert_eq!(
        layout.hit_test(point.0, point.1),
        Some(ui::LauncherHitTarget::EmptyContent)
    );
    fixture.input(InputEvent::mouse_down(PointerButton::Right, point));
    let menu = fixture.state.to_launcher_view_model().context_menu.unwrap();
    assert!(
        !menu
            .actions()
            .any(|action| action
                == ui::LauncherEntryAction::Toolbar(ui::LauncherToolbarAction::Open))
    );
    let add = menu
        .actions()
        .position(|action| {
            action == ui::LauncherEntryAction::Toolbar(ui::LauncherToolbarAction::Add)
        })
        .unwrap();
    let (_, rows) = ui::launcher_context_menu_layout(fixture.main_area(), &menu);
    fixture.draw();
    fixture.click(rows[add], false);
    assert!(
        fixture
            .state
            .pages
            .launcher
            .launcher_run_settings
            .as_ref()
            .is_some_and(|form| form.custom)
    );
}

#[test]
fn launcher_search_mouse_focus_and_caret_share_the_rendered_field() {
    let mut fixture = Fixture::new();
    let layout = fixture.draw();
    let prefix =
        ratatui::text::Line::from(format!("{} ", i18n::tr!("ui-launcher-search"))).width() as u16;
    let start = (layout.search.x + prefix, layout.search.y);
    fixture.input(InputEvent::mouse_down(PointerButton::Left, start));
    fixture.input(InputEvent::Paste("abcd".into()));
    fixture.input(InputEvent::mouse_down(
        PointerButton::Left,
        (start.0 + 1, start.1),
    ));
    fixture.input(InputEvent::key(InputKey::Char('中')));
    assert_eq!(
        fixture.state.to_launcher_view_model().search.value(),
        "a中bcd"
    );
    assert_eq!(
        fixture.state.page_keyboard_identity().as_deref(),
        Some("launcher-search")
    );
}
