use super::*;

#[test]
fn explorer_file_times_follow_system_timezone() {
    const CHILD: &str = "TUNDRA_TEST_EXPLORER_SYSTEM_TIMEZONE";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session::presentation::view_helpers::tests::explorer_file_times_follow_system_timezone"])
            .env(CHILD, "1")
            .env("TZ", "Asia/Shanghai")
            .status().unwrap();
        assert!(result.success());
        return;
    }
    let value = std::time::UNIX_EPOCH;
    assert_eq!(
        explorer_system_time_label(value, storage::ExplorerDateZone::ConfiguredTimezone, None),
        "1970-01-01 08:00"
    );
    assert_eq!(
        explorer_system_time_label(value, storage::ExplorerDateZone::Utc, None),
        "1970-01-01 00:00"
    );
    assert_eq!(
        explorer_system_time_label(
            value,
            storage::ExplorerDateZone::ConfiguredTimezone,
            Some("America/New_York")
        ),
        "1969-12-31 19:00"
    );
}
