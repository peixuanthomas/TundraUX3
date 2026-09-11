use super::*;

#[test]
fn terminal_size_requirement_covers_assets_shell_and_lockscreen() {
    let asset_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    let store =
        ui::AsciiAssetStore::load_with_root(asset_root, "default").expect("canonical ASCII assets");
    let assets = ui::RuntimeAsciiAssets::from_store(store);

    assert_eq!(
        ShellTerminalSizeRequirement::from_assets(&assets),
        ShellTerminalSizeRequirement {
            width: 108,
            height: 20,
        }
    );
}

#[test]
fn terminal_size_validation_accepts_the_boundary_and_larger_sizes() {
    let requirement = ShellTerminalSizeRequirement {
        width: 108,
        height: 20,
    };

    assert!(requirement.validate((108, 20)).is_ok());
    assert!(requirement.validate((160, 48)).is_ok());
}

#[test]
fn terminal_size_requirement_tracks_larger_assets_and_keeps_layout_floors() {
    assert_eq!(
        ShellTerminalSizeRequirement::from_asset_dimensions(ui::AssetDimensions {
            width: 137,
            height: 23,
        }),
        ShellTerminalSizeRequirement {
            width: 137,
            height: 23,
        }
    );
    assert_eq!(
        ShellTerminalSizeRequirement::from_asset_dimensions(ui::AssetDimensions {
            width: 1,
            height: 1,
        }),
        ShellTerminalSizeRequirement {
            width: 70,
            height: 20,
        }
    );
}

#[test]
fn terminal_size_validation_rejects_each_undersized_dimension() {
    let requirement = ShellTerminalSizeRequirement {
        width: 108,
        height: 20,
    };

    for size in [(107, 20), (108, 19), (107, 19)] {
        let error = requirement
            .validate(size)
            .expect_err("undersized terminal must be rejected");
        assert_eq!((error.width, error.height), size);
        assert_eq!(error.required, requirement);
    }
}

#[test]
fn terminal_size_error_is_one_actionable_line() {
    let error = ShellTerminalSizeRequirement {
        width: 108,
        height: 20,
    }
    .validate((80, 18))
    .expect_err("undersized terminal must be rejected")
    .to_string();

    assert_eq!(error.lines().count(), 1);
    assert!(error.contains("80x18"));
    assert!(error.contains("108x20"));
    assert!(error.contains("resize"));
}

#[test]
fn checked_terminal_size_rejects_before_the_caller_can_render() {
    let requirement = ShellTerminalSizeRequirement {
        width: 108,
        height: 20,
    };

    let error = checked_terminal_size_with(requirement, || Ok((107, 20)))
        .expect_err("undersized detected terminal must be rejected");
    assert!(error.to_string().contains("resize"));
    assert_eq!(
        checked_terminal_size_with(requirement, || Ok((108, 20)))
            .expect("boundary terminal should pass"),
        (108, 20)
    );
}

#[test]
fn checked_terminal_size_does_not_replace_detection_failures_with_a_fallback() {
    let requirement = ShellTerminalSizeRequirement {
        width: 108,
        height: 20,
    };
    let error = checked_terminal_size_with(requirement, || {
        Err(io::Error::new(io::ErrorKind::NotConnected, "no terminal"))
    })
    .expect_err("terminal detection failure must stop startup");

    assert_eq!(error.kind(), io::ErrorKind::NotConnected);
    assert!(
        error
            .to_string()
            .contains("could not determine terminal size")
    );
}

#[test]
fn terminal_size_ui_localizes_while_diagnostics_and_raw_causes_stay_stable() {
    use std::{
        fs,
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct LocaleRoot(PathBuf);
    impl Drop for LocaleRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = LocaleRoot(std::env::temp_dir().join(format!(
        "tundra-early-locale-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
    )));
    for (code, contents) in [
        (
            "en-US",
            include_str!("../../../ascii-assets/assets/locales/en-US/recovery/early.ftl"),
        ),
        (
            "zh-CN",
            include_str!("../../../ascii-assets/assets/locales/zh-CN/recovery/early.ftl"),
        ),
    ] {
        let locale = root.0.join("locales").join(code);
        fs::create_dir_all(locale.join("recovery")).unwrap();
        fs::write(
            locale.join("manifest.toml"),
            format!("format_version = 1\ncode = \"{code}\"\nnative_name = \"{code}\"\n"),
        )
        .unwrap();
        let contents = if code == "en-US" {
            contents.replace("terminal is too small", "CUSTOM terminal is too small")
        } else {
            contents.to_string()
        };
        fs::write(locale.join("recovery/early.ftl"), contents).unwrap();
    }
    let requirement = ShellTerminalSizeRequirement {
        width: 108,
        height: 20,
    };
    let error = requirement.validate((80, 18)).unwrap_err();
    let english = i18n::LanguageSnapshot::load(&root.0, "en-US", 1)
        .unwrap()
        .snapshot;
    let _english = i18n::enter_snapshot(Arc::new(english));
    assert!(
        error
            .localized_message()
            .render_current()
            .starts_with("CUSTOM terminal")
    );
    assert_eq!(
        error.to_string(),
        "terminal is too small (80x18); resize it to at least 108x20 and try again"
    );
    {
        let chinese = i18n::LanguageSnapshot::load(&root.0, "zh-CN", 2)
            .unwrap()
            .snapshot;
        let _chinese = i18n::enter_snapshot(Arc::new(chinese));
        assert_eq!(
            error.localized_message().render_current(),
            "终端尺寸过小（80x18）；请将终端调整为至少 108x20 后重试"
        );
        assert_eq!(
            error.to_string(),
            "terminal is too small (80x18); resize it to at least 108x20 and try again"
        );
        let detected = checked_terminal_size_with(requirement, || {
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "raw OS detail /dev/tty",
            ))
        })
        .unwrap_err();
        assert_eq!(detected.kind(), io::ErrorKind::NotConnected);
        assert_eq!(
            detected.to_string(),
            "could not determine terminal size: raw OS detail /dev/tty"
        );
        let structured = detected
            .get_ref()
            .unwrap()
            .downcast_ref::<TerminalSizeDetectionError>()
            .unwrap();
        assert_eq!(
            structured.localized_message().render_current(),
            "无法确定终端尺寸：raw OS detail /dev/tty"
        );
        assert_eq!(structured.source.kind(), io::ErrorKind::NotConnected);
        assert_eq!(
            std::error::Error::source(structured).unwrap().to_string(),
            "raw OS detail /dev/tty"
        );
    }
    assert!(error.to_string().contains("resize"));
}
