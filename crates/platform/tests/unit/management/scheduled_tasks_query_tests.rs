use super::*;

#[test]
fn native_scheduler_failures_keep_short_localized_summaries_and_complete_details() {
    let language = i18n::LanguageSnapshotData {
        code: "zh-CN".into(),
        generation: 1,
        current: vec![
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
    let mut snapshot = ManagementSnapshot::default();
    let detail = "Failed to connect to the service manager.\nOriginal tool diagnostic and recovery information.";
    append_query_problem(
        &mut snapshot,
        "systemd".into(),
        ManagementError::Failed(detail.into()),
    )
    .unwrap();
    assert!(snapshot.notices[0].contains("查询或操作失败"));
    assert!(!snapshot.notices[0].contains("Failed to connect"));
    assert_eq!(snapshot.details, [("systemd".into(), detail.into())]);
    append_query_problem(
        &mut snapshot,
        "crontab".into(),
        ManagementError::MissingDependency("/usr/bin/crontab".into()),
    )
    .unwrap();
    assert!(snapshot.notices[1].contains("缺少所需工具"));
    assert_eq!(snapshot.details[1].1, "/usr/bin/crontab");
    let before = snapshot.clone();
    assert!(matches!(
        append_query_problem(&mut snapshot, "systemd".into(), ManagementError::Cancelled),
        Err(ManagementError::Cancelled)
    ));
    assert_eq!(snapshot, before);
}
