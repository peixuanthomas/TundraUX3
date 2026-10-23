#!/usr/bin/env python3
"""Test real Vim through tundra-cli ui in an isolated Linux terminal.

Usage: python3 scripts/tests/editor-vim-ui.py /path/to/tundra-shell
"""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("vim_smoke", Path(__file__).with_name("editor-vim-smoke.py"))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


def main():
    os.environ["TUNDRA_ASCII_ASSETS_DIR"] = str(Path(__file__).resolve().parents[2] / "crates/ascii-assets/assets")
    binary = Path(sys.argv[1]).resolve()
    cli = binary.with_name("tundra-cli")
    with tempfile.TemporaryDirectory(prefix="tundra-vim-ui-") as temporary:
        root = Path(temporary)
        files = root / "files"
        files.mkdir()
        document = files / "sample.txt"
        document.write_text(smoke.BASE, encoding="utf-8")
        missing_engine = "--missing-engine" in sys.argv[2:]
        if missing_engine:
            isolated = root / "bin"
            isolated.mkdir()
            for executable in (binary, cli):
                target = isolated / executable.name
                try:
                    os.link(executable, target)
                except OSError:
                    shutil.copy2(executable, target)
            binary = isolated / binary.name
            cli = isolated / cli.name
        env = os.environ.copy()
        for name in ("CONFIG", "DATA", "CACHE", "STATE", "RUNTIME"):
            path = root / name.lower()
            path.mkdir(mode=0o700)
            env[f"XDG_{name}_HOME" if name != "RUNTIME" else "XDG_RUNTIME_DIR"] = str(path)
        subprocess.run([str(cli), "config", "set", "automation.enabled", "true"], env=env, check=True,
                       capture_output=True, text=True)
        shell = smoke.ShellPty(binary, root, None)

        def request(*args):
            with subprocess.Popen([str(cli), "ui", *args, "--timeout", "15000"], env=env,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as process:
                deadline = time.monotonic() + 20
                while True:
                    # A real terminal consumes output while the control client waits for its reply.
                    shell.drain(0.01)
                    try:
                        output, error = process.communicate(timeout=0.02)
                        return json.loads(output)
                    except subprocess.TimeoutExpired:
                        if time.monotonic() >= deadline:
                            process.kill()
                            process.communicate()
                            raise AssertionError("UI control client timed out")

        def ui(*args):
            response = request(*args)
            assert response.get("ok"), response
            return response.get("snapshot")

        def until(predicate, timeout=20):
            deadline = time.monotonic() + timeout
            last = None
            while time.monotonic() < deadline:
                shell.drain(0.01)
                response = request("snapshot")
                last = response.get("snapshot")
                if last and predicate(last):
                    return last
                assert shell.child.poll() is None, shell.screen()
                time.sleep(0.03)
            raise AssertionError(f"UI condition timed out on {last and last['page']}")

        def screen_contains(text):
            return until(lambda current: text in "\n".join(current["lines"]))

        def key(name, *options):
            return ui("key", name, *options)

        def keys(sequence):
            for character in sequence:
                key("space" if character == " " else character)

        def ex(command):
            key("esc")
            key(":")
            ui("text", command)
            key("enter")

        checkpoint = 0

        def expect_file(path, expected):
            nonlocal checkpoint
            checkpoint += 1
            marker = f"Vim UI checkpoint {checkpoint}"
            ex("echo '" + marker + "'")
            screen_contains(marker)
            until(lambda _: path.read_text(encoding="utf-8") == expected)

        def open_case(name, text=smoke.BASE):
            path = files / (name + ".txt")
            path.write_text(text, encoding="utf-8")
            ex("edit " + str(path))
            screen_contains(text.splitlines()[0])
            return path

        try:
            current = until(lambda current: bool(current["nodes"]))
            for _ in range(10):
                current = ui("snapshot")
                if current["page"] == "Home" and not any(node["id"].startswith("modal/") for node in current["nodes"]):
                    break
                modal = any(node["id"].startswith("modal/") and node["actionable"] for node in current["nodes"])
                key("F10" if current["page"] == "FirstRunSetup" and not modal else "enter")
                shell.drain(0.2)
            until(lambda current: current["page"] == "Home")
            key("e")
            until(lambda current: current["page"] == "Explorer")
            key("l", "--modifiers", "ctrl")
            key("a", "--modifiers", "ctrl")
            ui("text", str(files))
            key("enter")
            current = until(lambda current: any(node["role"] == "listitem" and "sample.txt" in node["text"] for node in current["nodes"]))
            row = next(node for node in current["nodes"] if node["role"] == "listitem" and "sample.txt" in node["text"])
            ui("click", "--id", row["id"], "--double")
            until(lambda current: current["page"] == "Editor")
            screen_contains(smoke.BASE.splitlines()[0])
            key("F8")
            if missing_engine:
                current = screen_contains("Install Vim engine")
                assert any(node["role"] == "progressbar" for node in current["nodes"]), current
                cancel = next(node for node in current["nodes"] if node["id"].endswith("editor.vim-install.cancel"))
                ui("click", "--id", cancel["id"])
                screen_contains("Vim installation cancelled")
                assert document.read_text() == smoke.BASE
                print("PASS: missing engine opens the real installation dialog and progress bar; pointer cancellation preserves the document", flush=True)
                key("F8")
                screen_contains("has not been published yet")
                assert document.read_text() == smoke.BASE
                print("PASS: an unpublished engine is reported without changing the document", flush=True)
                return
            screen_contains("F8 VIM mode enabled")
            screen_contains(smoke.BASE.splitlines()[0])
            print("PASS: UI navigation and F8 entered the real Vim terminal", flush=True)

            keys("gg0i")
            ui("text", "中文😀-")
            key("esc")
            ex("write")
            expect_file(document, "中文😀-" + smoke.BASE)
            key("u")
            ex("write")
            expect_file(document, smoke.BASE)
            key("r", "--modifiers", "ctrl")
            ex("write")
            expect_file(document, "中文😀-" + smoke.BASE)
            print("PASS: Insert, Esc, Unicode, undo and Ctrl+R", flush=True)

            path = open_case("motions")
            keys("gg0wdw")
            ex("write")
            expect_file(path, "alpha gamma\nsecond line\nthird line\n")
            keys("ugg02wciw")
            ui("text", "GAMMA")
            key("esc")
            ex("write")
            expect_file(path, "alpha beta GAMMA\nsecond line\nthird line\n")
            print("PASS: word motions, counts, dw and ciw text object", flush=True)

            path = open_case("visual")
            keys("gg0Vj")
            screen_contains("VISUAL LINE")
            keys("yGp")
            ex("write")
            expect_file(path, smoke.BASE + "alpha beta gamma\nsecond line\n")
            print("PASS: Visual-line selection, yank and paste", flush=True)

            path = open_case("register-macro")
            keys('gg0"ayyG"apgg0qaxjq@a')
            ex("write")
            expect_file(path, "lpha beta gamma\necond line\nthird line\nalpha beta gamma\n")
            print("PASS: named registers and recorded macros", flush=True)

            path = open_case("search", "one needle\nneedle two\nlast\n")
            keys("gg0/needle")
            key("enter")
            keys("nciw")
            ui("text", "FOUND")
            key("esc")
            ex("write")
            expect_file(path, "one needle\nFOUND two\nlast\n")
            ex("%s/needle/replaced/g")
            ex("write")
            expect_file(path, "one replaced\nFOUND two\nlast\n")
            print("PASS: / search, n and Ex substitution", flush=True)

            ex("vsplit")
            key("w", "--modifiers", "ctrl")
            key("w")
            ex("close")
            ex("help")
            screen_contains("VIM - main help file")
            ex("close")
            print("PASS: native split windows, Ctrl+W and :help", flush=True)

            path = open_case("toggle")
            keys("gg0i")
            ui("text", "DRAFT-")
            key("esc")
            key("F8")
            screen_contains("VIM mode disabled")
            screen_contains("DRAFT-alpha")
            assert path.read_text() == smoke.BASE
            key("F8")
            screen_contains("F8 VIM mode enabled")
            key("u")
            ex("write")
            expect_file(path, smoke.BASE)
            print("PASS: F8 preserves unsaved text and undo without saving", flush=True)

            keys("gg0i")
            ui("text", "DISCARD-")
            key("esc")
            ex("quit")
            screen_contains("E37")
            ex("quit!")
            until(lambda current: current["page"] == "Explorer")
            assert path.read_text() == smoke.BASE
            print("PASS: :q rejects unsaved changes and :q! discards them", flush=True)
            print("PASS: 8 Vim workflows through tundra-cli ui", flush=True)
        except Exception:
            print(shell.screen(), file=sys.stderr)
            raise
        finally:
            shell.close()


if __name__ == "__main__":
    main()
