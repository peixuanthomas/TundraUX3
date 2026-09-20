use super::*;

fn settings_language_snapshots() -> Vec<std::sync::Arc<i18n::LanguageSnapshot>> {
    let root = std::env::temp_dir().join(format!(
        "tux3-settings-locales-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let canonical =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets/locales");
    let mut snapshots = Vec::new();
    for code in ["en-US", "zh-CN"] {
        let locale = root.join("locales").join(code);
        std::fs::create_dir_all(locale.join("modules")).unwrap();
        for relative in [
            "manifest.toml",
            "modules/settings.ftl",
            "modules/ui-settings.ftl",
        ] {
            std::fs::copy(canonical.join(code).join(relative), locale.join(relative)).unwrap();
        }
        snapshots.push(std::sync::Arc::new(
            i18n::LanguageSnapshot::load(&root, code, 1)
                .unwrap()
                .snapshot,
        ));
    }
    std::fs::remove_dir_all(root).unwrap();
    snapshots
}

#[test]
fn saved_settings_and_validation_messages_rerender_with_their_arguments() {
    let saved: i18n::LocalizedText = settings_saved_field(ui::SettingsField::BorderShape).into();
    let category: i18n::LocalizedText = i18n::msg!(
        "settings-category-status",
        category = format!("{:?}", ui::SettingsCategory::RegionTime)
    )
    .into();
    let invalid = parse_editor_explorer_open_extensions("bad/suffix").unwrap_err();
    let relation = update_relation_label(&app::update::UpdateRelation::Diverged {
        remote_ahead: 2,
        local_ahead: 3,
    });
    let error: i18n::LocalizedText = i18n::msg!(
        "settings-update-failed",
        reason = i18n::msg!("settings-admin-required")
    )
    .into();
    let retained = (
        saved.clone(),
        category.clone(),
        invalid.clone(),
        relation.clone(),
    );
    for (snapshot, expected_saved, expected_category, expected_invalid, expected_relation) in
        settings_language_snapshots()
            .into_iter()
            .zip([
                (
                    "Saved Border shape",
                    "Settings: Region & Time",
                    "Invalid suffix",
                    "Builds diverged",
                ),
                (
                    "已保存边框形状",
                    "设置：区域与时间",
                    "无效后缀",
                    "构建已分叉",
                ),
            ])
            .map(|(snapshot, (saved, category, invalid, relation))| {
                (snapshot, saved, category, invalid, relation)
            })
    {
        let expected_error = if snapshot.code() == "en-US" {
            "Update failed: Administrator permission is required"
        } else {
            "更新失败：需要管理员权限"
        };
        let _language = i18n::enter_snapshot(snapshot);
        assert_eq!(saved.render_current(), expected_saved);
        assert_eq!(category.render_current(), expected_category);
        assert!(invalid.render_current().starts_with(expected_invalid));
        assert!(invalid.render_current().contains("bad/suffix"));
        let rendered_relation = relation.render_current();
        assert!(rendered_relation.starts_with(expected_relation));
        assert!(rendered_relation.contains('2'));
        assert!(rendered_relation.contains('3'));
        assert_eq!(error.render_current(), expected_error);
        assert_eq!(
            (&saved, &category, &invalid, &relation),
            (&retained.0, &retained.1, &retained.2, &retained.3)
        );
    }
}

#[test]
fn update_cards_and_picker_labels_rerender_without_changing_action_or_color_values() {
    let identity = app::update::BuildIdentity {
        package_version: "1.2.3".to_string(),
        commit_sha: Some("1111111111111111".to_string()),
        dirty: false,
    };
    let update = checked_update_state(app::update::UpdateRelation::Behind { remote_ahead: 1 });
    let picker = SettingsPickerState {
        kind: ui::SettingsPickerKind::BorderColor,
        query: String::new(),
        selected_index: 0,
        window_start: 0,
        image_icons_supported: false,
    };
    for (snapshot, (start_label, custom_label)) in settings_language_snapshots().into_iter().zip([
        ("Start update", "Custom color…"),
        ("开始更新", "自定义颜色…"),
    ]) {
        let _language = i18n::enter_snapshot(snapshot);
        let cards = update_settings_cards(&identity, &update, true, true);
        let start = cards
            .iter()
            .flat_map(|card| &card.items)
            .find(|item| item.field == ui::SettingsField::StartUpdate)
            .unwrap();
        assert_eq!(start.label, start_label);
        assert!(start.enabled);
        let options = settings_picker_options(&picker, &[]);
        let custom = options.last().unwrap();
        assert_eq!(custom.label, custom_label);
        assert_eq!(custom.detail, "#RRGGBB");
        assert_eq!(options[0].detail, "white");
    }
}

fn checked_update_state(relation: app::update::UpdateRelation) -> SettingsUpdateState {
    SettingsUpdateState {
        rpm: None,
        activity: None,
        check_result: Some(app::update::UpdateCheckResult {
            default_branch: "master".to_string(),
            head_sha: "abcdef1234567890".to_string(),
            relation,
            commits: vec![app::update::UpdateCommit {
                sha: "abcdef1234567890".to_string(),
                message: "Complete commit message\nwith body".to_string(),
            }],
        }),
        checked_at: Some(Utc::now()),
        phase: None,
        status: "Checked".into(),
        error: None,
        confirmation_open: false,
        confirm_selected: true,
        busy: false,
        checked_once: true,
    }
}

#[test]
fn update_settings_enable_install_only_for_supported_admin_builds() {
    let identity = app::update::BuildIdentity {
        package_version: "0.1.1".to_string(),
        commit_sha: Some("1111111111111111".to_string()),
        dirty: false,
    };
    let update = checked_update_state(app::update::UpdateRelation::Behind { remote_ahead: 1 });
    let admin_cards = update_settings_cards(&identity, &update, true, true);
    let admin_start = admin_cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == ui::SettingsField::StartUpdate)
        .unwrap();
    assert!(admin_start.enabled);
    assert_eq!(admin_start.label, "Start update");

    let user_cards = update_settings_cards(&identity, &update, true, false);
    let user_start = user_cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == ui::SettingsField::StartUpdate)
        .unwrap();
    assert!(!user_start.enabled);

    let unsupported = update_settings_cards(&identity, &update, false, true);
    assert!(
        unsupported
            .iter()
            .flat_map(|card| &card.items)
            .filter(|item| {
                matches!(
                    item.field,
                    ui::SettingsField::CheckUpdates | ui::SettingsField::StartUpdate
                )
            })
            .all(|item| !item.enabled)
    );
}

#[test]
fn update_settings_warn_for_dirty_and_diverged_builds() {
    let identity = app::update::BuildIdentity {
        package_version: "0.1.1".to_string(),
        commit_sha: Some("1111111111111111".to_string()),
        dirty: true,
    };
    let update = checked_update_state(app::update::UpdateRelation::Diverged {
        remote_ahead: 2,
        local_ahead: 3,
    });
    let cards = update_settings_cards(&identity, &update, true, true);
    let start = cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == ui::SettingsField::StartUpdate)
        .unwrap();
    assert!(start.enabled);
    assert_eq!(start.label, "Replace with GitHub version");
    let remote = cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == ui::SettingsField::RemoteVersion)
        .unwrap();
    assert!(remote.description.contains("Builds diverged"));
    assert!(remote.description.contains("abcdef1234567890"));
}

#[test]
fn settings_scroll_stops_at_the_content_boundaries() {
    assert_eq!(settings_scroll_offset(2_001, 6, 2_004), 2_004);
    assert_eq!(settings_scroll_offset(2_007, -6, 2_100), 2_001);
    assert_eq!(settings_scroll_offset(0, -6, 2_100), 0);
    assert_eq!(settings_scroll_offset(0, 6, 0), 0);
}
