use std::collections::HashMap;
use std::fmt;
use std::io::Write;

use tundra_platform::Platform;
use tundra_storage::{StorageLayout, StorageManager};
use tundra_watchdog::{AppWatchdog, IncidentReceipt, IncidentSeverity, ProcessWatchdog};
use tundra_weathr::{LaunchLocation, LaunchOptions, WeathrRunError};

pub(crate) fn run_weathr<Stderr, Launcher, LaunchError>(
    platform: &dyn Platform,
    stderr: &mut Stderr,
    weathr_launcher: Launcher,
) -> i32
where
    Stderr: Write,
    Launcher: FnOnce(LaunchOptions) -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    match weathr_launcher(weathr_launch_options(platform)) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not launch weathr: {error}");
            1
        }
    }
}

pub(crate) fn run_weathr_managed<Stderr, Launcher>(
    platform: &dyn Platform,
    stderr: &mut Stderr,
    process_watchdog: &ProcessWatchdog,
    weathr_watchdog: AppWatchdog,
    weathr_launcher: Launcher,
) -> i32
where
    Stderr: Write,
    Launcher: FnOnce(LaunchOptions, AppWatchdog) -> Result<(), WeathrRunError>,
{
    let launch_result = weathr_launcher(weathr_launch_options(platform), weathr_watchdog);
    if launch_result.is_err() {
        tundra_weathr::restore_terminal_best_effort();
    }
    let incidents = drain_watchdog_incidents(process_watchdog, stderr);

    match launch_result {
        Ok(()) => 0,
        Err(WeathrRunError::Panic {
            incident_id,
            reason,
        }) => {
            let report = incident_report(&incidents, &incident_id)
                .unwrap_or_else(|| "report path unavailable".to_string());
            let body = format!(
                "Weathr could not recover after rebuilding its UI once.\n\n\
                 Error: {reason}\nIncident: {incident_id}\nCrash report: {report}"
            );
            let _ = writeln!(stderr, "ERROR: {body}");
            if should_show_critical_dialog(&incidents, &incident_id)
                && let Err(error) =
                    platform.show_critical_error("Tundra Weathr could not recover", &body)
            {
                let _ = writeln!(stderr, "WARNING: critical error dialog failed: {error}");
            }
            1
        }
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not launch weathr: {error}");
            1
        }
    }
}

pub(crate) fn drain_watchdog_incidents(
    process_watchdog: &ProcessWatchdog,
    stderr: &mut impl Write,
) -> Vec<IncidentReceipt> {
    let incidents = unique_incidents(process_watchdog.drain_incidents());
    for incident in &incidents {
        let report = incident_report(std::slice::from_ref(incident), &incident.incident_id)
            .unwrap_or_else(|| "report path unavailable".to_string());
        let _ = writeln!(
            stderr,
            "WATCHDOG {}: {} (incident {}; recovery: {:?}; report: {})",
            severity_label(incident.severity),
            incident.summary,
            incident.incident_id,
            incident.recovery,
            report
        );
    }
    incidents
}

fn unique_incidents(incidents: Vec<IncidentReceipt>) -> Vec<IncidentReceipt> {
    let mut positions: HashMap<_, usize> = HashMap::new();
    let mut unique: Vec<IncidentReceipt> = Vec::new();
    for incident in incidents {
        if let Some(index) = positions.get(&incident.incident_id).copied() {
            if severity_rank(incident.severity) >= severity_rank(unique[index].severity) {
                unique[index] = incident;
            }
        } else {
            positions.insert(incident.incident_id.clone(), unique.len());
            unique.push(incident);
        }
    }
    unique
}

fn severity_rank(severity: IncidentSeverity) -> u8 {
    match severity {
        IncidentSeverity::Warning => 0,
        IncidentSeverity::Error => 1,
        IncidentSeverity::Critical => 2,
    }
}

fn severity_label(severity: IncidentSeverity) -> &'static str {
    match severity {
        IncidentSeverity::Warning => "WARNING",
        IncidentSeverity::Error => "ERROR",
        IncidentSeverity::Critical => "CRITICAL",
    }
}

fn should_show_critical_dialog(incidents: &[IncidentReceipt], incident_id: &str) -> bool {
    incidents
        .iter()
        .find(|incident| incident.incident_id == incident_id)
        .map(|incident| incident.severity == IncidentSeverity::Critical)
        // A typed Weathr panic without a receipt is still an unrecoverable UI
        // panic; report persistence may have failed, so retain the OS fallback.
        .unwrap_or(true)
}

fn incident_report(incidents: &[IncidentReceipt], incident_id: &str) -> Option<String> {
    let incident = incidents
        .iter()
        .find(|incident| incident.incident_id == incident_id)?;
    incident
        .text_report_path
        .as_ref()
        .or(incident.json_report_path.as_ref())
        .map(|path| path.display().to_string())
}

fn weathr_launch_options(platform: &dyn Platform) -> LaunchOptions {
    let Some(config) = platform
        .app_paths()
        .ok()
        .map(|paths| StorageLayout::from_app_paths(&paths))
        .map(StorageManager::from_layout)
        .and_then(|storage| storage.load_config().ok())
    else {
        return LaunchOptions::default();
    };

    let mut options = LaunchOptions {
        timezone_id: Some(config.timezone.clone()),
        ..LaunchOptions::default()
    };

    if let Some(timezone) = tundra_ui::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == config.timezone)
    {
        options.location_override = Some(LaunchLocation {
            latitude: timezone.latitude,
            longitude: timezone.longitude,
            city: Some(timezone.label),
        });
    }

    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use tundra_watchdog::{IncidentKind, RecoveryOutcome};

    fn receipt(id: &str, severity: IncidentSeverity) -> IncidentReceipt {
        IncidentReceipt {
            incident_id: id.to_string(),
            kind: IncidentKind::Error,
            severity,
            app_id: None,
            component: None,
            task_id: None,
            task_group: None,
            boundary: "test".to_string(),
            panic_action: None,
            operation_kind: None,
            operation_id: None,
            recovery_handler_version: None,
            restart_attempt: 0,
            summary: "test incident".to_string(),
            recovery: RecoveryOutcome::Recovered("test".to_string()),
            json_report_path: None,
            text_report_path: None,
        }
    }

    #[test]
    fn incident_router_deduplicates_by_incident_id() {
        let incidents = unique_incidents(vec![
            receipt("same", IncidentSeverity::Warning),
            receipt("same", IncidentSeverity::Critical),
            receipt("other", IncidentSeverity::Error),
        ]);

        assert_eq!(incidents.len(), 2);
        assert_eq!(incidents[0].severity, IncidentSeverity::Critical);
    }

    #[test]
    fn warning_incident_never_requests_a_critical_dialog() {
        let incidents = vec![receipt("warning", IncidentSeverity::Warning)];

        assert!(!should_show_critical_dialog(&incidents, "warning"));
        assert_eq!(severity_label(IncidentSeverity::Warning), "WARNING");
        assert_eq!(severity_label(IncidentSeverity::Error), "ERROR");
        assert_eq!(severity_label(IncidentSeverity::Critical), "CRITICAL");
    }
}
