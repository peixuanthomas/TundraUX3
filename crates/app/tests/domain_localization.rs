use app::explorer::{
    ExplorerClipboardMode, ExplorerDialog, ExplorerOperationPhase, ExplorerOperationProgress,
    ExplorerTaskOperation,
};
use app::explorer_tasks::{
    ExplorerDeletePlan, ExplorerTaskError, ExplorerTaskPlan, ExplorerTaskSubmitError,
    ExplorerTransferOperation, ExplorerTransferPlan,
};
use app::launcher::LauncherItemStatus;
use i18n::{LanguageSnapshot, LocalizedText, msg};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct LocaleFixture(PathBuf);
impl LocaleFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tundra-domain-locale-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let zh = root.join("locales/zh-CN");
        fs::create_dir_all(zh.join("modules")).unwrap();
        fs::write(
            zh.join("manifest.toml"),
            "format_version = 1\ncode = \"zh-CN\"\nnative_name = \"简体中文\"\n",
        )
        .unwrap();
        for module in ["explorer", "launcher", "tasks", "catalog"] {
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../ascii-assets/assets/locales/zh-CN/modules/app-{module}.ftl"
            ));
            fs::copy(source, zh.join(format!("modules/app-{module}.ftl"))).unwrap();
        }
        Self(root)
    }
    fn chinese(&self) -> LanguageSnapshot {
        LanguageSnapshot::load(&self.0, "zh-CN", 2)
            .unwrap()
            .snapshot
    }
}
impl Drop for LocaleFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn retained_dialog_and_status_rerender_without_changing_raw_paths() {
    let fixture = LocaleFixture::new();
    let english = LanguageSnapshot::embedded(1);
    let chinese = fixture.chinese();
    let path = Path::new("/tmp/报告-{literal}.txt");
    let dialog = ExplorerDialog::delete(path);
    assert!(matches!(&dialog.message, LocalizedText::Message(_)));
    assert!(
        english
            .render_text(&dialog.message)
            .contains("system Trash")
    );
    assert!(chinese.render_text(&dialog.message).contains("系统废纸篓"));
    for locale in [&english, &chinese] {
        assert!(
            locale
                .render_text(&dialog.message)
                .contains(&path.display().to_string())
        );
    }
    assert_eq!(dialog.targets, [path.to_path_buf()]);
    let status = LauncherItemStatus::Ready.localized_label();
    assert_eq!(english.render_text(&status), "Ready");
    assert_eq!(chinese.render_text(&status), "就绪");
}

#[test]
fn task_operation_is_independent_of_translated_or_arbitrary_labels() {
    let plans = [
        (
            ExplorerTaskPlan::Transfer(ExplorerTransferPlan::new(
                ExplorerTransferOperation::Copy,
                vec!["/source".into()],
                "/target",
            )),
            ExplorerTaskOperation::Copy,
        ),
        (
            ExplorerTaskPlan::Transfer(ExplorerTransferPlan::new(
                ExplorerTransferOperation::Move,
                vec!["/source".into()],
                "/target",
            )),
            ExplorerTaskOperation::Move,
        ),
        (
            ExplorerTaskPlan::DeleteToTrash(ExplorerDeletePlan::new(vec!["/source".into()])),
            ExplorerTaskOperation::DeleteToTrash,
        ),
    ];
    for (plan, expected) in plans {
        let mut progress = ExplorerOperationProgress {
            operation: plan.operation(),
            phase: ExplorerOperationPhase::Executing,
            label: expected.localized_label(),
            completed_items: 0,
            total_items: None,
            completed_bytes: 0,
            total_bytes: None,
            cancellable: true,
        };
        assert_eq!(progress.operation, expected);
        progress.label = LocalizedText::Raw("任意标签".into());
        assert_eq!(progress.operation, expected);
    }
    assert_eq!(
        ExplorerTaskOperation::from(ExplorerClipboardMode::Cut),
        ExplorerTaskOperation::Move
    );
}

#[test]
fn structured_task_errors_keep_identity_across_languages() {
    let fixture = LocaleFixture::new();
    let english = LanguageSnapshot::embedded(1);
    let chinese = fixture.chinese();
    let error = ExplorerTaskError::PartialMove {
        path: "/tmp/source".into(),
    }
    .localized();
    assert_eq!(error.event_code, "EXPLORER_PARTIAL_MOVE");
    assert!(english.render(&error.message).contains("partially moved"));
    assert!(chinese.render(&error.message).contains("仅部分移动"));
    let plan = ExplorerTaskError::InvalidPlan {
        message: msg!("app-tasks-source-required").into(),
    }
    .localized();
    assert_eq!(plan.message.id, "app-tasks-source-required");
    assert_eq!(chinese.render(&plan.message), "传输至少需要一个源项目");
    let submit = ExplorerTaskSubmitError::RecoveryRequired.localized();
    assert!(chinese.render(&submit.message).contains("中断的操作"));
}

#[test]
fn catalog_localizes_presentation_without_changing_application_or_timezone_ids() {
    let fixture = LocaleFixture::new();
    let english = LanguageSnapshot::embedded(1);
    let chinese = fixture.chinese();
    for application in app::BUILT_IN_LAUNCHER_APPLICATIONS {
        let original = *application;
        assert_eq!(
            english.render_text(&application.localized_name()),
            application.name
        );
        assert_eq!(
            english.render_text(&application.localized_description()),
            application.description
        );
        assert_eq!(
            english.render_text(&application.localized_type_label()),
            application.type_label
        );
        assert_ne!(
            chinese.render_text(&application.localized_name()),
            application.name
        );
        assert_eq!(
            chinese.render_text(&application.localized_type_label()),
            "内置应用"
        );
        assert_eq!(*application, original);
    }
    let timezones = app::setup_timezone_options();
    for timezone in &timezones {
        assert_eq!(
            english.render_text(&timezone.localized_name()),
            timezone.label
        );
        assert_eq!(
            english.render_text(&timezone.localized_description()),
            timezone.description
        );
        assert!(
            !chinese
                .render_text(&timezone.localized_description())
                .starts_with('[')
        );
    }
    let shanghai = timezones
        .iter()
        .find(|zone| zone.id == "Asia/Shanghai")
        .unwrap();
    assert_eq!(chinese.render_text(&shanghai.localized_name()), "上海");
    assert_eq!(shanghai.id, "Asia/Shanghai");
    assert_eq!(timezones, app::setup_timezone_options());
}

#[test]
fn quick_location_labels_preserve_ids_custom_names_and_volume_names() {
    use app::explorer::ExplorerQuickLocation;
    let fixture = LocaleFixture::new();
    let english = LanguageSnapshot::embedded(1);
    let chinese = fixture.chinese();
    let documents = ExplorerQuickLocation::new("documents", "Documents", "/documents", "documents");
    assert_eq!(
        english.render_text(&documents.localized_label()),
        "Documents"
    );
    assert_eq!(chinese.render_text(&documents.localized_label()), "文档");
    assert_eq!(documents.id, "documents");
    assert_eq!(documents.path, Path::new("/documents"));
    for custom in [
        ExplorerQuickLocation::new("custom", "My {raw} folder", "/custom", "folder"),
        ExplorerQuickLocation::volume("documents", "My {raw} folder", "/drive"),
    ] {
        assert_eq!(
            chinese.render_text(&custom.localized_label()),
            "My {raw} folder"
        );
    }
    assert_eq!(
        chinese.render_text(&ExplorerQuickLocation::trash().localized_label()),
        "废纸篓"
    );
}

#[test]
fn localized_file_types_do_not_change_type_sort_keys_or_selection() {
    use app::explorer::{
        ExplorerEntry, ExplorerEntryKind, ExplorerFileType, ExplorerSortField, ExplorerState,
    };
    use std::sync::Arc;
    let fixture = LocaleFixture::new();
    let english = Arc::new(LanguageSnapshot::embedded(1));
    let chinese = Arc::new(fixture.chinese());
    let mut state = ExplorerState::new(&fixture.0, true);
    for (name, sort_key) in [
        ("b.txt", "TXT file"),
        ("a.rs", "RS file"),
        ("c.txt", "TXT file"),
    ] {
        let path = fixture.0.join(name);
        fs::write(&path, b"fixture").unwrap();
        let attributes = platform::default_file_attributes(&path).unwrap();
        state.all_entries.push(ExplorerEntry {
            name: name.into(),
            path: path.clone(),
            trash_id: None,
            original_path: None,
            kind: ExplorerEntryKind::File,
            size: attributes.len,
            modified: attributes.modified,
            attributes,
            open_policy: platform::FileOpenPolicy::SystemDefault,
            type_label: sort_key.into(),
            icon_key: "file".into(),
            metadata_warning: None,
        });
    }
    state.sort_field = ExplorerSortField::Type;
    state.selected_paths.insert(fixture.0.join("b.txt"));
    let mut orders = Vec::new();
    for snapshot in [&english, &chinese] {
        i18n::with_snapshot(snapshot, || {
            state.apply_projection();
            orders.push(
                state
                    .entries
                    .iter()
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>(),
            );
            let txt = state
                .entries
                .iter()
                .find(|entry| entry.name == "b.txt")
                .unwrap();
            assert_eq!(
                txt.file_type(),
                ExplorerFileType::File {
                    extension: Some("TXT".into())
                }
            );
            assert_eq!(txt.type_label, "TXT file");
            let display = txt.localized_type_label().render_current();
            assert!(display.contains(if snapshot.code() == "zh-CN" {
                "文件"
            } else {
                "file"
            }));
        });
    }
    assert_eq!(orders[0], orders[1]);
    assert_eq!(
        state
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["a.rs", "b.txt", "c.txt"]
    );
    assert!(state.selected_paths.contains(&fixture.0.join("b.txt")));
}

#[test]
fn diagnostic_display_ignores_current_language_and_custom_english_resources() {
    use std::sync::Arc;
    let fixture = LocaleFixture::new();
    let chinese = Arc::new(fixture.chinese());
    let explorer = app::explorer::ExplorerError::Localized(i18n::LocalizedError::new(
        "EXPLORER_INVALID_OPERATION",
        msg!("app-explorer-nothing-selected"),
    ));
    let launcher = app::launcher::LauncherError::Localized(i18n::LocalizedError::new(
        "LAUNCHER_INVALID_PATH",
        msg!(
            "app-launcher-absolute-path-required",
            path = "/tmp/{raw}.exe"
        ),
    ));
    let task = ExplorerTaskError::InvalidPlan {
        message: msg!("app-tasks-source-required").into(),
    };
    let errors: [&dyn std::fmt::Display; 3] = [&explorer, &launcher, &task];
    let expected = errors.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(expected[0], "nothing selected");
    assert!(expected[1].contains("/tmp/{raw}.exe"));
    assert_eq!(expected[2], "a transfer requires at least one source");
    i18n::with_snapshot(&chinese, || {
        assert_eq!(
            errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            explorer.localized().message.render_current(),
            "未选择任何项目"
        );
    });
    for (module, text) in [
        (
            "explorer",
            "app-explorer-nothing-selected = Customized selection error\n",
        ),
        (
            "launcher",
            "app-launcher-absolute-path-required = Customized path { $path }\n",
        ),
        (
            "tasks",
            "app-tasks-source-required = Customized source error\n",
        ),
    ] {
        fs::write(
            fixture
                .0
                .join(format!("locales/en-US/modules/app-{module}.ftl")),
            text,
        )
        .unwrap();
    }
    let custom_english = Arc::new(
        LanguageSnapshot::load(&fixture.0, "en-US", 3)
            .unwrap()
            .snapshot,
    );
    i18n::with_snapshot(&custom_english, || {
        assert_eq!(
            explorer.localized().message.render_current(),
            "Customized selection error"
        );
        assert_eq!(
            errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
            expected
        );
    });
    assert_eq!(
        ExplorerTaskError::InvalidPlan {
            message: LocalizedText::Raw("raw {diagnostic}".into())
        }
        .to_string(),
        "raw {diagnostic}"
    );
}
