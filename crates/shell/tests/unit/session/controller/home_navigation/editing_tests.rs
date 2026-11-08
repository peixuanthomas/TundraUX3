use super::*;

fn order(state: &ShellSession) -> Vec<String> {
    state
        .user_home_entries()
        .iter()
        .map(|entry| entry.icon_identity().to_owned())
        .collect()
}

fn click(state: &mut ShellSession, point: CellPosition) {
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
}

fn drag_points(state: &ShellSession) -> (CellPosition, CellPosition) {
    let layout = state.home_content_layout().unwrap();
    let source = layout.items[0].area;
    let target = layout.items.last().unwrap().area;
    (
        (source.x + 1, source.y + source.height / 2),
        (target.right() - 1, target.y + target.height / 2),
    )
}

#[test]
fn home_edit_keyboard_moves_preserve_selection_and_escape_finishes() {
    let mut state = state((120, 40));
    let original = order(&state);
    state.apply_input(InputEvent::key_with_modifiers(
        InputKey::F(2),
        InputModifiers::ALT,
    ));
    assert!(!state.pages.home.editing);
    state.apply_input(InputEvent::from_key_label("F2"));
    assert!(state.pages.home.editing);
    for key in ["Enter", " ", "a", "s", "e", "m"] {
        state.apply_input(InputEvent::from_key_label(key));
        assert_eq!(state.active_screen(), ShellScreen::Home);
    }
    state.apply_input(InputEvent::key_with_modifiers(
        InputKey::Right,
        InputModifiers::SHIFT,
    ));
    assert_eq!(
        &order(&state)[..2],
        &[original[1].clone(), original[0].clone()]
    );
    assert_eq!(state.selected_home_entry_index(), 1);
    state.apply_input(InputEvent::key_with_modifiers(
        InputKey::Left,
        InputModifiers::SHIFT,
    ));
    assert_eq!(order(&state), original);
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert!(!state.pages.home.editing);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::HomeEdit);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(state.pages.home.editing);
    state.apply_input(InputEvent::from_key_label(" "));
    assert!(!state.pages.home.editing);
}

#[test]
fn home_mouse_and_touch_edit_reorder_only_on_release_and_open_after_done() {
    for (size, hover) in [(120, 40), (108, 20)]
        .into_iter()
        .flat_map(|size| [false, true].map(|hover| (size, hover)))
    {
        let mut state = state(size);
        let original = order(&state);
        draw_buttons(&mut state);
        let (start, end) = drag_points(&state);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, start));
        state.apply_input(InputEvent::mouse_drag(PointerButton::Left, end));
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, end));
        assert_eq!(order(&state), original);
        assert_eq!(state.active_screen(), ShellScreen::Home);

        let edit = ui::home_edit_area(main_area(&state), &state.to_home_view_model()).unwrap();
        if hover {
            state.apply_input(InputEvent::mouse_moved((edit.x, edit.y)));
        }
        state.apply_input(InputEvent::mouse_down(
            PointerButton::Left,
            (edit.x, edit.y),
        ));
        assert!(!state.pages.home.editing);
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, (edit.x, edit.y)));
        assert!(state.pages.home.editing);
        draw_buttons(&mut state);
        click(&mut state, start);
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(order(&state), original);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, start));
        state.apply_input(InputEvent::mouse_drag(PointerButton::Left, end));
        assert!(state.to_home_view_model().drop_target.is_some());
        assert_eq!(order(&state), original);
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, end));
        let mut expected = original.clone();
        expected.rotate_left(1);
        assert_eq!(order(&state), expected);
        assert_eq!(state.selected_home_entry_index(), expected.len() - 1);
        assert!(state.pages.home.drag.is_none());
        draw_buttons(&mut state);
        let done = ui::home_edit_area(main_area(&state), &state.to_home_view_model()).unwrap();
        click(&mut state, (done.x, done.y));
        assert!(!state.pages.home.editing);
        draw_buttons(&mut state);
        let tasks = order(&state).iter().position(|id| id == "tasks").unwrap();
        let tile = state.home_content_layout().unwrap().items[tasks].area;
        click(&mut state, (tile.x + 1, tile.y + tile.height / 2));
        assert_eq!(state.active_screen(), ShellScreen::Management);
        assert!(
            state
                .to_management_view_model()
                .title
                .starts_with(&i18n::tr!("tasks-title"))
        );
    }
}

#[test]
fn home_drag_cancellation_never_reorders_or_opens_an_entry() {
    for cancel in [
        InputEvent::from_key_label("Esc"),
        InputEvent::FocusLost,
        InputEvent::Resize {
            width: 100,
            height: 30,
        },
        InputEvent::mouse_scroll(ScrollDirection::Down, (10, 10)),
        InputEvent::from_key_label("F1"),
    ] {
        let mut state = state((120, 40));
        state.apply_input(InputEvent::from_key_label("F2"));
        draw_buttons(&mut state);
        let original = order(&state);
        let (start, end) = drag_points(&state);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, start));
        state.apply_input(InputEvent::mouse_drag(PointerButton::Left, end));
        state.apply_input(cancel);
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, end));
        assert_eq!(order(&state), original);
        assert!(state.pages.home.drag.is_none());
        assert!(state.pages.home.editing);
        assert_eq!(state.active_screen(), ShellScreen::Home);
    }
    let mut state = state((120, 40));
    state.apply_input(InputEvent::from_key_label("F2"));
    draw_buttons(&mut state);
    let original = order(&state);
    let (start, end) = drag_points(&state);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, start));
    state.apply_input(InputEvent::mouse_drag(PointerButton::Left, end));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, (0, 0)));
    assert_eq!(order(&state), original);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, start));
    state.apply_input(InputEvent::mouse_drag(PointerButton::Left, end));
    state.toggle_tasks();
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, end));
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(order(&state), original);
    assert!(state.pages.home.drag.is_none());
}

struct StoredHome {
    state: ShellSession,
    storage: StorageManager,
    root: PathBuf,
}

impl StoredHome {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tundra-home-order-{}-{}",
            std::process::id(),
            PageInstanceId::new().get()
        ));
        let storage = StorageManager::open(
            platform::build_windows_app_paths(
                root.join("Roaming"),
                root.join("Local"),
                root.join("Temp"),
            )
            .unwrap(),
        )
        .unwrap()
        .manager;
        let mut state = state((120, 40));
        state.storage_manager = Some(storage.clone());
        state.refresh_hit_map();
        Self {
            state,
            storage,
            root,
        }
    }
}

impl Drop for StoredHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn home_order_survives_reopening_and_is_private_to_each_user() {
    let mut fixture = StoredHome::new();
    let original = order(&fixture.state);
    fixture.state.apply_input(InputEvent::from_key_label("F2"));
    fixture.state.apply_input(InputEvent::key_with_modifiers(
        InputKey::Right,
        InputModifiers::SHIFT,
    ));
    let expected = order(&fixture.state);
    let saved = fixture.storage.load_state().unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&saved.values["home.order.admin"]).unwrap(),
        expected
    );

    let mut reopened = state((120, 40));
    reopened.storage_manager = Some(fixture.storage.clone());
    reopened.refresh_hit_map();
    assert_eq!(order(&reopened), expected);
    assert!(!reopened.pages.home.editing);
    let mut account = reopened.app.auth_session().unwrap().clone();
    account.user_id = "another-user".into();
    reopened.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(account)),
        Instant::now(),
    );
    reopened.refresh_hit_map();
    assert_eq!(order(&reopened), original);
    reopened.apply_input(InputEvent::from_key_label("F2"));
    reopened.apply_input(InputEvent::key_with_modifiers(
        InputKey::Right,
        InputModifiers::SHIFT,
    ));
    let saved = fixture.storage.load_state().unwrap();
    assert!(saved.values.contains_key("home.order.another-user"));
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&saved.values["home.order.admin"]).unwrap(),
        expected
    );
}

#[test]
fn home_order_ignores_obsolete_ids_and_does_not_apply_unsaved_changes() {
    let mut fixture = StoredHome::new();
    let mut saved = fixture.storage.load_state().unwrap();
    saved.values.insert(
        "home.order.admin".into(),
        r#"["removed", "settings", "settings"]"#.into(),
    );
    fixture.storage.save_state(&saved).unwrap();
    fixture.state.reset_navigation(ShellScreen::Home);
    fixture.state.refresh_hit_map();
    assert_eq!(order(&fixture.state)[0], "settings");
    assert_eq!(order(&fixture.state).len(), order(&state((120, 40))).len());
    let original = order(&fixture.state);
    let path = &fixture.storage.layout().state_path;
    std::fs::remove_file(path).unwrap();
    std::fs::create_dir(path).unwrap();
    fixture.state.apply_input(InputEvent::from_key_label("F2"));
    fixture.state.apply_input(InputEvent::key_with_modifiers(
        InputKey::Right,
        InputModifiers::SHIFT,
    ));
    assert_eq!(order(&fixture.state), original);
    assert_eq!(fixture.state.selected_home_entry_index(), 0);
    assert!(
        fixture
            .state
            .to_shell_chrome_view_model()
            .status
            .error
            .is_some()
    );
}
