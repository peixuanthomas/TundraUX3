//! CPU, memory, process and other system metric snapshots.

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq)]
pub enum MetricState<T> {
    Loading,
    Ready(T),
    Stale { last_good: T, error: String },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuSnapshot {
    pub usage_percent: f32,
    pub per_core_percent: Vec<f32>,
    pub logical_core_count: usize,
    pub physical_core_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySnapshot {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadSnapshot {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UptimeSnapshot {
    pub seconds: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkIoInterfaceSnapshot {
    pub name: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub received_bytes_per_second: f64,
    pub transmitted_bytes_per_second: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkIoSnapshot {
    pub interfaces: Vec<NetworkIoInterfaceSnapshot>,
    pub total_received_bytes: u64,
    pub total_transmitted_bytes: u64,
    pub total_received_bytes_per_second: f64,
    pub total_transmitted_bytes_per_second: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThermalSensorSnapshot {
    pub label: String,
    pub temperature_celsius: f32,
    pub critical_celsius: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryState {
    Charging,
    Discharging,
    Full,
    Empty,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatterySnapshot {
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub state: BatteryState,
    pub charge_percent: f32,
    pub energy_wh: f32,
    pub energy_full_wh: f32,
    pub time_to_empty_seconds: Option<u64>,
    pub time_to_full_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessMetricSnapshot {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessRankingsSnapshot {
    pub top_cpu: Vec<ProcessMetricSnapshot>,
    pub top_memory: Vec<ProcessMetricSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemIdentitySnapshot {
    pub host_name: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemMetricsSnapshot {
    pub identity: MetricState<SystemIdentitySnapshot>,
    pub cpu: MetricState<CpuSnapshot>,
    pub memory: MetricState<MemorySnapshot>,
    pub load: MetricState<LoadSnapshot>,
    pub uptime: MetricState<UptimeSnapshot>,
    pub network_io: MetricState<NetworkIoSnapshot>,
    pub thermal: MetricState<Vec<ThermalSensorSnapshot>>,
    pub batteries: MetricState<Vec<BatterySnapshot>>,
    pub processes: MetricState<ProcessRankingsSnapshot>,
    pub sampled_at: DateTime<Utc>,
}

impl SystemMetricsSnapshot {
    pub fn loading() -> Self {
        Self {
            identity: MetricState::Loading,
            cpu: MetricState::Loading,
            memory: MetricState::Loading,
            load: MetricState::Loading,
            uptime: MetricState::Loading,
            network_io: MetricState::Loading,
            thermal: MetricState::Loading,
            batteries: MetricState::Loading,
            processes: MetricState::Loading,
            sampled_at: Utc::now(),
        }
    }
}
