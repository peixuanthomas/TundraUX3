"""Exercise the bundled debugger independently of Rust compilation, on Linux."""
import os
import base64
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parents[3]
with tempfile.TemporaryDirectory(prefix="tundra-bashdb-check-") as directory:
    target = Path(directory)
    for name in ["init", "lib", "command"]:
        shutil.copytree(root / "third_party/bashdb" / name, target / name)
    for script in target.rglob("*.sh"):
        script.write_text(script.read_text())
    for name in ["dbg-main.sh", "getopts_long.sh", "set-d-vars.sh"]:
        (target / name).write_text((root / "third_party/bashdb" / name).read_text())
    trace = (root / "third_party/bashdb/bashdb-trace.in").read_text()
    for key, value in {"@PACKAGE@": "bashdb", "@SH_PROG@": "/bin/bash", "@prefix@": "/", "@PKGDATADIR@": "/none"}.items():
        trace = trace.replace(key, value)
    (target / "bashdb-trace").write_text(trace)
    (target / "bootstrap.sh").write_text((root / "crates/script-runtime/src/bridge.sh").read_text())
    (target / "script.sh").write_text('set -euo pipefail\nx=1\nprintf "result=%s\\n" "$x"\n')
    os.mkfifo(target / "control.in", 0o600)
    descriptor = os.open(target / "control.in", os.O_RDWR | os.O_NONBLOCK)
    (target / "control.out").touch()
    environment = dict(os.environ, BASH_ENV=str(target / "bootstrap.sh"), TUNDRA_SCRIPT_DEBUG_ROOT=str(target))
    process = subprocess.Popen(["bash", "-e", "-u", str(target / "script.sh")], env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        deadline = time.monotonic() + 5
        while "__TUNDRA__\tSTOP " not in (target / "control.out").read_text():
            assert process.poll() is None, process.communicate()
            assert time.monotonic() < deadline, (target / "control.out").read_text()
            time.sleep(.02)
        os.write(descriptor, b"tundra inspect\n")
        deadline = time.monotonic() + 5
        while "INSPECTED" not in (target / "control.out").read_text():
            assert time.monotonic() < deadline, (target / "control.out").read_text()
            time.sleep(.02)
        os.write(descriptor, b"eval capture='a  b\\c'\ntundra inspect\n")
        deadline = time.monotonic() + 5
        while (target / "control.out").read_text().count("INSPECTED") < 2:
            assert time.monotonic() < deadline, (target / "control.out").read_text()
            time.sleep(.02)
        variables = [line.split(" ", 1)[1] for line in (target / "control.out").read_text().splitlines() if line.startswith("__TUNDRA__\tVARS ")][-1]
        declarations = base64.b64decode(variables).decode()
        assert 'capture="a  b\\\\c"' in declarations, declarations
        for request, (operation, expression, expected) in enumerate([
            ("examine", "$capture", "a  b\\c"),
            ("eval", "printf '<%s>' a\\\n", "<a>"),
        ], start=1):
            encoded = base64.b64encode(expression.encode()).decode()
            os.write(descriptor, f"tundra {operation} {encoded} {request}\n".encode())
            deadline = time.monotonic() + 5
            while f"__TUNDRA__\tEVAL_END {request}" not in (target / "control.out").read_text():
                assert time.monotonic() < deadline, (target / "control.out").read_text()
                time.sleep(.02)
            evaluation = (target / "control.out").read_text().split(f"__TUNDRA__\tEVAL_BEGIN {request}", 1)[1].split(f"__TUNDRA__\tEVAL_END {request}", 1)[0]
            assert expected in evaluation, evaluation
        os.write(descriptor, b"continue\n")
        output, errors = process.communicate(timeout=5)
        assert process.returncode == 0, (output, errors)
        assert output == b"result=1\n", (output, errors)
        print("native bashdb bridge: initial stop, inspection, expression whitespace/backslashes, continue and exit passed")
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate()
        os.close(descriptor)
