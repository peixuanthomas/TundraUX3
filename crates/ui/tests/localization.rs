use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use i18n::{LanguageSnapshot, LocalizedMessage, with_snapshot};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::components::{UpdateActivity, UpdateActivityViewModel};
use ui::{
    HomeViewModel, LogsViewModel, RenderContext, ShellEntry, TundraTheme, home_logout_area,
    logs_hit_test, logs_layout,
};
use unicode_width::UnicodeWidthStr;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct LocaleFixture(PathBuf);
impl LocaleFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tux-ui-locales-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets/locales");
        for locale in ["en-US", "zh-CN"] {
            let target = root.join("locales").join(locale);
            fs::create_dir_all(&target).unwrap();
            fs::write(
                target.join("manifest.toml"),
                format!("format_version = 1\ncode = \"{locale}\"\nnative_name = \"{locale}\"\n"),
            )
            .unwrap();
            for group in ["common", "modules"] {
                fs::create_dir_all(target.join(group)).unwrap();
                for entry in fs::read_dir(source.join(locale).join(group)).unwrap() {
                    let entry = entry.unwrap();
                    if entry.file_name().to_string_lossy().starts_with("ui-") {
                        fs::copy(entry.path(), target.join(group).join(entry.file_name())).unwrap();
                    }
                }
            }
        }
        Self(root)
    }
    fn snapshot(&self, locale: &str) -> Arc<LanguageSnapshot> {
        Arc::new(LanguageSnapshot::load(&self.0, locale, 1).unwrap().snapshot)
    }
}
impl Drop for LocaleFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn resource_messages(root: &Path, locale: &str) -> BTreeMap<String, String> {
    let mut messages = BTreeMap::new();
    for group in ["common", "modules"] {
        for entry in fs::read_dir(root.join("locales").join(locale).join(group)).unwrap() {
            let entry = entry.unwrap();
            if !entry.file_name().to_string_lossy().starts_with("ui-") {
                continue;
            }
            let content = fs::read_to_string(entry.path()).unwrap();
            let mut current: Option<String> = None;
            for line in content.lines() {
                if line.starts_with("ui-") {
                    let (id, pattern) = line.split_once(" = ").unwrap();
                    assert!(
                        messages.insert(id.to_owned(), pattern.to_owned()).is_none(),
                        "duplicate {id}"
                    );
                    current = Some(id.to_owned());
                } else if line.starts_with(' ')
                    && let Some(id) = &current
                {
                    messages.get_mut(id).unwrap().push_str(line);
                }
            }
        }
    }
    messages
}
fn parameters(pattern: &str) -> BTreeSet<String> {
    pattern
        .split('$')
        .skip(1)
        .map(|tail| {
            tail.chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
                .collect()
        })
        .collect()
}

#[test]
fn every_ui_message_has_matching_chinese_parameters_and_resolves() {
    let fixture = LocaleFixture::new();
    let english = resource_messages(&fixture.0, "en-US");
    let chinese = resource_messages(&fixture.0, "zh-CN");
    assert_eq!(
        english.keys().collect::<Vec<_>>(),
        chinese.keys().collect::<Vec<_>>()
    );
    assert!(english.len() > 500, "all UI modules must be covered");
    let snapshots = [fixture.snapshot("en-US"), fixture.snapshot("zh-CN")];
    for (id, pattern) in &english {
        let args = parameters(pattern);
        assert_eq!(args, parameters(&chinese[id]), "parameter mismatch: {id}");
        for count in [0_i64, 1, 2, 27] {
            let mut message = LocalizedMessage::new(id);
            for name in &args {
                message = message.with_arg(name, count);
            }
            for snapshot in &snapshots {
                let rendered = snapshot.render(&message);
                assert_ne!(
                    rendered,
                    format!("[{id}]"),
                    "unresolved {id} in {}",
                    snapshot.code()
                );
            }
        }
    }
}

#[test]
fn retained_update_labels_follow_the_render_snapshot_and_preserve_raw_output() {
    let fixture = LocaleFixture::new();
    let english = fixture.snapshot("en-US");
    let chinese = fixture.snapshot("zh-CN");
    let mut model = with_snapshot(&english, UpdateActivityViewModel::default);
    model
        .output
        .push("Compiling external-crate /tmp/build.rs".into());
    let context = RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    let render = |snapshot: &Arc<LanguageSnapshot>| {
        with_snapshot(snapshot, || {
            let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
            terminal
                .draw(|frame| {
                    UpdateActivity::new(&model).render_scrolled(frame, frame.area(), 0, &context)
                })
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        })
    };
    let english_text = render(&english);
    let chinese_text = render(&chinese);
    assert!(english_text.contains("Download: waiting"));
    assert!(chinese_text.replace(' ', "").contains("下载：等待中"));
    assert!(chinese_text.replace(' ', "").contains("编译：等待中"));
    assert!(chinese_text.contains("Compiling external-crate /tmp/build.rs"));
    assert_eq!(
        render(&english),
        english_text,
        "switching back restores the same display"
    );
}

#[test]
fn localized_home_logout_and_log_controls_use_terminal_cell_widths() {
    let fixture = LocaleFixture::new();
    for locale in ["en-US", "zh-CN"] {
        let snapshot = fixture.snapshot(locale);
        with_snapshot(&snapshot, || {
            let main = Rect::new(0, 0, 120, 32);
            let home = HomeViewModel::user("张三🙂", "12:00", Vec::new())
                .with_account_logout("张三🙂", false);
            let area = home_logout_area(main, &home);
            assert_eq!(
                usize::from(area.width),
                i18n::tr!("ui-home-logout-button").width()
            );
            let mut model = LogsViewModel {
                can_view_system: true,
                linux_available: true,
                ..Default::default()
            };
            model.diagnostics.can_view_details = true;
            let layout = logs_layout(main, &model);
            let keys = [
                "ui-logs-r-refresh",
                "ui-logs-o-open",
                "ui-logs-l-level",
                "ui-logs-m-module",
                "ui-logs-t-time",
            ];
            for (control, key) in layout.controls.iter().zip(keys) {
                assert_eq!(usize::from(control.area.width), i18n::tr!(key).width() + 2);
                if control.target != ui::LogsHitTarget::Open {
                    assert_eq!(
                        logs_hit_test(main, &model, (control.area.x, control.area.y)),
                        Some(control.target)
                    );
                    assert_eq!(
                        logs_hit_test(main, &model, (control.area.right() - 1, control.area.y)),
                        Some(control.target)
                    );
                }
            }
        });
    }
}

#[test]
fn home_icon_identity_is_independent_of_the_display_language() {
    let english = ShellEntry::new("Explorer", "Files").with_icon_key("explorer");
    let chinese = ShellEntry::new("文件管理器", "文件").with_icon_key("explorer");
    assert_eq!(english.icon_identity(), chinese.icon_identity());
    let legacy = ShellEntry::new("Explorer", "Files");
    assert_eq!(legacy.icon_identity(), "Explorer");
}

#[test]
fn numeric_counts_use_fluent_plural_rules() {
    let fixture = LocaleFixture::new();
    with_snapshot(&fixture.snapshot("en-US"), || {
        assert_eq!(
            i18n::tr!("ui-launcher-item-count", count = 1),
            "1 item · Enter launch · Esc Home"
        );
        assert_eq!(
            i18n::tr!("ui-launcher-item-count", count = 2),
            "2 items · Enter launch · Esc Home"
        );
        assert_eq!(i18n::tr!("ui-editor-cells", count = 1), "1 cell");
    });
    with_snapshot(&fixture.snapshot("zh-CN"), || {
        assert_eq!(
            i18n::tr!("ui-launcher-item-count", count = 2),
            "2 项 · Enter 启动 · Esc 主页"
        );
        assert_eq!(i18n::tr!("ui-editor-cells", count = 2), "2 格");
    });
}
