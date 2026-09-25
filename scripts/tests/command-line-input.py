#!/usr/bin/env python3
"""Real-PTY regression for Escape followed by ordinary REPL input (Unix)."""
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import sys
import termios
import time

binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/tundra-cli").resolve()
pid, master = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.environ["TUNDRA_COMMAND_LINE_USERNAME"] = "input-test"
    os.execv(str(binary), [str(binary), "repl", "--embedded"])
fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
output = bytearray()


def wait_for(marker, timeout=5.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if marker in output:
            return
        if select.select([master], [], [], max(0, deadline - time.monotonic()))[0]:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                break
            output.extend(chunk)
            if os.geteuid() == 0 and b"any other key cancels: " in output:
                os.write(master, b"y")
                output.clear()
    raise AssertionError(f"missing {marker!r}: {bytes(output)!r}")


try:
    wait_for(b"input-test >>")
    output.clear()
    os.write(master, b"\x1b")
    time.sleep(0.25)  # A standalone Escape, not an Alt chord.
    started = time.monotonic()
    os.write(master, b"e")
    wait_for(b"e", 0.5)
    latency = time.monotonic() - started
    # Left/right are complete CSI sequences and must still work after setting
    # the Escape timeout. A lost first e leaves 'xit', so exit status verifies it.
    os.write(master, b"\x1b[D\x1b[Cxit\r")
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        completed, status = os.waitpid(pid, os.WNOHANG)
        if completed:
            assert os.waitstatus_to_exitcode(status) == 0, status
            pid = None
            print(f"PASS: first e after Escape echoed in {latency:.3f}s; arrow keys and exit succeeded")
            break
        if select.select([master], [], [], 0.05)[0]:
            try:
                os.read(master, 65536)
            except OSError:
                pass
    else:
        raise AssertionError("exit did not terminate the REPL; a character may have been lost")
finally:
    if pid is not None:
        os.kill(pid, signal.SIGTERM)
        os.waitpid(pid, 0)
    os.close(master)
