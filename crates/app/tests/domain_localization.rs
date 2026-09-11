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
        for module in ["explorer", "launcher", "tasks"] {
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
