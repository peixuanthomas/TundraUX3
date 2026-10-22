use chrono::{Local, Utc};
use std::sync::Arc;
use system_services::{
    NetworkState, StorageState, SystemMetricsSnapshot, SystemSnapshot, TimeState, WeatherState,
};
use weathr::EmbeddedDisplay;

#[test]
fn embedded_weather_draws_from_snapshots_and_resizes_without_a_terminal() {
    const CHILD: &str = "TUNDRA_TEST_EMBEDDED_WEATHER_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "embedded_weather_draws_from_snapshots_and_resizes_without_a_terminal",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(!output.stdout.contains(&0x1b));
        assert!(!output.stderr.contains(&0x1b));
        return;
    }
    let snapshot = SystemSnapshot {
        revision: 1,
        observed_at: Utc::now(),
        time: TimeState::Local {
            local_time: Local::now().fixed_offset(),
        },
        weather: WeatherState::Loading,
        storage: StorageState::Loading,
        network: NetworkState::Loading,
        metrics: SystemMetricsSnapshot::loading(),
    };
    let (tx, rx) = tokio::sync::watch::channel(snapshot);
    let localize = Arc::new(|id: &str, args: &[(&str, String)]| {
        args.iter()
            .find(|(name, _)| matches!(*name, "prompt" | "content"))
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| id.into())
    });
    let mut display = EmbeddedDisplay::new(rx, localize, (140, 40)).unwrap();
    let buffer = display.render((140, 40)).unwrap();
    let text = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("weathr-resume-prompt"));
    assert!(text.contains("weathr-awaiting-data"));
    assert!(!text.contains("weathr-start-prompt"));
    tx.send_modify(|snapshot| {
        snapshot.weather = WeatherState::Unavailable {
            reason: "offline".into(),
        }
    });
    let buffer = display.render((120, 35)).unwrap();
    assert_eq!(buffer.area.width, 120);
    assert_eq!(buffer.area.height, 35);
    let text = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("weathr-unavailable"));
    assert!(text.contains("weathr-resume-prompt"));
}
