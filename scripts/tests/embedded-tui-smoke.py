#!/usr/bin/env python3
"""Exercise Neovim, Yazi and Launcher terminals through an isolated real PTY.

Build Shell and CLI together before running. Requires Python 3.11+, Neovim and
Yazi; pass --nvim/--yazi when the tools are outside PATH. A bare PTY exercises
text and image fallback, not native image rendering on a physical terminal.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import tomllib
import unicodedata
import zlib


sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "vim_smoke", Path(__file__).with_name("editor-vim-smoke.py")
)
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


def png_bytes():
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(
            ">I", zlib.crc32(kind + data) & 0xFFFFFFFF
        )

    # Different upper/lower colors prevent Halfblocks from reducing the preview to spaces.
    rows = b"".join(b"\0" + bytes((255 - y * 16,) * 3) * 16 for y in range(16))
    return b"\x89PNG\r\n\x1a\n" + b"".join([
        chunk(b"IHDR", struct.pack(">IIBBBBB", 16, 16, 8, 2, 0, 0, 0)),
        chunk(b"IDAT", zlib.compress(rows)),
        chunk(b"IEND", b""),
    ])


def executable(value, name):
    selected = value or shutil.which(name)
    if not selected:
        raise SystemExit(f"{name} is required; install it or pass --{name}")
    path = Path(selected).resolve()
    if not path.is_file() or not os.access(path, os.X_OK):
        raise SystemExit(f"not an executable: {path}")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("shell", nargs="?", default="target/debug/tundra-shell")
    parser.add_argument("--nvim", default=os.environ.get("TUNDRA_TEST_NVIM"))
    parser.add_argument("--yazi", default=os.environ.get("TUNDRA_TEST_YAZI"))
    args = parser.parse_args()
    assert sys.platform == "linux", "This smoke requires a Linux PTY"
    assert os.geteuid() != 0, "Run this smoke as a normal Linux user"
    binary = executable(args.shell, "tundra-shell")
    cli = binary.with_name("tundra-cli")
    assert cli.is_file() and os.access(cli, os.X_OK), cli
    nvim = executable(args.nvim, "nvim")
    yazi = executable(args.yazi, "yazi")
    for name, program in (("Neovim", nvim), ("Yazi", yazi)):
        version = subprocess.run([str(program), "--version"], check=True,
                                 capture_output=True, text=True, timeout=10)
        summary = " ".join(line.strip() for line in version.stdout.splitlines()[:2])
        print(f"Using {name}: {summary} ({program})")

    with tempfile.TemporaryDirectory(prefix="tundra-embedded-tui-") as temporary:
        root = Path(temporary)
        files = root / "files"
        files.mkdir()
        workspace = files / "space and 中文"
        workspace.mkdir()
        env = os.environ.copy()
        env["TUNDRA_ASCII_ASSETS_DIR"] = str(
            Path(__file__).resolve().parents[2] / "crates/ascii-assets/assets"
        )
        env["YAZI_CONFIG_HOME"] = str(root / "yazi-config")
        for name in ("CONFIG", "DATA", "CACHE", "STATE", "RUNTIME"):
            directory = root / name.lower()
            directory.mkdir(mode=0o700)
            key = f"XDG_{name}_HOME" if name != "RUNTIME" else "XDG_RUNTIME_DIR"
            env[key] = str(directory)

        def command(*arguments):
            result = subprocess.run([str(cli), *arguments], env=env,
                                    capture_output=True, text=True, timeout=15)
            assert result.returncode == 0, (arguments, result.stdout, result.stderr)
            return result.stdout

        command("config", "set", "automation.enabled", "true")
        command("launcher", "pin", str(nvim))
        python = Path(sys.executable).resolve()
        command("launcher", "pin", str(python))
        fixture = workspace / "launcher-fixture.py"
        fixture.write_text(
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "code = int(sys.argv[1])\n"
            "Path(sys.argv[2]).write_text(json.dumps({"
            "'cwd': os.getcwd(), 'arguments': sys.argv[3:]}))\n"
            "print(f'LAUNCHER_EXTERNAL_EXIT_{code}', flush=True)\n"
            "raise SystemExit(code)\n",
            encoding="utf-8",
        )
        document = workspace / "neovim.txt"
        document.write_text("NEOVIM_INITIAL_LINE\nSECOND_LINE\n", encoding="utf-8")
        launcher_document = workspace / "launcher-neovim.txt"
        launcher_document.write_text("LAUNCHER_NEOVIM_LINE\n", encoding="utf-8")
        yazi_workspace = workspace / "yazi-workspace"
        yazi_workspace.mkdir()
        (yazi_workspace / "preview.png").write_bytes(png_bytes())
        (yazi_workspace / "sample.txt").write_text("YAZI_PREVIEW_TEXT\n", encoding="utf-8")
        old_environment = os.environ.copy()
        os.environ.update(env)
        try:
            shell = smoke.ShellPty(binary, root, None)
        finally:
            os.environ.clear()
            os.environ.update(old_environment)
        current = None

        def request(*arguments, stdin=None):
            with subprocess.Popen(
                [str(cli), "ui", *map(str, arguments), "--timeout", "15000"],
                env=env, stdin=subprocess.PIPE if stdin is not None else None,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            ) as process:
                if stdin is not None:
                    process.stdin.write(stdin)
                    process.stdin.close()
                    process.stdin = None
                deadline = time.monotonic() + 20
                while True:
                    shell.drain(0.01)
                    try:
                        output, error = process.communicate(timeout=0.02)
                        assert output, (arguments, process.returncode, error)
                        return json.loads(output)
                    except subprocess.TimeoutExpired:
                        if time.monotonic() >= deadline:
                            process.kill()
                            process.communicate()
                            raise AssertionError(f"UI request timed out: {arguments}")

        def ui(*arguments, stdin=None):
            response = request(*arguments, stdin=stdin)
            assert response.get("ok"), response
            return response.get("snapshot")

        def text(value):
            return ui("text", "--stdin", stdin=value)

        def until(predicate, timeout=20):
            nonlocal current
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                response = request("snapshot")
                current = response.get("snapshot")
                if current and predicate(current):
                    return current
                assert shell.child.poll() is None, shell.screen()
                shell.drain(0.05)
            raise AssertionError(f"UI condition timed out on {current and current['page']}")

        def key(name, *options):
            return ui("key", name, *options)

        def body(snapshot):
            return "\n".join(node["text"] for node in snapshot["nodes"]
                             if node["role"] == "terminal")

        def terminal_area():
            return next(node["rect"] for node in ui("snapshot")["nodes"]
                        if node["role"] == "terminal")

        def has_halfblocks(snapshot):
            terminals = [node["rect"] for node in snapshot["nodes"]
                         if node["role"] == "terminal"]
            for area in terminals:
                for line in snapshot["lines"][area["y"]:area["y"] + area["height"]]:
                    column = 0
                    for character in line:
                        if character == "▀" and area["x"] <= column < area["x"] + area["width"]:
                            return True
                        if not unicodedata.combining(character):
                            column += 2 if unicodedata.east_asian_width(character) in ("W", "F") else 1
            return False

        def contains(marker):
            return until(lambda snapshot: snapshot["page"] == "CommandLine"
                         and marker in body(snapshot))

        def ex(value):
            key("Escape")
            key(":")
            text(value)
            key("Enter")

        def repl_command(value, expected):
            text(value)
            key("Enter")
            return contains(expected)

        def settings(path, arguments):
            key("End")
            if path == nvim:
                key("Left")
            key("F4")
            until(lambda snapshot: any("launcher-run:" in node["id"]
                                       for node in snapshot["nodes"]))
            config_path = next(Path(env["XDG_CONFIG_HOME"]).rglob("config.toml"))
            entries = tomllib.loads(config_path.read_text())["launcher"]["entries"]
            previous = next(entry for entry in entries if entry["path"] == str(path))
            if "terminal" not in previous:
                key("Enter")
            key("Tab")
            key("a", "--modifiers", "ctrl")
            text(shlex.join(arguments))
            key("Tab")
            key("a", "--modifiers", "ctrl")
            text(str(workspace))
            key("F10")
            until(lambda snapshot: not any("launcher-run:" in node["id"]
                                           for node in snapshot["nodes"]))
            saved = tomllib.loads(config_path.read_text())["launcher"]["entries"]
            saved = next(entry for entry in saved if entry["path"] == str(path))["terminal"]
            assert saved["arguments"] == arguments, saved
            assert saved["working_directory"] == str(workspace), saved

        try:
            until(lambda snapshot: bool(snapshot["nodes"]), timeout=30)
            for _ in range(12):
                current = ui("snapshot")
                modal = any(node["id"].startswith("modal/") and node["actionable"]
                            for node in current["nodes"])
                if current["page"] == "Home" and not modal:
                    break
                key("F10" if current["page"] == "FirstRunSetup" and not modal else "Enter")
                shell.drain(0.1)
            until(lambda snapshot: snapshot["page"] == "Home")
            key("a")
            current = until(lambda snapshot: snapshot["page"] == "Launcher")
            command_node = next(node for node in current["nodes"]
                                if node["role"] == "button" and "Command Line" in node["name"])
            ui("click", "--id", command_node["id"])
            until(lambda snapshot: snapshot["page"] == "CommandLine" and ">>" in body(snapshot))

            repl_command(shlex.join([str(nvim), "--clean", "-n", "-i", "NONE",
                                     "--cmd", "set mouse=a", str(document)]),
                         "NEOVIM_INITIAL_LINE")
            area = terminal_area()
            x, y = area["x"] + 5, area["y"] + 1
            shell.send(f"\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m".encode())
            ui("snapshot")
            mouse_probe = workspace / "mouse-position.json"
            ex(f"call writefile([json_encode(getcurpos())], {smoke.vim_string(mouse_probe)})")
            until(lambda _: mouse_probe.is_file())
            assert json.loads(mouse_probe.read_text())[1:3] == [1, 5], mouse_probe.read_text()
            shell.resize(150, 44)
            until(lambda snapshot: snapshot["width"] == 150 and snapshot["height"] == 44)
            shell.drain(0.15)
            area = terminal_area()
            size_probe = workspace / "nvim-size.json"
            ex(f"call writefile([json_encode([&columns, &lines])], {smoke.vim_string(size_probe)})")
            until(lambda _: size_probe.is_file())
            assert json.loads(size_probe.read_text()) == [area["width"], area["height"]]
            ex("normal! gg0")
            key("i")
            text("smoke中文 pasted\n")
            key("Escape")
            ex("write")
            until(lambda _: document.read_text(encoding="utf-8").startswith("smoke中文 pasted\n"))
            ex("qa!")
            repl_command("printf 'REPL_AFTER_NVIM_%s\\n' ready", "REPL_AFTER_NVIM_ready")
            print("PASS: Neovim mouse, Unicode paste, write, live resize and return to REPL")

            yazi_cwd = workspace / "yazi-cwd.txt"
            repl_command(shlex.join([str(yazi), "--cwd-file", str(yazi_cwd),
                                     str(yazi_workspace)]), "preview.png")
            contains("sample.txt")
            key("g")
            key("g")
            until(has_halfblocks)
            key("j")
            until(lambda snapshot: "YAZI_PREVIEW_TEXT" in body(snapshot)
                  and not has_halfblocks(snapshot))
            print("PASS: real Yazi PNG preview rendered halfblocks and switching to text cleared them")
            key("k")
            area = terminal_area()
            x, y = area["x"] + area["width"] // 2 + 1, area["y"] + 4
            shell.send(f"\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m".encode())
            ui("snapshot")
            key("f")
            text("sample")
            key("Enter")
            until(lambda snapshot: "sample.txt" in body(snapshot)
                  and "preview.png" not in body(snapshot))
            key("q")
            repl_command("printf 'REPL_AFTER_YAZI_%s\\n' ready", "REPL_AFTER_YAZI_ready")
            assert yazi_cwd.read_text().strip() == str(yazi_workspace)
            text("exit")
            key("Enter")
            until(lambda snapshot: snapshot["page"] == "Launcher")
            print("PASS: Yazi image/text files, navigation, mouse, pasted filter and return to REPL")

            settings(nvim, ["--clean", "-n", "-i", "NONE", str(launcher_document)])
            key("Enter")
            contains("LAUNCHER_NEOVIM_LINE")
            ex("qa!")
            until(lambda snapshot: snapshot["page"] == "Launcher")
            print("PASS: saved Launcher settings started real Neovim and success returned to Launcher")

            for code in (75, 76):
                report = workspace / f"launcher-{code}.json"
                literal_arguments = ["two words", "$HOME", "$(printf must-not-run)"]
                settings(python, [str(fixture), str(code), str(report), *literal_arguments])
                key("Enter")
                contains(f"LAUNCHER_EXTERNAL_EXIT_{code}")
                until(lambda _: report.is_file())
                assert json.loads(report.read_text()) == {
                    "cwd": str(workspace), "arguments": literal_arguments,
                }
                shell.drain(0.3)
                assert ui("snapshot")["page"] == "CommandLine"
                ui("click", "--id", "chrome/shell.back")
                until(lambda snapshot: snapshot["page"] == "Launcher")
                print(f"PASS: external exit {code} retained output and did not reset or panic Shell")
        except Exception:
            summary = {key: current[key] for key in ("page", "number", "width", "height")} if current else None
            print("Last snapshot:", json.dumps(summary, ensure_ascii=False), file=sys.stderr)
            print("Shell output:\n" + shell.screen(), file=sys.stderr)
            raise
        finally:
            shell.close()
        print("PASS: Shell restored its outer PTY modes; native image devices remain a separate check")


if __name__ == "__main__":
    main()
