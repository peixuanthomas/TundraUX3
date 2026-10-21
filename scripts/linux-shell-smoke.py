#!/usr/bin/env python3
"""Verify mouse flood handling and button releases, then stop the shell cleanly.

This intentionally uses only the Python standard library so Ubuntu CI can verify
keyboard priority, terminal input, and lifecycle without a desktop session or
third-party test harness.
"""

from __future__ import annotations

import fcntl
import json
import os
import pty
import pwd
import re
import select
import shlex
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unicodedata
from pathlib import Path
from typing import Iterable, Optional

MOUSE_CAPTURE_SEQUENCE = b"\x1b[?1003h"
# The startup animation never renders the final boxed Status panel. Matching
# its UTF-8 border/title survives Ratatui's debug/release diff differences.
SHELL_READY_SEQUENCE = "╭Status".encode()
# Type one ordinary character into the existing Appearance color input. Ratatui
# may emit only changed cells, so a multi-character value is not a reliable byte
# marker. Keep this deadline independent of saving preferences and loading Home.
KEYBOARD_SENTINEL = "Ω".encode("utf-8")
KEYBOARD_SENTINEL_SEQUENCE = KEYBOARD_SENTINEL
MOUSE_FLOOD_EVENT_COUNT = int(os.environ.get("TUNDRA_PTY_MOUSE_EVENT_COUNT", "64"))
MOUSE_FLOOD_WRITE_TIMEOUT = 8.0
KEYBOARD_SENTINEL_TIMEOUT = float(
    os.environ.get("TUNDRA_PTY_KEYBOARD_TIMEOUT", "0.25")
)
SHELL_READY_TIMEOUT = 20.0
MAX_CAPTURED_OUTPUT_BYTES = 16 * 1024 * 1024
DIAGNOSTIC_OUTPUT_BYTES = 64 * 1024
CHECK_CRASH_SCREEN = os.environ.get("TUNDRA_PTY_CHECK_CRASH") == "1"


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
    ignore_spaces: bool = False,
) -> bool:
    deadline = time.monotonic() + timeout
    def contains_sequence() -> bool:
        if output.find(sequence, start_offset) >= 0:
            return True
        # Ratatui can switch styles between a border and its title.
        visible = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(output[start_offset:]))
        if ignore_spaces:
            # Ratatui can skip already-blank cells with cursor positioning.
            return sequence.replace(b" ", b"") in visible.replace(b" ", b"")
        return sequence in visible

    while not contains_sequence() and time.monotonic() < deadline:
        if child.poll() is not None:
            return False
        read_available(fd, output, 0.1)
    return contains_sequence()


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


def check_status_details(master: int, output: bytearray, child: subprocess.Popen) -> None:
    # A 40-row Shell keeps its status message at row 39 (SGR is one-based).
    offset = len(output)
    os.write(master, b"\x1b[<0;12;39M")
    read_available(master, output, 0.1)
    if b"Status details" in output[offset:]:
        raise SystemExit("status details opened before mouse release")
    os.write(master, b"\x1b[<0;12;39m")
    if not wait_for_output(master, output, b"Status details", child, 5.0, offset):
        raise SystemExit("status message did not open its details:\n" + output_diagnostic(output[offset:]))
    # Let modal entry finish before testing its keyboard dismissal.
    deadline = time.monotonic() + 0.7
    while time.monotonic() < deadline:
        read_available(master, output, 0.05)
    os.write(master, b"\x1b")
    deadline = time.monotonic() + 0.7
    while time.monotonic() < deadline:
        read_available(master, output, 0.05)


def painted_screen_lines(data: bytes, columns: int, rows: int) -> list[str]:
    """Read ASCII cells from a complete Ratatui repaint and its later diffs."""
    screen = [[" "] * columns for _ in range(rows)]
    x = y = 0
    for token in re.findall(r"\x1b\[[0-?]*[ -/]*[@-~]|[\s\S]", data.decode(errors="replace")):
        if token.startswith("\x1b["):
            command, raw = token[-1], token[2:-1]
            if not re.fullmatch(r"[0-9;]*", raw):
                continue
            values = [int(value or "0") for value in raw.split(";")]
            amount = values[0] or 1
            if command in "Hf":
                y = amount - 1
                x = ((values[1] if len(values) > 1 else 1) or 1) - 1
            elif command == "G":
                x = amount - 1
            elif command == "d":
                y = amount - 1
            elif command in "ABCD":
                x += amount * ((command == "C") - (command == "D"))
                y += amount * ((command == "B") - (command == "A"))
            elif command == "J" and values[0] in (2, 3):
                screen = [[" "] * columns for _ in range(rows)]
            elif command == "K" and 0 <= y < rows:
                start, end = (0, columns) if values[0] == 2 else ((0, x + 1) if values[0] == 1 else (x, columns))
                for column in range(max(0, start), min(columns, end)):
                    screen[y][column] = " "
        elif token == "\r":
            x = 0
        elif token == "\n":
            y += 1
        elif token.isprintable() and not unicodedata.combining(token):
            width = 2 if unicodedata.east_asian_width(token) in "WF" else 1
            if 0 <= y < rows and 0 <= x < columns:
                screen[y][x] = token if width == 1 else " "
                if width == 2 and x + 1 < columns:
                    screen[y][x + 1] = " "
            x += width
    return ["".join(row) for row in screen]


def painted_label_position(data: bytes, label: str, columns: int, rows: int, first_row: int = 0) -> tuple[int, int]:
    """Locate an ASCII label in a complete Ratatui repaint; return SGR coordinates."""
    screen = painted_screen_lines(data, columns, rows)
    for row, cells in enumerate(screen[first_row:], start=first_row):
        column = cells.find(label)
        if column >= 0:
            return column + 1, row + 1
    raise SystemExit(f"Button {label!r} was not painted:\n" + "\n".join(screen))


def wait_for_painted_text(fd, output, label, child, present=True, timeout=5.0):
    deadline = time.monotonic() + timeout
    columns, rows = os.get_terminal_size(fd)
    while True:
        visible = any(label in line for line in painted_screen_lines(output, columns, rows))
        if visible == present:
            return True
        if child.poll() is not None or time.monotonic() >= deadline:
            return False
        read_available(fd, output, 0.1)


def check_management_menu(master: int, slave: int, output: bytearray, child: subprocess.Popen) -> None:
    """Exercise the real Network UI without submitting any system operation."""
    interfaces = json.loads(subprocess.check_output(["ip", "-j", "address", "show"]))
    first_interface = next(
        (entry["ifname"] for entry in interfaces if entry.get("ifname") not in (None, "lo")),
        None,
    )
    if first_interface is None:
        raise SystemExit("management PTY requires a visible network interface")
    offset = len(output)
    # The fixed catalog orders Network eighth; Home removes any previous
    # Command Line selection. This isolated profile has no external entries.
    os.write(master, b"\x1b[H" + b"\x1b[C" * 7 + b"\r")
    if not wait_for_output(master, output, b"More actions", child, 10.0, offset):
        raise SystemExit("Network did not load its operation menu:\n" + output_diagnostic(output[offset:]))
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Network query did not settle")

    # Read the button from the resized frame: common actions can move it.
    repaint_offset = len(output)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 20, 108, 0, 0))
    os.kill(child.pid, signal.SIGWINCH)
    if not wait_for_output(master, output, b"More actions", child, 5.0, repaint_offset, ignore_spaces=True):
        raise SystemExit("Network did not repaint its actions after resizing:\n" + output_diagnostic(output[repaint_offset:]))
    more_x, more_y = painted_label_position(bytes(output[repaint_offset:]), "More actions", 108, 20)
    press_more = f"\x1b[<0;{more_x};{more_y}M".encode()
    release_more = f"\x1b[<0;{more_x};{more_y}m".encode()
    offset = len(output)
    os.write(master, f"\x1b[<35;{more_x};{more_y}M".encode() + press_more)
    read_available(master, output, 0.2)
    if b"Esc Close" in output[offset:]:
        raise SystemExit("More actions opened on mouse-down")
    os.write(master, b"\x1b[<32;108;3M\x1b[<0;108;3m")
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("management drag-out did not settle:\n" + output_diagnostic(output[offset:]))
    if b"Esc Close" in output[offset:]:
        raise SystemExit("dragging out of More actions executed it")
    os.write(master, press_more + release_more)
    if not wait_for_output(master, output, b"Esc Close", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("More actions did not open after release:\n" + output_diagnostic(output[offset:]))
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("management menu did not settle")
    offset = len(output)
    os.write(master, b"n")
    if not wait_for_output(master, output, b"TCP port", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("keyboard menu selection did not open diagnosis fields:\n" + output_diagnostic(output[offset:]))
    # Inspect the painted frame: closing a form need not redraw Search.
    offset = len(output)
    os.write(master, b"\x1b")
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline:
        read_available(master, output, 0.05)
        frame = bytes(output[repaint_offset:])
        try:
            painted_label_position(frame, "Search:", 108, 20)
        except SystemExit:
            continue
        try:
            painted_label_position(frame, "TCP port", 108, 20)
        except SystemExit:
            break
    else:
        raise SystemExit("diagnosis form did not return to the operation page:\n" + output_diagnostic(output[offset:]))
    # Search and clear through the combined input. The query remains read-only.
    offset = len(output)
    os.write(master, b"sunmatched-network-for-pty\r")
    if not wait_for_output(master, output, b"unmatched-network-for-pty", child, 5.0, offset):
        raise SystemExit("management search did not accept keyboard input")
    if not wait_for_output(master, output, b"No matching items", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("management search did not filter network rows")
    offset = len(output)
    os.write(master, b"\x15")
    if not wait_for_output(master, output, first_interface.encode(), child, 10.0, offset, ignore_spaces=True):
        raise SystemExit("clearing the combined search did not restore network rows")
    offset = len(output)
    os.write(master, b"\x1b")
    if not wait_for_output(master, output, b"Launcher", child, 5.0, offset):
        raise SystemExit("Network did not return to Launcher")
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Launcher did not settle after management verification")


def check_package_and_log_views(master: int, slave: int, output: bytearray, child: subprocess.Popen) -> None:
    """Visit read-only package views and the log menu through the real Shell."""
    columns = 140

    def step(keys: bytes, marker: bytes, label: str, timeout: float = 15.0) -> None:
        nonlocal columns
        offset = len(output)
        os.write(master, keys)
        deadline = time.monotonic() + 0.6
        while time.monotonic() < deadline:
            read_available(master, output, 0.05)
        # Force a complete frame: incremental terminal writes can leave the
        # expected title unchanged, especially after dismissing a small menu.
        columns = 139 if columns == 140 else 140
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, columns, 0, 0))
        os.kill(child.pid, signal.SIGWINCH)
        if not wait_for_output(master, output, marker, child, timeout, offset, ignore_spaces=True):
            raise SystemExit(f"{label}:\n" + output_diagnostic(output[offset:]))
        # Let modal entry/exit complete before injecting the next action.
        deadline = time.monotonic() + 0.6
        while time.monotonic() < deadline:
            read_available(master, output, 0.05)

    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
    os.kill(child.pid, signal.SIGWINCH)
    wait_for_output_quiet(master, output, child, quiet_period=0.2)
    # At 140 columns, Packages is directly below Command Line; Up returns to it.
    step(b"\x1b[H\x1b[B\x1b[A\x1b[B\r", b"Installed packages", "Launcher row navigation did not open Packages")
    # Installed rows must return after leaving the full detail page, even
    # when a detail query targeted only the first package.
    if Path("/usr/bin/dpkg-query").is_file():
        installed = subprocess.check_output([
            "/usr/bin/dpkg-query", "-W", "-f=${db:Status-Abbrev} ${Package}\\n"
        ], env={**os.environ, "LC_ALL": "C.UTF-8"}).decode().splitlines()
        names = sorted({line[4:] for line in installed if line.startswith("ii ")})
        if len(names) >= 2:
            if not wait_for_output(master, output, names[1].encode(), child, 15.0):
                raise SystemExit("Installed package list did not load")
            step(b"\x1bOS", names[0].encode(), "Package detail did not load")
            step(b"\x1b", names[1].encode(), "Escape from details did not restore package rows")
    for key, title in ((b"\x1b[17~", b"Search packages"),
                       (b"\x1b[19~", b"Available updates"),
                       (b"\x1b[18~", b"Installed packages")):
        step(key, title, "Package scope shortcut did not switch views", timeout=45.0)
        if key == b"\x1b[17~" and Path("/usr/bin/dpkg-query").is_file():
            step(b"GNU Bourne Again\r", b"Bourne Again", "Package search did not load matching metadata", timeout=45.0)
    if Path("/usr/bin/dpkg-query").is_file() and len(names) >= 2:
        step(b"\x15", names[1].encode(), "Clearing package search did not restore installed rows")
    # Each main page owns its advanced entry. F3, Down and Enter reach it
    # without a pointer; these navigation steps never submit system writes.
    for root, title in ((b"\x1b[17~", b"Software sources"), (b"\x1b[18~", b"Configuration conflicts"), (b"\x1b[19~", b"Package status")):
        step(root + b"\x1bOR", b"Advanced", "Package advanced section did not open")
        step(b"\x1b[B\r", title,
             "Package auxiliary view did not open", timeout=45.0)
        step(b"\x1b[18~", b"Installed packages", "Auxiliary view could not return to packages")
    step(b"\x1b", b"Launcher \xc2\xb7 Large icons", "Packages did not return to Launcher")
    step(b"\x1b[H" + b"\x1b[C" * 3 + b"\r", b"UX log", "Logs did not open")
    step(b"f", b"Service scope", "Visible log filter shortcut did not open its form")
    step(b"\x1b", b"UX events", "Log filter cancellation did not restore its page")
    step(b"\x1b[21~", b"L Level", "Log More actions did not open")
    # End selects the explicit close action, and Space activates it.
    step(b"\x1b[F ", b"UX events", "Log menu close did not restore its page")
    step(b"\x1b[21~", b"L Level", "Log menu did not reopen")
    # Clicking outside the centered menu closes it without opening a covered
    # category or toolbar control. The layout tests check exact bounds/colors.
    step(b"\x1b[<0;3;6M\x1b[<0;3;6m", b"UX log", "Log menu outside click did not close it")
    step(b"\x1b", b"Launcher \xc2\xb7 Large icons", "Logs did not return to Launcher")
    print("PASS: all six package views, detail return, visible log filters, and log menu keyboard/outside-click dismissal")


def check_keyboard_paste(master: int, output: bytearray, child: subprocess.Popen, start: int) -> None:
    """The isolated child has no desktop clipboard; failed reads must be harmless."""
    def click(label: str) -> None:
        x, y = painted_label_position(bytes(output[start:]), label, 140, 40)
        os.write(master, f"\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m".encode())
        deadline = time.monotonic() + 0.7
        while time.monotonic() < deadline:
            read_available(master, output, 0.05)

    click("Show keyboard")
    # Send the next clicks in one burst, including held movement inside each
    # keycap. The real CLI must receive every letter, including the repeated s.
    positions = {
        letter: painted_label_position(bytes(output[start:]), f" {letter} ", 140, 40, first_row=20)
        for letter in "asd"
    }
    os.write(master, b"printf 'RAPID_%s\\n' ")
    reports = bytearray()
    for letter in "assd":
        x, y = positions[letter]
        x += 1  # Move from the surrounding space to the key's legend.
        reports.extend(
            f"\x1b[<0;{x};{y}M\x1b[<32;{x};{y}M"
            f"\x1b[<32;{x + 1};{y}M\x1b[<0;{x + 1};{y}m".encode()
        )
    offset = len(output)
    os.write(master, reports + b"\r")
    if not wait_for_output(master, output, b"RAPID_assd", child, 5.0, offset):
        raise SystemExit("rapid keyboard clicks lost letters:\n" + output_diagnostic(output[offset:]))
    if not wait_for_output(master, output, b"[system exit code: 0]", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("rapid keyboard command did not complete")
    for _ in range(3):
        click("Paste")
    click("Hide keyboard")
    if not wait_for_output(master, output, b"Clipboard unavailable", child, 5.0, start, ignore_spaces=True):
        raise SystemExit("failed paste did not show a clipboard message:\n" + output_diagnostic(output[start:]))
    offset = len(output)
    os.write(master, b"printf 'PASTE_%s\\n' ALIVE\r")
    if not wait_for_output(master, output, b"PASTE_ALIVE", child, 5.0, offset):
        raise SystemExit("terminal stopped accepting commands after Paste")
    if not wait_for_output(master, output, b"[system exit code: 0]", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("command after Paste did not complete")
    print("PASS: rapid screen-keyboard clicks with small held movements, repeated unavailable-clipboard clicks and subsequent terminal command")


def check_crash_screen(master: int, output: bytearray, child: subprocess.Popen, isolated: Path) -> None:
    # The existing final check leaves the power dialog open on Home.
    os.write(master, b"\x1b")
    wait_for_output_quiet(master, output, child, quiet_period=0.3)
    offset = len(output)
    os.write(master, b"a")
    if not wait_for_output(master, output, b"Command Line", child, 5.0, offset):
        raise SystemExit("could not open Launcher for crash-page test")
    wait_for_output_quiet(master, output, child, quiet_period=0.3)
    offset = len(output)
    os.write(master, b"\r")
    if not wait_for_output(master, output, b" >>", child, 10.0, offset, ignore_spaces=True):
        raise SystemExit("embedded CLI did not start for crash-page test")
    offset = len(output)
    os.write(master, b"/debug test-watchdog-panic\r")
    for marker in (b"PANIC", b"Reason:", b"Version:", b"Component:", b"Location:", b"Full report:", b"R: Restart | Q: Exit"):
        if not wait_for_output(master, output, marker, child, 8.0, offset, ignore_spaces=True):
            raise SystemExit(f"crash page omitted {marker!r}:\n" + output_diagnostic(output[offset:]))
    reports = list(isolated.rglob("crash-*.json"))
    if len(reports) != 1:
        raise SystemExit(f"expected one intentional crash report, got {reports}")
    report = json.loads(reports[0].read_text())
    if report.get("panic", {}).get("payload") != "Intentional watchdog panic test requested from Command Line":
        raise SystemExit(f"unexpected crash: {report}")
    if not report["panic"]["source_line"] or not reports[0].with_suffix(".txt").is_file():
        raise SystemExit("crash page had no saved source location/text report")
    if b"Backtrace:" in output[offset:]:
        raise SystemExit("crash page displayed the full backtrace")
    os.write(master, b"q")
    deadline = time.monotonic() + 5.0
    while child.poll() is None and time.monotonic() < deadline:
        read_available(master, output, 0.05)
    if child.poll() is None:
        raise SystemExit("Q did not exit the crash page")
    print("PASS: crash reason/version/component/location/report, Q exit and saved report")


def check_permission_draft(master, slave, output, child, path):
    original = path.stat()
    offset = len(output)
    os.write(master, b"\x1b[21;2~")
    if not wait_for_output(master, output, b"Properties", child, 5.0, offset):
        raise SystemExit("Explorer context menu did not offer Properties")
    wait_for_output_quiet(master, output, child, quiet_period=0.3)
    offset = len(output)
    # The text-file menu starts with Open, Cut, Copy, Rename, Delete, Properties.
    os.write(master, b"\x1b[B" * 5 + b"\r")
    if not wait_for_output(master, output, b"Edit permissions and ownership", child, 5.0, offset):
        raise SystemExit("file Properties did not offer permissions editing:\n" + output_diagnostic(output[offset:]))
    wait_for_output_quiet(master, output, child, quiet_period=0.3)
    offset = len(output)
    os.write(master, b"\r")
    if not wait_for_output(master, output, b"Permissions and ownership", child, 5.0, offset):
        raise SystemExit("Properties did not open the permissions page")
    wait_for_output_quiet(master, output, child, quiet_period=0.3)
    offset = len(output)
    os.write(master, b"1")
    if not wait_for_output(master, output, b"Left/Right", child, 5.0, offset):
        raise SystemExit("permission draft form did not open")

    # Resize to obtain a complete repaint before reading incremental cell changes.
    repaint = len(output)
    for columns in (117, 116):
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 41, columns, 0, 0))
        os.kill(child.pid, signal.SIGWINCH)
        wait_for_output_quiet(master, output, child, quiet_period=0.3)

    def screen_after(keys):
        if keys:
            os.write(master, keys)
        deadline = time.monotonic() + 0.75
        while time.monotonic() < deadline:
            if child.poll() is not None:
                raise SystemExit("shell exited during permission draft regression")
            read_available(master, output, 0.05)
        return painted_screen_lines(bytes(output[repaint:]), 116, 41)

    screen = screen_after(b"")
    owner_row = next((i for i, line in enumerate(screen) if "Owner" in line and "[x]" in line), None)
    if owner_row is None:
        raise SystemExit("permission Owner row was not painted:\n" + "\n".join(screen))
    column = screen[owner_row].index("[x]")
    if screen_after(b" ")[owner_row][column:column + 3] != "[ ]":
        raise SystemExit("physical Space did not clear the selected permission box")
    if screen_after(b"\r")[owner_row][column:column + 3] != "[x]":
        raise SystemExit("Enter did not restore the selected permission box")
    screen = screen_after(b"\t\t")
    recursive_row = next((i + 1 for i, line in enumerate(screen[:-1]) if "Include directory contents" in line), None)
    if recursive_row is None or "[ ]" not in screen[recursive_row]:
        raise SystemExit("recursive checkbox was not painted unchecked:\n" + "\n".join(screen))
    column = screen[recursive_row].index("[ ]")
    if screen_after(b" ")[recursive_row][column:column + 3] != "[x]":
        raise SystemExit("physical Space did not enable recursive draft selection")
    offset = len(output)
    os.write(master, b"\x1b")
    if not wait_for_output(master, output, b"Discard unsaved changes?", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("changed permission draft did not request discard confirmation")
    if screen_after(b"\x1b")[recursive_row][column:column + 3] != "[x]":
        raise SystemExit("cancelling discard lost the permission draft")
    screen_after(b"\x1b")
    screen_after(b"\r")
    offset = len(output)
    os.write(master, b"\x1b")
    if not wait_for_painted_text(master, output, "Quick access", child):
        raise SystemExit("discarded permission draft did not return to Explorer")
    current = path.stat()
    if (current.st_mode, current.st_uid, current.st_gid, current.st_ctime_ns) != (
        original.st_mode, original.st_uid, original.st_gid, original.st_ctime_ns
    ):
        raise SystemExit("permission draft changed filesystem metadata without applying")
    print("PASS: physical Space and Enter toggle permission drafts; cancel preserves draft and discard leaves metadata unchanged")


def check_animation_settings(master, output, child):
    os.write(master, b"s")
    if not wait_for_painted_text(master, output, "Animation speed", child):
        raise SystemExit("Settings did not paint the animation percentage")
    os.write(master, b"\x1b[B" * 5 + b"\x1b[C\x1b[B\r")
    if not wait_for_painted_text(master, output, "Acceleration", child):
        raise SystemExit("Animation advanced options did not open as a dialog")
    os.write(master, b"\x1b[C")
    if not wait_for_painted_text(master, output, "56 ms", child):
        raise SystemExit("Page acceleration did not increase by one millisecond")
    os.write(master, b"\x1b[F\x1b[A\r\x1b")
    wait_for_output_quiet(master, output, child, quiet_period=0.2)
    os.write(master, b"\x1b[H" + b"\x1b[B" * 5)
    if not wait_for_painted_text(master, output, "100%", child):
        raise SystemExit("Restore animation defaults did not restore the percentage")
    os.write(master, b"\x1b[B\r")
    if not wait_for_painted_text(master, output, "55 ms", child):
        raise SystemExit("Restore animation defaults did not restore phase times")
    os.write(master, b"\x1b")
    wait_for_output_quiet(master, output, child, quiet_period=0.2)
    os.write(master, b"\x1b")
    if not wait_for_painted_text(master, output, "Launcher", child):
        raise SystemExit("Animation settings did not return to Home")
    wait_for_output_quiet(master, output, child, quiet_period=0.2)
    print("PASS: animation advanced options adjust milliseconds and restore phase times and percentage")


def check_system_settings_environment(master, output, child):
    if Path("/proc/1/comm").read_text().strip() == "systemd":
        return
    offset = len(output)
    os.write(master, b"s")
    if not wait_for_output(master, output, b"OS settings", child, 5.0, offset):
        raise SystemExit("Settings did not expose the operating system entry")
    wait_for_output_quiet(master, output, child, quiet_period=0.3)
    offset = len(output)
    os.write(master, b"\x1b[19~")
    if not wait_for_output(master, output, b"This environment does not support the feature", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("OS settings did not explain the unavailable systemd environment:\n" + output_diagnostic(output[offset:]))
    wait_for_output_quiet(master, output, child, quiet_period=0.3)
    offset = len(output)
    os.write(master, b"\x1bOS")
    if not wait_for_output(master, output, b"PID 1", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("OS settings F4 details omitted the specific systemd cause:\n" + output_diagnostic(output[offset:]))
    for marker in [b"systemd", b"extra permissions cannot"]:
        if not wait_for_output(master, output, marker, child, 5.0, offset, ignore_spaces=True):
            raise SystemExit(f"OS settings details omitted {marker!r}:\n" + output_diagnostic(output[offset:]))
    os.write(master, b"\x1b")
    wait_for_output_quiet(master, output, child, quiet_period=0.3)
    for marker in [b"OS settings", b"Launcher"]:
        wait_for_output_quiet(master, output, child, quiet_period=0.3)
        offset = len(output)
        os.write(master, b"\x1b")
        if not wait_for_output(master, output, marker, child, 5.0, offset, ignore_spaces=True):
            raise SystemExit("OS settings did not return through its original navigation path")
    print("PASS: unsupported OS settings shows an environment limit and F4 explains systemd requirements without authorization")


def check_editor_resize(master, slave, output, child, isolated):
    directory = isolated / "resize-workflow"
    directory.mkdir()
    path = directory / "notes.txt"
    original = "resize baseline\n"
    addition = " resize regression unsaved"
    path.write_text(original)

    offset = len(output)
    os.write(master, b"e")
    if not wait_for_painted_text(master, output, "Quick access", child):
        raise SystemExit("Explorer did not open for the resize regression")
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Explorer did not settle before the resize regression")
    offset = len(output)
    os.write(master, b"\x0c")
    if not wait_for_painted_text(master, output, "Absolute path", child):
        raise SystemExit("Explorer address input did not open for the resize regression")
    os.write(master, b"\x01\x1b[200~" + str(directory).encode() + b"\x1b[201~\r")
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Explorer did not settle at the resize fixture")
    offset = len(output)
    os.write(master, b"\r")
    if not wait_for_output(master, output, b"resize baseline", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("Editor did not open the resize fixture")
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Editor did not settle before typing")
    for keys, marker in [
        (b"\x1b[19~", b"Normal"),
        (b"ggi\x1b[200~Vim regression \x1b[201~", b"Insert"),
        (b"\x1b", b"Normal"),
        (b"u\x1b[19~", b"Off"),
    ]:
        offset = len(output)
        os.write(master, keys)
        if not wait_for_output(master, output, marker, child, 5.0, offset):
            raise SystemExit("Vim mode did not show " + repr(marker) + ":\n" + output_diagnostic(output[offset:]))
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Editor did not settle after the Vim input")
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 41, 116, 0, 0))
    os.kill(child.pid, signal.SIGWINCH)
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Editor did not settle at 116x41")
    os.write(master, b"\x1b[1;5F\x1b[200~" + addition.encode() + b"\x1b[201~")
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Editor did not accept the unsaved resize text")
    if path.read_text() != original:
        raise SystemExit("resize fixture was saved before requesting a save")
    offset = len(output)
    os.write(master, b"\x1b")
    if not wait_for_output(master, output, b"Unsaved document", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("dirty Editor did not show its close confirmation")

    offset = len(output)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    os.kill(child.pid, signal.SIGWINCH)
    if not wait_for_output(master, output, b"Terminal is too small (80x24)", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("small-terminal overlay did not render")
    os.write(master, b"UNEXPECTED_SMALL_TERMINAL_INPUT\r")
    deadline = time.monotonic() + 0.5
    while time.monotonic() < deadline:
        read_available(master, output, 0.05)
    if child.poll() is not None:
        raise SystemExit("shrinking the terminal exited the shell")
    if path.read_text() != original:
        raise SystemExit("small-terminal input submitted the hidden save dialog")

    offset = len(output)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 41, 116, 0, 0))
    os.kill(child.pid, signal.SIGWINCH)
    if not wait_for_output(master, output, b"Unsaved document", child, 5.0, offset, ignore_spaces=True):
        raise SystemExit("restoring the terminal lost the active close confirmation")
    deadline = time.monotonic() + 0.75
    while time.monotonic() < deadline:
        read_available(master, output, 0.05)
    os.write(master, b"\x1b")
    deadline = time.monotonic() + 0.75
    while time.monotonic() < deadline:
        read_available(master, output, 0.05)
    os.write(master, b"\x13")
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline and path.read_text() != original + addition:
        if child.poll() is not None:
            raise SystemExit("shell exited before saving the preserved edit")
        read_available(master, output, 0.05)
    if path.read_text() != original + addition:
        raise SystemExit(
            f"resize save mismatch: expected {original + addition!r}, got {path.read_text()!r}; output:\n"
            + output_diagnostic(output[offset:])
        )
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Editor did not settle after saving the preserved edit")
    offset = len(output)
    os.write(master, b"\x1b")
    if not wait_for_painted_text(master, output, "Quick access", child):
        raise SystemExit("saved Editor did not return to Explorer")
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Explorer did not settle after the editor save")
    offset = len(output)
    os.write(master, b"\x1bOQ")
    if not wait_for_output(master, output, b"Rename", child, 5.0, offset):
        raise SystemExit("Explorer rename did not open")
    deadline = time.monotonic() + 0.75
    while time.monotonic() < deadline:
        read_available(master, output, 0.05)
    os.write(master, b"\x01renamed.txt\r")
    renamed = directory / "renamed.txt"
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline and not renamed.exists():
        read_available(master, output, 0.05)
    if path.exists() or not renamed.exists() or renamed.read_text() != original + addition:
        raise SystemExit(
            f"Ctrl+A rename did not replace the complete file name: {list(directory.iterdir())}; output:\n"
            + output_diagnostic(output[offset:])
        )
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Explorer did not settle after renaming")
    check_permission_draft(master, slave, output, child, renamed)
    offset = len(output)
    os.write(master, b"\x1b")
    if not wait_for_output(master, output, b"Launcher", child, 5.0, offset):
        raise SystemExit("resize workflow did not return to Home")
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
    os.kill(child.pid, signal.SIGWINCH)
    if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
        raise SystemExit("Home did not settle after the resize workflow")
    print("PASS: 116x41 to 80x24 preserves unsaved text and dialog; restore, save and Ctrl+A rename succeed")


def main() -> int:
    binary = Path(sys.argv[1] if len(sys.argv) == 2 else "target/debug/tundra-shell")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SystemExit(f"shell binary is not executable: {binary}")

    isolated = Path(tempfile.mkdtemp(prefix="tundraux3-pty-"))
    env = os.environ.copy()
    if CHECK_CRASH_SCREEN:
        # Exercise a failed clipboard read without touching the user's clipboard.
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)
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

        if os.geteuid() == 0:
            if not wait_for_output(
                master, output, b"any other key cancels: ", child, timeout=5.0
            ):
                raise SystemExit("root startup confirmation did not appear")
            if MOUSE_CAPTURE_SEQUENCE in output or b"\x1b[?1049h" in output:
                raise SystemExit("root entered the TUI before confirmation")
            os.write(master, b"y")

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

        # A bare PTY has no graphics responder. Acknowledge the real warning
        # before testing Appearance keyboard navigation.
        if b"No terminal graphics response" in output:
            os.write(master, b"\r")
            if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
                raise SystemExit("shell did not settle after the graphics warning")
            # A quiet gap can occur before the close animation starts. Do not
            # send Language's Enter while the warning still owns keyboard input.
            deadline = time.monotonic() + 0.8
            while time.monotonic() < deadline:
                read_available(master, output, 0.05)

        # Linux onboarding uses the system timezone and skips account creation.
        # Match page-specific controls: incremental rendering can split the
        # step title into multiple cursor writes when letters are unchanged.
        for next_page, marker in (("Appearance", b"Frame shape"),):
            page_offset = len(output)
            os.write(master, b"\r")
            if not wait_for_output(master, output, marker, child, 5.0, start_offset=page_offset):
                raise SystemExit(
                    f"setup did not reach {next_page}; output:\n"
                    f"{output_diagnostic(output[page_offset:])}"
                )
            if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
                raise SystemExit("setup page did not settle")

        os.write(master, b"\t\t\r")
        if not wait_for_painted_text(master, output, "Custom theme color", child):
            raise SystemExit("Appearance color input did not open")
        os.write(master, b"\x7f" * 32)
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Appearance color input did not settle")
        if KEYBOARD_SENTINEL_SEQUENCE in output:
            raise SystemExit("keyboard sentinel unexpectedly appears in the initial frame")

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

        # Cancel the temporary color, then complete the real first-use workflow.
        # Saving a profile is a separate assertion, not an input dispatch timer.
        os.write(master, b"\x1b")
        # Instant transitions can reuse title cells instead of re-emitting a line.
        # Check the painted dialog rather than requiring contiguous output bytes.
        if not wait_for_painted_text(
            master, output, "Custom theme color", child, present=False
        ):
            raise SystemExit("Appearance color input did not return to its control")
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Appearance color input did not close")
        home_offset = len(output)
        os.write(master, b"\x1b[21~")
        if not wait_for_output(
            master, output, b"Explorer", child, 10.0, start_offset=home_offset
        ):
            raise SystemExit(
                "Appearance setup did not finish at Home; output:\n"
                f"{output_diagnostic(output[home_offset:])}"
            )

        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Home did not settle before pointer regression")

        check_animation_settings(master, output, child)
        check_editor_resize(master, slave, output, child, isolated)
        check_system_settings_environment(master, output, child)
        check_status_details(master, output, child)

        # Exercise the companion CLI launched by the real Shell, not just a
        # standalone REPL. Rebuilding Shell alone can leave an older CLI beside it.
        launcher_offset = len(output)
        os.write(master, b"a")
        if not wait_for_output(master, output, b"Command Line", child, 5.0, launcher_offset):
            raise SystemExit("Launcher did not show Command Line")
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Launcher did not settle")
        prompt_offset = len(output)
        os.write(master, b"\r")
        username = pwd.getpwuid(os.geteuid()).pw_name
        if not wait_for_output(master, output, f"{username}@/".encode(), child, 10.0, prompt_offset):
            raise SystemExit("embedded CLI did not show an absolute-path prompt; rebuild both shell and cli")
        # Command Line's live cursor can keep painting. Drain through the entry
        # animation rather than requiring the entire terminal to become silent.
        entry_deadline = time.monotonic() + 1.0
        while time.monotonic() < entry_deadline:
            read_available(master, output, 0.1)
        if "○".encode() not in output[prompt_offset:]:
            raise SystemExit("embedded CLI did not render a pending command marker")
        if CHECK_CRASH_SCREEN:
            check_keyboard_paste(master, output, child, prompt_offset)
        check_status_details(master, output, child)
        command_directory = isolated / "command path 中文 $; ' [x]"
        command_directory.mkdir()
        os.write(master, ("cd " + shlex.quote(str(command_directory)) + "\r").encode())
        if not wait_for_output(master, output, b"[system exit code: 0]", child, 5.0, prompt_offset, ignore_spaces=True):
            raise SystemExit("embedded CLI could not change directory:\n" + output_diagnostic(output[prompt_offset:]))
        if not wait_for_output(master, output, "●".encode(), child, 5.0, prompt_offset):
            raise SystemExit("successful cd did not render a completed command marker")
        help_offset = len(output)
        os.write(master, b"/help\r")
        if not wait_for_output(master, output, b"Usage: tundra-cli", child, 5.0, help_offset, ignore_spaces=True):
            raise SystemExit("embedded CLI did not run the prefixed UX help command")
        failure_offset = len(output)
        os.write(master, b"invalid-command\r")
        if not wait_for_output(master, output, "×".encode(), child, 5.0, failure_offset):
            raise SystemExit("invalid command did not render a failure marker")
        # Resize to get a full frame: incremental updates can omit the unchanged
        # username and path prefix, so raw output alone cannot verify that prompt.
        prompt_offset = len(output)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 139, 0, 0))
        os.kill(child.pid, signal.SIGWINCH)
        expected_prompt = f"{username}@{command_directory} >>".encode()
        if not wait_for_output(master, output, expected_prompt, child, 5.0, prompt_offset, ignore_spaces=True):
            raise SystemExit("embedded CLI did not display the new absolute path after cd:\n" + output_diagnostic(output[prompt_offset:]))
        # A full frame also contains old prompts in the scrollback. Let the
        # child finish handling SIGWINCH before sending the next command.
        resize_deadline = time.monotonic() + 0.5
        while time.monotonic() < resize_deadline:
            read_available(master, output, 0.05)
        launcher_offset = len(output)
        os.write(master, b"exit\r")
        if not wait_for_output(master, output, b"Editor", child, 5.0, launcher_offset):
            raise SystemExit("embedded CLI did not return to Launcher:\n" + output_diagnostic(output[launcher_offset:]))
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Launcher did not settle after CLI exit:\n" + output_diagnostic(output[launcher_offset:]))
        check_management_menu(master, slave, output, child)
        check_package_and_log_views(master, slave, output, child)
        home_offset = len(output)
        os.write(master, b"\x1b")
        if not wait_for_output(master, output, b"Explorer", child, 5.0, home_offset):
            raise SystemExit("Launcher did not return to Home")
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
        os.kill(child.pid, signal.SIGWINCH)
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Home did not settle after CLI verification")

        # Open the same exact directory through Explorer's blank-space menu.
        # Quotes, dollar signs and semicolons must stay part of the directory
        # name: this entry sets the child cwd rather than typing a cd command.
        explorer_offset = len(output)
        os.write(master, b"e")
        if not wait_for_painted_text(master, output, "Quick access", child):
            raise SystemExit("Explorer did not open before terminal directory verification")
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Explorer did not settle before editing its directory")
        address_offset = len(output)
        os.write(master, b"\x0c")
        if not wait_for_output(master, output, b"Absolute path", child, 5.0, address_offset, ignore_spaces=True):
            raise SystemExit("Explorer address editor did not open:\n" + output_diagnostic(output[address_offset:]))
        os.write(master, b"\x01\x1b[200~" + str(command_directory).encode() + b"\x1b[201~\r")
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Explorer did not settle at the requested directory")
        menu_offset = len(output)
        os.write(master, b"\x1b[<2;80;20M")
        if not wait_for_painted_text(master, output, "Open Terminal Here", child):
            raise SystemExit("Explorer blank-space menu did not offer Open Terminal Here:\n" + output_diagnostic(output[menu_offset:]))
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Explorer context menu did not settle")
        prompt_offset = len(output)
        os.write(master, b"\x1b[B\x1b[B\x1b[B\r")
        if not wait_for_output(master, output, f"{username}@/".encode(), child, 10.0, prompt_offset):
            raise SystemExit("Explorer terminal entry did not launch the embedded CLI")
        entry_deadline = time.monotonic() + 1.0
        while time.monotonic() < entry_deadline:
            read_available(master, output, 0.1)
        prompt_offset = len(output)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 139, 0, 0))
        os.kill(child.pid, signal.SIGWINCH)
        if not wait_for_output(master, output, expected_prompt, child, 5.0, prompt_offset, ignore_spaces=True):
            raise SystemExit("Explorer terminal did not start in the exact directory:\n" + output_diagnostic(output[prompt_offset:]))
        resize_deadline = time.monotonic() + 0.5
        while time.monotonic() < resize_deadline:
            read_available(master, output, 0.05)
        cwd_offset = len(output)
        os.write(master, b"printf 'EXPLORER_CWD=%s\\n' \"$PWD\"\r")
        expected_cwd = f"EXPLORER_CWD={command_directory}".encode()
        if not wait_for_output(master, output, expected_cwd, child, 5.0, cwd_offset, ignore_spaces=True):
            raise SystemExit("Explorer terminal child had the wrong working directory:\n" + output_diagnostic(output[cwd_offset:]))
        explorer_offset = len(output)
        os.write(master, b"exit\r")
        if not wait_for_painted_text(master, output, "Quick access", child):
            raise SystemExit("Explorer terminal did not return to Explorer")
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Explorer did not settle after terminal exit")
        home_offset = len(output)
        os.write(master, b"\x1b")
        if not wait_for_output(master, output, b"Launcher", child, 5.0, home_offset):
            raise SystemExit("Explorer did not return to Home after terminal verification")
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
        os.kill(child.pid, signal.SIGWINCH)
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Home did not settle after Explorer terminal verification")

        # Reproduce Escape and an SGR report in the same terminal read. The
        # report's final M must never become Home's System Status shortcut.
        for report in (b"\x1b[<35;45;12M", b"\x1b[<35;90;22M"):
            explorer_offset = len(output)
            os.write(master, b"e")
            if not wait_for_painted_text(master, output, "Quick access", child):
                raise SystemExit("Explorer did not open before Escape/mouse regression")
            if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
                raise SystemExit("Explorer did not settle")
            home_offset = len(output)
            os.write(master, b"\x1b" + report)
            if not wait_for_output(master, output, b"Launcher", child, 5.0, home_offset):
                raise SystemExit("Escape/mouse did not return to Home")
            if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
                raise SystemExit("Escape/mouse did not settle at Home")
            if b"Dashboard" in output[home_offset:]:
                raise SystemExit("mouse report tail activated System Status")

        invalid_offset = len(output)
        # Allow an incomplete report to expire, then deliver its late M. Also
        # inject a zero coordinate and an extra field. None may activate Home.
        os.write(master, b"\x1b[<35;45;")
        deadline = time.monotonic() + 0.5
        while time.monotonic() < deadline:
            read_available(master, output, 0.05)
        os.write(master, b"12M\x1b[<35;0;12M\x1b[<35;45;12;1M")
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("invalid input did not settle")
        if b"Dashboard" in output[invalid_offset:]:
            raise SystemExit("invalid terminal input activated System Status")

        # A real shortcut still works immediately after discarded reports.
        status_offset = len(output)
        os.write(master, b"m")
        if not wait_for_output(master, output, b"Dashboard", child, 5.0, status_offset):
            raise SystemExit("valid System Status shortcut was lost after filtering")
        # Live metrics redraw continuously; allow the page entrance animation
        # to finish while draining output instead of requiring a quiet screen.
        deadline = time.monotonic() + 0.5
        while time.monotonic() < deadline:
            read_available(master, output, 0.05)
        home_offset = len(output)
        os.write(master, b"\x1b")
        if not wait_for_output(master, output, b"Launcher", child, 5.0, home_offset):
            raise SystemExit("System Status did not return to Home")
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("Home did not settle after input validation")

        # The 140-column frame places the shared Back button at x=133..139.
        # SGR coordinates are one-based. Hover/hold must only repaint the button;
        # the power dialog opens after release and a drag-out must cancel it.
        pointer_offset = len(output)
        os.write(master, b"\x1b[<35;137;2M")
        read_available(master, output, 0.2)
        os.write(master, b"\x1b[<0;137;2M")
        read_available(master, output, 0.2)
        if not wait_for_painted_text(master, output, "Exit TundraUX", child, present=False, timeout=0):
            raise SystemExit("Back activated on mouse-down instead of release")
        os.write(master, b"\x1b[<32;1;4M\x1b[<0;1;4m")
        if not wait_for_output_quiet(master, output, child, quiet_period=0.2):
            raise SystemExit("drag cancellation did not settle")
        if not wait_for_painted_text(master, output, "Exit TundraUX", child, present=False, timeout=0):
            raise SystemExit("dragging out of Back activated it")
        os.write(master, b"\x1b[<0;137;2M")
        read_available(master, output, 0.2)
        if not wait_for_painted_text(master, output, "Exit TundraUX", child, present=False, timeout=0):
            raise SystemExit("second press activated Back before release")
        os.write(master, b"\x1b[<0;137;2m")
        if not wait_for_painted_text(master, output, "Exit TundraUX", child):
            raise SystemExit("releasing Back did not open the power dialog")

        if CHECK_CRASH_SCREEN:
            check_crash_screen(master, output, child, isolated)

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

        incidents = list(isolated.rglob("crash-*.json"))
        if incidents and not CHECK_CRASH_SCREEN:
            raise SystemExit(
                "normal startup/shutdown generated watchdog incidents:\n"
                + "\n".join(path.read_text() for path in incidents)
            )

        input_warnings = []
        for path in isolated.rglob("*.jsonl"):
            for line in path.read_text().splitlines():
                record = json.loads(line)
                if record.get("context", {}).get("module") == "ux.terminal.input":
                    input_warnings.append(record)
        codes = {record.get("error_code") for record in input_warnings}
        if not {"UX_TERMINAL_INPUT_INCOMPLETE", "UX_TERMINAL_INPUT_MALFORMED"} <= codes:
            raise SystemExit(f"missing terminal input warning logs: {codes}")
        if any(record.get("level") != "warning" for record in input_warnings):
            raise SystemExit("discarded terminal input was not logged as a warning")

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
        "Linux PTY management menu/search, status details, Explorer terminal cwd/return, CLI paths/status markers, Escape/mouse, input filtering/logging, button release and keyboard priority smoke passed "
        f"({MOUSE_FLOOD_EVENT_COUNT} queued mouse events before the keyboard sentinel; "
        f"input accepted in {flood_duration:.3f}s; "
        f"sentinel visible in {sentinel_latency:.3f}s)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
