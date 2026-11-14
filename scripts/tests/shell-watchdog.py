#!/usr/bin/env python3
"""Exercise the guardian against a stopped Shell in a real Unix terminal session."""

import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time


def read_terminal(master, output):
    if select.select([master], [], [], 0.05)[0]:
        try:
            output.extend(os.read(master, 65536))
        except OSError as error:
            if error.errno != errno.EIO:
                raise


def processes(cli, env):
    result = subprocess.run(
        [str(cli), "watchdog", "list"], env=env, capture_output=True, timeout=5, check=True
    )
    return [worker for run in json.loads(result.stdout)["runs"] for worker in run["workers"]]


def assert_gone(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return
    raise AssertionError(f"managed process {pid} is still alive")


def terminal_restored(master, slave, before, foreground):
    try:
        return termios.tcgetattr(slave) == before and os.tcgetpgrp(master) == foreground
    except termios.error as error:
        if error.args[0] != errno.ENOTTY:
            raise
        return False


def tty_diagnostic(master, slave):
    try:
        return (os.tcgetpgrp(master), termios.tcgetattr(slave))
    except OSError as error:
        return str(error)


def run(shell, cli, env, *, force):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
    before = termios.tcgetattr(slave)
    completion_input, completion_output = os.pipe()
    caller = os.fork()
    if caller == 0:
        os.close(master)
        os.close(completion_output)
        os.login_tty(slave)
        result = subprocess.run([str(shell)], env=env, check=False)
        print(f"GUARDIAN_STATUS={result.returncode}", flush=True)
        # Keep the controlling tty alive so the parent can inspect the restored settings.
        os.read(completion_input, 1)
        os.close(0)
        os.close(1)
        os.close(2)
        os._exit(0)
    os.close(completion_input)

    outcome = None
    verified = False
    workers = []
    output = bytearray()
    try:
        deadline = time.monotonic() + 20
        confirmed = False
        while time.monotonic() < deadline:
            read_terminal(master, output)
            if b"any other key cancels: " in output and not confirmed:
                os.write(master, b"y")
                confirmed = True
            if b"\x1b[?1003h" in output and termios.tcgetattr(slave) != before:
                workers = processes(cli, env)
                if {worker["worker"]["role"] for worker in workers} == {"shell", "services"}:
                    break
            child, status = os.waitpid(caller, os.WNOHANG)
            if child:
                outcome = status
                raise AssertionError(output[-4000:].decode(errors="replace"))
        else:
            raise AssertionError(output[-4000:].decode(errors="replace"))

        shell_worker = next(worker for worker in workers if worker["worker"]["role"] == "shell")
        shell_pid = shell_worker["pid"]
        assert shell_pid != caller
        assert os.tcgetpgrp(master) == shell_pid
        os.kill(shell_pid, signal.SIGSTOP)
        started = time.monotonic()
        if force:
            result = subprocess.run(
                [str(cli), "watchdog", "kill", "--run", shell_worker["worker"]["run"], "--shell"],
                env=env, capture_output=True, timeout=5,
            )
            assert result.returncode == 0 and result.stdout, (
                result.stderr, time.monotonic() - started, processes(cli, env), workers,
            )
        else:
            safety_probe = started + 0.5
            while time.monotonic() < safety_probe:
                read_terminal(master, output)
            assert any(worker["pid"] == shell_pid for worker in processes(cli, env))
            assert not terminal_restored(master, slave, before, caller)
            while time.monotonic() - started < 5.0:
                read_terminal(master, output)
                if terminal_restored(master, slave, before, caller):
                    break
            assert terminal_restored(master, slave, before, caller), (
                "watchdog did not restore the tty in 5 seconds",
                processes(cli, env), (caller, before), tty_diagnostic(master, slave), output[-400:],
            )

        while time.monotonic() - started < (3.0 if force else 7.5):
            read_terminal(master, output)
            if b"GUARDIAN_STATUS=" in output:
                break
        assert b"GUARDIAN_STATUS=1" in output, output[-4000:].decode(errors="replace")
        assert terminal_restored(master, slave, before, caller)
        assert b"\x1b[?1049l" in output and b"\x1b[?25h" in output
        assert not processes(cli, env)
        for worker in workers:
            assert_gone(worker["pid"])
        verified = True
    finally:
        if not verified:
            for pid in [worker["pid"] for worker in workers]:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        os.close(master)
        os.close(slave)
        os.close(completion_output)
        if outcome is None:
            os.waitpid(caller, 0)


def main():
    shell = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/tundra-shell").resolve()
    cli = shell.with_name("tundra-cli")
    with tempfile.TemporaryDirectory(prefix="tux-watchdog-") as folder:
        root = Path(folder).resolve()
        (root / ".Trash").mkdir()
        env = dict(os.environ, HOME=str(root), TERM="xterm-256color")
        for variable in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR"):
            path = root / variable.lower()
            path.mkdir(mode=0o700)
            env[variable] = str(path)
        run(shell, cli, env, force=False)
        print("PASS: stopped Shell restored the terminal within 5 seconds and cleaned up its run")
        run(shell, cli, env, force=True)
        print("PASS: independent watchdog CLI killed a stopped Shell and cleaned up its run")


if __name__ == "__main__":
    main()
