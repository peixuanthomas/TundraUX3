# Tasks and execution history

Tasks in the title bar and Launcher open the same page. The list shows name, state, start time, and owning feature, with search, state filtering, output viewing, return to the associated page, and task cancellation. Each batch displays 50 records.

If the associated page still exists, return to its original instance; if it has closed, open only the associated feature without repeating the original operation.

## State and storage

States include awaiting approval, awaiting input, running, canceling, succeeded, failed, canceled, and needs verification. A successful systemd call means the start request was submitted; the separate execution history determines the actual result.

| Source | Location |
| --- | --- |
| Ordinary pages | `task-history/` in the current user's data directory |
| Linux authorization helper | `/var/lib/tundraux3/task-history/UID/` |
| Scheduled tasks | `/var/lib/tundraux3/scheduled-results/UID/history/` |

Pages and helpers use the same stable ID; the list merges duplicate results and gives precedence to system archives. Scheduled tasks use separate systemd invocation IDs and can record results while Tundra is closed.

Output is written to disk in chunks and read at most 64 KiB at a time, with forward and backward browsing. History stores summaries and output, not passwords, keystrokes, pasted input, or configuration-query contents. Failed output writes are marked incomplete.

After a restart, state is checked using the boot marker, PID, and process start time. Records that cannot be confirmed are shown as needing verification and are not automatically rerun.

## Cleanup

History is deleted only through manual cleanup. The page shows total storage use and use by source. The cleanup preview lists tasks, times, results, and space to be freed; after confirmation, the same records are checked again. Running records cannot be deleted.

System archives and personal copies are cleaned up separately. After a system archive is deleted, a retained personal copy may reappear with its source identified. History belonging to other execution accounts can be read with authorization, but its cleanup buttons remain disabled.

Reading Linux system records requires explicit authorization. Automatic refresh updates personal history; clicking Refresh rereads system changes. System-tool diagnostics retain their original text in the output.

## Validation

Automated regressions cover output chunking, duplicate IDs, PID reuse, rebooting, states needing verification, cancellation cleanup, write failures, and cleanup version checks. Historical device acceptance results are in the [Operations usability acceptance record](../../../docs/operations-usability-validation.md).
