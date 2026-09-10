//! Platform sampling and preservation of the last successful samples.

use super::{SystemServicesConfig, snapshot, telemetry};
use crate::model::*;
use chrono::Utc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

const MIN_SYSTEM_STATUS_REFRESH_INTERVAL: Duration = Duration::from_millis(10);

#[allow(clippy::too_many_arguments)]
pub(super) fn refresh_due_system_sources(
    now: Instant,
    config: &SystemServicesConfig,
    active: bool,
    sender: &watch::Sender<SystemSnapshot>,
    platform: &dyn platform::Platform,
    monitor: &mut Result<Box<dyn platform::SystemMonitor>, platform::PlatformError>,
    system_status_due: &mut Instant,
    system_fast_due: &mut Instant,
    system_slow_due: &mut Instant,
) {
    if now >= *system_status_due {
        refresh_system_status(sender, platform, config.storage_thresholds);
        *system_status_due = now + system_status_refresh_interval(config, active);
    }
    if now >= *system_fast_due {
        if monitor.is_err() && !matches!(monitor, Err(platform::PlatformError::Unsupported { .. }))
        {
            *monitor = platform.create_system_monitor();
            match monitor {
                Ok(_) => telemetry::recovered("system_status", "monitor_create"),
                Err(error) => {
                    telemetry::typed_failure("system_status", "monitor_create", error, false)
                }
            }
        }
        refresh_fast_metrics(sender, monitor);
        *system_fast_due = now
            + if active {
                config
                    .system_status_active_refresh_interval
                    .max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
            } else {
                config
                    .system_status_background_refresh_interval
                    .max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
            };
    }
    if now >= *system_slow_due {
        refresh_slow_metrics(sender, monitor);
        *system_slow_due = now
            + if active {
                config
                    .system_status_active_slow_refresh_interval
                    .max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
            } else {
                config
                    .system_status_background_refresh_interval
                    .max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
            };
    }
}

fn system_status_refresh_interval(config: &SystemServicesConfig, active: bool) -> Duration {
    let configured = if active {
        config.system_status_active_slow_refresh_interval
    } else {
        config.system_status_background_refresh_interval
    };
    configured.max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
}

pub(super) fn refresh_system_status(
    sender: &watch::Sender<SystemSnapshot>,
    platform: &dyn platform::Platform,
    thresholds: StorageThresholds,
) {
    let previous = sender.borrow().clone();
    let storage_result = platform.local_volumes();
    match &storage_result {
        Ok(_) => telemetry::recovered("system_status", "storage_sample"),
        Err(error) => telemetry::typed_failure(
            "system_status",
            "storage_sample",
            error,
            matches!(
                previous.storage,
                StorageState::Ready(_) | StorageState::Stale { .. }
            ),
        ),
    }
    let storage = match storage_result {
        Ok(volumes) => StorageState::Ready(map_storage(volumes, thresholds)),
        Err(error) => match previous.storage {
            StorageState::Ready(last_good) | StorageState::Stale { last_good, .. } => {
                StorageState::Stale {
                    last_good,
                    error: error.to_string(),
                }
            }
            StorageState::Loading | StorageState::Unavailable { .. } => StorageState::Unavailable {
                reason: error.to_string(),
            },
        },
    };
    let network_result = platform.network_status();
    match &network_result {
        Ok(_) => telemetry::recovered("system_status", "network_sample"),
        Err(error) => telemetry::typed_failure(
            "system_status",
            "network_sample",
            error,
            matches!(
                previous.network,
                NetworkState::Ready(_) | NetworkState::Stale { .. }
            ),
        ),
    }
    let network = match network_result {
        Ok(status) => NetworkState::Ready(map_network(status)),
        Err(error) => match previous.network {
            NetworkState::Ready(last_good) | NetworkState::Stale { last_good, .. } => {
                NetworkState::Stale {
                    last_good,
                    error: error.to_string(),
                }
            }
            NetworkState::Loading | NetworkState::Unavailable { .. } => NetworkState::Unavailable {
                reason: error.to_string(),
            },
        },
    };
    let _ = sender.send(snapshot(
        previous.revision.saturating_add(1),
        previous.weather,
        previous.time,
        storage,
        network,
        previous.metrics,
    ));
}

fn metric_result<T, U>(operation: &str, result: &Result<T, String>, previous: &MetricState<U>) {
    match result {
        Ok(_) => telemetry::recovered("system_status", operation),
        Err(reason) if !reason.contains("unsupported") && !reason.contains("not supported") => {
            telemetry::failure(
                "system_status",
                operation,
                reason,
                matches!(previous, MetricState::Ready(_) | MetricState::Stale { .. }),
            )
        }
        Err(_) => {}
    }
}

fn unavailable_or_stale<T: Clone>(previous: &MetricState<T>, error: String) -> MetricState<T> {
    match previous {
        MetricState::Ready(last_good) | MetricState::Stale { last_good, .. } => {
            MetricState::Stale {
                last_good: last_good.clone(),
                error,
            }
        }
        MetricState::Loading | MetricState::Unavailable { .. } => {
            MetricState::Unavailable { reason: error }
        }
    }
}

pub(super) fn refresh_fast_metrics(
    sender: &watch::Sender<SystemSnapshot>,
    monitor: &mut Result<Box<dyn platform::SystemMonitor>, platform::PlatformError>,
) {
    let previous = sender.borrow().clone();
    let mut metrics = previous.metrics.clone();
    match monitor {
        Ok(monitor) => match monitor.sample_fast().inspect_err(|error| {
            telemetry::typed_failure(
                "system_status",
                "fast_sample",
                error,
                matches!(
                    metrics.cpu,
                    MetricState::Ready(_) | MetricState::Stale { .. }
                ),
            )
        }) {
            Ok(sample) => {
                telemetry::recovered("system_status", "fast_sample");
                metrics.cpu = MetricState::Ready(CpuSnapshot {
                    usage_percent: sample.cpu.usage_percent,
                    per_core_percent: sample.cpu.per_core_percent,
                    logical_core_count: sample.cpu.logical_core_count,
                    physical_core_count: sample.cpu.physical_core_count,
                });
                metrics.memory = MetricState::Ready(MemorySnapshot {
                    total_bytes: sample.memory.total_bytes,
                    used_bytes: sample.memory.used_bytes,
                    available_bytes: sample.memory.available_bytes,
                    swap_total_bytes: sample.memory.swap_total_bytes,
                    swap_used_bytes: sample.memory.swap_used_bytes,
                });
                metrics.uptime = MetricState::Ready(UptimeSnapshot {
                    seconds: sample.uptime_seconds,
                });
                metrics.load = if sample.load.supported {
                    MetricState::Ready(LoadSnapshot {
                        one: sample.load.one,
                        five: sample.load.five,
                        fifteen: sample.load.fifteen,
                    })
                } else {
                    MetricState::Unavailable {
                        reason: "load average is unsupported on this platform".into(),
                    }
                };
                let interfaces = sample
                    .network_interfaces
                    .into_iter()
                    .map(|value| NetworkIoInterfaceSnapshot {
                        name: value.name,
                        received_bytes: value.received_bytes,
                        transmitted_bytes: value.transmitted_bytes,
                        received_bytes_per_second: value.received_bytes_per_second,
                        transmitted_bytes_per_second: value.transmitted_bytes_per_second,
                    })
                    .collect::<Vec<_>>();
                let aggregate = interfaces
                    .iter()
                    .filter(|value| !is_loopback_interface(&value.name));
                let (mut rx, mut tx, mut rx_rate, mut tx_rate) = (0, 0, 0.0, 0.0);
                for value in aggregate {
                    rx += value.received_bytes;
                    tx += value.transmitted_bytes;
                    rx_rate += value.received_bytes_per_second;
                    tx_rate += value.transmitted_bytes_per_second;
                }
                metrics.network_io = MetricState::Ready(NetworkIoSnapshot {
                    interfaces,
                    total_received_bytes: rx,
                    total_transmitted_bytes: tx,
                    total_received_bytes_per_second: rx_rate,
                    total_transmitted_bytes_per_second: tx_rate,
                });
            }
            Err(error) => {
                let error = error.to_string();
                metrics.cpu = unavailable_or_stale(&metrics.cpu, error.clone());
                metrics.memory = unavailable_or_stale(&metrics.memory, error.clone());
                metrics.load = unavailable_or_stale(&metrics.load, error.clone());
                metrics.uptime = unavailable_or_stale(&metrics.uptime, error.clone());
                metrics.network_io = unavailable_or_stale(&metrics.network_io, error);
            }
        },
        Err(error) => {
            let error = error.to_string();
            metrics.cpu = unavailable_or_stale(&metrics.cpu, error.clone());
            metrics.memory = unavailable_or_stale(&metrics.memory, error.clone());
            metrics.load = unavailable_or_stale(&metrics.load, error.clone());
            metrics.uptime = unavailable_or_stale(&metrics.uptime, error.clone());
            metrics.network_io = unavailable_or_stale(&metrics.network_io, error);
        }
    }
    metrics.sampled_at = Utc::now();
    let _ = sender.send(snapshot(
        previous.revision.saturating_add(1),
        previous.weather,
        previous.time,
        previous.storage,
        previous.network,
        metrics,
    ));
}

pub(super) fn refresh_slow_metrics(
    sender: &watch::Sender<SystemSnapshot>,
    monitor: &mut Result<Box<dyn platform::SystemMonitor>, platform::PlatformError>,
) {
    let previous = sender.borrow().clone();
    let mut metrics = previous.metrics.clone();
    match monitor {
        Ok(monitor) => match monitor.sample_slow().inspect_err(|error| {
            telemetry::typed_failure(
                "system_status",
                "slow_sample",
                error,
                matches!(
                    metrics.identity,
                    MetricState::Ready(_) | MetricState::Stale { .. }
                ),
            )
        }) {
            Ok(sample) => {
                telemetry::recovered("system_status", "slow_sample");
                metric_result("identity_sample", &sample.identity, &metrics.identity);
                metric_result("thermal_sample", &sample.thermal, &metrics.thermal);
                metric_result("battery_sample", &sample.batteries, &metrics.batteries);
                metrics.identity = match sample.identity {
                    Ok(value) => MetricState::Ready(SystemIdentitySnapshot {
                        host_name: value.host_name,
                        os_name: value.os_name,
                        os_version: value.os_version,
                        kernel_version: value.kernel_version,
                    }),
                    Err(reason) => unavailable_or_stale(&metrics.identity, reason),
                };
                metrics.thermal = match sample.thermal {
                    Ok(values) => MetricState::Ready(
                        values
                            .into_iter()
                            .map(|v| ThermalSensorSnapshot {
                                label: v.label,
                                temperature_celsius: v.temperature_celsius,
                                critical_celsius: v.critical_celsius,
                            })
                            .collect(),
                    ),
                    Err(reason) => unavailable_or_stale(&metrics.thermal, reason),
                };
                metrics.batteries = match sample.batteries {
                    Ok(values) => MetricState::Ready(
                        values
                            .into_iter()
                            .map(|v| BatterySnapshot {
                                vendor: v.vendor,
                                model: v.model,
                                state: match v.state {
                                    platform::BatterySampleState::Charging => {
                                        BatteryState::Charging
                                    }
                                    platform::BatterySampleState::Discharging => {
                                        BatteryState::Discharging
                                    }
                                    platform::BatterySampleState::Full => BatteryState::Full,
                                    platform::BatterySampleState::Empty => BatteryState::Empty,
                                    platform::BatterySampleState::Unknown => BatteryState::Unknown,
                                },
                                charge_percent: v.charge_percent,
                                energy_wh: v.energy_wh,
                                energy_full_wh: v.energy_full_wh,
                                time_to_empty_seconds: v.time_to_empty_seconds,
                                time_to_full_seconds: v.time_to_full_seconds,
                            })
                            .collect(),
                    ),
                    Err(reason) => unavailable_or_stale(&metrics.batteries, reason),
                };
                metrics.processes = MetricState::Ready(ProcessRankingsSnapshot {
                    top_cpu: sample.top_cpu.into_iter().map(map_process).collect(),
                    top_memory: sample.top_memory.into_iter().map(map_process).collect(),
                });
            }
            Err(error) => {
                let error = error.to_string();
                metrics.identity = unavailable_or_stale(&metrics.identity, error.clone());
                metrics.thermal = unavailable_or_stale(&metrics.thermal, error.clone());
                metrics.batteries = unavailable_or_stale(&metrics.batteries, error.clone());
                metrics.processes = unavailable_or_stale(&metrics.processes, error);
            }
        },
        Err(error) => {
            let error = error.to_string();
            metrics.identity = unavailable_or_stale(&metrics.identity, error.clone());
            metrics.thermal = unavailable_or_stale(&metrics.thermal, error.clone());
            metrics.batteries = unavailable_or_stale(&metrics.batteries, error.clone());
            metrics.processes = unavailable_or_stale(&metrics.processes, error);
        }
    }
    metrics.sampled_at = Utc::now();
    let _ = sender.send(snapshot(
        previous.revision.saturating_add(1),
        previous.weather,
        previous.time,
        previous.storage,
        previous.network,
        metrics,
    ));
}

fn map_process(value: platform::ProcessMetricSample) -> ProcessMetricSnapshot {
    ProcessMetricSnapshot {
        pid: value.pid,
        name: value.name,
        cpu_percent: value.cpu_percent,
        memory_bytes: value.memory_bytes,
    }
}

fn is_loopback_interface(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "lo"
        || name
            .strip_prefix("lo")
            .is_some_and(|suffix| suffix.chars().all(|character| character.is_ascii_digit()))
        || name.starts_with("loopback")
}

pub(super) fn map_storage(
    volumes: Vec<platform::LocalVolume>,
    thresholds: StorageThresholds,
) -> StorageSnapshot {
    let volumes: Vec<_> = volumes
        .into_iter()
        .map(|volume| StorageVolumeSnapshot {
            identifier: volume.root.to_string_lossy().into_owned(),
            label: volume.label,
            kind: match volume.kind {
                platform::VolumeKind::Fixed => StorageVolumeKind::Fixed,
                platform::VolumeKind::Removable => StorageVolumeKind::Removable,
            },
            is_system: volume.is_system,
            access: match volume.access {
                platform::VolumeAccess::ReadWrite => StorageVolumeAccess::ReadWrite,
                platform::VolumeAccess::ReadOnly => StorageVolumeAccess::ReadOnly,
                platform::VolumeAccess::Unavailable => StorageVolumeAccess::Unavailable,
            },
            total_bytes: volume.total_bytes,
            available_bytes: volume.available_bytes,
            pressure: thresholds.classify(volume.total_bytes, volume.available_bytes),
        })
        .collect();
    let detected = volumes.iter().position(|volume| volume.is_system);
    let fallback = volumes
        .iter()
        .position(|volume| volume.kind == StorageVolumeKind::Fixed);
    let (system_volume_index, system_volume_source) = if let Some(index) = detected {
        (Some(index), SystemVolumeSource::Detected)
    } else if let Some(index) = fallback {
        (Some(index), SystemVolumeSource::FixedVolumeFallback)
    } else {
        (None, SystemVolumeSource::Unavailable)
    };
    let overall_pressure = volumes
        .iter()
        .map(|volume| volume.pressure)
        .filter(|pressure| *pressure != StoragePressure::Unknown)
        .max()
        .unwrap_or(StoragePressure::Unknown);
    StorageSnapshot {
        volumes,
        overall_pressure,
        system_volume_index,
        system_volume_source,
        sampled_at: Utc::now(),
    }
}

pub(super) fn map_network(status: platform::NetworkStatus) -> NetworkSnapshot {
    let active_link_count = status.active_link_count();
    let has_active_link = status.has_active_link();
    let interfaces = status
        .interfaces
        .into_iter()
        .map(|interface| NetworkInterfaceSnapshot {
            name: interface.name,
            display_name: interface.display_name,
            kind: match interface.kind {
                platform::NetworkInterfaceKind::Wired => NetworkInterfaceKind::Wired,
                platform::NetworkInterfaceKind::Wireless => NetworkInterfaceKind::Wireless,
                platform::NetworkInterfaceKind::Virtual => NetworkInterfaceKind::Virtual,
                platform::NetworkInterfaceKind::Unknown => NetworkInterfaceKind::Unknown,
            },
            link_state: match interface.link_state {
                platform::NetworkLinkState::Up => NetworkLinkState::Up,
                platform::NetworkLinkState::Down => NetworkLinkState::Down,
                platform::NetworkLinkState::Unknown => NetworkLinkState::Unknown,
            },
            addresses: interface
                .addresses
                .into_iter()
                .map(|address| address.to_string())
                .collect(),
        })
        .collect();
    NetworkSnapshot {
        interfaces,
        active_link_count,
        has_active_link,
        sampled_at: Utc::now(),
    }
}
