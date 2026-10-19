use super::*;

#[test]
fn an_installed_systemctl_does_not_imply_a_running_system_manager() {
    assert!(check_system_manager("systemd\n").is_ok());
    let error = check_system_manager("init\n").unwrap_err();
    assert!(matches!(error, ManagementError::UnsupportedEnvironment(_)));
    let problem = super::super::problem::OperationProblem::from_error(&error);
    assert_eq!(problem.code, "unsupported-environment");
    assert_eq!(problem.next_action, "environment");
    assert!(problem.detail.contains("init"));
}

#[test]
fn missing_dependencies_permissions_and_query_failures_have_distinct_recovery_actions() {
    for (error, code, next_action) in [
        (
            ManagementError::MissingDependency("ip".into()),
            "missing-dependency",
            "requirements",
        ),
        (
            ManagementError::PermissionDenied("protected settings".into()),
            "permission",
            "authorize",
        ),
        (
            ManagementError::Failed("lsblk failed".into()),
            "failed",
            "details",
        ),
    ] {
        let problem = super::super::problem::OperationProblem::from_error(&error);
        assert_eq!(problem.code, code);
        assert_eq!(problem.next_action, next_action);
        assert!(!problem.detail.is_empty());
    }
}
