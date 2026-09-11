#!/usr/bin/env python3
"""Verify a real SSH identity or trace early root rejection without a seat change.

The ordinary-user check needs the distro python3-dbus package and a real logind
session. The root check uses Linux ptrace; it never invokes a power operation.
"""

import argparse
import ctypes
import json
import os
from pathlib import Path
import pwd
import signal
import subprocess
import tempfile


def ordinary(cli: Path) -> None:
    import dbus

    assert os.getuid() != 0, "ordinary mode must run as the actual login user"
    user = pwd.getpwuid(os.getuid())
    status = json.loads(subprocess.check_output([str(cli), "session", "status"], text=True))
    assert status["user"] == {
        "uid": user.pw_uid, "gid": user.pw_gid, "username": user.pw_name,
        "home": user.pw_dir, "shell": user.pw_shell,
    }, status
    bus = dbus.SystemBus()
    manager = dbus.Interface(bus.get_object("org.freedesktop.login1", "/org/freedesktop/login1"),
                             "org.freedesktop.login1.Manager")
    path = manager.GetSessionByPID(os.getpid())
    props = dbus.Interface(bus.get_object("org.freedesktop.login1", path),
                           "org.freedesktop.DBus.Properties")
    session_id = str(props.Get("org.freedesktop.login1.Session", "Id"))
    assert bool(props.Get("org.freedesktop.login1.Session", "Remote")), "expected real SSH session"
    assert status["logind"] == {"uid": user.pw_uid, "logind_session_id": session_id}, status
    assert status["managed"] is None, "standalone SSH process must not claim a managed desktop"
    print(json.dumps({"passed": "NSS and remote logind identity", "status": status}))


def root_refusal(shell: Path) -> None:
    assert os.getuid() == 0, "root-refusal mode must run as root"
    libc = ctypes.CDLL(None, use_errno=True)
    libc.ptrace.restype = ctypes.c_long

    def ptrace(request: int, pid: int, data: int = 0) -> None:
        if libc.ptrace(request, pid, None, ctypes.c_void_p(data)) == -1:
            raise OSError(ctypes.get_errno(), "ptrace failed")

    # TRACEFORK|TRACEVFORK|TRACECLONE|TRACEEXEC|EXITKILL: observe descendants
    # and execs, and never leave a traced process running if this test crashes.
    options = 0x2 | 0x4 | 0x8 | 0x10 | 0x100000
    with tempfile.TemporaryFile() as output:
        child = os.fork()
        if child == 0:
            os.dup2(output.fileno(), 1)
            os.dup2(output.fileno(), 2)
            ptrace(0, 0)  # TRACEME
            os.kill(os.getpid(), signal.SIGSTOP)
            os.execv(str(shell), [str(shell)])
        tracked = {child}
        execs = 0
        forks = 0
        exit_code = None
        try:
            _, initial = os.waitpid(child, 0)
            assert os.WIFSTOPPED(initial)
            ptrace(0x4200, child, options)  # SETOPTIONS
            ptrace(7, child)  # CONT
            while tracked:
                pid, state = os.waitpid(-1, 0)
                if os.WIFEXITED(state) or os.WIFSIGNALED(state):
                    tracked.discard(pid)
                    if pid == child:
                        exit_code = os.waitstatus_to_exitcode(state)
                    continue
                event = state >> 16
                if event in (1, 2, 3):
                    new_pid = ctypes.c_ulong()
                    if libc.ptrace(0x4201, pid, None, ctypes.byref(new_pid)) == -1:
                        raise OSError(ctypes.get_errno(), "GETEVENTMSG failed")
                    tracked.add(new_pid.value)
                    forks += 1
                elif event == 4:
                    execs += 1
                delivered = os.WSTOPSIG(state)
                ptrace(7, pid, 0 if delivered in (signal.SIGTRAP, signal.SIGSTOP) else delivered)
        finally:
            for pid in tracked:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        output.seek(0)
        diagnostic = output.read().decode(errors="replace")
    assert exit_code == 1, (exit_code, diagnostic)
    assert "requires an unprivileged, non-setuid user process" in diagnostic, diagnostic
    assert execs == 1 and forks == 0, (execs, forks)
    assert "\x1b[?1049h" not in diagnostic, "root shell initialized fullscreen terminal"
    print(json.dumps({"passed": "root refused before subprocess or terminal initialization",
                      "exit_code": exit_code, "execs": execs, "forks_or_clones": forks,
                      "diagnostic": diagnostic.strip()}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("ordinary", "root-refusal"))
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    # Bound a failed startup/ptrace experiment even on an unresponsive binary.
    signal.alarm(20)
    (ordinary if args.mode == "ordinary" else root_refusal)(args.binary.resolve())
