#![cfg(any(windows, target_os = "linux"))]

use std::fs;
use std::process::Command;

#[path = "support/update_vim.rs"]
mod vim;

#[test]
fn update_staging_requires_complete_vim_and_uses_the_validated_new_helper() {
    let identity = app::update::current_build_identity();
    let sha = identity.commit_sha.as_deref().unwrap_or("unknown");
    let suffix = std::env::consts::EXE_SUFFIX;
    for with_assets in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "tundra-update-staging-{}-{with_assets}",
            std::process::id()
        ));
        let install = root.join("install with spaces");
        let prepared_dir = root.join("prepared");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&prepared_dir).unwrap();
        #[cfg(target_os = "linux")]
        fs::write(
            install.join("tundra-installation.json"),
            include_str!("../../../packaging/linux/tundra-installation.json"),
        )
        .unwrap();
        let shell_name = format!("tundra-shell{suffix}");
        let cli_name = format!("tundra-cli{suffix}");
        fs::write(install.join(&shell_name), b"old shell").unwrap();
        // This cannot run as a helper; staging must use the new CLI instead.
        fs::write(install.join(&cli_name), b"old CLI").unwrap();
        let assets = install.join("assets/themes");
        if with_assets {
            for theme in ["default", "custom"] {
                fs::create_dir_all(assets.join(theme)).unwrap();
                fs::write(assets.join(theme).join("sentinel"), theme).unwrap();
            }
        }
        let shell_exe = prepared_dir.join(&shell_name);
        let cli_exe = prepared_dir.join(&cli_name);
        fs::write(&shell_exe, b"new shell").unwrap();
        fs::copy(env!("CARGO_BIN_EXE_tundra-cli"), &cli_exe).unwrap();
        let prepared = app::update::PreparedUpdate {
            work_dir: prepared_dir.clone(),
            target_sha: sha.into(),
            shell_exe,
            cli_exe,
        };
        assert!(app::update::stage_update_for_apply(&prepared, &install).is_err());
        vim::write_bundle(&install.join("vim"), b"old Vim");
        vim::write_bundle(&prepared_dir.join("vim"), b"new Vim");
        let staged = app::update::stage_update_for_apply(&prepared, &install).unwrap();
        let transaction = staged.manifest_path.parent().unwrap();
        let mut payload = fs::read_dir(transaction.join("new"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        payload.sort();
        let mut expected = vec![
            std::ffi::OsString::from(cli_name),
            std::ffi::OsString::from(shell_name),
            std::ffi::OsString::from("vim"),
        ];
        expected.sort();
        assert_eq!(payload, expected);
        assert_eq!(
            fs::read(transaction.join("new/vim/runtime/syntax/test.vim")).unwrap(),
            b"new Vim"
        );
        assert_eq!(
            fs::read(install.join("vim/runtime/syntax/test.vim")).unwrap(),
            b"old Vim"
        );
        let output = Command::new(transaction.join(format!("update-helper{suffix}")))
            .arg("__update-probe")
            .output()
            .unwrap();
        assert!(output.status.success());
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(
            output
                .lines()
                .any(|line| line == format!("protocol={}", app::update::UPDATE_PROTOCOL_VERSION))
        );
        assert!(output.lines().any(|line| line == format!("commit={sha}")));
        assert_eq!(
            fs::read(install.join(format!("tundra-cli{suffix}"))).unwrap(),
            b"old CLI"
        );
        if with_assets {
            for theme in ["default", "custom"] {
                assert_eq!(
                    fs::read_to_string(assets.join(theme).join("sentinel")).unwrap(),
                    theme
                );
            }
        } else {
            assert!(!install.join("assets").exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
