#!/bin/bash
set -euo pipefail

mount --make-rprivate /
if [[ $fixture_case == ssh || $fixture_case == settings ]]; then
    for name in passwd group nsswitch.conf protocols services; do cp "/etc/$name" "$output/$name"; done
    mount -t tmpfs -o mode=755 tmpfs /etc
    mount -t tmpfs -o mode=700 tmpfs /root
    for name in passwd group nsswitch.conf protocols services; do cp "$output/$name" "/etc/$name"; done
    printf 'root:private-test-invalid-hash:20000:0:99999:7:::\n' > /etc/shadow
    chmod 600 /etc/shadow
    if [[ $fixture_case == ssh ]] && ! grep -q '^sshd:' /etc/passwd; then
        printf 'sshd:x:9999:9999:Private SSH test:/nonexistent:/usr/sbin/nologin\n' >> /etc/passwd
    fi
    mkdir -p /etc/systemd/system /etc/ssh
    if [[ $fixture_case == ssh ]]; then echo private > /etc/tundra-ssh-systemd-isolated; fi
fi
for target in /run /tmp /var/lib /var/log /var/spool /usr/lib/systemd/system /etc/systemd/system; do
    mount -t tmpfs -o mode=755 tmpfs "$target"
done
chmod 1777 /tmp
if [[ $fixture_case == settings ]]; then
    mount -t tmpfs -o mode=755 tmpfs /usr/sbin
    mkdir -p /etc/dbus-1 /run/dbus /etc/systemd/ntp-units.d /etc/chrony/conf.d /run/chrony-dhcp
    echo private > /etc/tundra-system-settings-isolated
    echo private > /run/tundra-system-settings-isolated
    echo tundra-fixture > /etc/hostname
    echo 7315d2a789cb4cd2a731ebd021201367 > /etc/machine-id
    ln -s /usr/share/zoneinfo/Etc/UTC /etc/localtime
    printf 'systemd-timesyncd.service\nchrony.service\n' > /etc/systemd/ntp-units.d/50-tundra-fixture.list
    printf '[Time]\nNTP=old.example\nFallbackNTP = fallback.example\n' > /etc/systemd/timesyncd.conf
    cat > /run/time-bus.conf <<'EOF'
<busconfig><type>system</type><listen>unix:path=/run/dbus/system_bus_socket</listen><policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>
EOF
    cat > /usr/sbin/chronyd <<EOF
#!/bin/bash
if [[ \$1 == -p ]]; then exec env LD_LIBRARY_PATH="$time_bundle/lib/x86_64-linux-gnu:$time_bundle/usr/lib/x86_64-linux-gnu" "$time_bundle/usr/sbin/chronyd" "\$@"; fi
exec -a chronyd /usr/bin/sleep infinity
EOF
    chmod 755 /usr/sbin/chronyd
fi
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
if [[ $fixture_case == settings ]]; then
    printf '[Unit]\nDefaultDependencies=no\n[Socket]\nListenStream=/run/dbus/system_bus_socket\n' > /etc/systemd/system/dbus.socket
    for spec in 'dbus:/usr/bin/dbus-daemon --nofork --nopidfile --address=systemd: --systemd-activation --config-file=/run/time-bus.conf' 'systemd-hostnamed:/usr/lib/systemd/systemd-hostnamed' 'systemd-timedated:/usr/lib/systemd/systemd-timedated'; do
        name=${spec%%:*}
        executable=${spec#*:}
        printf '[Unit]\nDefaultDependencies=no\n[Service]\nExecStart=%s\n' "$executable" > "/etc/systemd/system/$name.service"
        printf 'StandardOutput=append:%s/%s.log\nStandardError=inherit\n' "$output" "$name" >> "/etc/systemd/system/$name.service"
        if [[ $name != dbus ]]; then printf '[Unit]\nAfter=dbus.service\n' >> "/etc/systemd/system/$name.service"; fi
        if [[ $name == dbus ]]; then
            printf '[Unit]\nRequires=dbus.socket\nAfter=dbus.socket\n[Service]\nType=notify\nNotifyAccess=main\n' >> "/etc/systemd/system/$name.service"
        else
            bus_name=org.freedesktop.hostname1
            if [[ $name == systemd-timedated ]]; then bus_name=org.freedesktop.timedate1; fi
            printf '[Service]\nType=dbus\nBusName=%s\n' "$bus_name" >> "/etc/systemd/system/$name.service"
        fi
    done
    printf '[Unit]\nDefaultDependencies=no\n[Service]\nExecStart=/usr/bin/sleep infinity\n[Install]\nWantedBy=fixture.target\n' > /etc/systemd/system/systemd-timesyncd.service
    printf '[Unit]\nDefaultDependencies=no\n[Service]\nExecStart=/usr/sbin/chronyd -f /etc/chrony/chrony.conf\n[Install]\nWantedBy=fixture.target\n' > /etc/systemd/system/chrony.service
    printf '[Unit]\nRequires=dbus.service systemd-hostnamed.service systemd-timedated.service\nAfter=dbus.service systemd-hostnamed.service systemd-timedated.service\n[Service]\nExecStartPre=/usr/bin/sleep 1\nEnvironment=TUNDRA_HOST_MNT_NS=%s\nEnvironment=TUNDRA_HOST_NET_NS=%s\nEnvironment=TUNDRA_HOST_PID_NS=%s\nEnvironment=TUNDRA_HOST_UTS_NS=%s\n' "$TUNDRA_HOST_MNT_NS" "$TUNDRA_HOST_NET_NS" "$TUNDRA_HOST_PID_NS" "$TUNDRA_HOST_UTS_NS" >> /etc/systemd/system/fixture.service
fi
if [[ $fixture_case == ssh ]]; then
    printf 'Environment=TUNDRA_TEST_SSHD=%s\nEnvironment=LD_LIBRARY_PATH=%s\nEnvironment=TUNDRA_HOST_MNT_NS=%s\nEnvironment=TUNDRA_HOST_NET_NS=%s\nEnvironment=TUNDRA_HOST_PID_NS=%s\n' "$TUNDRA_TEST_SSHD" "$LD_LIBRARY_PATH" "$TUNDRA_HOST_MNT_NS" "$TUNDRA_HOST_NET_NS" "$TUNDRA_HOST_PID_NS" >> /etc/systemd/system/fixture.service
fi
export container=tundra-scheduled-fixture
exec /usr/lib/systemd/systemd --system --unit=fixture.target --log-target=console --log-level=warning
