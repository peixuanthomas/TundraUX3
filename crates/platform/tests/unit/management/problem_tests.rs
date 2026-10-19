use super::*;

#[test]
fn diagnostics_preserve_lines_while_removing_controls_and_sensitive_values() {
    let error = ManagementError::Failed(
        "Native query failed\r\n\nPassword: hidden-value\nNext step\u{7}\n".into(),
    );
    let problem = OperationProblem::from_error(&error);
    assert_eq!(
        problem.detail,
        "Native query failed\n\n[REDACTED]\nNext step\n"
    );
    assert!(!problem.detail.contains("hidden-value"));
    assert!(
        problem
            .detail
            .chars()
            .all(|value| !value.is_control() || value == '\n')
    );
}
