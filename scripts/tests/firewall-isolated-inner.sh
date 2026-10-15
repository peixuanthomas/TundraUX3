#!/bin/bash
set -euo pipefail
[[ $EUID == 0 && $(readlink /proc/self/ns/net) != "$TUNDRA_HOST_NET_NS" && $(readlink /proc/self/ns/mnt) != "$TUNDRA_HOST_MNT_NS" && $(readlink /proc/self/ns/pid) != "$TUNDRA_HOST_PID_NS" ]]
mount --make-rprivate /
cp /etc/passwd "$output/passwd"
cp /etc/group "$output/group"
cp /etc/protocols "$output/protocols"
cp /etc/services "$output/services"
for target in /etc /run /tmp /var/lib /var/log; do mount -t tmpfs -o mode=755 tmpfs "$target"; done
chmod 1777 /tmp
mkdir /run/usr-upper /run/usr-work
mount -t overlay overlay -o "lowerdir=$bundle/usr:/usr,upperdir=/run/usr-upper,workdir=/run/usr-work" /usr
cp "$output/passwd" /etc/passwd
cp "$output/group" /etc/group
cp "$output/protocols" /etc/protocols
cp "$output/services" /etc/services
cp -a "$bundle/etc/." /etc/
chown -R 0:0 /etc
printf 'root:*:20000:0:99999:7:::\n' > /etc/shadow
chmod 600 /etc/shadow
mkdir -p /etc/ufw /run/lock /run/dbus
cp /usr/share/ufw/iptables/*.rules /etc/ufw/
printf 'ENABLED=no\nLOGLEVEL=off\n' > /etc/ufw/ufw.conf
printf 'private\n' > /run/tundra-firewall-isolated
printf 'private\n' > /etc/tundra-firewall-isolated
for suffix in '' -save -restore; do
    ln -sf xtables-nft-multi "/usr/sbin/iptables$suffix"
    ln -sf xtables-nft-multi "/usr/sbin/ip6tables$suffix"
done
for name in ebtables arptables; do for suffix in '' -save -restore; do ln -sf "$name-nft$suffix" "/usr/sbin/$name$suffix"; done; done
# Kernel modules are host-wide even with a private network namespace.
if [[ -e /usr/sbin/modprobe ]]; then mount --bind /usr/bin/false /usr/sbin/modprobe; fi
if [[ $TUNDRA_FIREWALL_CASE == firewalld-boot ]]; then
    "$binary" --exact management::firewall::tests::real_boot_firewalld_offline_rules --ignored --nocapture --test-threads=1
    exit
fi
cat > /run/dbus-private.conf <<'EOF'
<busconfig><type>system</type><listen>unix:path=/run/dbus/system_bus_socket</listen><auth>EXTERNAL</auth><policy context="default"><allow user="root"/><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>
EOF
dbus-daemon --config-file=/run/dbus-private.conf --fork --nopidfile
if [[ $TUNDRA_FIREWALL_CASE == ssh ]]; then
    mkdir -p /run/sshd
    /usr/sbin/ufw --force enable
    TUNDRA_SSH_WITH_UFW=1 "$binary" --exact management::ssh::connection::real_tests::real_isolated_ssh_connections --ignored --nocapture --test-threads=1
    exit
fi
if [[ $TUNDRA_FIREWALL_CASE == all ]]; then "$binary" --exact private_ufw_active_inactive_delete_and_complete_recovery --ignored --nocapture --test-threads=1; fi
/usr/sbin/firewalld --nofork --nopid --debug=1 --log-target=console > /run/firewalld-test.log 2>&1 &
firewall_pid=$!
ready=false
for attempt in $(seq 1 100); do
    if /usr/bin/firewall-cmd --state >/run/firewall-state.log 2>&1; then ready=true; break; fi
    if ! kill -0 "$firewall_pid" 2>/dev/null; then break; fi
    sleep 0.1
done
if [[ $ready != true ]]; then cat /run/firewall-state.log /run/firewalld-test.log >&2; exit 1; fi
"$binary" --exact private_firewalld_runtime_permanent_delete_and_complete_recovery --ignored --nocapture --test-threads=1
