use crate::*;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tux-i18n-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        let fixture = Self(path);
        fixture.locale("en-US", "English", "");
        fixture
    }
    fn write(&self, relative: &str, source: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn locale(&self, code: &str, name: &str, ftl: &str) {
        self.write(
            &format!("locales/{code}/manifest.toml"),
            &format!("format_version = 1\ncode = {code:?}\nnative_name = {name:?}\n"),
        );
        self.write(&format!("locales/{code}/test.ftl"), ftl);
    }
    fn snapshot(&self, code: &str, generation: u64) -> LanguageSnapshot {
        LanguageSnapshot::load(&self.0, code, generation)
            .unwrap()
            .snapshot
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn catalog_discovers_manifest_names_and_canonicalizes_alias() {
    let fixture = Fixture::new();
    fixture.locale("zh-CN", "简体中文", "");
    fixture.locale("fr-FR", "Français", "");
    fixture.locale("bad", "Invalid", "");
    fixture.write(
        "locales/xx/manifest.toml",
        "format_version = 2\ncode = 'xx'\nnative_name = 'Wrong'\n",
    );
    let catalog = LanguageCatalog::discover(&fixture.0).unwrap();
    assert_eq!(
        catalog
            .options()
            .iter()
            .map(|option| option.code.as_str())
            .collect::<Vec<_>>(),
        ["bad", "en-US", "fr-FR", "zh-CN"]
    );
    assert_eq!(catalog.options().last().unwrap().native_name, "简体中文");
    assert_eq!(catalog.diagnostics.len(), 1);
    assert_eq!(canonicalize_locale("zh-Hans").unwrap(), "zh-CN");
    assert_eq!(fixture.snapshot("zh-Hans", 7).code(), "zh-CN");
    assert!(canonicalize_locale("../en-US").is_err());
    assert_eq!(LanguageCatalog::built_in().options().len(), 2);
}

#[test]
fn named_arguments_plurals_recursive_resources_and_missing_message_fallback() {
    let fixture = Fixture::new();
    fixture.locale("en-US", "English", "test-greeting = Hello { $name }\ntest-items = { $count ->\n [one] One item\n *[other] { $count } items\n}\ntest-ref = { test-greeting }\n");
    fixture.locale("zh-CN", "简体中文", "test-greeting = 你好，{ $name }\n");
    fixture.write(
        "locales/zh-CN/modules/deep/nested.ftl",
        "test-extra = 附加\n",
    );
    let snapshot = fixture.snapshot("zh-CN", 3);
    assert_eq!(
        msg!("test-greeting", name = "Ada").render(&snapshot),
        "你好，Ada"
    );
    assert_eq!(
        msg!("test-ref", name = "Ada").render(&snapshot),
        "Hello Ada"
    );
    assert_eq!(msg!("test-items", count = 1).render(&snapshot), "One item");
    assert_eq!(msg!("test-items", count = 2).render(&snapshot), "2 items");
    assert_eq!(msg!("test-extra").render(&snapshot), "附加");
    assert!(
        LanguageSnapshot::load(&fixture.0, "zh-CN", 3)
            .unwrap()
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == RepairKind::MissingTranslations)
    );
    assert_eq!(msg!("test-unknown").render(&snapshot), "[test-unknown]");
    assert_eq!(msg!("test-greeting").render(&snapshot), "[test-greeting]");
}

#[test]
fn strict_rejects_syntax_duplicates_missing_references_cycles_and_contract_mismatch() {
    let fixture = Fixture::new();
    fixture.locale("en-US", "English", "test-msg = Hi { $name }\n");
    for (source, kind) in [
        ("test-msg = {\n", LanguageErrorKind::Syntax),
        ("test-msg = A\ntest-msg = B\n", LanguageErrorKind::Duplicate),
        ("test-msg = { unknown-id }\n", LanguageErrorKind::Reference),
        (
            "test-msg = { test-other }\ntest-other = { test-msg }\n",
            LanguageErrorKind::Reference,
        ),
        ("test-msg = Hi { $other }\n", LanguageErrorKind::Parameters),
        ("test-msg = Hi\n", LanguageErrorKind::Parameters),
        (
            "test-msg = { UNKNOWN($name) }\n",
            LanguageErrorKind::Reference,
        ),
    ] {
        fixture.locale("zh-CN", "简体中文", source);
        let error = LanguageSnapshot::load(&fixture.0, "zh-CN", 2).unwrap_err();
        assert_eq!(error.kind, kind, "{source}: {error}");
    }
    fixture.locale("zh-CN", "简体中文", "test-msg = { $name }\n");
    fixture.write(
        "locales/zh-CN/modules/duplicate.ftl",
        "test-msg = { $name }\n",
    );
    assert_eq!(
        LanguageSnapshot::load(&fixture.0, "zh-CN", 2)
            .unwrap_err()
            .kind,
        LanguageErrorKind::Duplicate
    );
}

#[test]
fn transitive_contracts_terms_attributes_and_number_function() {
    let fixture = Fixture::new();
    fixture.locale("en-US", "English", "-test-brand = Brand\ntest-message = { -test-brand } { test-child }\ntest-child = Hello { $name }\ntest-number = { NUMBER($count) }\ntest-button = Button\n .label = Label { $name }\n");
    fixture.locale("zh-CN", "简体中文", "-test-brand = 品牌\ntest-message = { -test-brand } { test-child }\ntest-child = 你好 { $name }\n");
    let snapshot = fixture.snapshot("zh-CN", 2);
    assert_eq!(
        msg!("test-message", name = "Ada").render(&snapshot),
        "品牌 你好 Ada"
    );
    assert_eq!(
        msg!("test-button.label", name = "Ada").render(&snapshot),
        "Label Ada"
    );
    assert_eq!(msg!("test-number", count = 12).render(&snapshot), "12");
    fixture.locale(
        "zh-CN",
        "简体中文",
        "test-message = { test-child }\ntest-child = Wrong { $other }\n",
    );
    assert_eq!(
        LanguageSnapshot::load(&fixture.0, "zh-CN", 2)
            .unwrap_err()
            .kind,
        LanguageErrorKind::Parameters
    );
}

#[test]
fn startup_keeps_valid_partial_translation_and_records_rejected_files() {
    let fixture = Fixture::new();
    fixture.locale(
        "en-US",
        "English",
        "test-valid = Original\ntest-broken = Original broken\ntest-other = { test-broken }\n",
    );
    fixture.locale("zh-CN", "简体中文", "test-valid = 翻译\n");
    fixture.write("locales/zh-CN/modules/broken.ftl", "test-broken = {\n");
    fixture.write(
        "locales/zh-CN/modules/other.ftl",
        "test-other = { test-broken }\n",
    );
    let load = LanguageSnapshot::load_startup(&fixture.0, "zh-CN", 8);
    assert_eq!(load.snapshot.code(), "zh-CN");
    assert_eq!(msg!("test-valid").render(&load.snapshot), "翻译");
    assert_eq!(
        msg!("test-broken").render(&load.snapshot),
        "Original broken"
    );
    assert_eq!(msg!("test-other").render(&load.snapshot), "Original broken");
    assert!(
        load.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == RepairKind::InvalidResource)
    );
    let unknown = LanguageSnapshot::load_startup(&fixture.0, "../../unknown", 9);
    assert_eq!(unknown.snapshot.code(), "en-US");
    assert!(
        unknown
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == RepairKind::StartupFallback)
    );
}

#[test]
fn repair_preserves_healthy_bytes_and_appends_missing_messages() {
    let fixture = Fixture::new();
    let first = LanguageSnapshot::load(&fixture.0, "en-US", 1).unwrap();
    assert!(
        first
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.repaired)
    );
    let (relative, original) = EMBEDDED_FILES
        .iter()
        .find(|(_, text)| {
            !crate::resource::identifiers(&[crate::resource::Source {
                path: PathBuf::new(),
                text: text.to_string(),
            }])
            .is_empty()
        })
        .unwrap();
    let path = fixture.0.join("locales/en-US").join(relative);
    let healthy = format!("# User comment retained\n{original}\n");
    fs::write(&path, &healthy).unwrap();
    let load = LanguageSnapshot::load(&fixture.0, "en-US", 2).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), healthy);
    assert!(
        !load
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.path == path)
    );
    fs::write(&path, "# User content\ntest-local-extra = Custom\n").unwrap();
    let load = LanguageSnapshot::load(&fixture.0, "en-US", 3).unwrap();
    let repaired = fs::read_to_string(&path).unwrap();
    assert!(repaired.starts_with("# User content\ntest-local-extra = Custom\n"));
    assert_eq!(msg!("test-local-extra").render(&load.snapshot), "Custom");
    assert!(load.diagnostics.iter().any(|diagnostic| diagnostic.kind == RepairKind::MissingMessages && diagnostic.repaired));
    fs::write(&path, "broken = {\n").unwrap();
    let load = LanguageSnapshot::load(&fixture.0, "en-US", 4).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), *original);
    assert!(
        load.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == RepairKind::CorruptFile && diagnostic.repaired)
    );
    assert!(!fs::read_dir(path.parent().unwrap()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}

#[test]
fn unwritable_root_still_loads_embedded_and_reports_write_failures() {
    let fixture = Fixture::new();
    let root = fixture.0.join("not-a-directory");
    fs::write(&root, "blocking file").unwrap();
    let load = LanguageSnapshot::load(&root, "en-US", 11).unwrap();
    assert_eq!(load.snapshot.generation(), 11);
    assert!(
        load.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == RepairKind::WriteFailed && !diagnostic.repaired)
    );
    let id = first_embedded_message();
    assert_eq!(
        msg!(&id).render(&load.snapshot),
        msg!(&id).render(&LanguageSnapshot::embedded(11))
    );
    assert_eq!(fs::read_to_string(root).unwrap(), "blocking file");
}

fn first_embedded_message() -> String {
    for (_, source) in EMBEDDED_FILES {
        if let Ok(resource) = fluent_syntax::parser::parse(*source) {
            for entry in resource.body {
                if let fluent_syntax::ast::Entry::Message(message) = entry {
                    return message.id.name.to_owned();
                }
            }
        }
    }
    panic!("embedded resources must contain messages")
}

#[test]
fn snapshots_are_immutable_send_sync_and_compare_content() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LanguageSnapshot>();
    let fixture = Fixture::new();
    fixture.locale("en-US", "English", "test-stable = First\n");
    let first = fixture.snapshot("en-US", 1);
    assert_eq!(first, fixture.snapshot("en-US", 1));
    assert_ne!(first, fixture.snapshot("en-US", 2));
    fixture.write("locales/en-US/test.ftl", "test-stable = Second\n");
    assert_ne!(first, fixture.snapshot("en-US", 1));
    fs::remove_dir_all(fixture.0.join("locales")).unwrap();
    assert_eq!(msg!("test-stable").render(&first), "First");
    assert_eq!(
        std::thread::spawn(move || msg!("test-stable").render(&first))
            .join()
            .unwrap(),
        "First"
    );
}

#[test]
fn scopes_are_nested_panic_safe_thread_local_and_raw_text_stays_raw() {
    let fixture = Fixture::new();
    fixture.locale("en-US", "English", "test-scoped = English\n");
    fixture.locale("zh-CN", "简体中文", "test-scoped = 中文\n");
    let english = Arc::new(fixture.snapshot("en-US", 1));
    let chinese = Arc::new(fixture.snapshot("zh-CN", 2));
    with_snapshot(&english, || {
        assert_eq!(tr!("test-scoped"), "English");
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_snapshot(&chinese, || {
                assert_eq!(tr!("test-scoped"), "中文");
                panic!("test scope unwind");
            })
        }));
        assert!(panic.is_err());
        assert_eq!(tr!("test-scoped"), "English");
        assert_eq!(
            LocalizedText::from("test-scoped").render_current(),
            "test-scoped"
        );
        assert_eq!(
            LocalizedText::from(msg!("test-scoped")).render_current(),
            "English"
        );
        assert_eq!(
            std::thread::spawn(|| tr!("test-scoped")).join().unwrap(),
            "[test-scoped]"
        );
    });
    assert_eq!(tr!("test-scoped"), "[test-scoped]");
    let outer = enter_snapshot(english);
    let inner = enter_snapshot(chinese);
    drop(outer);
    assert_eq!(tr!("test-scoped"), "中文");
    drop(inner);
    assert_eq!(tr!("test-scoped"), "[test-scoped]");
}

#[test]
fn message_serialization_and_error_chain_preserve_machine_identity() {
    let message = msg!("test-event", name = "Ada", count = 42_u32);
    let encoded = toml::to_string(&message).unwrap();
    assert_eq!(
        toml::from_str::<LocalizedMessage>(&encoded).unwrap(),
        message
    );
    let cause = LocalizedError::new("disk.failure", msg!("disk-failed"));
    let error = LocalizedError::new("language.failure", message).with_cause(cause);
    assert_eq!(error.event_code, "language.failure");
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(
        MessageArg::from(u64::MAX),
        MessageArg::String(u64::MAX.to_string())
    );
}

#[test]
fn canonical_english_and_chinese_resources_validate() {
    let root = Path::new(ascii_assets::CANONICAL_ASSETS_DIR);
    if !root.join("locales/en-US").is_dir() {
        // The crate bootstraps independently while canonical assets are being created.
        return;
    }
    let english = crate::resource::read_sources(&root.join("locales/en-US")).unwrap();
    crate::resource::validate(&english, None, true).unwrap();
    let chinese = crate::resource::read_sources(&root.join("locales/zh-CN")).unwrap();
    crate::resource::validate(&chinese, Some(&english), true).unwrap();
}

#[test]
fn emergency_recovery_messages_are_always_available() {
    let snapshot = LanguageSnapshot::embedded(0);
    assert_eq!(
        msg!("resources-recovery-title").render(&snapshot),
        "Resource recovery"
    );
    assert_eq!(msg!("resources-recovery-ok").render(&snapshot), "OK");
    let rendered = msg!("resources-repaired", count = 1, files = "manifest.toml").render(&snapshot);
    assert!(rendered.contains("manifest.toml"));
    assert!(!rendered.contains("[resources-repaired]"));
}

#[test]
fn missing_translation_reference_falls_back_to_whole_english_message() {
    let fixture = Fixture::new();
    fixture.locale(
        "en-US",
        "English",
        "test-parent = English { test-child }\ntest-child = Child\n",
    );
    fixture.locale("zh-CN", "简体中文", "test-parent = 中文 { test-child }\n");
    let snapshot = fixture.snapshot("zh-CN", 1);
    assert_eq!(msg!("test-parent").render(&snapshot), "English Child");
}

#[test]
fn startup_preserves_valid_files_when_neighbor_is_invalid_utf8() {
    let fixture = Fixture::new();
    fixture.locale("en-US", "English", "test-good = Good\n");
    fixture.locale("zh-CN", "简体中文", "test-good = 好\n");
    fs::write(fixture.0.join("locales/zh-CN/bad.ftl"), [0xff, 0xfe]).unwrap();
    assert_eq!(
        LanguageSnapshot::load(&fixture.0, "zh-CN", 1)
            .unwrap_err()
            .kind,
        LanguageErrorKind::Io
    );
    let load = LanguageSnapshot::load_startup(&fixture.0, "zh-CN", 1);
    assert_eq!(msg!("test-good").render(&load.snapshot), "好");
    assert!(
        load.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == RepairKind::InvalidResource)
    );
}

#[test]
fn manifest_repair_and_moved_messages_preserve_valid_customization() {
    let fixture = Fixture::new();
    fixture.snapshot("en-US", 1);
    let (relative, _) = EMBEDDED_FILES
        .iter()
        .find(|(_, text)| !text.trim().is_empty())
        .unwrap();
    let path = fixture.0.join("locales/en-US").join(relative);
    let original = fs::read_to_string(&path).unwrap();
    fs::rename(&path, fixture.0.join("locales/en-US/moved.ftl")).unwrap();
    fixture.write(
        "locales/en-US/manifest.toml",
        "format_version = 7\ncode = 'en-US'\nnative_name = 'Wrong'\n",
    );
    let load = LanguageSnapshot::load(&fixture.0, "en-US", 2).unwrap();
    assert!(load.diagnostics.iter().any(|diagnostic| diagnostic.kind == RepairKind::InvalidManifest && diagnostic.repaired));
    assert_eq!(
        fs::read_to_string(fixture.0.join("locales/en-US/moved.ftl")).unwrap(),
        original
    );
    assert!(fs::read_to_string(path).unwrap().trim().is_empty());
    assert_eq!(
        fs::read_to_string(fixture.0.join("locales/en-US/manifest.toml")).unwrap(),
        EMBEDDED_MANIFEST
    );
}

#[test]
fn nested_messages_stay_localized_and_serialize_original_ids() {
    let fixture = Fixture::new();
    fixture.locale(
        "en-US",
        "English",
        "test-outer = Failed: { $reason }\ntest-inner = File { $name }\n",
    );
    fixture.locale(
        "zh-CN",
        "简体中文",
        "test-outer = 失败：{ $reason }\ntest-inner = 文件 { $name }\n",
    );
    let nested = msg!(
        "test-outer",
        reason = LocalizedText::Message(msg!("test-inner", name = "a.txt"))
    );
    assert_eq!(
        nested.render(&fixture.snapshot("en-US", 1)),
        "Failed: File a.txt"
    );
    assert_eq!(
        nested.render(&fixture.snapshot("zh-CN", 2)),
        "失败：文件 a.txt"
    );
    let raw = msg!(
        "test-outer",
        reason = LocalizedText::Raw("test-inner".to_owned())
    );
    assert_eq!(
        raw.render(&fixture.snapshot("zh-CN", 2)),
        "失败：test-inner"
    );
    let encoded = toml::to_string(&nested).unwrap();
    assert!(encoded.contains("test-inner"));
    assert!(encoded.contains("a.txt"));
    assert_eq!(
        toml::from_str::<LocalizedMessage>(&encoded).unwrap(),
        nested
    );
    assert!(matches!(
        MessageArg::from(msg!("test-inner")),
        MessageArg::Message(_)
    ));
}

#[test]
fn deeply_nested_messages_stop_at_emergency_depth_limit() {
    let fixture = Fixture::new();
    fixture.locale(
        "en-US",
        "English",
        "test-wrapper = { $detail }\ntest-leaf = Leaf\n",
    );
    let snapshot = fixture.snapshot("en-US", 1);
    let mut message = msg!("test-leaf");
    for _ in 0..40 {
        message = msg!("test-wrapper", detail = message);
    }
    assert_eq!(message.render(&snapshot), "Translation unavailable");
}

#[test]
fn startup_fallback_preserves_english_repairs_before_selected_manifest_failure() {
    for missing_manifest in [true, false] {
        let fixture = Fixture::new();
        fixture.write("locales/en-US/manifest.toml", "invalid manifest");
        if !missing_manifest {
            fixture.write(
                "locales/zh-CN/manifest.toml",
                "format_version = 2\ncode = 'zh-CN'\nnative_name = '简体中文'\n",
            );
        }
        let load = LanguageSnapshot::load_startup(&fixture.0, "zh-CN", 23);
        assert_eq!(load.snapshot.code(), "en-US");
        assert_eq!(load.snapshot.generation(), 23);
        let manifest_path = fixture.0.join("locales/en-US/manifest.toml");
        assert!(
            load.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path == manifest_path
                    && diagnostic.kind == RepairKind::InvalidManifest
                    && diagnostic.repaired),
            "manifest repair must survive selected-locale fallback: {:?}",
            load.diagnostics
        );
        for (relative, _) in EMBEDDED_FILES {
            let path = fixture.0.join("locales/en-US").join(relative);
            assert!(path.is_file());
            assert_eq!(
                load.diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.path == path
                        && diagnostic.kind == RepairKind::MissingFile
                        && diagnostic.repaired)
                    .count(),
                1,
                "report every repaired English file exactly once: {}",
                path.display()
            );
        }
        assert!(
            load.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == RepairKind::StartupFallback
                    && diagnostic.path == fixture.0.join("locales/zh-CN/manifest.toml"))
        );
        assert_eq!(
            msg!("resources-recovery-title").render(&load.snapshot),
            "Resource recovery"
        );
        assert!(
            !LanguageSnapshot::load(&fixture.0, "en-US", 24)
                .unwrap()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.repaired)
        );
    }
}

#[test]
fn startup_fallback_reports_repeated_write_failures_once() {
    let fixture = Fixture::new();
    let root = fixture.0.join("blocked-assets");
    fs::write(&root, "keep this file").unwrap();
    let load = LanguageSnapshot::load_startup(&root, "zh-CN", 25);
    assert_eq!(load.snapshot.code(), "en-US");
    assert!(
        load.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == RepairKind::WriteFailed)
    );
    assert!(
        load.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == RepairKind::StartupFallback)
    );
    for (index, diagnostic) in load.diagnostics.iter().enumerate() {
        assert!(
            !load.diagnostics[..index].contains(diagnostic),
            "duplicate startup diagnostic: {diagnostic:?}"
        );
    }
    assert_eq!(fs::read_to_string(root).unwrap(), "keep this file");
}
