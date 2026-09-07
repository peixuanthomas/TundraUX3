#![cfg(target_os = "linux")]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const HELPER_TEST_MANIFEST: &str = "TUNDRAUX3_TEST_HELPER_MANIFEST";

fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn linux_update_helper_replaces_or_restores_and_waits_for_shell() {
    if let Some(manifest) = std::env::var_os(HELPER_TEST_MANIFEST) {
        app::update::launch_update_helper(Path::new(&manifest), std::process::id()).unwrap();
        panic!("Linux must exec the helper in the foreground job");
    }
    for fail_start in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "tundra-linux-helper-{}-{fail_start}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let install = root.join("install with spaces");
        let prepared_dir = root.join("prepared");
        fs::create_dir_all(install.join("assets/themes/default")).unwrap();
        fs::create_dir_all(install.join("assets/themes/custom")).unwrap();
        fs::create_dir_all(prepared_dir.join("default")).unwrap();
        fs::write(install.join("assets/themes/default/version"), "old").unwrap();
        fs::write(install.join("assets/themes/custom/version"), "custom").unwrap();
        fs::write(prepared_dir.join("default/version"), "new").unwrap();
        fs::copy(env!("CARGO_BIN_EXE_tundra-cli"), install.join("tundra-cli")).unwrap();
        executable(
            &install.join("tundra-shell"),
            "#!/bin/sh\nprintf 'restored\\n'\nread answer\n",
        );
        executable(
            &prepared_dir.join("tundra-cli"),
            "#!/bin/sh\nprintf 'protocol=1\\ncommit=abc\\n'\n",
        );
        executable(
            &prepared_dir.join("tundra-shell"),
            if fail_start {
                "#!/bin/sh\nexit 1\n"
            } else {
                "#!/bin/sh\nprintf 'ready\\n' > \"$TUNDRAUX3_UPDATE_READY_FILE\"\nprintf 'new\\n'\nread answer\n"
            },
        );
        let staged = app::update::stage_update_for_apply(
            &app::update::PreparedUpdate {
                work_dir: prepared_dir.clone(),
                target_sha: "abc".into(),
                shell_exe: prepared_dir.join("tundra-shell"),
                cli_exe: prepared_dir.join("tundra-cli"),
                default_assets: prepared_dir.join("default"),
            },
            &install,
        )
        .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_update_helper_replaces_or_restores_and_waits_for_shell",
                "--nocapture",
            ])
            .env(HELPER_TEST_MANIFEST, &staged.manifest_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line == "new" || line == "restored" {
                    let _ = tx.send(line);
                }
            }
        });
        let ready = rx.recv_timeout(Duration::from_secs(15));
        if ready.is_err() {
            let _ = child.kill();
        }
        assert_eq!(ready.unwrap(), if fail_start { "restored" } else { "new" });
        let expected = if fail_start {
            "rolled_back"
        } else {
            "committed"
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if fs::read_to_string(&staged.manifest_path)
                .unwrap()
                .contains(expected)
            {
                break;
            }
            assert!(Instant::now() < deadline, "update journal did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "helper must keep the foreground job alive"
        );
        assert_eq!(
            fs::read_to_string(install.join("assets/themes/default/version")).unwrap(),
            if fail_start { "old" } else { "new" }
        );
        assert_eq!(
            fs::read_to_string(install.join("assets/themes/custom/version")).unwrap(),
            "custom"
        );
        assert_ne!(
            fs::metadata(install.join("tundra-shell"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        child.stdin.take().unwrap().write_all(b"done\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                panic!("helper did not exit with Shell");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        reader.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
