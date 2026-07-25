#!/usr/bin/env python3
"""Flood the ready Tundra shell with mouse motion, then stop it cleanly.

This intentionally uses only the Python standard library so Ubuntu CI can verify
keyboard priority, terminal input, and lifecycle without a desktop session or
third-party test harness.
"""

from __future__ import annotations

import fcntl
import os
import pty
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path
from typing import Iterable, Optional

MOUSE_CAPTURE_SEQUENCE = b"\x1b[?1003h"
# The startup animation never renders the final boxed Status panel. Matching
# its UTF-8 border/title survives Ratatui's debug/release diff differences.
SHELL_READY_SEQUENCE = "╭Status".encode()
KEYBOARD_SENTINEL = b" "
# Space safely advances the isolated first-run setup from Language to
# Timezone. It follows the same ordinary character path as Editor typing and,
# unlike debug-only input diagnostics, is visible in release builds too.
KEYBOARD_SENTINEL_SEQUENCE = b"Timezone"
MOUSE_FLOOD_EVENT_COUNT = int(os.environ.get("TUNDRA_PTY_MOUSE_EVENT_COUNT", "64"))
MOUSE_FLOOD_WRITE_TIMEOUT = 8.0
KEYBOARD_SENTINEL_TIMEOUT = float(
    os.environ.get("TUNDRA_PTY_KEYBOARD_TIMEOUT", "0.25")
)
SHELL_READY_TIMEOUT = 20.0
MAX_CAPTURED_OUTPUT_BYTES = 16 * 1024 * 1024
DIAGNOSTIC_OUTPUT_BYTES = 64 * 1024


def append_output(output: bytearray, chunk: bytes) -> None:
    if len(output) + len(chunk) > MAX_CAPTURED_OUTPUT_BYTES:
        raise SystemExit(
            "tundra-shell produced more than "
            f"{MAX_CAPTURED_OUTPUT_BYTES // (1024 * 1024)} MiB during PTY smoke; "
            "this usually indicates an unbounded redraw loop"
        )
    output.extend(chunk)


def output_diagnostic(output: bytearray) -> str:
    return bytes(output[-DIAGNOSTIC_OUTPUT_BYTES:]).decode(errors="replace")


def read_available(fd: int, output: bytearray, timeout: float) -> None:
    readable, _, _ = select.select([fd], [], [], timeout)
    if not readable:
        return
    try:
        chunk = os.read(fd, 65536)
    except OSError:
        return
    if chunk:
        append_output(output, chunk)


def wait_for_output(
    fd: int,
    output: bytearray,
    sequence: bytes,
    child: subprocess.Popen,
    timeout: float,
    start_offset: int = 0,
) -> bool:
    deadline = time.monotonic() + timeout
    while output.find(sequence, start_offset) < 0 and time.monotonic() < deadline:
        if child.poll() is not None:
            return False
        read_available(fd, output, 0.1)
    return output.find(sequence, start_offset) >= 0


def wait_for_output_quiet(
    fd: int,
    output: bytearray,
    child: subprocess.Popen,
    quiet_period: float = 0.05,
    timeout: float = 2.0,
) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if child.poll() is not None:
            return False
        previous_size = len(output)
        read_available(fd, output, quiet_period)
        if len(output) == previous_size:
            return True
    return False


def mouse_motion_events(count: int) -> Iterable[bytes]:
    for index in range(count):
        yield (
            f"\x1b[<35;{index % 140 + 1};{(index // 140) % 40 + 1}M".encode(
                "ascii"
            )
        )


def write_events_while_draining_output(
    fd: int,
    events: Iterable[bytes],
    output: bytearray,
    timeout: float,
) -> float:
    started_at = time.monotonic()
    deadline = started_at + timeout
    for event_index, event in enumerate(events):
        offset = 0
        view = memoryview(event)
        while offset < len(event):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise SystemExit(
                    "tundra-shell did not consume the terminal event stream "
                    f"within {timeout:.1f}s (stalled at event {event_index})"
                )

            readable, writable, _ = select.select(
                [fd], [fd], [], min(0.05, remaining)
            )
            if readable:
                try:
                    chunk = os.read(fd, 65536)
                except BlockingIOError:
                    chunk = b""
                except OSError as error:
                    raise SystemExit(
                        f"could not read shell output during input flood: {error}"
                    ) from error
                if chunk:
                    append_output(output, chunk)
            if writable:
                try:
                    written = os.write(fd, view[offset:])
                except BlockingIOError:
                    continue
                except OSError as error:
                    raise SystemExit(
                        "could not inject terminal event "
                        f"{event_index} at byte {offset}: {error}"
                    ) from error
                offset += written

    return time.monotonic() - started_at


def signal_process_group(child: subprocess.Popen, signal_number: int) -> None:
    if child.poll() is not None:
        return
    try:
        os.killpg(child.pid, signal_number)
    except ProcessLookupError:
        pass


def main() -> int:
    binary = Path(sys.argv[1] if len(sys.argv) == 2 else "target/debug/tundra-shell")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SystemExit(f"shell binary is not executable: {binary}")

    isolated = Path(tempfile.mkdtemp(prefix="tundraux3-pty-"))
    env = os.environ.copy()
    for name in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR"):
        directory = isolated / name.lower()
        directory.mkdir(mode=0o700)
        env[name] = str(directory)

    master, slave = pty.openpty()
    # The real shell enforces its minimum terminal size before entering the
    # session, so give the PTY a realistic desktop-terminal geometry.
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
    terminal_before = termios.tcgetattr(slave)
    os.set_blocking(master, False)
    output = bytearray()
    child: Optional[subprocess.Popen] = None
    flood_duration = 0.0
    sentinel_latency = 0.0
    try:
        child = subprocess.Popen(
            [str(binary.resolve())],
            stdin=slave,
            stdout=slave,
            stderr=slave,
            env=env,
            start_new_session=True,
        )

        if not wait_for_output(
            master,
            output,
            MOUSE_CAPTURE_SEQUENCE,
            child,
            timeout=10.0,
        ):
            raise SystemExit(
                "tundra-shell did not enter all-motion mouse capture; output:\n"
                f"{output_diagnostic(output)}"
            )

        # Mouse capture begins before the first-run animation. Wait for an
        # explicit status line from the real Shell instead of assuming a fixed
        # animation duration, which varies substantially on WSL/NTFS.
        if not wait_for_output(
            master,
            output,
            SHELL_READY_SEQUENCE,
            child,
            SHELL_READY_TIMEOUT,
        ):
            raise SystemExit(
                "tundra-shell did not reach its ready event loop within "
                f"{SHELL_READY_TIMEOUT:.1f}s; output:\n"
                f"{output_diagnostic(output)}"
        )
        if not wait_for_output_quiet(master, output, child):
            raise SystemExit(
                "tundra-shell output did not become idle after the ready frame; "
                f"output:\n{output_diagnostic(output)}"
            )

        sentinel_offset = len(output)
        flood_duration = write_events_while_draining_output(
            master,
            mouse_motion_events(MOUSE_FLOOD_EVENT_COUNT),
            output,
            MOUSE_FLOOD_WRITE_TIMEOUT,
        )
        sentinel_started_at = time.monotonic()
        write_events_while_draining_output(
            master,
            (KEYBOARD_SENTINEL,),
            output,
            KEYBOARD_SENTINEL_TIMEOUT,
        )
        if not wait_for_output(
            master,
            output,
            KEYBOARD_SENTINEL_SEQUENCE,
            child,
            KEYBOARD_SENTINEL_TIMEOUT,
            start_offset=sentinel_offset,
        ):
            raise SystemExit(
                "tundra-shell did not process the keyboard sentinel after the "
                f"{MOUSE_FLOOD_EVENT_COUNT}-event mouse flood within "
                f"{KEYBOARD_SENTINEL_TIMEOUT:.1f}s; output:\n"
                f"{output_diagnostic(output)}"
            )
        sentinel_latency = time.monotonic() - sentinel_started_at

        if child.poll() is None:
            signal_process_group(child, signal.SIGTERM)

        deadline = time.monotonic() + 10.0
        while time.monotonic() < deadline and child.poll() is None:
            read_available(master, output, 0.1)
        read_available(master, output, 0.1)

        if child.poll() is None:
            signal_process_group(child, signal.SIGKILL)
            raise SystemExit("tundra-shell did not exit after SIGTERM")
        if child.returncode != 0:
            raise SystemExit(
                f"tundra-shell exited with {child.returncode}; "
                f"output:\n{output_diagnostic(output)}"
            )

        terminal_after = termios.tcgetattr(slave)
        # Raw mode changes input/output/local flags and control characters.
        # Comparing those fields catches a process that merely printed the
        # escape sequences but left the PTY in raw mode.
        if terminal_after[:4] != terminal_before[:4] or terminal_after[6] != terminal_before[6]:
            raise SystemExit(
                "terminal attributes were not restored after SIGTERM; "
                f"before={terminal_before!r}, after={terminal_after!r}"
            )

        # Crossterm's LeaveAlternateScreen and Show cursor sequences demonstrate
        # that the fullscreen terminal guard was unwound before process exit.
        for sequence, label in (
            (b"\x1b[?1049l", "leave alternate screen"),
            (b"\x1b[?25h", "show cursor"),
            (b"\x1b[?1003l", "disable all-motion mouse capture"),
        ):
            if sequence not in output:
                raise SystemExit(
                    f"missing terminal restore sequence ({label}); "
                    f"output:\n{output_diagnostic(output)}"
                )
    finally:
        if child is not None and child.poll() is None:
            signal_process_group(child, signal.SIGKILL)
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
        if slave >= 0:
            os.close(slave)
        os.close(master)
        shutil.rmtree(isolated, ignore_errors=True)

    print(
        "Linux PTY mouse/keyboard priority smoke passed "
        f"({MOUSE_FLOOD_EVENT_COUNT} queued mouse events before the keyboard sentinel; "
        f"input accepted in {flood_duration:.3f}s; "
        f"sentinel visible in {sentinel_latency:.3f}s)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
