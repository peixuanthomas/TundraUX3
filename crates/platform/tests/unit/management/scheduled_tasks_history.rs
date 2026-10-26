use super::*;
use crate::task_history::{HistoryStore, TaskStatus};
use std::os::unix::fs::PermissionsExt;

#[test]
fn native_recorder_keeps_each_invocation_and_quotes_unicode_labels() {
    let directory = tempfile::tempdir().unwrap();
    let label = "更新 '%s' 中文 🚀";
    let (start, stop) = commands_at(directory.path(), "tundra-scheduled-test", label);
    let store = HistoryStore::new(directory.path());
    for (id, result, code, exit, status) in [
        (
            "0123456789abcdef",
            "success",
            "exited",
            "0",
            TaskStatus::Succeeded,
        ),
        (
            "abcdef0123456789",
            "exit-code",
            "exited",
            "1",
            TaskStatus::Failed,
        ),
        (
            "fedcba9876543210",
            "success",
            "killed",
            "TERM",
            TaskStatus::Cancelled,
        ),
    ] {
        assert!(
            Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("{start}/usr/bin/printf '%s' '中文输出'"))
                .env("INVOCATION_ID", id)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(store.read(id).unwrap().name, label);
        assert!(
            Command::new("/bin/sh")
                .arg("-c")
                .arg(&stop)
                .env("INVOCATION_ID", id)
                .env("SERVICE_RESULT", result)
                .env("EXIT_CODE", code)
                .env("EXIT_STATUS", exit)
                .status()
                .unwrap()
                .success()
        );
        let summary = store.read(id).unwrap();
        assert_eq!(summary.status, status);
        assert!(summary.finished_at.is_some());
        assert_eq!(store.output(id, 0, 4096).unwrap().0, "中文输出".as_bytes());
        assert_eq!(
            fs::metadata(directory.path().join(format!("{id}.json")))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(!directory.path().join(format!("{id}.started")).exists());
    }
}
