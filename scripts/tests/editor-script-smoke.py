#!/usr/bin/env python3
"""Exercise the script workspace and real bashdb through a Linux Shell PTY.

Usage: python3 scripts/tests/editor-script-smoke.py [target/debug/tundra-shell] [--dump] [--skip-vim]
The companion CLI is used for visible controls. XDG state and scripts are temporary.
"""
from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "editor_vim_smoke", Path(__file__).with_name("editor-vim-smoke.py")
)
smoke = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(smoke)

SOURCE = """#!/usr/bin/env bash
value=3
bump() {
    local step=2
    value=$((value+step))
}
printf 'SCRIPT_START:%s\\n' "$0"
bump
printf 'SCRIPT_VALUE:%s\\n' "$value"
printf 'WAITING_FOR_INPUT\\n'
read -r answer
for output_line in {1..80}; do printf 'OUTPUT_LINE:%s\\n' "$output_line"; done; printf 'SCRIPT_ANSWER:%s\\n' "$answer"
"""


def main():
    assert os.name == "posix", "This test requires a Unix PTY"
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/tundra-shell").resolve()
    cli = binary.with_name("tundra-cli")
    assert binary.is_file() and cli.is_file(), (binary, cli)
    with tempfile.TemporaryDirectory(prefix="tundra-editor-script-") as temporary:
        root = Path(temporary)
        assets = root / "assets"
        shutil.copytree(Path(__file__).resolve().parents[2] / "crates/ascii-assets/assets", assets)
        os.environ["TUNDRA_ASCII_ASSETS_DIR"] = str(assets)
        files = root / "files"
        files.mkdir()
        document = files / "run 中文.sh"
        document.write_text("#!/usr/bin/env bash\nprintf 'ORIGINAL\\n'\n", encoding="utf-8")
        env = os.environ.copy()
        for name in ("CONFIG", "DATA", "CACHE", "STATE", "RUNTIME"):
            directory = root / name.lower()
            directory.mkdir(mode=0o700)
            env[f"XDG_{name}_HOME" if name != "RUNTIME" else "XDG_RUNTIME_DIR"] = str(directory)
        subprocess.run(
            [str(cli), "config", "set", "automation.enabled", "true"], env=env,
            check=True, capture_output=True, text=True, timeout=15,
        )
        shell = smoke.ShellPty(binary, root, None)
        shell.resize(144, 46)
        last = None

        def request(*arguments):
            with subprocess.Popen(
                [str(cli), "ui", *map(str, arguments), "--timeout", "15000"],
                env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            ) as process:
                deadline = time.monotonic() + 20
                while True:
                    shell.drain(0.01)
                    try:
                        output, error = process.communicate(timeout=0.02)
                        assert output, f"CLI returned no JSON: {error}"
                        return json.loads(output)
                    except subprocess.TimeoutExpired:
                        if time.monotonic() >= deadline:
                            process.kill()
                            process.communicate()
                            raise AssertionError("UI control client timed out")

        def ui(*arguments):
            nonlocal last
            result = request(*arguments)
            assert result.get("ok"), result
            last = result.get("snapshot")
            return last

        def until(predicate, timeout=20):
            nonlocal last
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                shell.drain(0.02)
                result = request("snapshot")
                last = result.get("snapshot")
                if last and predicate(last):
                    return last
                assert shell.child.poll() is None, shell.screen()
                time.sleep(0.04)
            raise AssertionError(f"UI condition timed out: {last and last['page']}\n{last and text(last)}")

        def text(current):
            return "\n".join(current["lines"])

        def key(name, modifiers=None):
            return ui("key", name, *(["--modifiers", modifiers] if modifiers else []))

        def node(suffix, current=None):
            current = current or ui("snapshot")
            return next(item for item in current["nodes"] if item["id"].endswith(suffix))

        def click(suffix):
            target = node(suffix)
            assert target["enabled"] and target["actionable"], target
            return ui("click", "--id", target["id"])

        def no_hover_click(suffix):
            target = node(suffix)
            assert target["enabled"] and target["actionable"], target
            rect = target["rect"]
            column = rect["x"] + max(0, rect["width"] // 2)
            row = rect["y"] + max(0, rect["height"] // 2)
            shell.send(f"\x1b[<0;{column+1};{row+1}M\x1b[<0;{column+1};{row+1}m".encode())
            return ui("snapshot")

        def has_terminal(current, fragment):
            return any(item["role"] == "terminal" and
                       (fragment in item["text"] or fragment in item["text"].replace("\n", ""))
                       for item in current["nodes"])

        def paused(line=None):
            return until(lambda current: "Paused" in text(current)
                         and (line is None or f"{document}:{line}" in text(current)))

        def source_focus():
            click("editor.document")

        def expression(value, execute=False):
            # Source -> Output -> Variables -> Stack -> Expression.
            source_focus()
            for _ in range(4):
                key("F6")
            key("a", "ctrl")
            ui("text", value)
            click("script.action.Evaluate" if execute else "script.action.Examine")

        def vim_flag():
            paths = list((root / "config").rglob("config.toml"))
            assert len(paths) == 1, paths
            match = re.search(r"(?m)^vim_mode_enabled\s*=\s*(true|false)\s*$", paths[0].read_text())
            assert match, paths[0].read_text()
            return match.group(1)

        def vim_processes():
            children_by_parent = {}
            for status in Path("/proc").glob("[0-9]*/status"):
                try:
                    parent = re.search(r"(?m)^PPid:\s*(\d+)\s*$", status.read_text())
                except FileNotFoundError:
                    continue
                if parent:
                    children_by_parent.setdefault(int(parent[1]), []).append(int(status.parent.name))
            pending = [shell.child.pid]
            found = set()
            while pending:
                parent = pending.pop()
                for pid in children_by_parent.get(parent, []):
                    pending.append(pid)
                    try:
                        executable = Path(os.readlink(f"/proc/{pid}/exe"))
                    except FileNotFoundError:
                        continue
                    if executable.name == "vim":
                        found.add(pid)
            return found

        def vim_ex(command):
            key("Escape")
            key(":")
            ui("text", command)
            key("Enter")

        try:
            if os.geteuid() == 0:
                shell.wait("any other key cancels:")
                shell.send(b"y")
            until(lambda current: bool(current["nodes"]), timeout=30)
            for _ in range(12):
                current = ui("snapshot")
                modal = any(item["id"].startswith("modal/") and item["actionable"] for item in current["nodes"])
                if current["page"] == "Home" and not modal:
                    break
                key("F10" if current["page"] == "FirstRunSetup" and not modal else "Enter")
            until(lambda current: current["page"] == "Home")
            key("a")
            current = until(lambda current: current["page"] == "Launcher")
            entry = next(item for item in current["nodes"] if item["role"] == "button" and item["name"] == "Script Editor")
            ui("click", "--id", entry["id"], "--double")
            until(lambda current: current["page"] == "Editor" and "Breakpoints" in text(current))
            print("PASS: Linux launcher opened source, output and optional detail tabs at 144x46", flush=True)

            key("o", "ctrl")
            until(lambda current: current["page"] == "Explorer")
            key("l", "ctrl")
            key("a", "ctrl")
            ui("text", str(files))
            key("Enter")
            current = until(lambda current: any(item["role"] == "listitem" and document.name in item["text"] for item in current["nodes"]))
            item = next(item for item in current["nodes"] if item["role"] == "listitem" and document.name in item["text"])
            ui("click", "--id", item["id"], "--double")
            until(lambda current: current["page"] == "Editor" and "ORIGINAL" in text(current))
            key("a", "ctrl")
            ui("text", SOURCE)
            key("F5", "ctrl")
            current = until(lambda current: has_terminal(current, "WAITING_FOR_INPUT"))
            assert has_terminal(current, f"SCRIPT_START:{document}"), "script $0 changed"
            assert document.read_text(encoding="utf-8") == SOURCE
            assert "text" not in node("editor.document")["actions"], "running source must be read-only"
            source_focus()
            key("F6")
            ui("text", "normal-input")
            key("Enter")
            current = until(lambda current: has_terminal(current, "SCRIPT_ANSWER:normal-input") and "Finished" in text(current))
            assert "text" in node("editor.document", current)["actions"], "editing did not resume"
            print("PASS: save-and-run preserved Unicode path/$0, accepted PTY stdin, and restored editing", flush=True)

            output = next(item["rect"] for item in current["nodes"] if item["role"] == "terminal")
            column, row = output["x"] + 1, output["y"] + output["height"] // 2
            shell.send((f"\x1b[<64;{column+1};{row+1}M" * 50).encode())
            current = until(lambda current: has_terminal(current, f"SCRIPT_START:{document}"))
            track = next(item["rect"] for item in current["nodes"] if item["role"] == "scrollbar"
                         and item["rect"]["x"] == output["x"] + output["width"]
                         and item["rect"]["y"] == output["y"])
            column, top = track["x"] + 1, track["y"] + 1
            bottom = track["y"] + track["height"]
            for start, end, expected in [(top, bottom, "SCRIPT_ANSWER:normal-input"),
                                          (bottom, top, f"SCRIPT_START:{document}"),
                                          (top, bottom, "SCRIPT_ANSWER:normal-input")]:
                shell.send(f"\x1b[<0;{column};{start}M\x1b[<32;{column};{end}M\x1b[<0;{column};{end}m".encode())
                until(lambda current: has_terminal(current, expected))
            print("PASS: finished output retained history for real wheel/scrollbar dragging and returned to latest", flush=True)

            source_focus()
            no_hover_click("script.action.Debug")
            paused(2)
            key("F7")
            paused()
            expression("printf 'EXPR_RESULT:%s\\n' \"$value\"", execute=True)
            current = until(lambda current: "EXPR_RESULT:3" in text(current))
            assert not has_terminal(current, "EXPR_RESULT:3"), "expression output leaked into program output"
            print("PASS: no-hover pointer started real bashdb; stepping and explicit evaluation returned separate results", flush=True)

            source_focus()
            key("g", "ctrl")
            for _ in range(16):
                key("Backspace")
            ui("text", "8")
            key("F10")
            key("F9")
            source_focus()
            key("F5")
            current = paused(8)
            if "--dump" in sys.argv:
                print("SCREEN 144x46:\n" + text(current), flush=True)
            key("F11")
            click("script.tab.Stack")
            current = until(lambda current: "Paused" in text(current) and any(
                "script.row.Stack." in item["id"] and "bump" in item["name"] for item in current["nodes"]))
            key("F7")
            paused()
            key("F11", "shift")
            paused()
            expression("printf 'AFTER_FUNCTION:%s\\n' \"$value\"", execute=True)
            until(lambda current: "AFTER_FUNCTION:5" in text(current))
            print("PASS: breakpoint, step-in, step-over and step-out followed a real Bash function", flush=True)

            shell.resize(108, 24)
            current = until(lambda current: current["width"] == 108 and current["height"] == 24 and "Evaluate" in text(current))
            tab = next(item for item in current["nodes"] if item["id"].endswith("script.tab.Expression"))
            ui("click", "--id", tab["id"])
            current = until(lambda current: any(item["role"] == "textbox" and "script.expression" in item["id"] for item in current["nodes"]))
            if "--dump" in sys.argv:
                print("SCREEN 108x24:\n" + text(current), flush=True)
            shell.resize(144, 46)
            until(lambda current: current["width"] == 144 and any(item["role"] == "textbox" and "script.expression" in item["id"] for item in current["nodes"]))
            print("PASS: compact 108x24 tabs and wide 144x46 layout kept the paused session", flush=True)

            source_focus()
            key("F5")
            until(lambda current: has_terminal(current, "WAITING_FOR_INPUT") and "Running" in text(current))
            no_hover_click("script.action.Stop")
            current = until(lambda current: "Finished" in text(current) and "text" in node("editor.document", current)["actions"])
            assert document.read_text(encoding="utf-8") == SOURCE
            print("PASS: stopping a waiting script cleaned its process and restored read-write source", flush=True)

            source_focus()
            key("F5", "ctrl")
            until(lambda current: has_terminal(current, "WAITING_FOR_INPUT") and "Running" in text(current))
            source_focus()
            key("F6")
            key("c", "ctrl")
            until(lambda current: "Finished" in text(current) and "text" in node("editor.document", current)["actions"])
            assert shell.child.poll() is None, "Ctrl+C exited the host instead of the script"
            print("PASS: Ctrl+C in Output interrupted the script while keeping Shell alive", flush=True)

            if "--skip-vim" in sys.argv:
                print("SKIP: real Vim handoff was explicitly excluded", flush=True)
                return
            assert binary.with_name("vim").joinpath("vim").is_file(), "Build the pinned Vim engine before this test"
            source_focus()
            key("F8")
            until(lambda current: "F8 VIM mode enabled" in text(current))
            before_vim = vim_processes()
            assert len(before_vim) == 1, before_vim
            assert vim_flag() == "true"
            vim_ex("let g:script_smoke_marker='same-vim'")
            key("G")
            key("o")
            ui("text", "# VIM_UNSAVED_SMOKE")
            key("Escape")
            assert document.read_text(encoding="utf-8") == SOURCE, "Vim unexpectedly saved the draft"
            key("F5")
            paused(2)
            assert document.read_text(encoding="utf-8") == SOURCE + "# VIM_UNSAVED_SMOKE\n"
            assert vim_flag() == "true", "debug launch changed the persisted Vim setting"
            assert vim_processes() == before_vim, "debug launch replaced the Vim process"
            key("F5", "shift")
            until(lambda current: "F8 VIM mode enabled" in text(current) and "Finished" in text(current))
            assert vim_processes() == before_vim, "ending debug replaced the Vim process"
            assert vim_flag() == "true", "ending debug changed the persisted Vim setting"
            vim_ex("echo 'VIM_SCRIPT:' . get(g:,'script_smoke_marker','missing')")
            until(lambda current: "VIM_SCRIPT:same-vim" in text(current))
            key("G")
            key("A")
            ui("text", "_EDITABLE")
            key("Escape")
            vim_ex("write")
            until(lambda _: document.read_text(encoding="utf-8") == SOURCE + "# VIM_UNSAVED_SMOKE_EDITABLE\n")
            print("PASS: Vim exported/saved its draft, kept its setting/PID/state, and resumed editing after debug", flush=True)
            key("F8")
            until(lambda current: "F8 VIM mode disabled" in text(current))
            key("F1")
            until(lambda current: "F6 / Shift+F6" in text(current) and "Shift+F11" in text(current))
            print("PASS: script-specific F1 help matches the implemented controls", flush=True)
        except Exception:
            print("LAST SNAPSHOT:\n" + (text(last) if last else shell.screen()), file=sys.stderr)
            raise
        finally:
            shell.close()
    print("PASS: PTY modes restored; temporary scripts and XDG state removed", flush=True)


if __name__ == "__main__":
    main()
