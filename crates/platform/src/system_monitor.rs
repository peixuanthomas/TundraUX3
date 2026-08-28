use std::cmp::Ordering;
use std::time::Instant;

use battery::units::{energy::watt_hour, ratio::percent, time::second};
use sysinfo::{Components, Networks, ProcessesToUpdate, System};

use crate::PlatformError;

#[derive(Debug, Clone, PartialEq)]
pub struct CpuSample {
    pub usage_percent: f32,
    pub per_core_percent: Vec<f32>,
    pub logical_core_count: usize,
    pub physical_core_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySample {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadSample {
    pub supported: bool,
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkIoInterfaceSample {
    pub name: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub received_bytes_per_second: f64,
    pub transmitted_bytes_per_second: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FastSystemSample {
    pub cpu: CpuSample,
    pub memory: MemorySample,
    pub uptime_seconds: u64,
    pub load: LoadSample,
    pub network_interfaces: Vec<NetworkIoInterfaceSample>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThermalSensorSample {
    pub label: String,
    pub temperature_celsius: f32,
    pub critical_celsius: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatterySampleState {
    Charging,
    Discharging,
    Full,
    Empty,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatterySample {
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub state: BatterySampleState,
    pub charge_percent: f32,
    pub energy_wh: f32,
    pub energy_full_wh: f32,
    pub time_to_empty_seconds: Option<u64>,
    pub time_to_full_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessMetricSample {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlowSystemSample {
    pub thermal: Result<Vec<ThermalSensorSample>, String>,
    pub batteries: Result<Vec<BatterySample>, String>,
    pub top_cpu: Vec<ProcessMetricSample>,
    pub top_memory: Vec<ProcessMetricSample>,
}

pub trait SystemMonitor: Send {
    fn sample_fast(&mut self) -> Result<FastSystemSample, PlatformError>;
    fn sample_slow(&mut self) -> Result<SlowSystemSample, PlatformError>;
}

pub struct NativeSystemMonitor {
    system: System,
    networks: Networks,
    components: Components,
    last_network_sample: Option<Instant>,
}

impl NativeSystemMonitor {
    pub fn new() -> Result<Self, PlatformError> {
        Ok(Self {
            system: System::new(),
            networks: Networks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
            last_network_sample: None,
        })
    }
}

impl SystemMonitor for NativeSystemMonitor {
    fn sample_fast(&mut self) -> Result<FastSystemSample, PlatformError> {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.networks.refresh(true);
        let now = Instant::now();
        let elapsed = self
            .last_network_sample
            .map(|last| now.duration_since(last).as_secs_f64());
        self.last_network_sample = Some(now);
        let mut network_interfaces = self
            .networks
            .iter()
            .map(|(name, data)| {
                let (rx_rate, tx_rate) = elapsed
                    .filter(|seconds| *seconds > 0.0)
                    .map(|seconds| {
                        (
                            data.received() as f64 / seconds,
                            data.transmitted() as f64 / seconds,
                        )
                    })
                    .unwrap_or((0.0, 0.0));
                NetworkIoInterfaceSample {
                    name: name.clone(),
                    received_bytes: data.total_received(),
                    transmitted_bytes: data.total_transmitted(),
                    received_bytes_per_second: rx_rate,
                    transmitted_bytes_per_second: tx_rate,
                }
            })
            .collect::<Vec<_>>();
        network_interfaces.sort_by(|a, b| a.name.cmp(&b.name));
        let cpus = self.system.cpus();
        let load = System::load_average();
        Ok(FastSystemSample {
            cpu: CpuSample {
                usage_percent: self.system.global_cpu_usage(),
                per_core_percent: cpus.iter().map(|cpu| cpu.cpu_usage()).collect(),
                logical_core_count: cpus.len(),
                physical_core_count: System::physical_core_count(),
            },
            memory: MemorySample {
                total_bytes: self.system.total_memory(),
                used_bytes: self.system.used_memory(),
                available_bytes: self.system.available_memory(),
                swap_total_bytes: self.system.total_swap(),
                swap_used_bytes: self.system.used_swap(),
            },
            uptime_seconds: System::uptime(),
            load: LoadSample {
                supported: !cfg!(target_os = "windows"),
                one: load.one,
                five: load.five,
                fifteen: load.fifteen,
            },
            network_interfaces,
        })
    }

    fn sample_slow(&mut self) -> Result<SlowSystemSample, PlatformError> {
        self.system.refresh_processes(ProcessesToUpdate::All, true);
        self.components.refresh(true);
        let thermal_values = self
            .components
            .iter()
            .filter_map(|component| {
                component
                    .temperature()
                    .map(|temperature| ThermalSensorSample {
                        label: component.label().to_string(),
                        temperature_celsius: temperature,
                        critical_celsius: component.critical(),
                    })
            })
            .collect::<Vec<_>>();
        let thermal = if thermal_values.is_empty() {
            Err("no temperature sensors detected".into())
        } else {
            Ok(thermal_values)
        };

        let batteries = match battery::Manager::new() {
            Err(error) => Err(error.to_string()),
            Ok(manager) => match manager.batteries() {
                Err(error) => Err(error.to_string()),
                Ok(iter) => {
                    let values = iter
                        .filter_map(Result::ok)
                        .map(|battery| BatterySample {
                            vendor: battery.vendor().map(str::to_string),
                            model: battery.model().map(str::to_string),
                            state: match battery.state() {
                                battery::State::Charging => BatterySampleState::Charging,
                                battery::State::Discharging => BatterySampleState::Discharging,
                                battery::State::Full => BatterySampleState::Full,
                                battery::State::Empty => BatterySampleState::Empty,
                                _ => BatterySampleState::Unknown,
                            },
                            charge_percent: battery.state_of_charge().get::<percent>(),
                            energy_wh: battery.energy().get::<watt_hour>(),
                            energy_full_wh: battery.energy_full().get::<watt_hour>(),
                            time_to_empty_seconds: battery
                                .time_to_empty()
                                .map(|v| v.get::<second>() as u64),
                            time_to_full_seconds: battery
                                .time_to_full()
                                .map(|v| v.get::<second>() as u64),
                        })
                        .collect::<Vec<_>>();
                    if values.is_empty() {
                        Err("no batteries detected".into())
                    } else {
                        Ok(values)
                    }
                }
            },
        };
        let processes = self
            .system
            .processes()
            .iter()
            .map(|(pid, process)| ProcessMetricSample {
                pid: pid.as_u32(),
                name: process.name().to_string_lossy().into_owned(),
                cpu_percent: process.cpu_usage(),
                memory_bytes: process.memory(),
            })
            .collect::<Vec<_>>();
        let mut top_cpu = processes.clone();
        top_cpu.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.pid.cmp(&b.pid))
        });
        top_cpu.truncate(20);
        let mut top_memory = processes;
        top_memory.sort_by(|a, b| {
            b.memory_bytes
                .cmp(&a.memory_bytes)
                .then_with(|| a.pid.cmp(&b.pid))
        });
        top_memory.truncate(20);
        Ok(SlowSystemSample {
            thermal,
            batteries,
            top_cpu,
            top_memory,
        })
    }
}
