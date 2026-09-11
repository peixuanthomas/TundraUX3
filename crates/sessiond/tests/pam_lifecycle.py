#!/usr/bin/env python3
"""Root-only real-PAM integration probe; run in a detached systemd transient unit.

Usage: pam_lifecycle.py /root/credential.json authenticate|session valid|invalid
Credential file contains username/password and must not be group/world-readable.
No password is printed or passed through argv or the environment.
"""
import json
import os
import pwd
import socket
import stat
import subprocess
import sys
import time

assert os.geteuid() == 0
path, mode, validity = sys.argv[1:]
assert mode in ("authenticate", "session")
assert validity in ("valid", "invalid")
metadata = os.stat(path, follow_symlinks=False)
assert stat.S_ISREG(metadata.st_mode) and metadata.st_uid == 0
assert stat.S_IMODE(metadata.st_mode) & 0o077 == 0
with open(path) as source:
    credentials = json.load(source)
user = pwd.getpwnam(credentials["username"])
parent, child = socket.socketpair()
process = subprocess.Popen(
    ["/usr/libexec/tundra/tundra-sessiond", "--pam-worker", user.pw_name, str(child.fileno()), mode],
    pass_fds=(child.fileno(),),
    env={"PATH": "/usr/bin:/bin"},
)
child.close()
parent.settimeout(30)
stream = parent.makefile("rb")
opened = None
complete = False
try:
    while True:
        line = stream.readline(65537)
        if not line:
            break
        assert len(line) <= 65536
        message = json.loads(line)
        kind = message["type"]
        if kind == "PamPrompt":
            style = message["style"]
            if style == "EchoOff":
                response = credentials["password"] if validity == "valid" else "deliberately-invalid-test-password"
            elif style == "EchoOn":
                response = user.pw_name
            else:
                response = ""
            parent.sendall(json.dumps({"type": "PamResponse", "id": message["id"], "response": response}).encode() + b"\n")
        elif kind == "Authenticated":
            assert validity == "valid" and message["uid"] == user.pw_uid
        elif kind == "SessionOpened":
            assert validity == "valid" and mode == "session"
            opened = message["identity"]
            assert opened["uid"] == user.pw_uid
            assert opened["logind_session_id"]
            env = message["environment"]
            assert env["HOME"] == user.pw_dir
            assert env["USER"] == env["LOGNAME"] == user.pw_name
            assert env["XDG_RUNTIME_DIR"] == f"/run/user/{user.pw_uid}"
            assert env["DBUS_SESSION_BUS_ADDRESS"] == f"unix:path=/run/user/{user.pw_uid}/bus"
            assert "SUDO_UID" not in env and "DISPLAY" not in env
            actual = subprocess.check_output(["loginctl", "show-session", opened["logind_session_id"], "-p", "User", "--value"], text=True).strip()
            assert actual == str(user.pw_uid)
            parent.sendall(b'{"type":"Logout"}\n')
        elif kind == "Complete":
            complete = True
            break
        else:
            raise AssertionError("unexpected worker message type")
finally:
    parent.close()
    stream.close()
status = process.wait(timeout=20)
if validity == "valid":
    assert status == 0 and complete
    if mode == "session":
        assert opened
        for _ in range(100):
            query = subprocess.run(["loginctl", "show-session", opened["logind_session_id"]], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if query.returncode:
                break
            time.sleep(0.1)
        else:
            raise AssertionError("PAM/logind session remained registered")
else:
    assert status != 0 and not complete and opened is None
print(json.dumps({"username": user.pw_name, "mode": mode, "credential_case": validity, "passed": True, "uid": user.pw_uid}))
