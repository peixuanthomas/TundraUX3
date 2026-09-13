#!/usr/bin/env python3
"""Exercise the real Shell, PackageKit and system agents in a disposable container.

The fixed password belongs only to the disposable tundra-test account. The test
harness types it into system-owned prompts and checks it never appears in Shell
output/state/logs. It is never a real user's password. Fedora /bin/login creates
the test OS session; Tundra itself must not create one. pyte answers terminal
cursor queries so TerminalGuard restoration can be checked with a real PTY.
See README.md for the signed fixture and fresh-baseline requirements.
"""
from pathlib import Path
import codecs, fcntl, os, pty, select, signal, struct, subprocess, sys, termios, time
import pyte
assert Path('/run/.containerenv').is_file() and os.getuid() == 0
case = sys.argv[1] if len(sys.argv) > 1 else 'preview'
assert case in ('preview', 'success', 'cancel', 'denied', 'crash', 'interrupt', 'eof', 'package_error', 'gui', 'power_denied')
state_name = f'pty-state-{case}-{os.getpid()}-{time.time_ns()}'
root = Path('/srv/tundra-fixture')
assert (root / 'prepared').is_file(), 'Prepare the signed disposable fixture first'
assert subprocess.check_output(['rpm', '-qf', '--qf', '%{NAME}', '/opt/tundra-fixture/bin/packagekit-update-probe'], text=True) == 'tundraux3'
secret = 'FixtureOnly-A7t9-password'
power_rule = Path('/etc/polkit-1/rules.d/00-tundra-power-denied.rules')
if case == 'power_denied':
    power_rule.write_text('polkit.addRule(function(action, subject) { if (subject.user == \"tundra-test\" && action.id.indexOf(\"org.freedesktop.login1.power-off\") == 0) return polkit.Result.NO; });\n')
fail_marker = root / 'fail-install'
if case == 'package_error':
    fail_marker.touch()
else:
    fail_marker.unlink(missing_ok=True)
if case != 'preview':
    subprocess.run(['usermod', '-aG', 'wheel', 'tundra-test'], check=True)
    subprocess.run(['chpasswd'], input=f'tundra-test:{secret}\n', text=True, check=True)
    rule = Path('/etc/polkit-1/rules.d/49-tundra-fixture.rules')
    if rule.exists():
        rule.unlink()
    time.sleep(1)
trace = (root / 'packagekit-signals.txt').open('wb')
monitor = subprocess.Popen(['dbus-monitor', '--system', "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',sender='org.freedesktop.PackageKit'"], stdout=trace, stderr=subprocess.DEVNULL)
xserver = None
xlog = None
if case == 'gui':
    xlog = (root / 'xvfb.log').open('wb')
    xserver = subprocess.Popen(['Xvfb', ':99', '-screen', '0', '1024x768x24', '-nolisten', 'tcp'], stdout=xlog, stderr=subprocess.STDOUT)
    gui_env = dict(os.environ, DISPLAY=':99')
    time.sleep(1)
pid, master = pty.fork()
if pid == 0:
    env = dict(os.environ, TERM='xterm-256color', LANG='en_US.UTF-8', LC_ALL='C.UTF-8')
    os.execve('/bin/login', ['login', '-f', 'tundra-test'], env)
front_pid = pid
fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 140, 0, 0))
initial = termios.tcgetattr(master)
os.set_blocking(master, False)
screen = pyte.Screen(140, 40)
screen.write_process_input = lambda data: os.write(master, data.encode())
stream = pyte.Stream(screen)
decoder = codecs.getincrementaldecoder('utf-8')('replace')
output = bytearray()

def frame():
    return '\n'.join(screen.display)

def read(timeout=0.1):
    if select.select([master], [], [], timeout)[0]:
        try:
            chunk = os.read(master, 65536)
        except (OSError, BlockingIOError):
            return
        if chunk:
            output.extend(chunk)
            stream.feed(decoder.decode(chunk))

def wait(text, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        read()
        if text in frame():
            return
    raise RuntimeError('Missing ' + text + '\n' + frame().replace(secret, '[REDACTED]'))

def send(value):
    os.write(master, value)
    end = time.monotonic() + 0.25
    while time.monotonic() < end:
        read(0.02)

def settle():
    until = time.monotonic() + 1
    while time.monotonic() < until:
        read()

def save(name):
    (root / (name + '.txt')).write_text(frame().replace(secret, '[REDACTED]'))
try:
    wait('$ ', 40)
    if case == 'gui':
        send(b'export DISPLAY=:99 QT_QPA_PLATFORM=xcb\r')
        send(f"exec dbus-run-session -- sh -c '/usr/libexec/kf6/polkit-kde-authentication-agent-1 > /home/tundra-test/gui-agent.log 2>&1 & echo $! > /home/tundra-test/gui-agent.pid; sleep 1; exec env TERM=xterm-256color LC_ALL=C.UTF-8 XDG_CONFIG_HOME=/home/tundra-test/{state_name}/xdg_config_home XDG_DATA_HOME=/home/tundra-test/{state_name}/xdg_data_home XDG_CACHE_HOME=/home/tundra-test/{state_name}/xdg_cache_home XDG_STATE_HOME=/home/tundra-test/{state_name}/xdg_state_home /opt/tundra-fixture/bin/packagekit-update-probe'\r".encode())
    else:
        send(f'exec env TERM=xterm-256color LC_ALL=C.UTF-8 XDG_CONFIG_HOME=/home/tundra-test/{state_name}/xdg_config_home XDG_DATA_HOME=/home/tundra-test/{state_name}/xdg_data_home XDG_CACHE_HOME=/home/tundra-test/{state_name}/xdg_cache_home XDG_STATE_HOME=/home/tundra-test/{state_name}/xdg_state_home /opt/tundra-fixture/bin/packagekit-update-probe\r'.encode())
    wait('Status', 40)
    print('Startup frame reached', flush=True)
    front_pid = os.tcgetpgrp(master)
    if case == 'gui':
        for p in Path('/proc').iterdir():
            try:
                if p.name.isdigit() and (p / 'comm').read_text().strip() == 'packagekit-upda':
                    front_pid = int(p.name)
                    break
            except OSError:
                continue
    assert front_pid != pid, 'Expected the Shell foreground job within Fedora login'
    status = Path(f'/proc/{front_pid}/status').read_text()
    assert next((line for line in status.splitlines() if line.startswith('Uid:'))).split()[1:3] == ['1000', '1000']
    until = time.monotonic() + 2
    while time.monotonic() < until:
        read()
    save('startup')
    for _ in range(3):
        if '[Continue]' not in frame():
            break
        send(b'\x1b')
        until = time.monotonic() + 1
        while time.monotonic() < until:
            read()
    if 'Appearance' in frame() and 'Explorer' not in frame():
        for _ in range(5):
            send(b'\t')
        send(b'\r')
    wait('Explorer')
    settle()
    save('home')
    print('Home reached', flush=True)
    if case == 'power_denied':
        send(b'\x1b')
        wait('Exit & power')
        offset = len(output)
        send(b'p')
        wait('Permission denied', 40)
        settle()
        attrs = termios.tcgetattr(master)
        assert not attrs[3] & termios.ICANON and not attrs[3] & termios.ECHO
        for sequence in (b'\x1b[?1049h', b'\x1b[?1003h', b'\x1b[?1004h', b'\x1b[?25l'):
            assert sequence in output[offset:], 'terminal mode not restored after denied power request'
        assert b'Password:' not in output and b'AUTHENTICATING' not in output
        save('auth-power-denied')
    else:
        send(b'\x1b[C')
        send(b'\x1b[C')
        send(b'\r')
        print('Settings requested', flush=True)
        wait('Sections')
        settle()
        print('Settings reached', flush=True)
        for _ in range(5):
            send(b'\t')
        wait('Settings: Update')
        wait('2.0.0-1', 40)
        save('update-check')
        send(b'\x1b[B' * 3 + b'\r')
        wait('Install reviewed update', 30)
        save('update-preview')
        if case == 'preview':
            send(b'\x1b')
        else:
            until = time.monotonic() + 1
            while time.monotonic() < until:
                read()
            offset = len(output)
            send(b'\r')
            print('Authorization requested', flush=True)
            if case == 'gui':
                agent_pid = Path('/home/tundra-test/gui-agent.pid').read_text().strip()
                handled = None
                prompts = 0
                deadline = time.monotonic() + 60
                while 'Update installed successfully' not in frame() and time.monotonic() < deadline:
                    read()
                    result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', agent_pid], env=gui_env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
                    windows = result.stdout.split()
                    if not windows:
                        handled = None
                        time.sleep(0.3)
                        continue
                    window = windows[0]
                    if window == handled:
                        continue
                    subprocess.run(['xdotool', 'windowfocus', '--sync', window], env=gui_env, check=True)
                    subprocess.run(['xdotool', 'type', '--clearmodifiers', '--file', '-'], env=gui_env, input=secret.encode(), check=True)
                    subprocess.run(['xdotool', 'key', 'Return'], env=gui_env, check=True)
                    prompts += 1
                    handled = window
                assert prompts > 0, 'No existing graphical agent challenge was observed'
                wait('Update installed successfully', 5)
                print('Existing graphical agent prompts:', prompts, flush=True)
                assert b'AUTHENTICATING' not in output and b'Password:' not in output, 'Unexpected TTY agent prompt'
                assert secret.encode() not in Path('/home/tundra-test/gui-agent.log').read_bytes()
            else:
                wait('Password:', 40)
                print('System password prompt reached', flush=True)
                assert b'\x1b[?1049l' in output[offset:], 'alternate screen was not suspended'
                attrs = termios.tcgetattr(master)
                assert not attrs[3] & termios.ECHO, 'system agent did not disable echo'
                if case in ('success', 'package_error'):
                    send(secret.encode() + b'\r')
                elif case == 'cancel':
                    send(b'\x03')
                elif case == 'denied':
                    send(b'wrong-fixture-password\r')
                elif case == 'interrupt':
                    os.kill(front_pid, signal.SIGTERM)
                elif case == 'eof':
                    send(b'\x04')
                elif case == 'crash':
                    children = subprocess.check_output(['pgrep', '-P', str(front_pid), 'pkttyagent'], text=True).split()
                    assert len(children) == 1
                    os.kill(int(children[0]), signal.SIGKILL)
                if case == 'success':
                    wait('Update installed successfully', 45)
                elif case == 'package_error':
                    wait('The system service could not complete the operation', 45)
                elif case in ('denied', 'crash', 'eof'):
                    deadline = time.monotonic() + 45
                    while b'\x1b[?1049h' not in output[offset:] and time.monotonic() < deadline:
                        read()
                    wait('Installation and package updates', 10)
                    settle()
                    assert 'Update installed successfully' not in frame()
                elif case in ('cancel', 'interrupt'):
                    deadline = time.monotonic() + 4
                    while time.monotonic() < deadline:
                        read()
            save('auth-' + case)
            assert secret.encode() not in output, 'fixture credential reached output'
            if case not in ('cancel', 'interrupt'):
                attrs = termios.tcgetattr(master)
                assert not attrs[3] & termios.ICANON and (not attrs[3] & termios.ECHO), 'raw mode was not resumed'
                assert b'\x1b[?1049h' in output[offset:], 'alternate screen was not resumed'
                assert b'\x1b[?1003h' in output[offset:], 'mouse capture was not resumed'
                assert b'\x1b[?1004h' in output[offset:], 'focus reporting was not resumed'
                assert b'\x1b[?25l' in output[offset:], 'cursor was not hidden after resume'
    user_state = Path(f'/home/tundra-test/{state_name}')
    for path in user_state.rglob('*'):
        assert path.stat().st_uid == 1000, 'user state contains a file owned by another UID'
        if path.is_file():
            assert secret.encode() not in path.read_bytes(), 'fixture credential reached user state/logs'
    installed = subprocess.check_output(['rpm', '-q', '--qf', '%{VERSION}', 'tundraux3'], text=True)
    assert installed == ('2.0.0' if case in ('success', 'gui') else '1.0.0'), 'Unexpected installed RPM after transaction'
    print('PTY stage passed:', case)
finally:
    if case == 'power_denied':
        power_rule.unlink(missing_ok=True)
    fail_marker.unlink(missing_ok=True)
    if xserver is not None:
        xserver.terminate()
        xserver.wait()
        xlog.close()
    monitor.terminate()
    monitor.wait()
    trace.close()
    try:
        os.kill(front_pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    deadline = time.monotonic() + 15
    ended = False
    while time.monotonic() < deadline:
        read()
        done, status = os.waitpid(pid, os.WNOHANG)
        if done:
            ended = True
            break
    if not ended:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
    restored = termios.tcgetattr(master)
    (root / ('pty-' + case + '.ansi')).write_bytes(bytes(output).replace(secret.encode(), b'[REDACTED]'))
    os.close(master)
    assert ended, 'Shell did not terminate cleanly'
    assert bool(restored[3] & termios.ICANON) == bool(initial[3] & termios.ICANON), 'canonical mode not restored'
    assert bool(restored[3] & termios.ECHO) == bool(initial[3] & termios.ECHO), 'echo not restored'
    print('Terminal canonical/echo restored:', case)
