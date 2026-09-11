#![cfg(target_os = "linux")]

use platform::{Platform, linux::LinuxPlatform};
use std::path::Path;
use std::process::Command;

#[test]
fn account_directories_use_nss_home_and_current_user_xdg_context() {
    // Resolve the real test account through NSS, independently of HOME.
    let account = Command::new("/usr/bin/getent")
        .args(["passwd", &unsafe { libc::getuid() }.to_string()])
        .output()
        .unwrap();
    assert!(account.status.success());
    let account = String::from_utf8(account.stdout).unwrap();
    let fields: Vec<_> = account.trim_end().split(':').collect();
    assert_eq!(fields.len(), 7);
    if unsafe { libc::getuid() } == 0 {
        assert!(LinuxPlatform.user_dirs_for_user(fields[0]).is_err());
        return;
    }
    let home = Path::new(fields[5]);
    let fixture = std::env::temp_dir().join(format!(
        "tundra-user-dirs-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir(&fixture).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(fixture.clone());
    let config = fixture.join("config");
    let data = fixture.join("data");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(
        config.join("user-dirs.dirs"),
        "XDG_DESKTOP_DIR=\"$HOME/TundraDesktop\"\nXDG_DOCUMENTS_DIR=\"$HOME/TundraDocuments\"\n",
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "account_directories_probe", "--nocapture"])
        .env("TUNDRA_DIR_TEST_USER", fields[0])
        .env("TUNDRA_DIR_TEST_HOME", fields[5])
        .env("TUNDRA_DIR_TEST_DESKTOP", home.join("TundraDesktop"))
        .env("TUNDRA_DIR_TEST_DOCUMENTS", home.join("TundraDocuments"))
        .env("HOME", "/nonexistent/tundra-elevated-home")
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", &data)
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
    let dirs = LinuxPlatform.user_dirs_for_user(&username).unwrap();
    // Explicit XDG roots belong to this user. Unlike the former root-session
    // adapter, personal directories and application paths share that context.
    assert_eq!(
        dirs.app_data(),
        Path::new(&std::env::var_os("XDG_DATA_HOME").unwrap())
    );
    assert!(LinuxPlatform.user_dirs_for_user("root").is_err());
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
