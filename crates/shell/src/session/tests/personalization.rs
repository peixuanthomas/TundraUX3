use super::*;

struct PersonalizationTempGuard(PathBuf);

impl Drop for PersonalizationTempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(pending: bool) -> (PersonalizationTempGuard, StorageManager, AuthSession) {
    static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "tundra-personalization-{sequence}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let paths = platform::build_linux_app_paths(
        root.join("config"),
        root.join("data"),
        root.join("cache"),
        root.join("state"),
        root.join("temp"),
    )
    .unwrap();
    let manager = StorageManager::open(paths).unwrap().manager;
    let session = AuthSession {
        system_user: None,
        session_id: "linux-session".into(),
        user_id: "linux-uid-1000".into(),
        username: "peixuan".into(),
        role: UserRole::Admin,
        started_at_epoch_ms: 1,
    };
    let mut document = manager.load_users().unwrap();
    document.users.push(storage::UserRecord {
        id: session.user_id.clone(),
        username: session.username.clone(),
        display_name: "Peixuan".into(),
        role: "Admin".into(),
        password_hash: String::new(),
        password_hint: None,
        appearance: storage::AppearanceConfig::default(),
        personalization_pending: pending,
        system_status_dashboard: storage::SystemStatusDashboardConfig::for_role("Admin"),
        enabled: true,
        failed_login_attempts: 0,
        locked_until_epoch_ms: None,
        created_at_epoch_ms: 1,
        updated_at_epoch_ms: 1,
        last_login_at_epoch_ms: Some(1),
    });
    manager.save_users(&document).unwrap();
    (PersonalizationTempGuard(root), manager, session)
}

fn linux_state(manager: &StorageManager, images: bool) -> ShellSession {
    let mut startup = ShellStartupState::clean(
        PlatformKind::Linux,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(manager.clone());
    startup.identity_backend = identity::IdentityBackend::Linux;
    let mut state =
        ShellSession::new_with_startup(ShellLaunchConfig::default(), (120, 40), startup);
    state.set_terminal_image_support(images);
    state
}

fn submit(state: &mut ShellSession) {
    for _ in 0..5 {
        state.apply_input(InputEvent::from_key_label("Tab"));
    }
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceSubmit
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
}

#[test]
fn linux_first_login_requires_personalization_and_persists_terminal_safe_choices() {
    for images in [false, true] {
        let (_guard, manager, session) = fixture(true);
        let original_config = manager.load_config().unwrap();
        let mut state = linux_state(&manager, images);
        state.login_password = "must be cleared".into();
        state.complete_login(session.clone());
        assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
        assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Appearance);
        assert!(state.auth_session().is_none());
        assert!(state.app.active_appearance().is_none());
        assert!(state.login_password.is_empty());
        assert!(state.clock_scheduler.is_none());
        // Home shortcuts cannot bypass setup. Appearance input uses existing routing.
        state.apply_input(InputEvent::from_key_label("e"));
        assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
        state.apply_input(InputEvent::from_key_label("Right"));
        let chosen_shape = state.setup_border_shape;
        submit(&mut state);
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(state.auth_session(), Some(&session));
        assert!(state.pending_personalization_session.is_none());
        let record = manager.load_users().unwrap().users.remove(0);
        assert!(!record.personalization_pending);
        assert_eq!(record.appearance.border_shape, chosen_shape);
        assert_eq!(
            record.appearance.icon_display_mode,
            if images {
                storage::IconDisplayMode::Image
            } else {
                storage::IconDisplayMode::Ascii
            }
        );
        assert_eq!(state.app.active_appearance(), Some(&record.appearance));
        assert_eq!(state.graphical_icons_enabled(), images);
        assert_eq!(manager.load_config().unwrap(), original_config);
        assert!(record.password_hash.is_empty());
        assert_eq!(record.display_name, "Peixuan");
        // Completion survives a new shell process/session.
        let mut reopened = linux_state(&manager, images);
        reopened.complete_login(session);
        assert_eq!(reopened.active_screen(), ShellScreen::Home);
    }
}

#[test]
fn interrupted_linux_personalization_resumes_and_existing_profiles_skip_it() {
    let (_guard, manager, session) = fixture(true);
    let mut state = linux_state(&manager, false);
    state.complete_login(session.clone());
    drop(state);
    assert!(manager.load_users().unwrap().users[0].personalization_pending);
    let mut reopened = linux_state(&manager, false);
    reopened.complete_login(session.clone());
    assert_eq!(reopened.active_screen(), ShellScreen::FirstRunSetup);
    // A saved profile for another username with the same UID is not inherited.
    let mut document = manager.load_users().unwrap();
    document.users[0].personalization_pending = false;
    document.users[0].username = "previous-owner".into();
    manager.save_users(&document).unwrap();
    let mut renamed = linux_state(&manager, false);
    renamed.complete_login(session);
    assert_eq!(renamed.active_screen(), ShellScreen::FirstRunSetup);
    assert!(renamed.auth_session().is_none());
}

#[test]
fn save_failure_keeps_linux_personalization_pending_and_allows_retry() {
    let (_guard, manager, session) = fixture(true);
    let mut state = linux_state(&manager, false);
    state.complete_login(session);
    let users_path = &manager.layout().users_path;
    let backup = users_path.with_extension("test-backup");
    std::fs::rename(users_path, &backup).unwrap();
    std::fs::create_dir(users_path).unwrap();
    submit(&mut state);
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert!(state.auth_session().is_none());
    assert!(state.pending_personalization_session.is_some());
    assert!(state.to_setup_view_model().error.is_some());
    std::fs::remove_dir(users_path).unwrap();
    std::fs::rename(backup, users_path).unwrap();
    assert!(manager.load_users().unwrap().users[0].personalization_pending);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert!(!manager.load_users().unwrap().users[0].personalization_pending);
}

#[test]
fn linux_ascii_runtime_fallback_applies_to_each_existing_user_login() {
    let (_guard, manager, session) = fixture(false);
    let mut state = linux_state(&manager, false);
    for _ in 0..2 {
        state.complete_login(session.clone());
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(
            state.app.active_appearance().unwrap().icon_display_mode,
            storage::IconDisplayMode::Ascii
        );
        assert!(!state.graphical_icons_enabled());
        // A missing NSS backend on non-Linux hosts can prevent saving the
        // fallback, but must never prevent the runtime's ASCII selection.
        state.return_to_login("Signed out");
    }
}
