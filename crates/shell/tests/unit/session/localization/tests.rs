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
        f.root.join("assets/locales/zh-CN/custom.ftl"),
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
    assert_eq!(modal.actions.len(), 2);
    assert_eq!(modal.actions[0].id, "repair-restart");
    assert_eq!(modal.actions[1].id, "exit");
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
fn resource_repair_action_restores_chinese_and_graphics_then_requests_restart() {
    let mut f = fixture();
    let chinese = f.root.join("assets/locales/zh-CN/recovery/startup.ftl");
    let image = f
        .root
        .join("assets/themes/default/home_icons/system_status.png");
    std::fs::write(&chinese, "broken = {\n").unwrap();
    std::fs::remove_file(&image).unwrap();
    f.state
        .fallback_resource_paths
        .push(chinese.display().to_string());
    f.state.show_resource_recovery_report();
    assert_eq!(
        f.state.apply_input(InputEvent::key(InputKey::Enter)),
        ShellAction::Exit
    );
    assert!(f.state.restart_requested);
    assert!(image.is_file());
    assert!(
        std::fs::read_to_string(chinese)
            .unwrap()
            .contains("resources-recovery-title = 资源恢复")
    );
}

#[test]
fn repaired_unreadable_chinese_file_is_not_reported_as_fallback() {
    let mut f = fixture();
    let path = f.root.join("assets/locales/zh-CN/recovery/startup.ftl");
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    f.state.save_region_picker_value(Some("zh-CN".into()), None);
    assert_eq!(f.state.language_code(), "zh-CN");
    assert!(
        f.state
            .repaired_resource_paths
            .iter()
            .any(|repaired| Path::new(repaired) == path)
    );
    assert!(f.state.fallback_resource_paths.is_empty());
    let modal = f.state.app.notification_center().active_modal().unwrap();
    assert_eq!(
        f.state.language.render_text(&modal.actions[0].label),
        "自动修复并重启"
    );
}

#[test]
fn failed_resource_repair_keeps_two_actions_and_exit_does_not_restart() {
    let mut f = fixture();
    let image = f
        .root
        .join("assets/themes/default/home_icons/system_status.png");
    std::fs::remove_file(&image).unwrap();
    std::fs::create_dir(&image).unwrap();
    f.state
        .fallback_resource_paths
        .push(image.display().to_string());
    f.state.show_resource_recovery_report();
    assert_eq!(
        f.state.apply_input(InputEvent::key(InputKey::Enter)),
        ShellAction::Redraw
    );
    assert!(!f.state.restart_requested);
    let modal = f.state.app.notification_center().active_modal().unwrap();
    assert_eq!(modal.actions.len(), 2);
    assert!(
        f.state
            .language
            .render_text(&modal.message)
            .contains("could not be repaired")
    );
    assert!(image.is_dir());
    assert_eq!(
        f.state.apply_input(InputEvent::key(InputKey::Escape)),
        ShellAction::Exit
    );
    assert!(!f.state.restart_requested);
}

#[test]
fn failed_resource_repair_can_retry_after_write_blocker_is_removed() {
    let mut f = fixture();
    let language_file = f.root.join("assets/locales/zh-CN/recovery/startup.ftl");
    std::fs::remove_file(&language_file).unwrap();
    std::fs::create_dir(&language_file).unwrap();
    f.state
        .fallback_resource_paths
        .push(language_file.display().to_string());
    f.state.show_resource_recovery_report();
    assert_eq!(
        f.state.apply_input(InputEvent::key(InputKey::Enter)),
        ShellAction::Redraw
    );
    assert!(!f.state.restart_requested);
    std::fs::remove_dir(&language_file).unwrap();
    assert_eq!(
        f.state.apply_input(InputEvent::key(InputKey::Enter)),
        ShellAction::Exit
    );
    assert!(f.state.restart_requested);
    assert!(language_file.is_file());
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
