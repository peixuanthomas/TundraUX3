#!/usr/bin/env python3
"""Run as root in a disposable Linux test environment, with both binary paths.

Uses the side-effect-free update probe to exercise the real executable entry
points. All XDG paths point into a temporary directory. No system files change.
Example: sudo python3 scripts/tests/linux-root-startup.py \
target/debug/tundra-shell target/debug/tundra-cli
"""
import fcntl
import os
from pathlib import Path
import pty
import select
import shutil
import subprocess
import sys
import tempfile
import termios
import time


assert sys.platform == "linux" and os.getuid() == os.geteuid() == 0
assert len(sys.argv) == 3, "Pass tundra-shell and tundra-cli paths"
PROMPT = b"any other key cancels: "


def terminal_case(binary, answer, expected_code, env):
    master, slave = pty.openpty()
    original = termios.tcgetattr(slave)

    def session():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    process = subprocess.Popen(
        [binary, "__update-probe"], stdin=slave, stdout=slave, stderr=slave,
        env=env, preexec_fn=session,
    )
    output = bytearray()
    sent = False
    deadline = time.monotonic() + 10
    try:
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.05)[0]:
                output.extend(os.read(master, 65536))
            if PROMPT in output and not sent:
                assert process.poll() is None, output
                assert b"protocol=" not in output, output
                assert not any(Path(env["XDG_CONFIG_HOME"]).parent.iterdir())
                os.write(master, answer)
                sent = True
            if process.poll() is not None:
                while select.select([master], [], [], 0)[0]:
                    output.extend(os.read(master, 65536))
                break
        assert process.poll() is not None, f"Startup hung: {output!r}"
        assert process.returncode == expected_code, output
        assert sent and b"WARNING:" in output, output
        assert (b"protocol=" in output) == (expected_code == 0), output
        assert termios.tcgetattr(slave) == original, "Terminal mode was not restored"
        assert not any(Path(env["XDG_CONFIG_HOME"]).parent.iterdir())
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()
        os.close(master)
        os.close(slave)


with tempfile.TemporaryDirectory(prefix="tundra-root-startup-") as root, \
        tempfile.TemporaryDirectory(prefix="tundra-root-binaries-") as binaries:
    # Build trees may be below a private home; permit the ordinary-user probe
    # to execute only temporary copies, without changing the build tree's modes.
    os.chmod(binaries, 0o755)
    env = dict(os.environ, TERM="xterm-256color")
    for name in ("CONFIG", "DATA", "CACHE", "STATE", "RUNTIME"):
        env[f"XDG_{name}_HOME" if name != "RUNTIME" else "XDG_RUNTIME_DIR"] = str(
            Path(root) / name.lower()
        )
    for binary_arg in sys.argv[1:]:
        binary = str(Path(binaries) / Path(binary_arg).name)
        shutil.copy2(binary_arg, binary)
        os.chmod(binary, 0o755)
        for answer in (b"n", b"\r", b"\x1b", b"\x03", b"\x04", b"Y"):
            terminal_case(binary, answer, 1, env)
        terminal_case(binary, b"y", 0, env)

        # A pipe containing y must never authorize root execution.
        result = subprocess.run(
            [binary, "__update-probe"], input=b"y\n", capture_output=True,
            env=env, timeout=10,
        )
        assert result.returncode == 1 and b"interactive terminal" in result.stderr
        assert b"protocol=" not in result.stdout

        # No prompt for ordinary users; set-ID identities still fail before it.
        for mismatch in (None, "uid", "gid"):
            def credentials():
                os.setgroups([])
                os.setresgid(65534, 0 if mismatch == "gid" else 65534, 65534)
                os.setresuid(65534, 0 if mismatch == "uid" else 65534, 65534)

            result = subprocess.run(
                [binary, "__update-probe"], capture_output=True, env=env,
                preexec_fn=credentials, timeout=10,
            )
            assert b"WARNING:" not in result.stderr, result
            if mismatch:
                assert result.returncode == 1 and b"Set-ID" in result.stderr, result
            else:
                assert result.returncode == 0 and b"protocol=" in result.stdout, result
        assert not any(Path(root).iterdir())
        print(f"PASS {Path(binary).name}: confirmation, cancellation, terminal restoration, pipes, ordinary user, set-ID")
