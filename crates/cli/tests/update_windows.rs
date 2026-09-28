#![cfg(windows)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MANIFEST_ENV: &str = "TUNDRAUX3_TEST_WINDOWS_UPDATE_MANIFEST";
const FAIL_ENV: &str = "TUNDRAUX3_TEST_WINDOWS_UPDATE_FAIL";
const RECOVERY_ENV: &str = "TUNDRAUX3_TEST_WINDOWS_UPDATE_RECOVERY";

#[test]
fn windows_update_keeps_original_process_alive_through_replacement_and_rollback() {
    if let Some(manifest) = std::env::var_os(MANIFEST_ENV) {
        if std::env::var_os(app::update::UPDATE_READY_FILE_ENV).is_some() {
            if std::env::var_os(FAIL_ENV).is_some() {
                std::process::exit(1);
            }
            app::update::mark_update_ready_from_env().unwrap();
            println!("NEW_SHELL_READY");
        } else if std::env::var_os(app::update::UPDATE_ROLLBACK_ENV).is_some() {
            println!("RESTORED_SHELL_READY");
        } else {
            if std::env::var_os(RECOVERY_ENV).is_some() {
                assert!(
                    app::update::recover_interrupted_update_from_current_exe(std::process::id())
                        .unwrap()
                );
            } else {
                app::update::launch_update_helper(Path::new(&manifest), std::process::id())
                    .unwrap();
            }
            return;
        }
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
        return;
    }

    for (fail_start, recover) in [(false, false), (true, false), (false, true)] {
        let root = std::env::temp_dir().join(format!(
            "tundra-windows-update-{}-{fail_start}-{recover}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let install = root.join("install with spaces");
        let prepared = root.join("prepared");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir(&prepared).unwrap();
        for dir in [&install, &prepared] {
            fs::copy(
                std::env::current_exe().unwrap(),
                dir.join("tundra-shell.exe"),
            )
            .unwrap();
            fs::copy(env!("CARGO_BIN_EXE_tundra-cli"), dir.join("tundra-cli.exe")).unwrap();
        }
        let staged = app::update::stage_update_for_apply(
            &app::update::PreparedUpdate {
                work_dir: prepared.clone(),
                target_sha: app::update::current_build_identity().commit_sha.unwrap(),
                shell_exe: prepared.join("tundra-shell.exe"),
                cli_exe: prepared.join("tundra-cli.exe"),
            },
            &install,
        )
        .unwrap();
        let mut command = Command::new(install.join("tundra-shell.exe"));
        command
            .args(["--nocapture", "--test-threads=1"])
            .env(MANIFEST_ENV, &staged.manifest_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        if fail_start {
            command.env(FAIL_ENV, "1");
        }
        if recover {
            command.env(RECOVERY_ENV, "1");
        }
        // Child Shells are test executables too: expose their readiness output.
        command.env("RUST_TEST_NOCAPTURE", "1");
        let mut original = command.spawn().unwrap();
        let input = original.stdin.take().unwrap();
        let stdout = original.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.contains("SHELL_READY") {
                    let _ = sender.send(line);
                }
            }
        });
        let ready = receiver.recv_timeout(Duration::from_secs(15));
        let expected_state = if fail_start {
            "rolled_back"
        } else {
            "committed"
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while ready.is_ok()
            && !fs::read_to_string(&staged.manifest_path)
                .unwrap()
                .contains(expected_state)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let stayed_alive = original.try_wait().unwrap().is_none();
        // Close inherited stdin before asserting, so the fixture and helper can exit.
        drop(input);
        let deadline = Instant::now() + Duration::from_secs(15);
        while !reader.is_finished() || original.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                // Only stop this fixture's own child tree, including an error dialog.
                let _ = Command::new("taskkill")
                    .args(["/PID", &original.id().to_string(), "/T", "/F"])
                    .output();
                let _ = original.wait();
                panic!("update processes did not exit; readiness: {ready:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        reader.join().unwrap();
        let status = original.wait().unwrap();
        let journal = fs::read_to_string(&staged.manifest_path).unwrap();
        let restored_bytes = fs::read(install.join("tundra-shell.exe")).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert!(ready.unwrap().contains(if fail_start {
            "RESTORED_SHELL_READY"
        } else {
            "NEW_SHELL_READY"
        }));
        assert!(
            stayed_alive,
            "update released PowerShell before Shell exited"
        );
        assert!(status.success());
        assert!(journal.contains(expected_state));
        assert_eq!(
            restored_bytes,
            fs::read(std::env::current_exe().unwrap()).unwrap()
        );
    }
}
