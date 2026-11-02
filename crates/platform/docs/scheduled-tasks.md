# Scheduled tasks

The Scheduled Tasks page combines system-wide and current-user systemd timers, the current user's crontab, `/etc/crontab`, and `/etc/cron.d`. The list includes source, execution user, schedule, state, last result, and next run time.

## Creating tasks

The wizard has three steps: What to Do → When to Run → Review and Create.

- Built-in actions include restarting a service, updating local packages, and running a script; advanced mode accepts a command, working directory, and account.
- Schedules support daily, weekly, and a single specified date, displaying the system timezone and the next five run times.
- Advanced mode retains `OnCalendar` expressions; previews use `systemd-analyze calendar --iterations=5`.
- Before creation, `systemd-analyze verify` checks the service/timer, then AA and system authorization save them to `/etc/systemd/system/` and enable them.
- Display names are separate from internal `tundra-scheduled-…` unit IDs; units with the same name are not simply overwritten.

Tasks use the selected user and primary group, with HOME, USER, LOGNAME, and PATH set. systemd handles subsequent triggers, so tasks continue running after Tundra closes. Run Once After a Missed Schedule corresponds to `Persistent` and is off by default.

Each execution writes to `/var/lib/tundraux3/scheduled-results/UID/history/` using the systemd invocation ID, saving its start, end, result, and output, which can be viewed on the Tasks page.

## Editing and deletion

All listed tasks can be edited. Simple daily and weekly rules use the schedule form; complex rules open advanced editing.

- Prefer overrides for systemd timers.
- For cron, modify only the selected line, preserving other lines, environment settings, comments, and pause markers.
- Tasks confirmed to have been created by the user can be deleted; system-provided tasks or tasks of unknown origin offer disable and resume actions.
- Recheck configuration versions before deletion; running tasks must first finish or be explicitly canceled.
- Deletion does not remove shared services, execution history, or application data files.

## Pausing, resuming, and running now

Multiple-selection actions handle all checked tasks and are available only when all targets support them. Changes from the same cron source are checked against the original version, then saved and backed up together.

- Pausing a timer stops future triggers and disables startup activation; a service already running continues.
- Resuming a timer enables and starts it; timers without installation rules are only started.
- Run Now starts the timer's corresponding service without changing its account, environment, or other definitions.
- Pausing cron adds `# TUNDRAUX3-PAUSED `; resuming removes that marker without converting cron to a timer.
- Running cron immediately creates a separate temporary systemd service using the original entry's account, environment, and percent-sign input rules.

cron usually does not retain exit codes for scheduled executions, so the list shows "Not recorded by cron"; manual execution results appear separately.

`croner` calculates five-field schedules and common aliases; `CRON_TZ` uses `chrono-tz`. Extensions that cannot be interpreted reliably according to the local cron implementation are shown as indeterminate.

## Changes and recovery

Before writing, reread the entire cron source or systemd definition and check its version. Cron files and new units use the safe system-configuration save flow; local crontabs are saved through the native `crontab` tool.

Configuration changes are written to the shared `scheduled-tasks` backup history, saving file contents, attributes, crontab text, and timer enabled/active states. Recovery checks both current files and timer states, stopping if later changes are found. Restoring timer state does not stop an already running service.

## Validation

```sh
cargo test --locked -p platform management::scheduled_tasks --lib
cargo test --locked -p platform --lib --no-run
sudo bash scripts/tests/isolated-systemd.sh /absolute/path/to/platform-test-binary "$(id -u)"
```

Regular tests cover schedule parsing, environment, pause/resume, concurrent changes, and output. The isolation script creates private mount, PID, network, cgroup, systemd, and crontab environments to verify creation, immediate execution, result recording, recovery, and failure paths.
