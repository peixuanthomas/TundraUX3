use super::*;
use std::path::Path;

struct Fixture {
    root: PathBuf,
    state: ShellSession,
    storage: StorageManager,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(parent) = self.storage.layout().config_path.parent() {
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
fn fixture() -> Fixture {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "tux3-language-session-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let assets = root.join("assets");
    let (store, _) = ui::AsciiAssetStore::load_default_with_root_and_recovery(&assets).unwrap();
    copy_tree(
        &Path::new(ascii_assets::CANONICAL_ASSETS_DIR).join("locales"),
        &assets.join("locales"),
    );
    let paths = platform::build_linux_app_paths(
        root.join("config"),
        root.join("data"),
        root.join("cache"),
        root.join("state"),
        root.join("temp"),
    )
    .unwrap();
    let storage = StorageManager::open(paths).unwrap().manager;
    let mut startup = ShellStartupState::clean(
        PlatformKind::Linux,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(storage.clone());
    let mut state = ShellSession::new_with_startup_and_assets(
        ShellLaunchConfig::default(),
        (120, 40),
        startup,
        ui::RuntimeAsciiAssets::from_store(store),
    );
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LocalAccount,
            session_id: "language-test".into(),
            user_id: "language-admin".into(),
            username: "admin".into(),
            role: UserRole::Admin,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    Fixture {
        root,
        state,
        storage,
    }
}

#[test]
fn same_language_selection_reloads_disk_and_keeps_retained_messages() {
    let mut f = fixture();
    f.state
        .save_region_picker_value(Some("zh-Hans".into()), None);
    assert_eq!(f.state.language_code(), "zh-CN");
    assert_eq!(f.storage.load_config().unwrap().language, "zh-CN");
    let generation = f.state.language.generation();
    let message = i18n::msg!("resources-recovery-ok");
    assert_eq!(f.state.language.render(&message), "确定");
    let path = f.root.join("assets/locales/zh-CN/recovery/startup.ftl");
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        original.replace(
            "resources-recovery-ok = 确定",
            "resources-recovery-ok = 重载成功",
        ),
    )
    .unwrap();
    f.state.save_region_picker_value(Some("zh-CN".into()), None);
    assert!(f.state.language.generation() > generation);
    assert_eq!(f.state.language.render(&message), "重载成功");
}

#[test]
fn failed_reload_preserves_snapshot_configuration_and_user_input() {
    let mut f = fixture();
    f.state.save_region_picker_value(Some("zh-CN".into()), None);
    f.state.login_username = "unchanged input".into();
    let snapshot = f.state.language.clone();
    let config = std::fs::read(&f.storage.layout().config_path).unwrap();
    std::fs::write(
        f.root.join("assets/locales/zh-CN/recovery/startup.ftl"),
        "broken = {\n",
    )
    .unwrap();
    f.state.save_region_picker_value(Some("zh-CN".into()), None);
    assert!(Arc::ptr_eq(&snapshot, &f.state.language));
    assert_eq!(
        std::fs::read(&f.storage.layout().config_path).unwrap(),
        config
    );
    assert_eq!(f.state.login_username, "unchanged input");
    assert!(
        f.state
            .app
            .notification_center()
            .alert_message_for_key("shell.language-reload")
            .is_some()
    );
}

#[test]
fn reloading_discovers_new_language_metadata_without_loading_it_during_render() {
    let mut f = fixture();
    let directory = f.root.join("assets/locales/fr-FR");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("manifest.toml"),
        "format_version = 1\ncode = \"fr-FR\"\nnative_name = \"Français\"\n",
    )
    .unwrap();
    std::fs::write(
        directory.join("messages.ftl"),
        "resources-recovery-ok = D’accord\n",
    )
    .unwrap();
    assert!(
        !f.state
            .language_options()
            .iter()
            .any(|option| option.code == "fr-FR")
    );
    f.state.save_region_picker_value(Some("en-US".into()), None);
    assert!(
        f.state
            .language_options()
            .iter()
            .any(|option| option.code == "fr-FR")
    );
    f.state.save_region_picker_value(Some("fr-FR".into()), None);
    std::fs::remove_dir_all(f.root.join("assets/locales")).unwrap();
    assert_eq!(
        f.state
            .language
            .render(&i18n::msg!("resources-recovery-ok")),
        "D’accord"
    );
    assert_eq!(
        f.state
            .language
            .render(&i18n::msg!("resources-recovery-title")),
        "Resource recovery"
    );
}

#[test]
fn recovery_summary_is_updated_in_place_and_distinguishes_fallback() {
    let mut f = fixture();
    f.state.repaired_resource_paths.push("assets/a".into());
    f.state.show_resource_recovery_report();
    let id = f.state.app.notification_center().active_modal_id();
    f.state.fallback_resource_paths.push("assets/b".into());
    f.state.show_resource_recovery_report();
    assert_eq!(f.state.app.notification_center().active_modal_id(), id);
    let modal = f.state.app.notification_center().active_modal().unwrap();
    assert!(
        f.state
            .language
            .render_text(&modal.message)
            .contains("Built-in resources are active")
    );
    assert_eq!(f.state.app.notification_center().queued_modal_count(), 0);
}

#[test]
fn configuration_write_failure_does_not_publish_valid_candidate() {
    let mut f = fixture();
    let before = f.state.language.clone();
    let disk = std::fs::read(&f.storage.layout().config_path).unwrap();
    let mut attempted = false;
    f.state
        .save_region_picker_value_with(Some("zh-CN".into()), None, |_, storage, candidate| {
            attempted = true;
            assert_eq!(candidate.language, "zh-CN");
            Err(storage::StorageError::Io {
                operation: "write test configuration",
                path: storage.layout().config_path.clone(),
                message: "injected persistence failure".into(),
            })
        });
    assert!(attempted);
    assert!(Arc::ptr_eq(&before, &f.state.language));
    assert_eq!(
        std::fs::read(&f.storage.layout().config_path).unwrap(),
        disk
    );
    assert_eq!(f.storage.load_config().unwrap().language, "en-US");
}

#[test]
fn healthy_reload_does_not_reopen_acknowledged_startup_recovery() {
    let mut f = fixture();
    f.state.repaired_resource_paths.push("old-repair".into());
    f.state.show_resource_recovery_report();
    f.state.app.dispatch_at(
        app::AppCommand::Notification(app::NotificationCommand::DismissModalByKey(
            "shell.resource-recovery".into(),
        )),
        Instant::now(),
    );
    f.state.save_region_picker_value(Some("en-US".into()), None);
    assert!(f.state.app.notification_center().active_modal().is_none());
    assert!(f.state.repaired_resource_paths.is_empty());
}
#[test]
fn post_rename_persistence_failure_restores_previous_configuration() {
    let mut f = fixture();
    let before = f.state.language.clone();
    let config = f.storage.load_config().unwrap();
    f.state
        .save_region_picker_value_with(Some("zh-CN".into()), None, |_, storage, candidate| {
            storage.save_config(candidate).unwrap();
            Err(storage::StorageError::Io {
                operation: "sync parent after rename",
                path: storage.layout().config_path.clone(),
                message: "injected directory sync failure".into(),
            })
        });
    assert!(Arc::ptr_eq(&before, &f.state.language));
    assert_eq!(f.storage.load_config().unwrap(), config);
}
#[test]
fn discovered_language_rows_share_setup_render_and_mouse_geometry() {
    let mut f = fixture();
    let directory = f.root.join("assets/locales/fr-FR");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("manifest.toml"),
        "format_version = 1\ncode = \"fr-FR\"\nnative_name = \"Français\"\n",
    )
    .unwrap();
    std::fs::write(
        directory.join("messages.ftl"),
        "resources-recovery-ok = Oui\n",
    )
    .unwrap();
    f.state.save_region_picker_value(Some("en-US".into()), None);
    f.state.screen_stack = vec![ShellScreen::FirstRunSetup];
    f.state.setup_step = ui::SetupStep::Language;
    f.state.focused_component = ShellComponent::SetupLanguage;
    f.state.refresh_hit_map();
    let count = f.state.language_options().len();
    assert_eq!(count, 3);
    let main = setup_main_rect(f.state.terminal_size).unwrap();
    let rendered = ui::setup_language_list_area(main, count);
    let region = f
        .state
        .hit_map
        .regions()
        .iter()
        .find(|region| region.component == ShellComponent::SetupLanguage)
        .unwrap();
    assert_eq!(region.area, rendered);
    let last_row = (rendered.x, rendered.y + 2);
    assert_eq!(f.state.setup_language_index_at(last_row), Some(2));
}
