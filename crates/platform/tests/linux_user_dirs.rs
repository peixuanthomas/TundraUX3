#![cfg(target_os = "linux")]

use platform::{Platform, linux::LinuxPlatform};
use std::path::Path;
use std::process::Command;

#[test]
fn current_account_ignores_forged_identity_and_accepts_valid_xdg() {
    // Resolve the real test account through NSS, independently of HOME.
    let account = Command::new("/usr/bin/getent")
        .args(["passwd", &unsafe { libc::getuid() }.to_string()])
        .output()
        .unwrap();
    assert!(account.status.success());
    let account = String::from_utf8(account.stdout).unwrap();
    let fields: Vec<_> = account.trim_end().split(':').collect();
    assert_eq!(fields.len(), 7);
    let expected = LinuxPlatform.user_dirs_for_user(fields[0]).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "account_directories_probe", "--nocapture"])
        .env("TUNDRA_DIR_TEST_USER", fields[0])
        .env("TUNDRA_DIR_TEST_HOME", fields[5])
        .env("TUNDRA_DIR_TEST_DESKTOP", expected.desktop())
        .env("TUNDRA_DIR_TEST_DOCUMENTS", expected.documents())
        .env("HOME", "/nonexistent/tundra-elevated-home")
        .env("USER", "root")
        .env("LOGNAME", "root")
        .env("XDG_CONFIG_HOME", "relative-invalid")
        .env("XDG_DATA_HOME", "/tmp/tundra-valid-xdg-data")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn account_directories_probe() {
    let Ok(username) = std::env::var("TUNDRA_DIR_TEST_USER") else {
        return;
    };
    let home = std::env::var_os("TUNDRA_DIR_TEST_HOME").unwrap();
    let context = platform::linux::identity::LinuxUserContext::current().unwrap();
    assert_eq!(context.username, username);
    assert_eq!(context.home, Path::new(&home));
    assert_eq!(context.process.uid, unsafe { libc::getuid() });
    assert!(
        LinuxPlatform
            .user_dirs_for_user("missing-other-user")
            .is_err()
    );
    let dirs = LinuxPlatform.user_dirs_for_user(&username).unwrap();
    assert_eq!(dirs.app_data(), Path::new("/tmp/tundra-valid-xdg-data"));
    assert_eq!(
        dirs.desktop(),
        Path::new(&std::env::var_os("TUNDRA_DIR_TEST_DESKTOP").unwrap())
    );
    assert_eq!(
        dirs.documents(),
        Path::new(&std::env::var_os("TUNDRA_DIR_TEST_DOCUMENTS").unwrap())
    );
    for path in [
        dirs.desktop(),
        dirs.documents(),
        dirs.downloads(),
        dirs.pictures(),
        dirs.videos(),
        dirs.music(),
    ] {
        if path.is_dir() {
            LinuxPlatform
                .read_directory(path)
                .expect("account folder must be readable");
            println!("READ_OK {}", path.display());
        }
    }
}

#[test]
fn native_child_uses_current_uid_gid_and_nss_environment() {
    use platform::ProcessSpec;
    let current = platform::linux::identity::LinuxUserContext::current().unwrap();
    for (argument, expected) in [
        ("-u", current.process.uid),
        ("-ru", current.process.uid),
        ("-g", current.process.gid),
        ("-rg", current.process.gid),
    ] {
        let output = LinuxPlatform
            .spawn_wait(&ProcessSpec::new("/usr/bin/id").arg(argument))
            .unwrap();
        assert_eq!(output.code, Some(0));
        assert_eq!(
            String::from_utf8_lossy(output.stdout.bytes()).trim(),
            expected.to_string()
        );
    }
    let output = LinuxPlatform
        .spawn_wait(
            &ProcessSpec::new("/usr/bin/env")
                .env("HOME", "/root")
                .env("USER", "root")
                .env("LOGNAME", "root"),
        )
        .unwrap();
    assert_eq!(output.code, Some(0));
    let environment = String::from_utf8(output.stdout.bytes().to_vec()).unwrap();
    for (name, value) in current.environment() {
        let expected = format!("{name}={}", value.to_string_lossy());
        assert!(
            environment.lines().any(|line| line == expected),
            "child environment mismatch for {name}"
        );
    }
}
