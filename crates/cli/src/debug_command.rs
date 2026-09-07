use std::io::Write;
use std::time::{Duration, Instant};

use watchdog::{
    AppWatchdog, BoundaryKind, BoundarySpec, ComponentId, ErrorContext, IncidentReceipt,
    IncidentSeverity, ProcessWatchdog, RecoveryOutcome,
};

use crate::CliCommand;

pub(crate) fn run_watchdog_test(
    command: CliCommand,
    managed: Option<(&ProcessWatchdog, &AppWatchdog)>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> i32 {
    let Some((process, app)) = managed else {
        let _ = writeln!(
            stderr,
            "ERROR: watchdog tests require the managed tundra-cli runtime"
        );
        return 1;
    };
    let app = app.child_component(ComponentId::from_static("debug"));
    let receipt = if command == CliCommand::TestWatchdogPanic {
        let caught = app
            .run_boundary(
                BoundarySpec::new("cli.debug.test-watchdog-panic", BoundaryKind::Worker),
                || panic!("Intentional watchdog panic test; no user operation was running"),
            )
            .expect_err("the watchdog test always panics");
        match caught.finalize(RecoveryOutcome::Recovered(
            "Intentional debug panic caught; command loop can continue".to_string(),
        )) {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = writeln!(
                    stderr,
                    "ERROR: watchdog panic test could not finalize: {error}"
                );
                return 1;
            }
        }
    } else {
        let (boundary, severity) = if command == CliCommand::TestWatchdogCritical {
            (
                "cli.debug.test-watchdog-critical",
                IncidentSeverity::Critical,
            )
        } else {
            ("cli.debug.test-watchdog-error", IncidentSeverity::Error)
        };
        let ticket = app.report_error(
            ErrorContext::new(boundary, severity),
            &std::io::Error::other("Intentional watchdog error test; no user operation failed"),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if Instant::now() >= deadline {
                let _ = writeln!(
                    stderr,
                    "ERROR: timed out waiting for watchdog report {}; persistence is unconfirmed",
                    ticket.incident_id
                );
                return 1;
            }
            if let Some(receipt) = process.try_recv_incident() {
                if receipt.incident_id == ticket.incident_id {
                    break receipt;
                }
                write_incident(stderr, &receipt);
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };

    let _ = writeln!(
        stdout,
        "Watchdog debug incident: {} ({:?}, {:?})",
        receipt.incident_id, receipt.kind, receipt.severity
    );
    if let Some(path) = &receipt.json_report_path {
        let _ = writeln!(stdout, "JSON report: {}", path.display());
    }
    if let Some(path) = &receipt.text_report_path {
        let _ = writeln!(stdout, "Text report: {}", path.display());
    }
    if receipt.json_report_path.is_none() || receipt.text_report_path.is_none() {
        let _ = writeln!(
            stderr,
            "ERROR: watchdog did not persist both report files; inspect watchdog stderr output"
        );
        return 1;
    }
    let _ = writeln!(
        stdout,
        "Watchdog test completed; Command Line can continue."
    );
    0
}

fn write_incident(output: &mut impl Write, incident: &IncidentReceipt) {
    let severity = match incident.severity {
        IncidentSeverity::Warning => "WARNING",
        IncidentSeverity::Error => "ERROR",
        IncidentSeverity::Critical => "CRITICAL",
    };
    let report = incident
        .text_report_path
        .as_ref()
        .or(incident.json_report_path.as_ref())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "report path unavailable".to_string());
    let _ = writeln!(
        output,
        "WATCHDOG {severity}: {} (incident {}; recovery: {:?}; report: {report})",
        incident.summary, incident.incident_id, incident.recovery
    );
}

pub(crate) fn drain_watchdog_incidents(process: &ProcessWatchdog, stderr: &mut impl Write) {
    let mut incidents: Vec<IncidentReceipt> = Vec::new();
    for incident in process.drain_incidents() {
        if let Some(existing) = incidents
            .iter_mut()
            .find(|existing| existing.incident_id == incident.incident_id)
        {
            if severity_rank(incident.severity) >= severity_rank(existing.severity) {
                *existing = incident;
            }
        } else {
            incidents.push(incident);
        }
    }
    for incident in incidents {
        write_incident(stderr, &incident);
    }
}

fn severity_rank(severity: IncidentSeverity) -> u8 {
    match severity {
        IncidentSeverity::Warning => 0,
        IncidentSeverity::Error => 1,
        IncidentSeverity::Critical => 2,
    }
}
