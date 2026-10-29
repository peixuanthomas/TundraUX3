#!/usr/bin/env python3
"""Verify launcher menus, bounded icons and custom commands in a real Linux PTY."""
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import shlex
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time


def main():
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/tundra-shell").resolve()
    cli = binary.with_name("tundra-cli")
    with tempfile.TemporaryDirectory(prefix="tundra-ui-control-") as temporary:
        root = Path(temporary)
        env = os.environ.copy()
        env["TERM"] = "xterm-256color"
        for key in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR"):
            folder = root / key.lower()
            folder.mkdir(mode=0o700)
            env[key] = str(folder)
        configured = subprocess.run([str(cli), "config", "set", "automation.enabled", "true"],
            env=env, capture_output=True, text=True, timeout=15)
        assert configured.returncode == 0, configured.stderr
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
        before = termios.tcgetattr(slave)
        output = bytearray()
        stopped = threading.Event()

        def drain():
            while not stopped.is_set():
                if select.select([master], [], [], 0.05)[0]:
                    try:
                        data = os.read(master, 65536)
                    except OSError:
                        return
                    output.extend(data)
                    if len(output) > 2 * 1024 * 1024:
                        del output[:1024 * 1024]

        child = subprocess.Popen([str(binary)], env=env, stdin=slave, stdout=slave, stderr=slave,
            start_new_session=True)
        thread = threading.Thread(target=drain, daemon=True)
        thread.start()

        def ui(*args, okay=True, stdin=None):
            result = subprocess.run([str(cli), "ui", *map(str, args)], env=env,
                input=stdin, capture_output=True, text=True, timeout=10)
            response = json.loads(result.stdout)
            if okay:
                assert result.returncode == 0 and response["ok"], response
            return response

        def snapshot():
            return ui("snapshot")["snapshot"]

        def until(predicate, timeout=15):
            deadline = time.monotonic() + timeout
            last = None
            while time.monotonic() < deadline:
                response = ui("snapshot", okay=False)
                last = response.get("snapshot")
                if last and predicate(last):
                    return last
                assert child.poll() is None, bytes(output[-8000:]).decode(errors="replace")
                time.sleep(0.05)
            raise AssertionError(f"Shell condition timed out: {last and last['page']}")

        def key(name, *options):
            return ui("key", name, *options)["snapshot"]

        def click(identifier, double=False):
            ui("wait", "--id", identifier)
            return ui("click", "--id", identifier, *(["--double"] if double else []))

        def node_by(current, predicate):
            return next(node for node in current["nodes"] if predicate(node))

        batch_children = []
        try:
            if os.geteuid() == 0:
                deadline = time.monotonic() + 5
                while b"any other key cancels: " not in output and time.monotonic() < deadline:
                    time.sleep(0.02)
                assert b"any other key cancels: " in output
                os.write(master, b"y")
            current = until(lambda current: bool(current["nodes"]))
            if "--dump" in sys.argv:
                print(json.dumps(current, ensure_ascii=False, indent=2))
                return
            # Bare PTYs have no graphics responder, so dismiss that real warning first.
            for _ in range(8):
                current = snapshot()
                if current["page"] == "Home" and not any(node["id"].startswith("modal/") for node in current["nodes"]):
                    break
                if any(node["id"].startswith("modal/") and node["actionable"] for node in current["nodes"]):
                    key("Enter")
                elif current["page"] == "FirstRunSetup":
                    key("F10")
                else:
                    key("Enter")
                time.sleep(0.1)
            current = until(lambda current: current["page"] == "Home")
            print("PASS: external CLI completed first-use setup")
            key("a")
            current = until(lambda current: current["page"] == "Launcher")
            def toolbar(label):
                current = snapshot()
                button = node_by(current, lambda node: node["role"] == "button" and label in node["name"])
                click(button["id"])

            def items():
                return [node for node in snapshot()["nodes"] if node["role"] == "button" and "launcher.item." in node["id"]]

            first_page = [node["name"] for node in items()]
            point = items()[0]["rect"]
            ui("scroll", "--x", point["x"], "--y", point["y"], "--direction", "down")
            assert [node["name"] for node in items()] != first_page
            ui("scroll", "--x", point["x"], "--y", point["y"], "--direction", "up", "--count", 10)
            assert [node["name"] for node in items()] == first_page
            print("PASS: mouse wheel changes launcher pages and clamps at the first page")
            toolbar("Organize")
            cards = items()
            first, second = cards[:2]
            target = second["rect"]
            ui("drag", "--from-id", first["id"], "--to-x", target["x"] + target["width"] - 1,
               "--to-y", target["y"] + 1)
            assert items()[0]["name"] == second["name"]
            print("PASS: organize mode drags built-in icons without launching")
            toolbar("Add launcher")
            ui("text", "discarded launcher")
            key("Escape")
            toolbar("Add launcher")
            ui("text", "Custom PTY tool")
            key("Tab")
            ui("text", "printf 'CUSTOM_LAUNCHER_PTY_OK\\n'; exit 75")
            key("Tab")
            key("Right")
            key("F10")
            toolbar("Done organizing")
            key("End")
            current = snapshot()
            assert "Custom PTY tool" in "\n".join(current["lines"])
            import tomllib
            config_file = next(Path(env["XDG_CONFIG_HOME"]).rglob("config.toml"))
            config = tomllib.loads(config_file.read_text())
            custom = next(entry for entry in config["launcher"]["entries"] if entry.get("custom", {}).get("name") == "Custom PTY tool")
            assert custom["custom"]["icon"] == ""
            assert custom["custom"]["wait_for_enter"]
            assert config["launcher"]["order"]
            custom_id = custom["id"]

            def open_context():
                card = next(node for node in items() if f"launcher.item.{custom_id}" in node["id"])
                ui("click", "--id", card["id"], "--button", "right")
                return until(lambda current: not current["animation_running"]
                    and any("launcher.menu.0" in node["id"] for node in current["nodes"]))

            def context_action(label):
                current = open_context()
                button = node_by(current, lambda node: node["role"] == "button" and node["name"] == label)
                click(button["id"])

            open_context()
            ui("click", "--x", 1, "--y", 4)
            current = until(lambda current: not current["animation_running"]
                and not any("launcher.menu." in node["id"] for node in current["nodes"]))
            assert current["page"] == "Launcher"
            assert not any("launcher.menu." in node["id"] for node in current["nodes"])
            context_action("Change name")
            key("a", "--modifiers", "ctrl")
            ui("text", "Renamed PTY tool")
            key("F10")
            context_action("Change launch command")
            key("a", "--modifiers", "ctrl")
            ui("text", "printf 'EDITED_LAUNCHER_PTY_OK\\n'; exit 75")
            key("F10")
            context_action("Change icon")
            ui("text", "中文x\n★")
            ui("text", "!")
            assert "Icons allow two rows" in "\n".join(snapshot()["lines"])
            key("F10")
            current = snapshot()
            assert "中文x" in "\n".join(current["lines"])
            config = tomllib.loads(config_file.read_text())
            custom = next(entry for entry in config["launcher"]["entries"] if entry["id"] == custom_id)
            assert custom["custom"]["name"] == "Renamed PTY tool"
            assert custom["custom"]["icon"] == "中文x\n★"
            assert custom["custom"]["wait_for_enter"]
            print("PASS: context menu closes outside; name, command and bounded Unicode icon persist")
            key("Enter")
            current = until(lambda current: current["page"] == "CommandLine"
                and "EDITED_LAUNCHER_PTY_OK" in "\n".join(current["lines"])
                and "Press Enter to return." in "\n".join(current["lines"]))
            assert any(node["role"] == "terminal" for node in current["nodes"])
            key("Enter")
            current = until(lambda current: current["page"] == "Launcher")
            print("PASS: custom settings persist; output stays in the real PTY until Enter returns")
            for code in [0, 9]:
                toolbar("Organize")
                toolbar("Add launcher")
                ui("text", f"Immediate exit {code}")
                key("Tab")
                marker = root / f"immediate-{code}.txt"
                ui("text", f"printf done > {shlex.quote(str(marker))}; exit {code}")
                key("F10")
                toolbar("Done organizing")
                key("End")
                key("Enter")
                current = until(lambda current: current["page"] == "Launcher" and marker.exists())
                assert marker.read_text() == "done"
            print("PASS: the default returns immediately after successful and failed custom commands")
            context_action("Delete this icon")
            key("Escape")
            assert any(entry["id"] == custom_id for entry in tomllib.loads(config_file.read_text())["launcher"]["entries"])
            context_action("Delete this icon")
            key("Enter")
            assert not any(entry["id"] == custom_id for entry in tomllib.loads(config_file.read_text())["launcher"]["entries"])
            key("Home")
            ui("click", "--id", items()[0]["id"], "--button", "right")
            assert not any("launcher.menu." in node["id"] for node in snapshot()["nodes"])
            print("PASS: deletion confirms or cancels; built-in entries retain their restrictions")
        except Exception:
            print("Last snapshot:", json.dumps(locals().get("current"), ensure_ascii=False), file=sys.stderr)
            visible = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(output[-16000:]))
            print("Shell status:", child.poll(), "\n", visible.decode(errors="replace"), file=sys.stderr)
            raise
        finally:
            for process in batch_children:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=5)
            stopped.set()
            thread.join(timeout=1)
            after = termios.tcgetattr(slave)
            os.close(master)
            os.close(slave)
            assert after == before, "Shell did not restore terminal modes"


if __name__ == "__main__":
    main()
