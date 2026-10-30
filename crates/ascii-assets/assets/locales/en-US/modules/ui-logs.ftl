# UI logs messages.

ui-logs-ux-log = UX log
ui-logs-linux-log = Linux log
ui-logs-refresh = R/F5 Refresh
ui-logs-open = Enter/O Open
ui-logs-pause-scroll = P Pause scrolling
ui-logs-back-latest = P Latest ({ $count } new)
ui-logs-more = F10 More actions
ui-logs-source-filters = F Service, boot or file
ui-logs-unit = Service (.service; blank for all)
ui-logs-scope = Service scope
ui-logs-boot = Boot (0, -1 or boot ID; blank for all)
ui-logs-invocation = Service invocation ID (optional)
ui-logs-file = Absolute log file path (optional)
ui-logs-file-absolute = Relative file path. Enter an absolute path.
ui-logs-invalid-service = Invalid service. Enter an exact .service name.
ui-logs-invalid-boot = Invalid boot. Enter 0, -1 or a 32-character boot ID.
ui-logs-invalid-invocation = Invalid invocation. Enter the 32-character service invocation ID.
ui-logs-file-replaced = File rotated or truncated. Following the current file.
ui-logs-buffer-limited = Log buffer full. Earlier entries were discarded; narrow the filters.
ui-logs-state-ready = Ready
ui-logs-state-partial = Some logs are unavailable. Check the details.
ui-logs-state-permission = Log access denied. Check the system permissions.
ui-logs-state-unavailable = Logs unavailable. Check the source and refresh.
ui-logs-state-unsupported = Source unsupported. Choose another log source.
ui-logs-state-cancelled = Query cancelled. Refresh to continue.
ui-logs-file-missing = File missing. Check the path or wait for rotation.
ui-logs-file-denied = File access denied. Check the file permissions.
ui-logs-file-unreadable = File could not be read. Check it and refresh.
ui-logs-file-tail = Showing the file tail. Earlier entries remain in the file.
ui-logs-file-unparsed = File event time and level are unknown. Clear event filters.
ui-logs-journal-denied = Journal access denied. Check system log permissions.
ui-logs-r-refresh = R/F5 Refresh
ui-logs-o-open = Enter/O Open
ui-logs-l-level = L Level
ui-logs-m-module = M Module
ui-logs-t-time = T Time
ui-logs-linux-log-is-unavailable-on-windows-and-macos-ux-log-remains-available = Linux log is unavailable on Windows and macOS. UX log remains available.
ui-logs-linux-log-requires-administrator-access-and-operating-system-log-permissions = Linux log requires administrator access and operating system log permissions.
ui-logs-sign-in-to-view-your-ux-log-guest-access-is-disabled = Sign in to view your UX log. Guest access is disabled.
ui-logs-logs = Logs
ui-logs-loading-logs = Loading logs...
ui-logs-linux-events = Linux events
ui-logs-loading-events = Loading events...
ui-logs-no-events-match-the-current-query = No events match the current query
ui-logs-select-an-event-to-inspect-its-operation-and-correlation-identifiers = Select an event to inspect its operation and correlation identifiers.

ui-logs-event-detail = Time: { $time }
    Level: { $level }
    Event: { $event }
    Operation: { $operation }
    { $detail }{ $incident }

ui-logs-incident-link = { "\u000A" }Incident: { $id }

ui-logs-clear-filters = C Clear filters
ui-logs-show-incident = I Open incident report
ui-logs-show-events = E Related events
ui-logs-open-folder = G Open folder
ui-logs-level-filter = L Level: { $level }
ui-logs-level-or-higher = { $level } and above
ui-logs-filter-summary = Module: { $module } | Time: { $time } | { $context }
ui-logs-level-trace = Trace
ui-logs-level-debug = Debug
ui-logs-level-info = Info
ui-logs-level-warning = Warning
ui-logs-level-error = Error
ui-logs-level-critical = Critical
