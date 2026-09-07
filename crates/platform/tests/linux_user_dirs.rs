#![cfg(target_os = "linux")]

use platform::{Platform, linux::LinuxPlatform};
use std::path::Path;
use std::process::Command;

#[test]
fn account_directories_ignore_elevated_process_environment() {
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
        .env("XDG_CONFIG_HOME", "/nonexistent/tundra-elevated-config")
        .env("XDG_DATA_HOME", "/nonexistent/tundra-elevated-data")
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
    let dirs = LinuxPlatform.user_dirs_for_user(&username).unwrap();
    assert_eq!(dirs.app_data(), Path::new(&home).join(".local/share"));
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
