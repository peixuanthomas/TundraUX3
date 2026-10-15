#!/bin/bash
set -euo pipefail

# Only this helper's cgroup is shared; every service/configuration mount is private.
if [[ $EUID != 0 ]]; then echo 'Run as root inside WSL.' >&2; exit 1; fi
binary=$(realpath "$1")
fixture_uid=$2
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
export binary fixture_uid output cgroup inner
bash -c '
    echo $$ > "$cgroup/cgroup.procs"
    exec unshare --mount --pid --fork --cgroup --net --uts --ipc --mount-proc bash "$inner"
'
cat "$output/test.log"
[[ $(cat "$output/status") == 0 ]]
