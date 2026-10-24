#!/usr/bin/env python3
"""Exercise the real Shell through its external CLI in an isolated Linux PTY."""
import fcntl
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
            assert current["width"] == 140 and current["height"] == 40
            assert len(current["lines"]) == 40
            ids = [node["id"] for node in current["nodes"]]
            assert len(ids) == len(set(ids))
            key("s")
            current = until(lambda current: current["page"] == "Settings")
            print("PASS: external key opened Settings and returned its painted UI tree")
            click("page/settings.category.system")
            current = ui("wait", "--id", "page/settings.field.automationenabled")["snapshot"]
            toggle = node_by(current, lambda node: node["id"] == "page/settings.field.automationenabled")
            assert toggle["role"] == "checkbox" and toggle["selected"]
            for _ in range(4):
                click("page/settings.field.screentimeout")
            config_path = next(Path(env["XDG_CONFIG_HOME"]).rglob("config.toml"))
            assert "screen_timeout_seconds = 30" in config_path.read_text()
            key("F1")
            until(lambda current: any(node["id"].startswith("modal/") for node in current["nodes"]))
            current = until(lambda current: current["page"] == "screen_saver", timeout=35)
            assert any(node["id"] == "screen_saver/resume" for node in current["nodes"])
            assert "Press any key or tap to resume" in "\n".join(current["lines"])
            current = ui("wake")["snapshot"]
            assert current["page"] == "Settings"
            assert any(node["id"].startswith("modal/") for node in current["nodes"])
            current = ui("wake")["snapshot"]
            assert current["page"] == "Settings"
            assert any(node["id"].startswith("modal/") for node in current["nodes"])
            print("PASS: wake dismissed idle weather and preserved the page and help dialog")
            until(lambda current: current["page"] == "screen_saver", timeout=35)
            os.write(master, b"\x1b")
            current = until(lambda current: current["page"] == "Settings")
            assert any(node["id"].startswith("modal/") for node in current["nodes"])
            assert not termios.tcgetattr(slave)[3] & (termios.ICANON | termios.ECHO)
            os.write(master, b"\x1b")
            until(lambda current: not any(node["id"].startswith("modal/") for node in current["nodes"]))
            print("PASS: configured idle weather and keyboard wake preserved Settings and its help dialog")
            click("chrome/shell.back")
            until(lambda current: current["page"] == "Home")
            key("e")
            until(lambda current: current["page"] == "Explorer")
            document = root / "sample.rs"
            document.write_text("// original\n", encoding="utf-8")
            key("l", "--modifiers", "ctrl")
            key("a", "--modifiers", "ctrl")
            ui("text", str(root))
            key("Enter")
            current = until(lambda current: any(node["role"] == "listitem" and "sample.rs" in node["text"] for node in current["nodes"]))
            row = node_by(current, lambda node: node["role"] == "listitem" and "sample.rs" in node["text"])
            click(row["id"], double=True)
            current = until(lambda current: current["page"] == "Editor" and "// original" in "\n".join(current["lines"]))
            click("page/editor.document")
            key("End", "--modifiers", "ctrl")
            pasted = ui("text", "--stdin", stdin="// 中文 external input\n")["snapshot"]
            assert "中文 external input" in "\n".join(pasted["lines"]), pasted["lines"]
            key("s", "--modifiers", "ctrl")
            until(lambda current: "中文 external input" in document.read_text(encoding="utf-8"))
            print("PASS: ID double click opened a file; UTF-8 stdin text saved through the real editor")
            current = snapshot()
            body = node_by(current, lambda node: node["id"].endswith("/editor.document"))
            area = body["rect"]
            ui("drag", "--from-x", area["x"], "--from-y", area["y"],
                "--to-x", area["x"] + 4, "--to-y", area["y"], "--duration", 160)
            ui("scroll", "--x", area["x"], "--y", area["y"], "--direction", "down", "--count", 2)
            key("F1")
            current = until(lambda current: any(node["id"].startswith("modal/") and node["role"] == "button" for node in current["nodes"]))
            blocked = node_by(current, lambda node: node["id"] == body["id"])
            assert not blocked["actionable"]
            refused = ui("click", "--id", blocked["id"], okay=False)
            assert refused["error"]["code"] == "target_unavailable"
            key("Escape")
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 41, 116, 0, 0))
            os.kill(child.pid, signal.SIGWINCH)
            current = until(lambda current: current["width"] == 116 and current["height"] == 41)
            assert len(current["lines"]) == 41
            print("PASS: drag, scroll, modal blocking and real terminal resize")
            click("chrome/shell.back")
            until(lambda current: current["page"] == "Explorer")
            click("chrome/shell.back")
            until(lambda current: current["page"] == "Home")
            key("a")
            current = until(lambda current: current["page"] == "Launcher")
            command = node_by(current, lambda node: node["role"] == "button" and "Command Line" in node["name"])
            click(command["id"], double=True)
            until(lambda current: current["page"] == "CommandLine")
            ui("text", "sleep 35; printf 'idle_background_ok\\n'")
            key("Enter")
            until(lambda current: current["page"] == "screen_saver", timeout=35)
            time.sleep(6)
            click("screen_saver/resume")
            current = until(lambda current: current["page"] == "CommandLine")
            assert not termios.tcgetattr(slave)[3] & (termios.ICANON | termios.ECHO)
            assert any(line.strip() == "idle_background_ok"
                for node in current["nodes"] if node["role"] == "terminal"
                for line in node["text"].splitlines())
            print("PASS: pointer wake returned to Command Line while its background process kept running")
            ui("text", "printf 'external_cli_terminal_ok\\n'")
            key("Enter")
            until(lambda current: any("external_cli_terminal_ok" in node["text"] for node in current["nodes"] if node["role"] == "terminal"))
            ui("text", "exit")
            key("Enter")
            until(lambda current: current["page"] == "Launcher")
            print("PASS: external input reached the embedded terminal and its output appeared in snapshots")
            click("chrome/shell.back")
            until(lambda current: current["page"] == "Home")
            key("s")
            until(lambda current: current["page"] == "Settings")
            click("page/settings.category.system")
            response = click("page/settings.field.automationenabled")
            assert response.get("closed") and response["ok"]
            assert ui("sessions")["sessions"] == []
            config = next(Path(env["XDG_CONFIG_HOME"]).rglob("config.toml")).read_text()
            assert "[automation]\nenabled = false" in config
            print("PASS: disabling control returns a closing response and persists the switch")
        except Exception:
            print("Last snapshot:", json.dumps(locals().get("current"), ensure_ascii=False), file=sys.stderr)
            visible = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(output[-16000:]))
            print("Shell status:", child.poll(), "\n", visible.decode(errors="replace"), file=sys.stderr)
            raise
        finally:
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
