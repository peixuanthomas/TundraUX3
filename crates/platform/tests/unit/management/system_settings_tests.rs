use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn hostname_validation_matches_persistent_host_names() {
    for name in ["tundra", "host-1", "tundra.example"] {
        assert!(valid_hostname(name));
    }
    for name in [
        "",
        "UPPER",
        "-host",
        "host-",
        "host..name",
        "host/name",
        "host\nname",
    ] {
        assert!(!valid_hostname(name));
    }
    assert!(!valid_hostname(&"x".repeat(65)));
}

#[test]
fn a_missing_system_bus_is_an_environment_limit_with_the_original_diagnostic() {
    let directory = tempfile::tempdir().unwrap();
    let address = format!(
        "unix:path={}",
        directory.path().join("missing-bus").display()
    );
    let native = Builder::address(address.as_str())
        .unwrap()
        .build()
        .unwrap_err();
    let diagnostic = native.to_string();
    let error = dbus_error(native);
    assert!(matches!(error, ManagementError::UnsupportedEnvironment(_)));
    assert!(error.to_string().contains(&diagnostic));
    let problem = problem::OperationProblem::from_error(&error);
    assert_eq!(problem.next_action, "environment");
    assert!(!problem.detail.is_empty());
}

#[test]
fn service_permissions_and_other_query_failures_have_distinct_recovery_paths() {
    for (native, expected) in [
        (
            zbus::Error::FDO(Box::new(zbus::fdo::Error::AccessDenied(
                "policy denied reading hostname".into(),
            ))),
            "permission",
        ),
        (
            zbus::Error::FDO(Box::new(zbus::fdo::Error::ServiceUnknown(
                "hostname1 is unavailable".into(),
            ))),
            "unsupported-environment",
        ),
        (
            zbus::Error::Failure("unexpected property data".into()),
            "failed",
        ),
    ] {
        let diagnostic = native.to_string();
        let error = dbus_error(native);
        let problem = problem::OperationProblem::from_error(&error);
        assert_eq!(problem.code, expected);
        assert!(problem.detail.contains(&diagnostic));
    }
}

#[test]
fn native_tool_errors_distinguish_missing_permissions_and_failed_commands() {
    let directory = tempfile::tempdir().unwrap();
    let flag = AtomicBool::new(false);
    let missing = directory.path().join("missing-tool");
    assert!(matches!(
        native(missing.to_str().unwrap(), &[], &flag),
        Err(ManagementError::MissingDependency(_))
    ));
    let denied = directory.path().join("not-executable");
    fs::write(&denied, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        native(denied.to_str().unwrap(), &[], &flag),
        Err(ManagementError::PermissionDenied(_))
    ));
    for (command, expected) in [
        (
            "printf 'partial output\\n'; printf 'Permission denied\\nmore diagnostic\\n' >&2; exit 17",
            "permission",
        ),
        (
            "printf 'partial output\\n'; printf 'native failure\\nmore diagnostic\\n' >&2; exit 17",
            "failed",
        ),
    ] {
        let error = native("/bin/sh", &["-c", command], &flag).unwrap_err();
        let problem = problem::OperationProblem::from_error(&error);
        assert_eq!(problem.code, expected);
        assert!(problem.detail.contains("17"));
        assert!(problem.detail.contains("partial output\n"));
        assert!(problem.detail.contains("\nmore diagnostic\n"));
    }
    assert_eq!(
        native(
            "/bin/sh",
            &["-c", "printf 'Europe/Paris\\nEtc/UTC\\n'"],
            &flag
        )
        .unwrap(),
        "Europe/Paris\nEtc/UTC\n"
    );
}

#[test]
fn partial_setting_failures_keep_short_summaries_and_full_details_without_authorization() {
    let language = i18n::LanguageSnapshotData {
        code: "zh-CN".into(),
        generation: 1,
        current: vec![
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ascii-assets/assets/locales/zh-CN/modules/system-settings.ftl"
            ))
            .into(),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ascii-assets/assets/locales/zh-CN/modules/regression-management.ftl"
            ))
            .into(),
        ],
        english: vec![],
    }
    .restore()
    .unwrap();
    let _language = i18n::enter_snapshot(std::sync::Arc::new(language));
    let mut notices = Vec::new();
    let mut details = Vec::new();
    let diagnostic = "systemd-analyze failed\ncomplete native diagnostic";
    let summary = query_problem(
        ManagementError::Failed(diagnostic.into()),
        "校时服务器",
        &mut notices,
        &mut details,
    )
    .unwrap();
    assert!(summary.starts_with("校时服务器查询受限"));
    assert!(!summary.contains("systemd-analyze"));
    assert_eq!(notices, [summary]);
    assert_eq!(details, [("校时服务器".into(), diagnostic.into())]);
    assert!(matches!(
        query_problem(
            ManagementError::Cancelled,
            "校时服务器",
            &mut notices,
            &mut details
        ),
        Err(ManagementError::Cancelled)
    ));
    assert_eq!(notices.len(), 1);
    assert_eq!(details.len(), 1);
}
