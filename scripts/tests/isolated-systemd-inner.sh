#!/bin/bash
set -euo pipefail

mount --make-rprivate /
for target in /run /tmp /var/lib /var/log /var/spool /usr/lib/systemd/system /etc/systemd/system; do
    mount -t tmpfs -o mode=755 tmpfs "$target"
done
chmod 1777 /tmp
mkdir -p /var/spool/cron/crontabs
chown root:crontab /var/spool/cron/crontabs
chmod 1730 /var/spool/cron/crontabs
umount /sys/fs/cgroup
mount -t cgroup2 none /sys/fs/cgroup
echo private > /run/tundra-scheduled-isolated
for name in basic sysinit timers shutdown; do
    printf '[Unit]\nDescription=Private fixture target\nDefaultDependencies=no\n' > "/etc/systemd/system/$name.target"
done
printf '[Unit]\nDescription=Private fixture root\nDefaultDependencies=no\nWants=fixture.service\n' > /etc/systemd/system/fixture.target
cat > /run/run-test.sh <<EOF
#!/bin/bash
"$binary" --exact management::scheduled_tasks::isolated::private_systemd_create_execute_persist_pause_resume_and_restore --ignored --nocapture > "$output/test.log" 2>&1
status=\$?
printf '%s\\n' "\$status" > "$output/status"
/usr/bin/systemctl --force --no-block exit
exit "\$status"
EOF
printf '[Unit]\nDefaultDependencies=no\n[Service]\nType=oneshot\nEnvironment=TUNDRA_FIXTURE_UID=%s\nExecStart=/bin/bash /run/run-test.sh\n' "$fixture_uid" > /etc/systemd/system/fixture.service
export container=tundra-scheduled-fixture
exec /usr/lib/systemd/systemd --system --unit=fixture.target --log-target=console --log-level=warning
