//! Exercise public commands without changing any system configuration.
use std::process::{Command, Output};

fn command(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tundra-cli"))
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn formal_help_lists_operations_and_explains_authorization() {
    // Library dispatch keeps this test independent of the executable's root prompt.
    for group in [
        "services",
        "processes",
        "packages",
        "network",
        "disks",
        "users",
        "system-config",
        "operations",
    ] {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        assert_eq!(cli::run([group, "help"], &mut output, &mut errors), 0);
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("Operations:"));
        assert!(text.contains("--non-interactive"));
        assert!(text.contains("--authorization-fd"));
        assert!(errors.is_empty());
    }
}

#[test]
fn secrets_and_missing_versions_are_rejected_before_running() {
    for arguments in [
        &[
            "network",
            "wifi-connect",
            "test",
            "--password",
            "never-store-this",
        ][..],
        &[
            "system-config",
            "apply",
            "/tmp/test",
            "--input",
            "candidate",
        ][..],
        &[
            "system-config",
            "restore",
            "/tmp/test",
            "--backup-id",
            "1-2",
        ][..],
        &["packages", "install", "demo", "--run", "unsafe"][..],
    ] {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        assert_eq!(
            cli::run(arguments.iter().copied(), &mut output, &mut errors),
            2
        );
        assert!(output.is_empty());
        assert!(
            !String::from_utf8(errors)
                .unwrap()
                .contains("never-store-this")
        );
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "tundra-public-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn successful_json(arguments: &[&str]) -> serde_json::Value {
        let output = command(arguments);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    #[test]
    fn configuration_read_and_difference_use_real_files_without_writing() {
        let directory = Directory::new();
        let current = directory.0.join("current.conf");
        let candidate = directory.0.join("candidate.conf");
        fs::write(&current, "Port 22\n").unwrap();
        fs::write(&candidate, "Port 2222\n").unwrap();
        let path = current.to_str().unwrap();
        let read = successful_json(&[
            "system-config",
            "read",
            path,
            "--json",
            "--yes",
            "--non-interactive",
        ]);
        assert_eq!(read["content"], "Port 22\n");
        assert!(
            read["version"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        let difference = successful_json(&[
            "system-config",
            "diff",
            path,
            "--input",
            candidate.to_str().unwrap(),
            "--json",
            "--yes",
            "--non-interactive",
        ]);
        let text = difference["diff"].as_str().unwrap();
        assert!(text.contains("-Port 22"));
        assert!(text.contains("+Port 2222"));
        assert_eq!(fs::read_to_string(&current).unwrap(), "Port 22\n");
        assert_eq!(fs::read_to_string(&candidate).unwrap(), "Port 2222\n");
    }
    #[test]
    fn account_and_process_queries_report_real_root_and_selected_process() {
        let root = successful_json(&[
            "users",
            "lookup",
            "root",
            "--json",
            "--yes",
            "--non-interactive",
        ]);
        assert!(
            root["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == "root")
        );
        let target = std::process::id().to_string();
        let process = successful_json(&[
            "processes",
            "show",
            &target,
            "--json",
            "--yes",
            "--non-interactive",
        ]);
        let row = &process["rows"][0];
        assert_eq!(row["id"], target);
        assert!(
            row["detail"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry[0] == "Disk bytes read")
        );
    }
    #[test]
    fn nonexistent_operation_uses_failure_code_and_json_result() {
        let directory = Directory::new();
        let missing = directory.0.join("missing.sock");
        let output = command(&[
            "operations",
            "status",
            missing.to_str().unwrap(),
            "--json",
            "--yes",
            "--non-interactive",
        ]);
        assert!(!output.status.success());
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            result["exit_code"]
                .as_i64()
                .is_some_and(|code| [1, 3, 5].contains(&code))
                || result["problem"]["exit_code"]
                    .as_i64()
                    .is_some_and(|code| [1, 3, 5].contains(&code))
        );
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
    }
    #[test]
    fn missing_secret_exits_before_an_account_operation_begins() {
        let username = format!("tundra_missing_{}", std::process::id());
        let output = command(&[
            "users",
            "create",
            &username,
            "--json",
            "--yes",
            "--non-interactive",
        ]);
        assert_eq!(output.status.code(), Some(6));
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["status"], "input_required");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("Operation:"));
        assert!(
            platform::management::users::accounts(None)
                .unwrap()
                .iter()
                .all(|account| account.username != username)
        );
    }
    #[test]
    fn native_terminal_hides_password_and_cancel_does_not_create_account() {
        if unsafe { libc::getuid() } == 0 {
            // Root startup is independently tested; this test exercises the normal terminal.
            return;
        }
        use portable_pty::{CommandBuilder, PtySize, native_pty_system};
        use std::{
            io::{Read, Write},
            sync::{Arc, Mutex},
            time::{Duration, Instant},
        };
        let username = format!("tundra_cancel_{}", std::process::id());
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 140,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_tundra-cli"));
        command.args(["users", "create", &username]);
        let child = pair.slave.spawn_command(command).unwrap();
        struct Child(Box<dyn portable_pty::Child + Send + Sync>);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(child);
        drop(pair.slave);
        let output = Arc::new(Mutex::new(Vec::new()));
        let collected = output.clone();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let reading = std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                collected.lock().unwrap().extend_from_slice(&bytes[..count]);
            }
        });
        let mut writer = pair.master.take_writer().unwrap();
        let wait_for = |needle: &str| {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                let text = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
                if text.contains(needle) {
                    break;
                }
                assert!(Instant::now() < deadline, "waiting for {needle}: {text}");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        wait_for("Password:");
        writer.write_all(b"never-echo-this-password\n").unwrap();
        writer.flush().unwrap();
        wait_for("Continue? [y/N]");
        writer.write_all(b"n\n").unwrap();
        writer.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "CLI did not cancel");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(status.exit_code(), 130);
        drop(writer);
        drop(pair.master);
        reading.join().unwrap();
        let text = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
        assert!(!text.contains("never-echo-this-password"));
        assert!(!text.contains("Operation:"));
        assert!(
            platform::management::users::accounts(None)
                .unwrap()
                .iter()
                .all(|account| account.username != username)
        );
    }
}
