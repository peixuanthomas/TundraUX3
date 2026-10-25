#!/bin/bash
set -euo pipefail

# Only this helper's cgroup is shared; every service/configuration mount is private.
if [[ $EUID != 0 ]]; then echo 'Run as root inside WSL.' >&2; exit 1; fi
binary=$(realpath "$1")
fixture_uid=$2
fixture_case=${3:-scheduled}
[[ $fixture_case == scheduled || $fixture_case == ssh || $fixture_case == settings || $fixture_case == selection ]]
fixture_test=management::scheduled_tasks::isolated::private_systemd_create_execute_persist_pause_resume_and_restore
if [[ $fixture_case == selection ]]; then
    fixture_test=management::scheduled_tasks::isolated::batches::private_management_batches_preserve_targets_and_cron_sources
fi
if [[ $fixture_case == ssh ]]; then
    ssh_bundle=$(realpath "$4")
    TUNDRA_TEST_SSHD="$ssh_bundle/usr/sbin/sshd"
    [[ -x $TUNDRA_TEST_SSHD ]]
    LD_LIBRARY_PATH="$ssh_bundle/lib/x86_64-linux-gnu:$ssh_bundle/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export TUNDRA_TEST_SSHD LD_LIBRARY_PATH
    fixture_test=management::ssh::connection::real_tests::systemd_tests::real_systemd_ssh_service_and_socket
fi
if [[ $fixture_case == settings ]]; then
    time_bundle=$(realpath "$4")
    [[ -x $time_bundle/usr/sbin/chronyd ]]
    export time_bundle
    fixture_test=management::system_settings::isolated::private_system_settings_and_time_configuration
fi
available_tests=$("$binary" --list)
[[ $available_tests == *"$fixture_test: test"* ]] || { echo "Test not found: $fixture_test" >&2; exit 1; }
output=$(mktemp -d "$(dirname "$binary")/.scheduled-isolated.XXXXXX")
inner=$(realpath "$(dirname "$0")/isolated-systemd-inner.sh")
cgroup="/sys/fs/cgroup/tundra-scheduled-test-$$"
mkdir "$cgroup"
cleanup() {
    if [[ -e "$cgroup/cgroup.kill" ]]; then echo 1 > "$cgroup/cgroup.kill"; fi
    find "$cgroup" -mindepth 1 -depth -type d -exec rmdir {} \; 2>/dev/null || true
    rmdir "$cgroup" 2>/dev/null || true
    rm -rf -- "$output"
}
trap cleanup EXIT
TUNDRA_HOST_MNT_NS=$(readlink /proc/self/ns/mnt)
TUNDRA_HOST_NET_NS=$(readlink /proc/self/ns/net)
TUNDRA_HOST_PID_NS=$(readlink /proc/self/ns/pid)
TUNDRA_HOST_UTS_NS=$(readlink /proc/self/ns/uts)
export binary fixture_uid fixture_case fixture_test output cgroup inner TUNDRA_HOST_MNT_NS TUNDRA_HOST_NET_NS TUNDRA_HOST_PID_NS TUNDRA_HOST_UTS_NS
bash -c '
    echo $$ > "$cgroup/cgroup.procs"
    exec unshare --mount --pid --fork --cgroup --net --uts --ipc --mount-proc bash "$inner"
'
cat "$output/test.log"
if [[ $fixture_case == settings ]]; then
    for log in "$output"/dbus.log "$output"/systemd-hostnamed.log "$output"/systemd-timedated.log; do
        if [[ -f $log ]]; then cat "$log"; fi
    done
fi
[[ $(cat "$output/status") == 0 ]]
