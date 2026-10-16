#!/bin/bash
set -euo pipefail

mount --make-rprivate /
if [[ $fixture_case == ssh ]]; then
    for name in passwd group nsswitch.conf protocols services; do cp "/etc/$name" "$output/$name"; done
    mount -t tmpfs -o mode=755 tmpfs /etc
    mount -t tmpfs -o mode=700 tmpfs /root
    for name in passwd group nsswitch.conf protocols services; do cp "$output/$name" "/etc/$name"; done
    printf 'root:private-test-invalid-hash:20000:0:99999:7:::\n' > /etc/shadow
    chmod 600 /etc/shadow
    if ! grep -q '^sshd:' /etc/passwd; then
        printf 'sshd:x:9999:9999:Private SSH test:/nonexistent:/usr/sbin/nologin\n' >> /etc/passwd
    fi
    mkdir -p /etc/systemd/system /etc/ssh
    echo private > /etc/tundra-ssh-systemd-isolated
fi
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
if [[ $fixture_case == ssh ]]; then
    mkdir /run/sshd
    echo private > /run/tundra-ssh-systemd-isolated
fi
for name in basic sysinit timers shutdown; do
    printf '[Unit]\nDescription=Private fixture target\nDefaultDependencies=no\n' > "/etc/systemd/system/$name.target"
done
printf '[Unit]\nDescription=Private fixture root\nDefaultDependencies=no\nWants=fixture.service\n' > /etc/systemd/system/fixture.target
cat > /run/run-test.sh <<EOF
#!/bin/bash
"$binary" --exact "$fixture_test" --ignored --nocapture --test-threads=1 > "$output/test.log" 2>&1
status=\$?
printf '%s\\n' "\$status" > "$output/status"
/usr/bin/systemctl --force --no-block exit
exit "\$status"
EOF
printf '[Unit]\nDefaultDependencies=no\n[Service]\nType=oneshot\nEnvironment=TUNDRA_FIXTURE_UID=%s\nExecStart=/bin/bash /run/run-test.sh\n' "$fixture_uid" > /etc/systemd/system/fixture.service
if [[ $fixture_case == ssh ]]; then
    printf 'Environment=TUNDRA_TEST_SSHD=%s\nEnvironment=LD_LIBRARY_PATH=%s\nEnvironment=TUNDRA_HOST_MNT_NS=%s\nEnvironment=TUNDRA_HOST_NET_NS=%s\nEnvironment=TUNDRA_HOST_PID_NS=%s\n' "$TUNDRA_TEST_SSHD" "$LD_LIBRARY_PATH" "$TUNDRA_HOST_MNT_NS" "$TUNDRA_HOST_NET_NS" "$TUNDRA_HOST_PID_NS" >> /etc/systemd/system/fixture.service
fi
export container=tundra-scheduled-fixture
exec /usr/lib/systemd/systemd --system --unit=fixture.target --log-target=console --log-level=warning
