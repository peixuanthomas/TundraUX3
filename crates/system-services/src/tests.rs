use super::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

fn mock_platform() -> platform::mock::MockPlatform {
    let root = std::env::temp_dir().join("system-services-platform-test");
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("Data"),
    )
    .unwrap();
    let app_paths = platform::build_linux_app_paths(
        root.join("Config"),
        root.join("Data"),
        root.join("Cache"),
        root.join("State"),
        root.join("Temp"),
    )
    .unwrap();
    platform::mock::MockPlatform::new(user_dirs, app_paths)
}

struct ScriptedSlowMonitor {
    samples: VecDeque<platform::SlowSystemSample>,
}

impl platform::SystemMonitor for ScriptedSlowMonitor {
    fn sample_fast(&mut self) -> Result<platform::FastSystemSample, platform::PlatformError> {
        Err(platform::PlatformError::Unsupported {
            capability: "test.fast_system_sample",
        })
    }

    fn sample_slow(&mut self) -> Result<platform::SlowSystemSample, platform::PlatformError> {
        Ok(self.samples.pop_front().expect("scripted slow sample"))
    }
}

fn slow_sample(
    thermal: Result<Vec<platform::ThermalSensorSample>, &str>,
    batteries: Result<Vec<platform::BatterySample>, &str>,
) -> platform::SlowSystemSample {
    platform::SlowSystemSample {
        identity: Ok(platform::SystemIdentitySample {
            host_name: Some("host".into()),
            os_name: Some("os".into()),
            os_version: Some("1".into()),
            kernel_version: Some("1".into()),
        }),
        thermal: thermal.map_err(str::to_string),
        batteries: batteries.map_err(str::to_string),
        top_cpu: Vec::new(),
        top_memory: Vec::new(),
    }
}

fn metrics_channel() -> (
    watch::Sender<SystemSnapshot>,
    watch::Receiver<SystemSnapshot>,
) {
    let observed_at = Utc::now();
    watch::channel(SystemSnapshot {
        revision: 0,
        observed_at,
        weather: WeatherState::Loading,
        time: TimeState::Local {
            local_time: observed_at.fixed_offset(),
        },
        storage: StorageState::Loading,
        network: NetworkState::Loading,
        metrics: SystemMetricsSnapshot::loading(),
    })
}

#[test]
fn thermal_inner_failures_retain_last_good_and_recover_independently() {
    let first_good = vec![platform::ThermalSensorSample {
        label: "CPU".into(),
        temperature_celsius: 42.0,
        critical_celsius: Some(100.0),
    }];
    let recovered = vec![platform::ThermalSensorSample {
        label: "CPU".into(),
        temperature_celsius: 39.0,
        critical_celsius: Some(100.0),
    }];
    let battery = vec![platform::BatterySample {
        vendor: Some("Vendor".into()),
        model: Some("Model".into()),
        state: platform::BatterySampleState::Charging,
        charge_percent: 50.0,
        energy_wh: 20.0,
        energy_full_wh: 40.0,
        time_to_empty_seconds: None,
        time_to_full_seconds: Some(600),
    }];
    let samples = VecDeque::from([
        slow_sample(Err("thermal unavailable"), Ok(battery.clone())),
        slow_sample(Ok(first_good.clone()), Ok(battery.clone())),
        slow_sample(Err("thermal read failed"), Ok(battery.clone())),
        slow_sample(Err("thermal retry failed"), Ok(battery.clone())),
        slow_sample(Ok(recovered.clone()), Ok(battery)),
    ]);
    let mut monitor: Result<Box<dyn platform::SystemMonitor>, _> =
        Ok(Box::new(ScriptedSlowMonitor { samples }));
    let (sender, receiver) = metrics_channel();

    refresh_slow_metrics(&sender, &mut monitor);
    assert!(
        matches!(receiver.borrow().metrics.thermal, MetricState::Unavailable { ref reason } if reason == "thermal unavailable")
    );
    assert!(matches!(
        receiver.borrow().metrics.batteries,
        MetricState::Ready(_)
    ));
    refresh_slow_metrics(&sender, &mut monitor);
    let expected_first = vec![ThermalSensorSnapshot {
        label: "CPU".into(),
        temperature_celsius: 42.0,
        critical_celsius: Some(100.0),
    }];
    assert_eq!(
        receiver.borrow().metrics.thermal,
        MetricState::Ready(expected_first.clone())
    );
    refresh_slow_metrics(&sender, &mut monitor);
    assert_eq!(
        receiver.borrow().metrics.thermal,
        MetricState::Stale {
            last_good: expected_first.clone(),
            error: "thermal read failed".into()
        }
    );
    assert!(matches!(
        receiver.borrow().metrics.batteries,
        MetricState::Ready(_)
    ));
    refresh_slow_metrics(&sender, &mut monitor);
    assert_eq!(
        receiver.borrow().metrics.thermal,
        MetricState::Stale {
            last_good: expected_first,
            error: "thermal retry failed".into()
        }
    );
    refresh_slow_metrics(&sender, &mut monitor);
    assert!(
        matches!(receiver.borrow().metrics.thermal, MetricState::Ready(ref values) if values[0].temperature_celsius == 39.0)
    );
}

#[test]
fn battery_inner_failures_retain_last_good_and_recover_independently() {
    let thermal = vec![platform::ThermalSensorSample {
        label: "CPU".into(),
        temperature_celsius: 42.0,
        critical_celsius: None,
    }];
    let battery = |charge_percent| platform::BatterySample {
        vendor: Some("Vendor".into()),
        model: Some("Model".into()),
        state: platform::BatterySampleState::Discharging,
        charge_percent,
        energy_wh: 20.0,
        energy_full_wh: 40.0,
        time_to_empty_seconds: Some(900),
        time_to_full_seconds: None,
    };
    let samples = VecDeque::from([
        slow_sample(Ok(thermal.clone()), Err("battery unavailable")),
        slow_sample(Ok(thermal.clone()), Ok(vec![battery(50.0)])),
        slow_sample(Ok(thermal.clone()), Err("battery read failed")),
        slow_sample(Ok(thermal.clone()), Err("battery retry failed")),
        slow_sample(Ok(thermal), Ok(vec![battery(75.0)])),
    ]);
    let mut monitor: Result<Box<dyn platform::SystemMonitor>, _> =
        Ok(Box::new(ScriptedSlowMonitor { samples }));
    let (sender, receiver) = metrics_channel();

    refresh_slow_metrics(&sender, &mut monitor);
    assert!(
        matches!(receiver.borrow().metrics.batteries, MetricState::Unavailable { ref reason } if reason == "battery unavailable")
    );
    assert!(matches!(
        receiver.borrow().metrics.thermal,
        MetricState::Ready(_)
    ));
    refresh_slow_metrics(&sender, &mut monitor);
    let expected_first = match receiver.borrow().metrics.batteries.clone() {
        MetricState::Ready(values) => values,
        state => panic!("expected ready battery state, got {state:?}"),
    };
    refresh_slow_metrics(&sender, &mut monitor);
    assert_eq!(
        receiver.borrow().metrics.batteries,
        MetricState::Stale {
            last_good: expected_first.clone(),
            error: "battery read failed".into()
        }
    );
    assert!(matches!(
        receiver.borrow().metrics.thermal,
        MetricState::Ready(_)
    ));
    refresh_slow_metrics(&sender, &mut monitor);
    assert_eq!(
        receiver.borrow().metrics.batteries,
        MetricState::Stale {
            last_good: expected_first,
            error: "battery retry failed".into()
        }
    );
    refresh_slow_metrics(&sender, &mut monitor);
    assert!(
        matches!(receiver.borrow().metrics.batteries, MetricState::Ready(ref values) if values[0].charge_percent == 75.0)
    );
}

#[test]
fn system_status_failures_are_independent_stale_and_recoverable() {
    let platform = mock_platform();
    platform.set_local_volumes_result(Ok(vec![platform::LocalVolume {
        root: PathBuf::from("/"),
        label: None,
        kind: platform::VolumeKind::Fixed,
        total_bytes: Some(1_000),
        available_bytes: Some(500),
        is_system: true,
        access: platform::VolumeAccess::ReadWrite,
    }]));
    let observed_at = Utc::now();
    let (sender, receiver) = watch::channel(SystemSnapshot {
        revision: 0,
        observed_at,
        weather: WeatherState::Loading,
        time: TimeState::Local {
            local_time: observed_at.fixed_offset(),
        },
        storage: StorageState::Loading,
        network: NetworkState::Loading,
        metrics: SystemMetricsSnapshot::loading(),
    });
    let thresholds = SystemServicesConfig::default().storage_thresholds;
    refresh_system_status(&sender, &platform, thresholds);
    assert!(matches!(receiver.borrow().storage, StorageState::Ready(_)));
    assert!(matches!(receiver.borrow().network, NetworkState::Ready(_)));

    platform.set_local_volumes_result(Err(platform::PlatformError::Unsupported {
        capability: "local_volumes",
    }));
    platform.set_network_status_result(Ok(platform::NetworkStatus::default()));
    refresh_system_status(&sender, &platform, thresholds);
    assert!(matches!(
        receiver.borrow().storage,
        StorageState::Stale { .. }
    ));
    assert!(matches!(receiver.borrow().network, NetworkState::Ready(_)));

    platform.set_local_volumes_result(Ok(Vec::new()));
    platform.set_network_status_result(Err(platform::PlatformError::Unsupported {
        capability: "network_status",
    }));
    refresh_system_status(&sender, &platform, thresholds);
    assert!(matches!(receiver.borrow().storage, StorageState::Ready(_)));
    assert!(matches!(
        receiver.borrow().network,
        NetworkState::Stale { .. }
    ));
    assert!(
        platform
            .calls()
            .iter()
            .filter(|call| matches!(call, platform::mock::MockCall::LocalVolumes))
            .count()
            >= 3
    );
}

#[test]
fn platform_storage_mapping_preserves_rows_pressure_and_fixed_fallback() {
    let thresholds = StorageThresholds {
        low_available_bytes: 500,
        low_percentage: 20,
        critical_available_bytes: 100,
        critical_percentage: 5,
    };
    let mapped = map_storage(
        vec![
            platform::LocalVolume {
                root: PathBuf::from("/fixed"),
                label: Some("Fixed".to_string()),
                kind: platform::VolumeKind::Fixed,
                total_bytes: Some(10_000),
                available_bytes: Some(1_000),
                is_system: false,
                access: platform::VolumeAccess::ReadWrite,
            },
            platform::LocalVolume {
                root: PathBuf::from("/media/removable"),
                label: None,
                kind: platform::VolumeKind::Removable,
                total_bytes: Some(1_000),
                available_bytes: Some(50),
                is_system: false,
                access: platform::VolumeAccess::ReadOnly,
            },
        ],
        thresholds,
    );
    assert_eq!(mapped.system_volume_index, Some(0));
    assert_eq!(
        mapped.system_volume_source,
        SystemVolumeSource::FixedVolumeFallback
    );
    assert!(!mapped.volumes[0].is_system);
    assert_eq!(mapped.overall_pressure, StoragePressure::Critical);
}

#[test]
fn platform_network_mapping_keeps_virtual_rows_but_excludes_them_from_active_count() {
    let mapped = map_network(platform::NetworkStatus::new(vec![
        platform::NetworkInterface {
            name: "eth0".to_string(),
            display_name: None,
            kind: platform::NetworkInterfaceKind::Wired,
            link_state: platform::NetworkLinkState::Up,
            addresses: vec!["192.0.2.1".parse().unwrap()],
        },
        platform::NetworkInterface {
            name: "vm0".to_string(),
            display_name: Some("Virtual".to_string()),
            kind: platform::NetworkInterfaceKind::Virtual,
            link_state: platform::NetworkLinkState::Up,
            addresses: vec!["2001:db8::1".parse().unwrap()],
        },
    ]));
    assert_eq!(mapped.interfaces.len(), 2);
    assert_eq!(mapped.active_link_count, 1);
    assert!(mapped.has_active_link);
    assert_eq!(mapped.interfaces[1].addresses, ["2001:db8::1"]);
}

struct FakeProvider {
    outcomes: Mutex<Vec<Result<WeatherData, String>>>,
    calls: AtomicUsize,
}

struct GatedProvider {
    started: std_mpsc::Sender<WeatherInvocation>,
    calls: Arc<AtomicUsize>,
}

struct WeatherInvocation {
    complete: tokio::sync::oneshot::Sender<()>,
}

#[async_trait]
impl WeatherProvider for GatedProvider {
    async fn current_weather(
        &self,
        _location: WeatherLocation,
        _units: WeatherUnits,
    ) -> Result<WeatherData, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (complete, completion) = tokio::sync::oneshot::channel();
        let _ = self.started.send(WeatherInvocation { complete });
        let _ = completion.await;
        Ok(sample())
    }
}

struct CountingMonitor {
    fast_calls: Arc<AtomicUsize>,
    slow_calls: Arc<AtomicUsize>,
}

struct ControllableHttpServer {
    url: String,
    accepted: std_mpsc::Receiver<usize>,
    respond: std_mpsc::Sender<usize>,
    accepted_count: Arc<AtomicUsize>,
    stop: std_mpsc::Sender<()>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl ControllableHttpServer {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted_tx, accepted) = std_mpsc::channel();
        let (respond, respond_rx) = std_mpsc::channel();
        let (stop, stop_rx) = std_mpsc::channel();
        let accepted_count = Arc::new(AtomicUsize::new(0));
        let thread_accepted_count = accepted_count.clone();
        let join = std::thread::spawn(move || {
            use std::io::Write as _;
            use std::net::Shutdown;
            let mut connections = std::collections::BTreeMap::<usize, std::net::TcpStream>::new();
            let mut next_id = 0usize;
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                while let Ok(id) = respond_rx.try_recv() {
                    if let Some(mut stream) = connections.remove(&id) {
                        if stream
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nDate: Wed, 21 Oct 2015 07:28:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .and_then(|()| stream.flush())
                            .is_ok()
                        {
                            let _ = stream.shutdown(Shutdown::Write);
                        }
                    }
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if read_http_request_header(&mut stream).is_err() {
                            continue;
                        }
                        let id = next_id;
                        next_id += 1;
                        connections.insert(id, stream);
                        thread_accepted_count.fetch_add(1, Ordering::SeqCst);
                        let _ = accepted_tx.send(id);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url: format!("http://{address}/"),
            accepted,
            respond,
            accepted_count,
            stop,
            join: Some(join),
        }
    }

    fn wait_for_request(&self) -> usize {
        self.accepted
            .recv_timeout(Duration::from_secs(5))
            .expect("time request must reach the controllable loopback server")
    }

    fn respond(&self, id: usize) {
        self.respond.send(id).unwrap();
    }

    fn accepted_count(&self) -> usize {
        self.accepted_count.load(Ordering::SeqCst)
    }
}

fn read_http_request_header(stream: &mut std::net::TcpStream) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind, Read as _};

    const MAX_HEADER_BYTES: usize = 32 * 1024;
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut header = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        if header.len() == MAX_HEADER_BYTES {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "HTTP request header exceeds test server limit",
            ));
        }
        let remaining = MAX_HEADER_BYTES - header.len();
        let read_limit = chunk.len().min(remaining);
        let read = stream.read(&mut chunk[..read_limit])?;
        if read == 0 {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "connection closed before HTTP request header completed",
            ));
        }
        header.extend_from_slice(&chunk[..read]);
        if header.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
    }
}

impl Drop for ControllableHttpServer {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl platform::SystemMonitor for CountingMonitor {
    fn sample_fast(&mut self) -> Result<platform::FastSystemSample, platform::PlatformError> {
        self.fast_calls.fetch_add(1, Ordering::SeqCst);
        Ok(platform::FastSystemSample {
            cpu: platform::CpuSample {
                usage_percent: 1.0,
                per_core_percent: vec![1.0],
                logical_core_count: 1,
                physical_core_count: Some(1),
            },
            memory: platform::MemorySample {
                total_bytes: 1,
                used_bytes: 0,
                available_bytes: 1,
                swap_total_bytes: 0,
                swap_used_bytes: 0,
            },
            uptime_seconds: 1,
            load: platform::LoadSample {
                supported: true,
                one: 0.0,
                five: 0.0,
                fifteen: 0.0,
            },
            network_interfaces: Vec::new(),
        })
    }

    fn sample_slow(&mut self) -> Result<platform::SlowSystemSample, platform::PlatformError> {
        self.slow_calls.fetch_add(1, Ordering::SeqCst);
        Ok(platform::SlowSystemSample {
            identity: Ok(platform::SystemIdentitySample {
                host_name: Some("host".into()),
                os_name: Some("os".into()),
                os_version: Some("1".into()),
                kernel_version: Some("1".into()),
            }),
            thermal: Err("no thermal sensors".into()),
            batteries: Err("no batteries".into()),
            top_cpu: Vec::new(),
            top_memory: Vec::new(),
        })
    }
}
#[async_trait]
impl WeatherProvider for FakeProvider {
    async fn current_weather(
        &self,
        _location: WeatherLocation,
        _units: WeatherUnits,
    ) -> Result<WeatherData, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcomes.lock().unwrap().remove(0)
    }
}

struct FakeSystemLocationDetector {
    outcomes: Mutex<Vec<Option<GeoLocation>>>,
    calls: AtomicUsize,
}

#[async_trait]
impl SystemLocationDetector for FakeSystemLocationDetector {
    async fn detect(&self) -> Option<GeoLocation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcomes.lock().unwrap().remove(0)
    }
}

struct PendingSystemLocationDetector;

#[async_trait]
impl SystemLocationDetector for PendingSystemLocationDetector {
    async fn detect(&self) -> Option<GeoLocation> {
        std::future::pending().await
    }
}
fn sample() -> WeatherData {
    WeatherData {
        condition: WeatherCondition::Clear,
        temperature: 20.0,
        precipitation: 0.0,
        wind_speed: 1.0,
        wind_direction: 0.0,
        sun: CelestialEvents::from_bool(true),
        moon_phase: None,
        timestamp: "now".to_string(),
        attribution: String::new(),
    }
}
fn watchdog() -> AppWatchdog {
    let config = watchdog::WatchdogConfig::new(
        std::env::temp_dir().join("ss-reports"),
        std::env::temp_dir().join("ss-fallback"),
        std::env::temp_dir().join("ss-data"),
        "ss-test",
        "1",
    )
    .with_unclean_exit_tracking(false);
    let (_runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
    process
        .register_app(watchdog::AppDescriptor::new(
            watchdog::AppId::from_static("system-services-test"),
            "test",
            "1",
            watchdog::AppCriticality::SessionCritical,
        ))
        .unwrap()
}
fn config() -> SystemServicesConfig {
    SystemServicesConfig {
        time_sync_mode: TimeSyncMode::OperatingSystem,
        weather_refresh_interval: Duration::from_secs(3600),
        cache_dir: Some(tempfile::tempdir().unwrap().keep()),
        ..SystemServicesConfig::default()
    }
}

#[test]
fn network_time_failure_publishes_degraded_state_and_shutdown_completes() {
    let (result_tx, result_rx) = std_mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let mut runtime_config = config();
            runtime_config.time_sync_mode = TimeSyncMode::Network;
            runtime_config.time_server_url = Some("not a valid URL".to_string());
            runtime_config.request_timeout = Duration::from_millis(100);
            runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
            let provider = Arc::new(FakeProvider {
                outcomes: Mutex::new(vec![Err("weather unavailable".to_string())]),
                calls: AtomicUsize::new(0),
            });
            let (handle, mut receiver) =
                SystemServicesRuntime::start_with_provider(runtime_config, watchdog(), provider);
            let wait_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .map_err(|error| error.to_string())?;
            let observed = wait_runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(3), async {
                    loop {
                        let snapshot = receiver.borrow_and_update();
                        if snapshot.revision > 0
                            && matches!(snapshot.time, TimeState::Degraded { .. })
                        {
                            return Ok::<(), watch::error::RecvError>(());
                        }
                        drop(snapshot);
                        receiver.changed().await?;
                    }
                })
                .await
            });
            observed
                .map_err(|_| "degraded time snapshot timed out".to_string())?
                .map_err(|error| error.to_string())?;
            handle.shutdown().map_err(|error| error.to_string())
        })();
        let _ = result_tx.send(result);
    });

    let result = result_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("time failure lifecycle must finish within five seconds");
    assert_eq!(result, Ok(()));
}
fn wait_until(
    receiver: &mut watch::Receiver<SystemSnapshot>,
    predicate: impl Fn(&SystemSnapshot) -> bool,
) {
    for _ in 0..200 {
        if predicate(&receiver.borrow()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("snapshot predicate was not reached")
}

async fn wait_for_snapshot_event(
    receiver: &mut watch::Receiver<SystemSnapshot>,
    predicate: impl Fn(&SystemSnapshot) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if predicate(&receiver.borrow_and_update()) {
                return;
            }
            receiver
                .changed()
                .await
                .expect("system service snapshot sender must remain open");
        }
    })
    .await
    .expect("system service snapshot condition timed out");
}
fn status_call_count(platform: &platform::mock::MockPlatform) -> usize {
    platform
        .calls()
        .iter()
        .filter(|call| matches!(call, platform::mock::MockCall::LocalVolumes))
        .count()
}

fn network_status_call_count(platform: &platform::mock::MockPlatform) -> usize {
    platform
        .calls()
        .iter()
        .filter(|call| matches!(call, platform::mock::MockCall::NetworkStatus))
        .count()
}

fn start_with_zero_status_intervals() -> (
    SystemServicesHandle,
    watch::Receiver<SystemSnapshot>,
    Arc<platform::mock::MockPlatform>,
) {
    let platform = Arc::new(mock_platform());
    let provider = Arc::new(FakeProvider {
        outcomes: Mutex::new(vec![Ok(sample()), Ok(sample())]),
        calls: AtomicUsize::new(0),
    });
    let mut config = config();
    config.timezone_location = Some(config.fallback_location.clone());
    config.system_status_background_refresh_interval = Duration::ZERO;
    config.system_status_active_refresh_interval = Duration::ZERO;
    config.system_status_active_slow_refresh_interval = Duration::ZERO;
    let platform_trait: Arc<dyn platform::Platform> = platform.clone();
    let (handle, receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        config,
        watchdog(),
        platform_trait,
        provider,
    );
    (handle, receiver, platform)
}

#[test]
fn zero_background_interval_is_bounded_and_commands_remain_responsive() {
    let (handle, mut receiver, platform) = start_with_zero_status_intervals();
    wait_until(&mut receiver, |snapshot| {
        matches!(snapshot.storage, StorageState::Ready(_))
    });
    std::thread::sleep(Duration::from_millis(75));
    let storage_calls = status_call_count(&platform);
    let network_calls = network_status_call_count(&platform);
    assert!(
        (1..=16).contains(&storage_calls),
        "storage calls: {storage_calls}"
    );
    assert!(
        (1..=16).contains(&network_calls),
        "network calls: {network_calls}"
    );

    handle.refresh_system_status().unwrap();
    wait_until(&mut receiver, |_| {
        status_call_count(&platform) > storage_calls
    });
    let after_refresh = status_call_count(&platform);
    let mut changed = config();
    changed.timezone_location = Some(changed.fallback_location.clone());
    changed.system_status_background_refresh_interval = Duration::ZERO;
    changed.system_status_active_refresh_interval = Duration::ZERO;
    changed.system_status_active_slow_refresh_interval = Duration::ZERO;
    handle.reconfigure(changed).unwrap();
    wait_until(&mut receiver, |_| {
        status_call_count(&platform) > after_refresh
    });

    let (shutdown_tx, shutdown_rx) = std_mpsc::channel();
    std::thread::spawn(move || {
        let _ = shutdown_tx.send(handle.shutdown());
    });
    assert_eq!(
        shutdown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        Ok(())
    );
}

#[test]
fn zero_active_interval_is_bounded() {
    let (handle, mut receiver, platform) = start_with_zero_status_intervals();
    wait_until(&mut receiver, |snapshot| {
        matches!(snapshot.network, NetworkState::Ready(_))
    });
    handle.set_system_status_active(true).unwrap();
    let storage_baseline = status_call_count(&platform);
    let network_baseline = network_status_call_count(&platform);
    std::thread::sleep(Duration::from_millis(75));
    let storage_calls = status_call_count(&platform) - storage_baseline;
    let network_calls = network_status_call_count(&platform) - network_baseline;
    assert!(
        (1..=16).contains(&storage_calls),
        "storage calls: {storage_calls}"
    );
    assert!(
        (1..=16).contains(&network_calls),
        "network calls: {network_calls}"
    );
    handle.shutdown().unwrap();
}

#[test]
fn injected_platform_refreshes_immediately_manually_and_at_active_interval() {
    let platform = Arc::new(mock_platform());
    let provider = Arc::new(FakeProvider {
        outcomes: Mutex::new(vec![Ok(sample())]),
        calls: AtomicUsize::new(0),
    });
    let mut config = config();
    config.timezone_location = Some(config.fallback_location.clone());
    config.system_status_background_refresh_interval = Duration::from_secs(60);
    config.system_status_active_refresh_interval = Duration::from_millis(30);
    config.system_status_active_slow_refresh_interval = Duration::from_millis(30);
    let platform_trait: Arc<dyn platform::Platform> = platform.clone();
    let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        config,
        watchdog(),
        platform_trait,
        provider,
    );
    wait_until(&mut receiver, |snapshot| {
        matches!(snapshot.storage, StorageState::Ready(_))
            && matches!(snapshot.network, NetworkState::Ready(_))
    });
    let initial_calls = status_call_count(&platform);
    handle.refresh_system_status().unwrap();
    wait_until(&mut receiver, |_| {
        status_call_count(&platform) > initial_calls
    });
    let manual_calls = status_call_count(&platform);
    handle.set_system_status_active(true).unwrap();
    wait_until(&mut receiver, |_| {
        status_call_count(&platform) >= manual_calls + 2
    });
    handle.set_system_status_active(false).unwrap();
    let before_background = status_call_count(&platform);
    wait_until(&mut receiver, |_| {
        status_call_count(&platform) > before_background
    });
    handle.shutdown().unwrap();
}

#[tokio::test]
async fn active_metrics_progress_while_weather_provider_is_pending() {
    let platform = Arc::new(mock_platform());
    let fast_calls = Arc::new(AtomicUsize::new(0));
    let slow_calls = Arc::new(AtomicUsize::new(0));
    platform.set_system_monitor_result(Ok(Box::new(CountingMonitor {
        fast_calls: fast_calls.clone(),
        slow_calls: slow_calls.clone(),
    })));
    let (started_tx, started_rx) = std_mpsc::channel();
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(GatedProvider {
        started: started_tx,
        calls: provider_calls.clone(),
    });
    let mut runtime_config = config();
    runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
    runtime_config.request_timeout = Duration::from_secs(5);
    runtime_config.system_status_background_refresh_interval = Duration::from_secs(60);
    runtime_config.system_status_active_refresh_interval = Duration::from_millis(20);
    runtime_config.system_status_active_slow_refresh_interval = Duration::from_millis(40);
    let platform_trait: Arc<dyn platform::Platform> = platform.clone();
    let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        runtime_config.clone(),
        watchdog(),
        platform_trait,
        provider,
    );

    let first_invocation = started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("weather provider must enter its initial pending operation");
    handle.set_system_status_active(true).unwrap();
    handle.refresh_system_status().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while (fast_calls.load(Ordering::SeqCst) < 3
        || slow_calls.load(Ordering::SeqCst) < 2
        || status_call_count(&platform) < 2)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(fast_calls.load(Ordering::SeqCst) >= 3);
    assert!(slow_calls.load(Ordering::SeqCst) >= 2);
    assert!(status_call_count(&platform) >= 2);
    first_invocation.complete.send(()).unwrap();
    wait_for_snapshot_event(&mut receiver, |snapshot| {
        matches!(snapshot.weather, WeatherState::Ready(_))
    })
    .await;
    let first_sampled_at = match &receiver.borrow().weather {
        WeatherState::Ready(weather) => weather.sampled_at,
        state => panic!("expected ready weather, got {state:?}"),
    };
    assert_eq!(provider_calls.load(Ordering::SeqCst), 1);

    handle.reconfigure(runtime_config).unwrap();
    let restarted = started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("reconfigure must cancel and restart pending weather promptly");
    restarted.complete.send(()).unwrap();
    wait_for_snapshot_event(&mut receiver, |snapshot| {
        matches!(&snapshot.weather, WeatherState::Ready(weather) if weather.sampled_at > first_sampled_at)
    })
    .await;
    assert_eq!(provider_calls.load(Ordering::SeqCst), 2);
    let (shutdown_tx, shutdown_rx) = std_mpsc::channel();
    std::thread::spawn(move || {
        let _ = shutdown_tx.send(handle.shutdown());
    });
    assert_eq!(
        shutdown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        Ok(())
    );
}

#[tokio::test]
async fn active_metrics_preserve_pending_time_sync_request() {
    let server = ControllableHttpServer::start();
    let platform = Arc::new(mock_platform());
    let fast_calls = Arc::new(AtomicUsize::new(0));
    let slow_calls = Arc::new(AtomicUsize::new(0));
    platform.set_system_monitor_result(Ok(Box::new(CountingMonitor {
        fast_calls: fast_calls.clone(),
        slow_calls: slow_calls.clone(),
    })));
    let provider = Arc::new(FakeProvider {
        outcomes: Mutex::new(vec![Ok(sample())]),
        calls: AtomicUsize::new(0),
    });
    let mut runtime_config = config();
    runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
    runtime_config.time_sync_mode = TimeSyncMode::Network;
    runtime_config.time_server_url = Some(server.url.clone());
    runtime_config.request_timeout = Duration::from_secs(5);
    runtime_config.system_status_background_refresh_interval = Duration::from_secs(60);
    runtime_config.system_status_active_refresh_interval = Duration::from_millis(20);
    runtime_config.system_status_active_slow_refresh_interval = Duration::from_millis(40);
    let platform_trait: Arc<dyn platform::Platform> = platform.clone();
    let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        runtime_config,
        watchdog(),
        platform_trait,
        provider,
    );

    let first_request = server.wait_for_request();
    handle.set_system_status_active(true).unwrap();
    handle.refresh_system_status().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while (fast_calls.load(Ordering::SeqCst) < 3
        || slow_calls.load(Ordering::SeqCst) < 2
        || status_call_count(&platform) < 2)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(fast_calls.load(Ordering::SeqCst) >= 3);
    assert!(slow_calls.load(Ordering::SeqCst) >= 2);
    assert!(status_call_count(&platform) >= 2);
    server.respond(first_request);
    wait_for_snapshot_event(&mut receiver, |snapshot| {
        matches!(snapshot.time, TimeState::Synced { .. })
    })
    .await;
    assert_eq!(server.accepted_count(), 1);

    let (shutdown_tx, shutdown_rx) = std_mpsc::channel();
    std::thread::spawn(move || {
        let _ = shutdown_tx.send(handle.shutdown());
    });
    assert_eq!(
        shutdown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        Ok(())
    );
}

#[test]
fn status_commands_preserve_pending_time_validation() {
    let server = ControllableHttpServer::start();
    let platform = Arc::new(mock_platform());
    let fast_calls = Arc::new(AtomicUsize::new(0));
    let slow_calls = Arc::new(AtomicUsize::new(0));
    platform.set_system_monitor_result(Ok(Box::new(CountingMonitor {
        fast_calls: fast_calls.clone(),
        slow_calls,
    })));
    let provider = Arc::new(FakeProvider {
        outcomes: Mutex::new(vec![Ok(sample())]),
        calls: AtomicUsize::new(0),
    });
    let mut runtime_config = config();
    runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
    runtime_config.system_status_background_refresh_interval = Duration::from_secs(60);
    runtime_config.system_status_active_refresh_interval = Duration::from_millis(20);
    runtime_config.system_status_active_slow_refresh_interval = Duration::from_millis(40);
    let platform_trait: Arc<dyn platform::Platform> = platform.clone();
    let (handle, _receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        runtime_config,
        watchdog(),
        platform_trait,
        provider,
    );
    let mut candidate = config();
    candidate.time_sync_mode = TimeSyncMode::Network;
    candidate.time_server_url = Some(server.url.clone());
    candidate.request_timeout = Duration::from_secs(5);
    let validation_handle = handle.clone();
    let (result_tx, result_rx) = std_mpsc::channel();
    std::thread::spawn(move || {
        let _ = result_tx.send(validation_handle.validate_time_source(candidate));
    });

    let first_request = server.wait_for_request();
    handle.set_system_status_active(true).unwrap();
    handle.refresh_system_status().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while (fast_calls.load(Ordering::SeqCst) < 2 || status_call_count(&platform) < 2)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(fast_calls.load(Ordering::SeqCst) >= 2);
    assert!(status_call_count(&platform) >= 2);
    server.respond(first_request);
    let expected = "2015-10-21T07:28:00Z".parse::<DateTime<Utc>>().unwrap();
    assert_eq!(
        result_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Ok(expected)
    );
    assert_eq!(server.accepted_count(), 1);
    handle.shutdown().unwrap();
}

fn network_validation_config(url: &str) -> SystemServicesConfig {
    let mut candidate = config();
    candidate.time_sync_mode = TimeSyncMode::Network;
    candidate.time_server_url = Some(url.to_string());
    candidate.request_timeout = Duration::from_secs(5);
    candidate
}

fn spawn_validation(
    handle: SystemServicesHandle,
    candidate: SystemServicesConfig,
) -> std_mpsc::Receiver<Result<DateTime<Utc>, SystemServicesError>> {
    let (result_tx, result_rx) = std_mpsc::channel();
    std::thread::spawn(move || {
        let _ = result_tx.send(handle.validate_time_source(candidate));
    });
    result_rx
}

#[test]
fn active_validation_replacement_reconfigure_and_shutdown_are_distinct() {
    let server = ControllableHttpServer::start();
    let provider = Arc::new(FakeProvider {
        outcomes: Mutex::new(vec![Ok(sample()), Ok(sample())]),
        calls: AtomicUsize::new(0),
    });
    let platform: Arc<dyn platform::Platform> = Arc::new(mock_platform());
    let mut runtime_config = config();
    runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
    let (handle, _receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        runtime_config.clone(),
        watchdog(),
        platform,
        provider,
    );
    let expected = "2015-10-21T07:28:00Z".parse::<DateTime<Utc>>().unwrap();

    let first = spawn_validation(handle.clone(), network_validation_config(&server.url));
    let _first_request = server.wait_for_request();
    let replacement = spawn_validation(handle.clone(), network_validation_config(&server.url));
    assert_eq!(
        first.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err(SystemServicesError::Cancelled)
    );
    let replacement_request = server.wait_for_request();
    server.respond(replacement_request);
    assert_eq!(
        replacement.recv_timeout(Duration::from_secs(5)).unwrap(),
        Ok(expected)
    );

    let reconfigured = spawn_validation(handle.clone(), network_validation_config(&server.url));
    let _reconfigured_request = server.wait_for_request();
    runtime_config.timezone_id = "Asia/Shanghai".into();
    handle.reconfigure(runtime_config).unwrap();
    assert_eq!(
        reconfigured.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err(SystemServicesError::Cancelled)
    );

    let shutting_down = spawn_validation(handle.clone(), network_validation_config(&server.url));
    let _shutdown_request = server.wait_for_request();
    handle.shutdown().unwrap();
    assert_eq!(
        shutting_down.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err(SystemServicesError::Shutdown)
    );
    assert_eq!(server.accepted_count(), 4);
}

#[tokio::test]
async fn queued_validation_replacement_and_reconfigure_are_cancelled() {
    let server = ControllableHttpServer::start();
    let (started_tx, started_rx) = std_mpsc::channel();
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(GatedProvider {
        started: started_tx,
        calls: provider_calls.clone(),
    });
    let platform: Arc<dyn platform::Platform> = Arc::new(mock_platform());
    let mut runtime_config = config();
    runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
    runtime_config.request_timeout = Duration::from_secs(5);
    let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        runtime_config.clone(),
        watchdog(),
        platform,
        provider,
    );
    let first_weather = started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("weather operation must be pending");
    let (first_queued_tx, first_queued) = std_mpsc::channel();
    handle
        .send(Command::Validate(
            network_validation_config(&server.url),
            first_queued_tx,
        ))
        .unwrap();
    let (newest_tx, newest) = std_mpsc::channel();
    handle
        .send(Command::Validate(
            network_validation_config(&server.url),
            newest_tx,
        ))
        .unwrap();
    assert_eq!(
        first_queued.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err(SystemServicesError::Cancelled)
    );
    first_weather.complete.send(()).unwrap();
    let newest_request = server.wait_for_request();
    server.respond(newest_request);
    assert!(newest.recv_timeout(Duration::from_secs(5)).unwrap().is_ok());
    wait_for_snapshot_event(&mut receiver, |snapshot| {
        matches!(snapshot.weather, WeatherState::Ready(_))
    })
    .await;
    let prior_sampled_at = match &receiver.borrow().weather {
        WeatherState::Ready(weather) => weather.sampled_at,
        state => panic!("expected ready weather, got {state:?}"),
    };

    handle.refresh_weather().unwrap();
    let pending_weather = started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("manual weather operation must be pending");
    let (queued_for_reconfigure_tx, queued_for_reconfigure) = std_mpsc::channel();
    handle
        .send(Command::Validate(
            network_validation_config(&server.url),
            queued_for_reconfigure_tx,
        ))
        .unwrap();
    runtime_config.timezone_id = "Asia/Shanghai".into();
    handle.reconfigure(runtime_config).unwrap();
    assert_eq!(
        queued_for_reconfigure
            .recv_timeout(Duration::from_secs(5))
            .unwrap(),
        Err(SystemServicesError::Cancelled)
    );
    let replacement_weather = started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("reconfigure must restart weather");
    assert!(
        pending_weather.complete.send(()).is_err(),
        "reconfigure must cancel the old weather future"
    );
    replacement_weather.complete.send(()).unwrap();
    wait_for_snapshot_event(&mut receiver, |snapshot| {
        matches!(&snapshot.weather, WeatherState::Ready(weather) if weather.sampled_at > prior_sampled_at)
    })
    .await;
    assert_eq!(provider_calls.load(Ordering::SeqCst), 3);
    handle.shutdown().unwrap();
}
#[tokio::test]
async fn ready_stale_unavailable_and_manual_refresh_are_published() {
    let provider = Arc::new(FakeProvider {
        outcomes: Mutex::new(vec![
            Ok(sample()),
            Err("offline".to_string()),
            Err("offline".to_string()),
        ]),
        calls: AtomicUsize::new(0),
    });
    let platform: Arc<dyn platform::Platform> = Arc::new(mock_platform());
    let mut runtime_config = config();
    runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
    let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        runtime_config,
        watchdog(),
        platform,
        provider,
    );
    wait_for_snapshot_event(&mut receiver, |snapshot| {
        matches!(snapshot.weather, WeatherState::Ready(_))
    })
    .await;
    handle.refresh_weather().unwrap();
    wait_for_snapshot_event(&mut receiver, |snapshot| {
        matches!(snapshot.weather, WeatherState::Stale { .. })
    })
    .await;
    handle.shutdown().unwrap();
    let unavailable = Arc::new(FakeProvider {
        outcomes: Mutex::new(vec![Err("offline".to_string())]),
        calls: AtomicUsize::new(0),
    });
    let platform: Arc<dyn platform::Platform> = Arc::new(mock_platform());
    let mut runtime_config = config();
    runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
    let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        runtime_config,
        watchdog(),
        platform,
        unavailable,
    );
    wait_for_snapshot_event(&mut receiver, |snapshot| {
        matches!(snapshot.weather, WeatherState::Unavailable { .. })
    })
    .await;
    handle.shutdown().unwrap();
}
#[tokio::test]
async fn reconfiguration_and_shutdown_are_controllable() {
    let provider = Arc::new(FakeProvider {
        outcomes: Mutex::new(vec![Ok(sample()), Ok(sample())]),
        calls: AtomicUsize::new(0),
    });
    let platform: Arc<dyn platform::Platform> = Arc::new(mock_platform());
    let mut runtime_config = config();
    runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
    let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
        runtime_config,
        watchdog(),
        platform,
        provider.clone(),
    );
    wait_for_snapshot_event(&mut receiver, |snapshot| {
        matches!(snapshot.weather, WeatherState::Ready(_))
    })
    .await;
    let mut changed = config();
    changed.timezone_id = "Asia/Shanghai".to_string();
    changed.timezone_location = Some(changed.fallback_location.clone());
    handle.reconfigure(changed).unwrap();
    wait_for_snapshot_event(&mut receiver, |_| {
        provider.calls.load(Ordering::SeqCst) >= 2
    })
    .await;
    handle.shutdown().unwrap();
    assert!(matches!(
        handle.refresh_weather(),
        Err(SystemServicesError::Shutdown)
    ));
}

#[test]
fn wmo_codes_match_mature_normalizer() {
    let cases = [
        (0, WeatherCondition::Clear),
        (1, WeatherCondition::PartlyCloudy),
        (2, WeatherCondition::PartlyCloudy),
        (3, WeatherCondition::Overcast),
        (45, WeatherCondition::Fog),
        (48, WeatherCondition::Fog),
        (51, WeatherCondition::Drizzle),
        (53, WeatherCondition::Drizzle),
        (55, WeatherCondition::Drizzle),
        (56, WeatherCondition::FreezingRain),
        (57, WeatherCondition::FreezingRain),
        (61, WeatherCondition::Rain),
        (63, WeatherCondition::Rain),
        (65, WeatherCondition::Rain),
        (66, WeatherCondition::FreezingRain),
        (67, WeatherCondition::FreezingRain),
        (71, WeatherCondition::Snow),
        (73, WeatherCondition::Snow),
        (75, WeatherCondition::Snow),
        (77, WeatherCondition::SnowGrains),
        (80, WeatherCondition::RainShowers),
        (81, WeatherCondition::RainShowers),
        (82, WeatherCondition::RainShowers),
        (85, WeatherCondition::SnowShowers),
        (86, WeatherCondition::SnowShowers),
        (95, WeatherCondition::Thunderstorm),
        (96, WeatherCondition::ThunderstormHail),
        (99, WeatherCondition::ThunderstormHail),
        (-1, WeatherCondition::Clear),
    ];
    for (code, expected) in cases {
        assert_eq!(normalize_open_meteo_code(code), expected, "code {code}");
    }
}

#[test]
fn met_office_codes_use_the_provider_specific_normalizer() {
    let cases = [
        (0, WeatherCondition::Clear),
        (1, WeatherCondition::Clear),
        (2, WeatherCondition::PartlyCloudy),
        (3, WeatherCondition::PartlyCloudy),
        (5, WeatherCondition::Fog),
        (6, WeatherCondition::Fog),
        (7, WeatherCondition::Cloudy),
        (8, WeatherCondition::Overcast),
        (9, WeatherCondition::RainShowers),
        (10, WeatherCondition::RainShowers),
        (11, WeatherCondition::Drizzle),
        (12, WeatherCondition::Rain),
        (13, WeatherCondition::RainShowers),
        (14, WeatherCondition::RainShowers),
        (15, WeatherCondition::Rain),
        (16, WeatherCondition::SnowShowers),
        (17, WeatherCondition::SnowShowers),
        (18, WeatherCondition::SnowGrains),
        (19, WeatherCondition::ThunderstormHail),
        (20, WeatherCondition::ThunderstormHail),
        (21, WeatherCondition::ThunderstormHail),
        (22, WeatherCondition::SnowShowers),
        (23, WeatherCondition::SnowShowers),
        (24, WeatherCondition::Snow),
        (25, WeatherCondition::SnowShowers),
        (26, WeatherCondition::SnowShowers),
        (27, WeatherCondition::Snow),
        (28, WeatherCondition::Thunderstorm),
        (29, WeatherCondition::Thunderstorm),
        (30, WeatherCondition::Thunderstorm),
        (31, WeatherCondition::Thunderstorm),
        (4, WeatherCondition::Clear),
        (-1, WeatherCondition::Drizzle),
    ];
    for (code, expected) in cases {
        assert_eq!(normalize_met_office_code(code), expected, "code {code}");
    }
}

#[tokio::test]
async fn weather_location_prioritizes_text_then_timezone_before_system_detection() {
    let configured = GeoLocation {
        latitude: 1.0,
        longitude: 2.0,
        city: Some("configured".into()),
    };
    let timezone = GeoLocation {
        latitude: 3.0,
        longitude: 4.0,
        city: Some("timezone".into()),
    };
    let detector = FakeSystemLocationDetector {
        outcomes: Mutex::new(vec![]),
        calls: AtomicUsize::new(0),
    };
    let mut text_config = config();
    text_config.weather_location = Some("configured city".into());
    text_config.timezone_location = Some(timezone.clone());
    save_location_cache(&text_config, "configured city", &configured).unwrap();
    let mut system_location = None;
    assert_eq!(
        resolve_location(&text_config, true, &mut system_location, &detector).await,
        configured
    );
    assert_eq!(detector.calls.load(Ordering::SeqCst), 0);

    let mut timezone_config = config();
    timezone_config.timezone_location = Some(timezone.clone());
    assert_eq!(
        resolve_location(&timezone_config, true, &mut system_location, &detector).await,
        timezone
    );
    assert_eq!(detector.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn weather_location_uses_successful_system_detection() {
    let detected = GeoLocation {
        latitude: 51.5072,
        longitude: -0.1276,
        city: Some("London".into()),
    };
    let detector = FakeSystemLocationDetector {
        outcomes: Mutex::new(vec![Some(detected.clone())]),
        calls: AtomicUsize::new(0),
    };
    let mut system_location = None;
    assert_eq!(
        resolve_location(&config(), true, &mut system_location, &detector).await,
        detected
    );
    assert_eq!(detector.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn weather_location_uses_default_after_system_detection_fails() {
    let mut config = config();
    config.fallback_location = GeoLocation {
        latitude: 31.2304,
        longitude: 121.4737,
        city: Some("Shanghai".into()),
    };
    let detector = FakeSystemLocationDetector {
        outcomes: Mutex::new(vec![None]),
        calls: AtomicUsize::new(0),
    };
    let mut system_location = None;
    assert_eq!(
        resolve_location(&config, true, &mut system_location, &detector).await,
        config.fallback_location
    );
    assert_eq!(detector.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn system_location_detection_obeys_the_request_timeout() {
    let mut config = config();
    config.request_timeout = Duration::from_millis(10);
    let mut system_location = None;
    let started = Instant::now();
    let resolved = resolve_location(
        &config,
        true,
        &mut system_location,
        &PendingSystemLocationDetector,
    )
    .await;
    assert_eq!(resolved, config.fallback_location);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn monotonic_anchor_advances_and_error_remains_degraded() {
    let instant = Instant::now();
    let utc = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let anchor = TimeAnchor {
        utc,
        sampled_at: utc,
        instant,
        source: TimeSource::Network("test".into()),
    };
    assert_eq!(
        anchor.utc_at(instant + Duration::from_secs(7)),
        utc + Duration::from_secs(7)
    );
    let state = current_time_state(
        &config(),
        Some(&anchor),
        Some("offline"),
        instant + Duration::from_secs(7),
    );
    assert!(
        matches!(state, TimeState::Degraded { last_sync: Some(value), ref error, .. } if value == utc && error == "offline")
    );
    let next = current_time_state(
        &config(),
        Some(&anchor),
        Some("offline"),
        instant + Duration::from_secs(8),
    );
    assert!(matches!(next, TimeState::Degraded { ref error, .. } if error == "offline"));
}

struct PendingProvider {
    entered: Mutex<Option<std_mpsc::Sender<()>>>,
}
#[async_trait]
impl WeatherProvider for PendingProvider {
    async fn current_weather(
        &self,
        _: WeatherLocation,
        _: WeatherUnits,
    ) -> Result<WeatherData, String> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        std::future::pending().await
    }
}

#[test]
fn shutdown_and_reconfigure_cancel_pending_provider_wait() {
    let mut initial = config();
    initial.timezone_location = Some(initial.fallback_location.clone());
    let (entered_tx, entered_rx) = std_mpsc::channel();
    let (handle, _) = SystemServicesRuntime::start_with_provider(
        initial.clone(),
        watchdog(),
        Arc::new(PendingProvider {
            entered: Mutex::new(Some(entered_tx)),
        }),
    );
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("pending provider must be entered before reconfigure");
    let mut changed = initial;
    changed.timezone_id = "Asia/Shanghai".into();
    handle.reconfigure(changed).unwrap();
    let started = Instant::now();
    handle.shutdown().unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
}
