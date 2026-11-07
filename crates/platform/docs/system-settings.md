# Operating-system settings

Operating System Settings [F8] manages the hostname; Region & Time manages the system timezone, automatic time synchronization, and servers. The Linux clock follows system time and timezone directly.

## Queries and dependencies

`SystemSettings` queries the hostname by default; `scope = "time"` queries timezone and time-synchronization configuration.

- The hostname uses `org.freedesktop.hostname1` and does not depend on `hostnamectl`.
- The timezone uses `org.freedesktop.timedate1`, with options supplied by `timedatectl list-timezones`.
- timesyncd configuration is queried through `systemd-analyze cat-config`.
- chrony reads its actual main configuration and include directories; `chronyc` provides the current source status.

These features require systemd, the system D-Bus, and the corresponding service interfaces. When a query fails, F4 details show the specific reason and affected operations. Failure to read one item does not discard other data already obtained in the same snapshot.

## Time-synchronization configuration

systemd-timesyncd and chrony are supported; the user's existing configuration determines the service. The server form can change, clear, or add addresses.

- chrony preserves server/pool/peer types and other parameters; dynamic network sources are read-only.
- timesyncd uses a separate override file and checks the merged result in the final read order.
- Show the complete diff before saving; chrony checks candidate configuration with `chronyd -p`.
- After checking the versions of all files, create one grouped backup, then save each file. If saving fails partway through, restore the previously written files.
- If the service was running, restart it after saving and reread its state; if it was stopped, leave it stopped.

Configuration saving, service restart, and actual time synchronization are reported separately. View recovery records in Configuration Backups and Recovery.

## Validation

```sh
cargo test --locked -p platform --lib
scripts/tests/isolated-systemd.sh TEST_BINARY UID settings CHRONY_BUNDLE
```

Real write tests require root. The script creates isolated mount, PID, network, hostname, D-Bus, and systemd environments. chrony tools are extracted from official packages into a private directory; test services use placeholder processes that do not synchronize time.

References: [systemd hostname interface](https://github.com/systemd/systemd/blob/main/man/org.freedesktop.hostname1.xml), [System time interface](https://github.com/systemd/systemd/blob/main/man/org.freedesktop.timedate1.xml), [chronyd](https://chrony-project.org/doc/4.7/chronyd.html).
