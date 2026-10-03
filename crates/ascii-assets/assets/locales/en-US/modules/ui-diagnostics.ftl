# UI diagnostics messages.

ui-diagnostics-system-status-diagnostics = System Status / Diagnostics
ui-diagnostics-restart-required = Restart required
ui-diagnostics-scanning-health-checks = Scanning health checks...
ui-diagnostics-no-health-checks-available = No health checks available
ui-diagnostics-system-healthy = System healthy
ui-diagnostics-not-yet-scanned = not yet scanned
ui-diagnostics-checks = Checks
ui-diagnostics-logs = Logs
ui-diagnostics-incidents = Incidents
ui-diagnostics-details = Details
ui-diagnostics-scanning-padded = { "  " }Scanning...
ui-diagnostics-no-checks-available-padded = { "  " }No checks available
ui-diagnostics-no-logs-found-padded = { "  " }No logs found
ui-diagnostics-logs-are-restricted-to-administrators-padded = { "  " }Logs are restricted to administrators
ui-diagnostics-no-incidents-recorded-padded = { "  " }No incidents recorded
ui-diagnostics-no-check-selected = No check selected
ui-diagnostics-no-incident-selected = No incident selected
ui-diagnostics-logs-are-restricted-to-administrators = Logs are restricted to administrators
ui-diagnostics-no-log-selected = No log selected
ui-diagnostics-press-o-to-open-read-only-or-e-to-explore-the-log-folder = Enter/O opens read-only; E explores the log folder
ui-diagnostics-detail-restricted-to-administrators = Detail: Restricted to administrators
ui-diagnostics-repair-disabled-until-restart = Repair disabled until restart
ui-diagnostics-repair-available-press-f = Repair available — press F
ui-diagnostics-repair-requires-administrator-access = Repair requires administrator access
ui-diagnostics-details-and-report-path-are-restricted-to-administrators = Details and report path are restricted to administrators
ui-diagnostics-r-rescan = R/F5 Rescan
ui-diagnostics-c-copy = C Copy
ui-diagnostics-f-repair = F Repair
ui-diagnostics-a-repair-all = A Repair all
ui-diagnostics-o-open-log = Enter/O Open log
ui-diagnostics-o-open-report = Enter/O Open report
ui-diagnostics-e-log-folder = E Log folder
ui-diagnostics-x-restart = X Restart
ui-diagnostics-repair-preview = Repair preview
ui-diagnostics-review-the-changes-before-repair = Review the changes before repair.
ui-diagnostics-storage-document-repairs-require-a-safe-restart = Storage document repairs require a safe restart.
ui-diagnostics-no-repair-actions-selected = No repair actions selected
ui-diagnostics-r-restart-repairs-run-in-order-completed-independent-repairs-are-kept = Enter selected action · Y Repair · R Restart · Esc Cancel
ui-diagnostics-confirm-repair-button = Y Repair
ui-diagnostics-restart-button = R Restart
ui-diagnostics-cancel-button = Esc Cancel
ui-diagnostics-r-restart = R Restart
ui-diagnostics-e-safe-exit = E Safe exit
ui-diagnostics-health = Health
ui-diagnostics-pass = Pass
ui-diagnostics-unsupported = Unsupported
ui-diagnostics-warning = Warning
ui-diagnostics-failure = Failure

ui-diagnostics-last-scan = { $state }    Last scan: { $scanned_at }

ui-diagnostics-log-row = { " " }{ $name }  { $modified }  { $size } bytes

ui-diagnostics-modified = Modified: { $modified }

ui-diagnostics-size-bytes = Size: { $size } bytes

ui-diagnostics-path = Path: { $path }

ui-diagnostics-category = Category: { $category }

ui-diagnostics-summary = Summary: { $summary }

ui-diagnostics-detail = Detail: { $detail }

ui-diagnostics-recommended = Recommended: { $remediation }

ui-diagnostics-incident-id = { $severity } Incident { $id }

ui-diagnostics-incident = { $severity } Incident

ui-diagnostics-occurred = Occurred: { $time }

ui-diagnostics-application = Application: { $app }

ui-diagnostics-recovery = Recovery: { $recovery }

ui-diagnostics-report = Report: { $path }

ui-diagnostics-restart-help = Restart required · Enter/R Restart · E Safe exit · { $close_hint }

ui-diagnostics-scanning-help = Scanning... · { $close_hint }

ui-diagnostics-attention-failures = System needs attention — { $warnings ->
        [one] { $warnings } warning
       *[other] { $warnings } warnings
    } / { $failures ->
        [one] { $failures } failure
       *[other] { $failures } failures
    }

ui-diagnostics-attention-warnings = { $count ->
        [one] System needs attention — { $count } warning
       *[other] System needs attention — { $count } warnings
    }

ui-diagnostics-unsupported-count = { $count ->
        [one] System healthy — { $count } unsupported capability
       *[other] System healthy — { $count } unsupported capabilities
    }
