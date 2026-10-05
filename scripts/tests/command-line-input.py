#!/usr/bin/env python3
"""Real-PTY regressions for REPL input and command environment/cwd (Unix)."""
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import shlex
import struct
import sys
import termios
import time
import tempfile

binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/tundra-cli").resolve()
pid, master = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.environ["TUNDRA_COMMAND_LINE_USERNAME"] = "input-test"
    os.execv(str(binary), [str(binary), "repl", "--embedded"])
fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
output = bytearray()
workspace = tempfile.TemporaryDirectory(prefix="tundra-command-state-")
current_directory = Path.cwd().resolve()


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


def wait_for_prompt():
    wait_for(f"input-test@{current_directory} >>".encode())


def run_command(command, expected=None, code=0):
    output.clear()
    os.write(master, command.encode() + b"\r")
    wait_for(f"[system exit code: {code}]".encode())
    wait_for_prompt()
    wait_for(f"\x1b[777;1;{int(code != 0)}z".encode())
    if expected is not None:
        assert expected in output, bytes(output)
    assert b"could not retain command state" not in output, bytes(output)


try:
    wait_for_prompt()
    assert "○\x1b[777;0z".encode() in output, bytes(output)
    folder = Path(workspace.name).resolve() / "space and 中文"
    folder.mkdir()
    run_command("export PERSISTED='kept=value'")
    current_directory = folder
    run_command("cd " + shlex.quote(str(folder)))
    run_command("cd /tundra-path-that-does-not-exist",
                code=1 if sys.platform == "darwin" else 2)
    output.clear()
    os.write(master, b"/help\r")
    wait_for(b"/config set motion reduced")
    wait_for_prompt()
    wait_for(b"\x1b[777;1;0z")
    for invalid in ("invalid-command", "/help '", "/repl", "/", "/echo MUST_NOT_RUN"):
        output.clear()
        os.write(master, invalid.encode() + b"\r")
        wait_for_prompt()
        wait_for(b"\x1b[777;1;1z")
    assert b"Remove the '/' prefix: echo MUST_NOT_RUN" in output, bytes(output)
    # A system executable can have the same name as a UX command. Only /help
    # should show UX help; help must run the executable on the session's PATH.
    system_help = folder / "help"
    system_help.write_text("#!/bin/sh\nprintf 'SYSTEM_HELP_RAN\\n'\n")
    system_help.chmod(0o700)
    run_command("export PATH=" + shlex.quote(str(folder)) + ':"$PATH"')
    run_command("help", b"SYSTEM_HELP_RAN")
    assert b"Usage: tundra-cli" not in output, bytes(output)
    run_command("/usr/bin/printf 'absolute:%s\\n' ok", b"absolute:ok")
    run_command("printf 'state:%s:%s\\n' \"$PERSISTED\" \"$PWD\"",
                f"state:kept=value:{folder}".encode())
    # The command still reads the real PTY; the state protocol must not
    # consume its stdin or route output through a non-terminal pipe.
    output.clear()
    os.write(master, b"printf 'read-%s' ready; read ANSWER; printf 'reply:%s\\n' \"$ANSWER\"\r")
    wait_for(b"read-ready")
    os.write(master, b"typed-in-terminal\r")
    wait_for(b"reply:typed-in-terminal")
    wait_for(b"[system exit code: 0]")
    wait_for_prompt()
    run_command("printf contents > relative-file")
    assert (folder / "relative-file").read_text() == "contents"
    run_command("export AFTER_FAILURE=retained; false", code=1)
    run_command("printf 'after:%s\\n' \"$AFTER_FAILURE\"", b"after:retained")
    run_command("unset PERSISTED")
    run_command("test \"${PERSISTED+x}\" != x")
    long_folder = folder / ("long-directory-" * 8)
    long_folder.mkdir()
    current_directory = long_folder
    run_command("cd " + shlex.quote(str(long_folder)))
    run_command("pwd", str(long_folder).encode())
    print("PASS: system default, UX prefix/hints, command name collisions, absolute executables/prompts, environment, cwd, failure recovery, and terminal stdin")
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
    workspace.cleanup()
