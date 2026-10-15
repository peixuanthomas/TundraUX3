#!/bin/bash
set -euo pipefail
[[ $EUID == 0 ]] || { echo 'Run as WSL root.' >&2; exit 1; }
binary=$(realpath "$1")
bundle=$(realpath "$2")
TUNDRA_FIREWALL_CASE=${3:-all}
[[ $TUNDRA_FIREWALL_CASE == all || $TUNDRA_FIREWALL_CASE == firewalld || $TUNDRA_FIREWALL_CASE == ssh || $TUNDRA_FIREWALL_CASE == firewalld-boot ]]
[[ -x "$binary" && -d "$bundle/usr/lib/firewalld" && -x "$bundle/usr/sbin/ufw" ]]
test_names=(private_firewalld_runtime_permanent_delete_and_complete_recovery)
if [[ $TUNDRA_FIREWALL_CASE == all ]]; then
    test_names+=(private_ufw_active_inactive_delete_and_complete_recovery)
elif [[ $TUNDRA_FIREWALL_CASE == ssh ]]; then
    ssh_bundle=$(realpath "$4")
    TUNDRA_TEST_SSHD="$ssh_bundle/usr/sbin/sshd"
    [[ -x $TUNDRA_TEST_SSHD ]]
    export TUNDRA_TEST_SSHD
    export LD_LIBRARY_PATH="$ssh_bundle/lib/x86_64-linux-gnu:$ssh_bundle/usr/lib/x86_64-linux-gnu:$bundle/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    test_names=(management::ssh::connection::real_tests::real_isolated_ssh_connections)
elif [[ $TUNDRA_FIREWALL_CASE == firewalld-boot ]]; then
    test_names=(management::firewall::tests::real_boot_firewalld_offline_rules)
fi
available_tests=$("$binary" --list)
for test_name in "${test_names[@]}"; do
    grep -Fxq -- "$test_name: test" <<< "$available_tests" || { echo "Test not found: $test_name" >&2; exit 1; }
done
output_root=$(dirname "$binary")
output=$(mktemp -d "$output_root/.firewall-isolated.XXXXXX")
inner=$(realpath "$(dirname "$0")/firewall-isolated-inner.sh")
host_rules() { LD_LIBRARY_PATH="$bundle/usr/lib/x86_64-linux-gnu" "$bundle/usr/sbin/nft" -s -j list ruleset; }
host_rules > "$output/host-before.json"
cleanup() {
    host_rules > "$output/host-after.json"
    if ! cmp -s "$output/host-before.json" "$output/host-after.json"; then echo 'Host nftables changed during the test.' >&2; return 1; fi
    echo 'Host nftables unchanged.'
    [[ $(realpath "$output") == "$output_root"/.firewall-isolated.* && -d $output ]] && rm -rf -- "$output"
}
trap cleanup EXIT
TUNDRA_HOST_NET_NS=$(readlink /proc/self/ns/net)
TUNDRA_HOST_MNT_NS=$(readlink /proc/self/ns/mnt)
TUNDRA_HOST_PID_NS=$(readlink /proc/self/ns/pid)
export binary bundle output inner TUNDRA_HOST_NET_NS TUNDRA_HOST_MNT_NS TUNDRA_HOST_PID_NS TUNDRA_FIREWALL_CASE
unshare --mount --pid --fork --kill-child=KILL --net --uts --ipc --mount-proc /bin/bash "$inner"
