use super::*;
use i18n::LanguageSnapshot;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "tundra-diagnostics-localization-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        let locale = root.join("locales/zh-CN");
        fs::create_dir_all(locale.join("modules")).unwrap();
        fs::write(
            locale.join("manifest.toml"),
            "format_version = 1\ncode = \"zh-CN\"\nnative_name = \"简体中文\"\n",
        )
        .unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../ascii-assets/assets/locales/zh-CN/modules/app-diagnostics.ftl"),
            locale.join("modules/app-diagnostics.ftl"),
        )
        .unwrap();
        Self(root)
    }
    fn chinese(&self) -> LanguageSnapshot {
        LanguageSnapshot::load(&self.0, "zh-CN", 2)
            .unwrap()
            .snapshot
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn check(id: &str, category: DiagnosticCategory, status: DiagnosticStatus) -> DiagnosticCheck {
    DiagnosticCheck {
        id: id.into(),
        category,
        status,
        label: "Unrelated custom diagnostic label".into(),
        summary: "RAW summary {literal}".into(),
        detail: "RAW detail /private/path".into(),
        remediation: Some("RAW guidance {literal}".into()),
        repair: None,
    }
}

#[test]
fn project_summaries_use_ids_and_typed_outcomes_without_changing_raw_diagnostics() {
    let fixture = Fixture::new();
    let english = LanguageSnapshot::embedded(1);
    let chinese = fixture.chinese();
    let mut checks = vec![
        check(
            "environment.terminal",
            DiagnosticCategory::Environment,
            DiagnosticStatus::Warning,
        ),
        check(
            "environment.capability.open_path",
            DiagnosticCategory::Environment,
            DiagnosticStatus::Pass,
        ),
        check(
            "path.data-path",
            DiagnosticCategory::Paths,
            DiagnosticStatus::Warning,
        ),
        check(
            "storage.state",
            DiagnosticCategory::Storage,
            DiagnosticStatus::Fail,
        ),
        check(
            "asset.home_icons/explorer.png",
            DiagnosticCategory::Assets,
            DiagnosticStatus::Warning,
        ),
        check(
            "incident-history.warning-0",
            DiagnosticCategory::Storage,
            DiagnosticStatus::Warning,
        ),
    ];
    checks[2].repair = Some(DiagnosticsRepairAction::CreateDirectory {
        label: "Never match this".into(),
        path: fixture.0.join("missing"),
    });
    checks[3].repair = Some(DiagnosticsRepairAction::RepairStorageDocument(
        StorageDocumentKind::State,
    ));
    for check in &checks {
        let original = check.clone();
        for text in [
            check.localized_label(),
            check.localized_summary(),
            check.localized_remediation().unwrap(),
        ] {
            let en = english.render_text(&text);
            let zh = chinese.render_text(&text);
            assert_ne!(en, zh, "{}", check.id);
            assert!(
                !en.starts_with('[') && !zh.starts_with('['),
                "{}: {en} / {zh}",
                check.id
            );
            assert!(!zh.contains("RAW") && !zh.contains("Unrelated"));
        }
        assert_eq!(*check, original);
    }
    assert_eq!(chinese.render_text(&checks[0].localized_label()), "终端");
    assert_eq!(
        chinese.render_text(&checks[3].localized_summary()),
        "存储文档已损坏"
    );
    checks[3].repair = None;
    assert_eq!(
        chinese.render_text(&checks[3].localized_summary()),
        "不支持此存储文档格式版本"
    );
    checks[0].remediation = None;
    assert!(checks[0].localized_remediation().is_none());
}

#[test]
fn unknown_checks_and_raw_details_are_preserved_verbatim() {
    let fixture = Fixture::new();
    let chinese = fixture.chinese();
    let unknown = check(
        "external.plugin-check",
        DiagnosticCategory::Environment,
        DiagnosticStatus::Fail,
    );
    assert_eq!(
        chinese.render_text(&unknown.localized_label()),
        unknown.label
    );
    assert_eq!(
        chinese.render_text(&unknown.localized_summary()),
        unknown.summary
    );
    assert_eq!(
        chinese.render_text(&unknown.localized_remediation().unwrap()),
        unknown.remediation.as_ref().unwrap().as_str()
    );
}

#[test]
fn repairs_retain_translatable_actions_and_keep_failure_details_raw() {
    let fixture = Fixture::new();
    let chinese = fixture.chinese();
    let action = DiagnosticsRepairAction::RepairStorageDocument(StorageDocumentKind::State);
    let original_label = action.label();
    assert_eq!(
        chinese.render_text(&action.localized_label()),
        "修复状态存储文档"
    );
    assert_eq!(action.label(), original_label);
    let mut result = DiagnosticsRepairResult {
        action,
        success: true,
        changed: true,
        message: "raw repair result".into(),
        backup_path: Some(fixture.0.join("backup")),
    };
    assert_eq!(
        chinese.render_text(&result.localized_summary()),
        "已备份并重建存储文档"
    );
    result.success = false;
    assert_eq!(
        chinese.render_text(&result.localized_summary()),
        "修复失败，请查看诊断详情"
    );
    assert_eq!(result.message, "raw repair result");
    assert_eq!(
        DiagnosticsTaskError::Busy.localized().event_code,
        "DIAGNOSTICS_BUSY"
    );
}

#[test]
fn platform_ids_are_stable_even_when_path_roles_alias_and_labels_change() {
    let fixture = Fixture::new();
    let shared = fixture.0.join("shared");
    fs::create_dir_all(&shared).unwrap();
    let paths = AppPaths::from_parts(
        shared.join("config.toml"),
        shared.clone(),
        shared.clone(),
        shared.clone(),
        shared.clone(),
    )
    .unwrap();
    let dirs = platform::UserDirs::new(
        shared.clone(),
        shared.clone(),
        shared.clone(),
        shared.clone(),
        shared.clone(),
        shared.clone(),
        shared.clone(),
    )
    .unwrap();
    let platform =
        platform::mock::MockPlatform::new(dirs, paths).with_kind(platform::PlatformKind::Macos);
    let report = platform::run_doctor_with(&platform).unwrap();
    let ids = report
        .path_checks
        .iter()
        .map(|check| check.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "config-parent",
            "data-path",
            "cache-path",
            "logs-path",
            "temp-path"
        ]
    );
    assert!(report.path_checks.iter().all(|check| check.path == shared));
    let env_ids = report
        .environment_checks
        .iter()
        .map(|check| check.id)
        .collect::<HashSet<_>>();
    assert_eq!(env_ids.len(), report.environment_checks.len());
    assert!(
        env_ids.contains("platform")
            && env_ids.contains("terminal")
            && env_ids.contains("startup-permissions")
    );
    for (key, status) in platform.capabilities().checks() {
        let mut check = platform::EnvironmentCheck::capability(key, status);
        assert_eq!(check.id, format!("capability.{key}"));
        assert_eq!(check.label, format!("Capability: {key}"));
        assert_eq!(check.message, status.as_str());
        check.label = "文案发生变化".into();
        assert_eq!(check.id, format!("capability.{key}"));
        let domain = super::super::environment_diagnostic(check);
        assert!(matches!(
            domain.localized_label(),
            LocalizedText::Message(_)
        ));
    }
}
