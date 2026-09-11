language-reload-failed = Could not reload the language: { $reason }. The previous language is still active.
resources-recovery-title = Resource recovery
resources-recovery-ok = OK
resources-repaired =
    { $count ->
        [one] A resource file was missing or damaged and has been automatically repaired.
       *[other] { $count } resource files were missing or damaged and have been automatically repaired.
    }
    { $files }
resources-fallback = Some resources could not be repaired. Built-in resources are active.
    Repaired: { $repaired }. Using built-in resources: { $failed }.
    Repaired files:
    { $repaired_files }
    Files using built-in resources:
    { $failed_files }
startup-theme-reload-title = Theme reload failed
startup-theme-reload-failed = Could not reload the active user's theme: { $reason }. The last valid theme is still active.
startup-lifecycle-failed = Desktop lifecycle monitoring failed: { $reason }
startup-session-refresh-failed = Desktop session refresh failed: { $reason }
startup-reboot-failed = Restart computer failed: { $reason }
startup-shutdown-failed = Shut down computer failed: { $reason }
startup-storage-recovered = Storage recovered defaults
startup-updated = Updated TundraUX to { $revision }
startup-update-restored = Update failed and the previous version was restored: { $reason }
startup-update-unchecked = Not checked
startup-continue = Continue
startup-open-report = Open report
startup-copy-summary = Copy summary
startup-exit = Exit
startup-critical-recovered = Program recovered from a critical error
startup-critical-failed = Program encountered a critical error
startup-critical-public = A TundraUX component reported a critical error.
    
    Recovery: { $recovery }
    Detailed incident data is restricted to administrators.
startup-critical-detail = { $summary }
    
    Recovery: { $recovery }
    Incident: { $incident }
    Report: { $report }
