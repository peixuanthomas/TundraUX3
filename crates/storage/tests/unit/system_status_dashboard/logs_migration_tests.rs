use super::*;
#[test]
fn legacy_logs_and_incidents_are_removed_without_repositioning_other_widgets() {
    let mut config: SystemStatusDashboardConfig = serde_json::from_str(r#"{"widgets":["cpu","activity","incidents"],"wide":{"placements":[{"kind":"cpu","column":4,"row":6,"size":"small"},{"kind":"logs","column":0,"row":0,"size":"small"}]},"narrow":{"placements":[]}}"#).unwrap();
    config.normalize();
    assert_eq!(config.widgets, vec![SystemStatusWidgetKind::Cpu]);
    assert_eq!(config.wide.placements.len(), 1);
    assert_eq!(
        (
            config.wide.placements[0].column,
            config.wide.placements[0].row
        ),
        (4, 6)
    );
    assert_eq!(config.narrow.placements.len(), 1);
    let encoded = serde_json::to_string(&config).unwrap();
    assert!(!encoded.contains("logs"));
    assert!(!encoded.contains("incidents"));
}
