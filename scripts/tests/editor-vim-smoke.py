#!/usr/bin/env python3
"""Exercise the real Vim editor through the Shell on a Unix PTY.

Usage: python3 scripts/tests/editor-vim-smoke.py [tundra-shell] [vim]
The optional Vim executable overrides TUNDRA_VIM. All files and XDG state are
temporary; TUNDRA_VIM_CLEAN disables personal Vim configuration for this test.
"""

from __future__ import annotations

import argparse
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import pty
import re
import select
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
shell_smoke = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(shell_smoke)

ESC = b"\x1b"
F8 = b"\x1b[19~"
F10 = b"\x1b[21~"
SPECIAL_KEYS = {
    "<Esc>": ESC,
    "<Enter>": b"\r",
    "<C-r>": b"\x12",
    "<C-v>": b"\x16",
    "<C-w>": b"\x17",
    "<F8>": F8,
}
BASE = "alpha beta gamma\nsecond line\nthird line\n"
VIM_READY = "F8 Return to editor"


class ShellPty:
    def __init__(self, binary: Path, root: Path, vim: str | None):
        env = dict(os.environ, TERM="xterm-256color", TUNDRA_VIM_CLEAN="1",
                   LC_MESSAGES="C", LANGUAGE="C")
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)
        if vim:
            env["TUNDRA_VIM"] = str(Path(vim).resolve())
        for name in ("CONFIG", "DATA", "CACHE", "STATE", "RUNTIME"):
            directory = root / name.lower()
            directory.mkdir(mode=0o700, exist_ok=True)
            key = f"XDG_{name}_HOME" if name != "RUNTIME" else "XDG_RUNTIME_DIR"
            env[key] = str(directory)
        self.master, self.slave = pty.openpty()
        self.columns, self.rows = 140, 45
        self.output = bytearray()
        self.closed = False
        self.resize(self.columns, self.rows)
        self.original_attributes = termios.tcgetattr(self.slave)
        os.set_blocking(self.master, False)
        self.child = subprocess.Popen(
            [str(binary)], stdin=self.slave, stdout=self.slave, stderr=self.slave,
            cwd=root / "files", env=env, start_new_session=True,
        )

    def resize(self, columns: int, rows: int):
        self.columns, self.rows = columns, rows
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ,
                    struct.pack("HHHH", rows, columns, 0, 0))
        if hasattr(self, "child") and self.child.poll() is None:
            os.killpg(self.child.pid, signal.SIGWINCH)

    def drain(self, seconds=0.15):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            readable, _, _ = select.select(
                [self.master], [], [], max(0, deadline - time.monotonic())
            )
            if readable:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    return
                if not data:
                    return
                shell_smoke.append_output(self.output, data)

    def screen(self):
        return "\n".join(shell_smoke.painted_screen_lines(
            self.output, self.columns, self.rows
        ))

    def wait(self, text: str, present=True, timeout=8.0):
        deadline = time.monotonic() + timeout
        while True:
            self.drain(0.05)
            if (text in self.screen()) == present:
                return
            if self.child.poll() is not None or time.monotonic() >= deadline:
                raise AssertionError(f"screen did not show {text!r} ({present}):\n{self.screen()}")

    def send(self, data: bytes):
        shell_smoke.write_events_while_draining_output(
            self.master, (data,), self.output, 5.0
        )
        self.drain()

    def keys(self, keys: str):
        for part in re.split(r"(<Esc>|<Enter>|<C-r>|<C-v>|<C-w>|<F8>)", keys):
            if part:
                self.send(SPECIAL_KEYS.get(part, part.encode()))

    def ex(self, command: str):
        self.keys("<Esc>" + ":" + command + "<Enter>")

    def close(self):
        if self.closed:
            return
        if self.child.poll() is None:
            os.killpg(self.child.pid, signal.SIGTERM)
            deadline = time.monotonic() + 8
            while self.child.poll() is None and time.monotonic() < deadline:
                self.drain(0.05)
        if self.child.poll() is None:
            os.killpg(self.child.pid, signal.SIGKILL)
            self.child.wait()
            raise AssertionError("Shell did not stop its Vim process cleanly")
        self.drain()
        assert self.child.returncode == 0, self.child.returncode
        assert termios.tcgetattr(self.slave) == self.original_attributes
        os.close(self.master)
        os.close(self.slave)
        self.closed = True


def vim_path(path: Path):
    return str(path).replace("\\", "\\\\").replace(" ", "\\ ").replace("|", "\\|")


def vim_string(path: Path):
    return "'" + str(path).replace("'", "''") + "'"


def assert_file(shell: ShellPty, path: Path, expected: str):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if path.is_file() and path.read_text() == expected:
            return
        shell.drain(0.05)
    raise AssertionError(
        f"{path.name}: expected {expected!r}, got "
        f"{path.read_text() if path.is_file() else '<missing>'!r}\n{shell.screen()}"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("shell", nargs="?", default="target/debug/tundra-shell")
    parser.add_argument("vim", nargs="?")
    args = parser.parse_args()
    assert os.name == "posix", "This test requires a Unix PTY"
    binary = Path(args.shell).resolve()
    assert binary.is_file() and os.access(binary, os.X_OK), binary
    with tempfile.TemporaryDirectory(prefix="tundra-editor-vim-") as temporary:
        root = Path(temporary)
        files = root / "files"
        files.mkdir()
        start = files / "00-start.txt"
        start.write_text(BASE)
        shell = ShellPty(binary, root, args.vim)
        checks = 0

        def open_file(name, content=BASE):
            path = files / (name + ".txt")
            path.write_text(content)
            shell.ex("edit! " + vim_path(path))
            shell.wait(path.name)
            return path

        def case(name, keys, expected, content=BASE):
            nonlocal checks
            path = open_file(name, content)
            shell.keys("gg0" + keys)
            shell.ex("write")
            assert_file(shell, path, expected)
            checks += 1
            print("PASS:", name, flush=True)

        def editor_from_explorer(path):
            shell.send(b"\x1b[15~")
            shell.send(b"\x06")
            shell.send(b"\x01")
            shell.send(b"\x1b[200~" + path.name.encode() + b"\x1b[201~")
            shell.send(b"\r")
            shell.send(b"\r")
            shell.wait("Editor")
            shell.wait(VIM_READY)

        try:
            if os.geteuid() == 0:
                shell.wait("any other key cancels:")
                shell.send(b"y")
            shell.wait("Language", timeout=25)
            if "No terminal graphics response" in shell.screen():
                shell.send(b"\r")
                shell.wait("No terminal graphics response", present=False)
            shell.send(b"\r")
            shell.wait("Frame shape")
            shell.send(F10)
            shell.wait("Home")
            shell.send(b"e")
            shell.wait("00-start.txt")
            shell.send(b"\r")
            shell.wait("Editor")
            shell.send(F8)
            shell.wait(VIM_READY)
            shell.ex("set showmode showcmd ruler laststatus=2 noswapfile")

            case("line-end-delete", "$x", BASE.replace("gamma", "gamm", 1))
            case("line-end-append", "$aX<Esc>", BASE.replace("gamma", "gammaX", 1))
            case("insert-escape-line-start", "j0i<Esc>iX<Esc>", BASE.replace("second", "Xsecond"))
            case("last-line-delete", "Gdd", "alpha beta gamma\nsecond line\n")
            case("last-line-yank", "Gyyp", BASE + "third line\n")
            case("last-line-delete-paste", "jjddp", BASE)
            case("counts-and-dot", "2x.", "a beta gamma\nsecond line\nthird line\n")
            case("text-object-delete", "diw", " beta gamma\nsecond line\nthird line\n")
            case("text-object-change", "ciwZ<Esc>", "Z beta gamma\nsecond line\nthird line\n")
            case("visual-character", "vllx", "ha beta gamma\nsecond line\nthird line\n")
            case("visual-lines", "Vjd", "third line\n")
            case("search-forward-delete", "/alpha<Enter>x", "alpha beta lpha\ngamma alpha delta\n",
                 "alpha beta alpha\ngamma alpha delta\n")

            path = open_file("undo-group")
            shell.keys("gg0i123<Esc>u")
            shell.ex("write")
            assert_file(shell, path, BASE)
            shell.keys("<C-r>")
            shell.ex("write")
            assert_file(shell, path, "123" + BASE)
            checks += 1
            print("PASS: insert-session undo/redo", flush=True)

            path = open_file("native-display")
            shell.keys("gg02d")
            shell.wait("2d")
            shell.keys("<Esc>i")
            shell.wait("-- INSERT --")
            shell.keys("<Esc>R")
            shell.wait("-- REPLACE --")
            shell.keys("<Esc>v")
            shell.wait("-- VISUAL --")
            shell.keys("<Esc><C-v>")
            shell.wait("-- VISUAL BLOCK --")
            shell.keys("<Esc>:%s/alpha/ALPHA/g")
            shell.wait(":%s/alpha/ALPHA/g")
            shell.keys("<Enter>")
            shell.ex("write")
            assert_file(shell, path, BASE.replace("alpha", "ALPHA"))
            shell.ex("TundraNonexistentCommand")
            shell.wait("E492")
            shell.keys("<Esc>")
            checks += 1
            print("PASS: native modes, pending operator, command line and errors", flush=True)

            shell.ex("help usr_01.txt")
            shell.ex('echo "VIM-HELP:" . &buftype')
            shell.wait("VIM-HELP:help")
            shell.keys("<C-w>c")
            shell.ex('echo "VIM-RETURN:" . expand("%:t")')
            shell.wait("VIM-RETURN:native-display.txt")
            checks += 1
            print("PASS: bundled :help and native Ctrl-W window command", flush=True)

            source = open_file("register-source")
            shell.keys('gg0"ayy')
            target = open_file("register-target", "target\n")
            shell.keys('gg0"ap')
            shell.ex("write")
            assert_file(shell, target, "target\nalpha beta gamma\n")
            shell.ex("buffer " + vim_path(source))
            shell.keys("gg0qaxjq@a")
            shell.ex("write")
            assert_file(shell, source, "lpha beta gamma\necond line\nthird line\n")
            checks += 1
            print("PASS: shared register across buffers and recorded macro", flush=True)

            original = open_file("save-as-original")
            renamed = files / "save-as-renamed.txt"
            shell.ex("saveas " + vim_path(renamed))
            assert_file(shell, renamed, BASE)
            shell.keys("gg0iRENAMED-<Esc>")
            shell.ex("write")
            assert_file(shell, renamed, "RENAMED-" + BASE)
            assert original.read_text() == BASE
            checks += 1
            print("PASS: :saveas changes the subsequent :write target", flush=True)

            path = open_file("external-reload")
            path.write_text("external update\n")
            shell.ex("edit! " + vim_path(path))
            shell.wait("external update")
            shell.keys("gg0A-edited<Esc>")
            shell.ex("write")
            assert_file(shell, path, "external update-edited\n")
            checks += 1
            print("PASS: :edit! refreshes an externally changed file before :write", flush=True)

            path = open_file("resize")
            shell.resize(108, 30)
            shell.drain(0.3)
            shell.keys("gg0iRESIZED-<Esc>")
            shell.resize(140, 45)
            shell.drain(0.3)
            shell.ex("write")
            assert_file(shell, path, "RESIZED-" + BASE)
            checks += 1
            print("PASS: native resize and continued editing", flush=True)

            path = open_file("toggle-dirty")
            shell.keys("gg0iDRAFT-<Esc><F8>")
            shell.wait("VIM_mode: Off")
            shell.wait("DRAFT-alpha")
            assert path.read_text() == BASE, "F8 must not save the user's document"
            shell.send(F8)
            shell.wait(VIM_READY)
            shell.ex("write")
            assert_file(shell, path, "DRAFT-" + BASE)
            checks += 1
            print("PASS: F8 transfers an unsaved buffer without writing it", flush=True)

            source = open_file("toggle-source")
            shell.keys("gg0iFIRST-<Esc>")
            target = files / "toggle-target.txt"
            target.write_text("second buffer\n")
            shell.ex("hide edit " + vim_path(target))
            shell.keys("gg0iSECOND-<Esc><F8>")
            shell.wait("VIM_mode: Off")
            shell.wait("SECOND-second buffer")
            assert source.read_text() == BASE
            assert target.read_text() == "second buffer\n"
            shell.send(F8)
            shell.wait(VIM_READY)
            shell.ex("hide buffer " + vim_path(source))
            shell.ex("write")
            assert_file(shell, source, "FIRST-" + BASE)
            shell.ex("buffer " + vim_path(target))
            shell.ex("write")
            assert_file(shell, target, "SECOND-second buffer\n")
            checks += 1
            print("PASS: F8 preserves multiple dirty Vim buffers", flush=True)

            removed = open_file("deleted-buffer")
            shell.keys("gg0iDISCARD-<Esc>")
            survivor = files / "surviving-buffer.txt"
            survivor.write_text("surviving buffer\n")
            shell.ex("hide edit " + vim_path(survivor))
            shell.ex("bdelete! " + vim_path(removed))
            shell.send(F8)
            shell.wait("VIM_mode: Off")
            shell.wait("surviving buffer")
            assert removed.read_text() == BASE
            shell.send(F8)
            shell.wait(VIM_READY)
            probe = files / "deleted-buffer-listed.txt"
            shell.ex("call writefile([string(buflisted(" + vim_string(removed)
                     + "))], " + vim_string(probe) + ")")
            assert_file(shell, probe, "0\n")
            checks += 1
            print("PASS: :bdelete! remains deleted after an F8 round trip", flush=True)

            path = open_file("gui-undo")
            shell.keys('gg0qcxqugg0"byygg0iVIM-<Esc><F8>')
            shell.wait("VIM_mode: Off")
            shell.send(b"\x01")
            shell.send(b"\x1b[200~GUI-only line\n\x1b[201~")
            shell.send(F8)
            shell.wait(VIM_READY)
            shell.wait("GUI-only line")
            probe = files / "retained-registers.txt"
            shell.ex("call writefile([getreg('b', 1, 1)[0], getreg('c')], "
                     + vim_string(probe) + ")")
            assert_file(shell, probe, "alpha beta gamma\nx\n")
            shell.keys("u")
            shell.ex("write")
            assert_file(shell, path, "VIM-" + BASE)
            shell.keys("@cu")
            shell.ex("write")
            assert_file(shell, path, "VIM-" + BASE)
            checks += 1
            print("PASS: F8 keeps registers, macros and undo across ordinary-editor changes", flush=True)

            path = open_file("quit-discard")
            shell.keys("gg0iDISCARD-<Esc>")
            shell.ex("quit")
            shell.wait("E37")
            shell.ex("quit!")
            shell.wait("Quick access")
            assert path.read_text() == BASE
            path = files / "zz-write-quit.txt"
            path.write_text(BASE)
            editor_from_explorer(path)
            shell.keys("gg0iSAVED-<Esc>")
            shell.ex("wq")
            shell.wait("Quick access")
            assert_file(shell, path, "SAVED-" + BASE)
            checks += 1
            print("PASS: :q refuses a dirty buffer; :q! discards; :wq saves and exits", flush=True)

            path = files / "zz-shutdown-recovery.txt"
            path.write_text(BASE)
            editor_from_explorer(path)
            shell.keys("gg0iOLD-VIM-<Esc><F8>")
            shell.wait("VIM_mode: Off")
            shell.send(b"\x01")
            shell.send(b"\x1b[200~GUI-RECOVERY\n\x1b[201~")
            shell.wait("GUI-RECOVERY")
            shell.close()
            assert path.read_text() == BASE
            records = [json.loads(p.read_text())
                       for p in (root / "data").glob("**/editor/recovery/**/*.json")]
            recovered = [record for record in records if record.get("path") == str(path)]
            assert len(recovered) == 1, records
            assert recovered[0]["payload"]["text"] == "GUI-RECOVERY\n", recovered
            shell = ShellPty(binary, root, args.vim)
            if os.geteuid() == 0:
                shell.wait("any other key cancels:")
                shell.send(b"y")
            shell.wait("E Explorer", timeout=25)
            shell.send(b"a")
            shell.wait("Launcher")
            shell.send(b"\x1b[H")
            shell.send(b"\x1b[C")
            shell.send(b"\r")
            shell.wait("GUI-RECOVERY")
            shell.send(b"\x13")
            assert_file(shell, path, "GUI-RECOVERY\n")
            checks += 1
            print("PASS: shutdown and recovery preserve ordinary-editor changes over suspended Vim", flush=True)
        except Exception:
            print(shell.screen(), file=sys.stderr)
            raise
        finally:
            shell.close()
        print(f"PASS: {checks} Vim workflows and terminal restoration")


if __name__ == "__main__":
    main()
