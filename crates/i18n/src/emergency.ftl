i18n-unavailable = Translation unavailable
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
language-reload-failed = Could not reload the language: { $reason }. The previous language is still active.
notifications-action-ok = OK
notifications-status-ready = Ready
