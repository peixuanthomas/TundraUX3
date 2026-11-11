#!/usr/bin/env python3
"""Check compact UI geometry and measure real PTY search-input echoes.

All application data is temporary. Timings include PTY input, application work,
and observable output; they do not measure the internal draw duration.
"""
from __future__ import annotations

import argparse
import fcntl
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import pty
import re
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "shell_smoke", Path(__file__).resolve().parents[1] / "linux-shell-smoke.py"
)
smoke = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(smoke)


def run_case(binary, language, color, motion, sizes, samples):
    cli = binary.with_name("tundra-cli")
    with tempfile.TemporaryDirectory(prefix="tundra-ui-refresh-") as temporary:
        env = dict(os.environ, TERM="xterm-256color", LC_ALL="C.UTF-8", NO_COLOR="")
        if color == "truecolor":
            env["COLORTERM"] = "truecolor"
        else:
            env.pop("COLORTERM", None)
            env.pop("WT_SESSION", None)
        for name in ("CONFIG", "DATA", "CACHE", "STATE", "RUNTIME"):
            folder = Path(temporary) / name.lower()
            folder.mkdir(mode=0o700)
            env[f"XDG_{name}_HOME" if name != "RUNTIME" else "XDG_RUNTIME_DIR"] = str(folder)
        for field, value in (("automation.enabled", "true"), ("language", language),
                             ("motion", motion), ("icon-mode", "ascii")):
            result = subprocess.run([str(cli), "config", "set", field, value],
                                    env=env, capture_output=True, text=True, timeout=15)
            assert result.returncode == 0, result.stderr
        master, slave = pty.openpty()
        before = termios.tcgetattr(slave)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
        os.set_blocking(master, False)
        output = bytearray()
        child = subprocess.Popen([str(binary)], env=env, stdin=slave, stdout=slave,
                                 stderr=slave, start_new_session=True)

        def snapshot():
            return smoke.control_snapshot(master, output, child, cli, env, timeout=15)

        def until(predicate, timeout=10):
            deadline = time.monotonic() + timeout
            last = None
            while time.monotonic() < deadline:
                last = snapshot()
                if predicate(last):
                    return last
                smoke.read_available(master, output, 0.02)
            raise AssertionError(f"UI condition timed out: {last}")

        def send(keys):
            smoke.write_events_while_draining_output(master, (keys,), output, 5)

        def resize(columns, rows):
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
            os.kill(child.pid, signal.SIGWINCH)
            return until(lambda frame: (frame["width"], frame["height"]) == (columns, rows))

        def node(frame, identifier):
            return next(item for item in frame["nodes"] if item["id"] == identifier)

        def tap(frame, identifier):
            x, y = smoke.control_position(frame, identifier)
            send(f"\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m".encode())

        def home_entry(frame, label):
            return next(item["id"] for item in frame["nodes"]
                        if item["role"] == "button" and "home.entry." in item["id"]
                        and label in item["name"])

        def has_modal(frame):
            return any(item["id"].startswith("modal/") for item in frame["nodes"])

        measurements = []
        try:
            if os.geteuid() == 0:
                assert smoke.wait_for_output(master, output, b"any other key cancels: ", child, 5)
                send(b"y")
            current = snapshot()
            for _ in range(12):
                if current["page"] == "Home" and not has_modal(current):
                    break
                send(b"\x1b[21~" if current["page"] == "FirstRunSetup" and not has_modal(current) else b"\r")
                smoke.read_available(master, output, 0.1)
                current = snapshot()
            current = until(lambda frame: frame["page"] == "Home" and not has_modal(frame))
            settings_label, launcher_label = (("设置", "启动器") if language == "zh-CN"
                                               else ("Settings", "Launcher"))
            tap(current, home_entry(current, settings_label))
            current = until(lambda frame: frame["page"] == "Settings")
            motion_id = "page/settings.field.motionpreference"
            current = until(lambda frame: any(item["id"] == motion_id and item["actionable"]
                                              for item in frame["nodes"]))
            expected_motion = ({"full": "完整", "reduced": "减少"} if language == "zh-CN"
                               else {"full": "Full", "reduced": "Reduced"})[motion]
            if node(current, motion_id)["text"].strip() != expected_motion:
                tap(current, motion_id)
            current = until(lambda frame: node(frame, motion_id)["text"].strip() == expected_motion)
            assert node(current, "page/settings.field.animationspeed")["enabled"] == (motion == "full")
            tap(current, "chrome/shell.back")
            current = until(lambda frame: frame["page"] == "Home")
            home_ids = {item["id"] for item in current["nodes"] if "home.entry." in item["id"]}
            for columns, rows in sizes:
                until(lambda frame: not frame["animation_running"])
                frame_offset = len(output)
                current = resize(columns, rows)
                smoke.read_available(master, output, 0.02)
                rgb = re.search(rb"\x1b\[[0-9;]*(?:38|48);2;", bytes(output[frame_offset:]))
                assert bool(rgb) == (color == "truecolor"), f"unexpected {color} color output"
                ids = [item["id"] for item in current["nodes"]]
                assert len(ids) == len(set(ids)), "duplicate automation IDs"
                visible_home = [item for item in current["nodes"] if "home.entry." in item["id"]]
                assert {item["id"] for item in visible_home} == home_ids, "resize lost a Home entry"
                for item in visible_home:
                    rect = item["rect"]
                    assert item["actionable"] and rect["y"] >= 1
                    assert rect["y"] + rect["height"] <= rows - 1, "Home entry overlaps status"
                for identifier in ("chrome/shell.back", "chrome/shell.help", "chrome/shell.tasks"):
                    rect = node(current, identifier)["rect"]
                    assert rect["height"] == 1 and rect["y"] == 0, (identifier, rect)
                for identifier in ("chrome/shell.status", "chrome/shell.status.time"):
                    rect = node(current, identifier)["rect"]
                    assert rect["height"] == 1 and rect["y"] == rows - 1, (identifier, rect)
                tap(current, home_entry(current, settings_label))
                send(b"\x1bOP")
                current = until(lambda frame: frame["page"] == "Settings" and has_modal(frame))
                assert not any(item["actionable"] for item in current["nodes"]
                               if item["id"].startswith("page/")), "modal leaked input to its page"
                resize(columns + 1, rows)
                current = resize(columns, rows)
                assert has_modal(current), "resize lost the active dialog"
                send(b"\x1b")
                current = until(lambda frame: frame["page"] == "Settings" and not has_modal(frame))
                tap(current, "chrome/shell.back")
                current = until(lambda frame: frame["page"] == "Home")

                tap(current, home_entry(current, launcher_label))
                current = until(lambda frame: frame["page"] == "Launcher")
                tap(current, "page/launcher.search")
                current = until(lambda frame: node(frame, "page/launcher.search")["focused"])
                keyboard = next(item for item in current["nodes"] if item["id"].endswith("/input.keyboard"))
                assert keyboard["rect"]["y"] == rows - 1
                assert keyboard["rect"]["x"] < node(current, "chrome/shell.status.time")["rect"]["x"]
                tap(current, keyboard["id"])
                current = until(lambda frame: not frame["animation_running"] and
                                any(item["id"] == "keyboard/screen-keyboard.letter.a"
                                    for item in frame["nodes"]))
                assert node(current, "chrome/shell.back")["rect"]["y"] == 0
                assert not any(item["id"].startswith("chrome/shell.status") for item in current["nodes"])
                assert node(current, "page/launcher.search")["actionable"]
                toggle = next(item for item in current["nodes"] if item["id"].endswith("/screen-keyboard.toggle"))
                tap(current, toggle["id"])
                current = until(lambda frame: not frame["animation_running"] and
                                any(item["id"].endswith("/input.keyboard") for item in frame["nodes"]))
                assert node(current, "chrome/shell.status")["rect"]["y"] == rows - 1
                wide = "中文e\u0301😀"
                send(b"\x1b[200~" + wide.encode() + b"\x1b[201~")
                current = until(lambda frame: wide in node(frame, "page/launcher.search")["text"])
                send(b"\x15")
                until(lambda frame: not frame["animation_running"])
                latencies = []
                for index in range(samples):
                    assert smoke.wait_for_output_quiet(master, output, child, quiet_period=0.04)
                    marker = chr(0x0400 + index).encode()
                    offset = len(output)
                    started = time.monotonic_ns()
                    send(marker)
                    assert smoke.wait_for_output(master, output, marker, child, 2, offset), "search input was not echoed"
                    latencies.append((time.monotonic_ns() - started) / 1_000_000)
                current = snapshot()
                assert chr(0x0400 + samples - 1) in node(current, "page/launcher.search")["text"]
                p95 = sorted(latencies)[math.ceil(len(latencies) * 0.95) - 1]
                measurements.append({
                    "size": f"{columns}x{rows}", "language": language, "color": color,
                    "motion": motion, "samples": samples, "input_to_output_echo_ms_p95": round(p95, 3),
                    "input_to_output_echo_ms_max": round(max(latencies), 3),
                })
                send(b"\x15\t")
                until(lambda frame: not node(frame, "page/launcher.search")["focused"])
                send(b"\x1b")
                current = until(lambda frame: frame["page"] == "Home")
                print(f"PASS: {columns}x{rows} {language} {color} {motion}; PTY echo P95 {p95:.3f}ms", flush=True)
        finally:
            smoke.signal_process_group(child, signal.SIGTERM)
            deadline = time.monotonic() + 10
            while child.poll() is None and time.monotonic() < deadline:
                smoke.read_available(master, output, 0.05)
            smoke.signal_process_group(child, signal.SIGKILL)
            child.wait(timeout=5)
            smoke.read_available(master, output, 0)
            try:
                assert child.returncode == 0, child.returncode
                assert termios.tcgetattr(slave) == before, "terminal attributes were not restored"
            finally:
                os.close(master)
                os.close(slave)
        return measurements


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--language", action="append", choices=("en-US", "zh-CN"))
    parser.add_argument("--color", action="append", choices=("truecolor", "ansi"))
    parser.add_argument("--motion", action="append", choices=("full", "reduced"))
    parser.add_argument("--samples", type=int, default=30)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    assert 1 <= args.samples <= 64, "samples must be between 1 and 64"
    binary = args.binary.resolve()
    assert binary.is_file() and binary.with_name("tundra-cli").is_file()
    measurements = []
    started = time.monotonic()
    for language in args.language or ("en-US", "zh-CN"):
        for color in args.color or ("truecolor", "ansi"):
            for motion in args.motion or ("full", "reduced"):
                measurements.extend(run_case(binary, language, color, motion,
                                            ((108, 20), (120, 36), (180, 50)), args.samples))
    stat = binary.stat()
    report = {
        "binary": str(binary), "binary_size": stat.st_size, "binary_mtime_ns": stat.st_mtime_ns,
        "platform": platform.platform(), "python": platform.python_version(),
        "terminal": "xterm-256color", "automation_enabled": True,
        "elapsed_seconds": round(time.monotonic() - started, 3),
        "measurement": "raw PTY search input to matching UTF-8 output; excludes later snapshot verification",
        "input_to_render_note": "PTY echo timing does not isolate application input-to-render duration.",
        "draw_duration_ms_p95": None, "draw_duration_note": "No internal draw timing is captured by this PTY test.",
        "cases": measurements,
    }
    if args.output:
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(report, ensure_ascii=False))


if __name__ == "__main__":
    main()
