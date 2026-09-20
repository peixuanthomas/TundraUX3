use super::restart_current_executable;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn restart_uses_pre_update_executable_path() {
    const CHILD: &str = "TUNDRA_RESTART_REPLACEMENT_TEST";
    if std::env::var_os(CHILD).is_some() {
        let original = std::env::current_exe().unwrap();
        let replacement = original.with_extension("replacement");
        fs::copy("/bin/true", &replacement).unwrap();
        fs::rename(replacement, &original).unwrap();
        assert_ne!(std::env::current_exe().unwrap(), original);
        // A successful exec exits through /bin/true, replacing this test runner.
        restart_current_executable(Ok(original)).unwrap();
        unreachable!("exec returned without an error");
    }

    let root = std::env::temp_dir().join(format!(
        "tundra-restart-replacement-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    fs::create_dir(&root).unwrap();
    let executable = root.join("shell-entry-tests");
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let output = Command::new(executable)
        .args([
            "--exact",
            "tests::restart_uses_pre_update_executable_path",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
