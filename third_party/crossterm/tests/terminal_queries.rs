#[test]
fn queue_works_when_ok_is_shadowed() {
    fn ok(_: ()) {}
    use ok as Ok;
    Ok(());
    let mut bytes = Vec::new();
    crossterm::queue!(bytes, crossterm::cursor::MoveTo(2, 3)).unwrap();
    assert_eq!(bytes, b"\x1b[4;3H");
}

#[cfg(unix)]
mod unix {
    use std::io::Write;

    #[test]
    fn queries_use_tty_and_ignore_stale_cursor_replies() {
        // Python provides PTY setup and bounded reads without another test dependency.
        let script = r#"
import fcntl, os, select, subprocess, sys, termios, time
master, slave = os.openpty()
def setup():
    os.setsid()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
child = subprocess.Popen([sys.argv[1], '--ignored', '--exact',
    'unix::query_child', '--nocapture'], stdin=slave, stdout=subprocess.PIPE,
    stderr=subprocess.PIPE, preexec_fn=setup)
os.close(slave)
output = bytearray()
terminal = bytearray()
keyboard = stale = cursor = False
deadline = time.monotonic() + 8
try:
    while child.poll() is None:
        assert time.monotonic() < deadline, (output, terminal)
        for fd in select.select([master, child.stdout], [], [], .05)[0]:
            try:
                data = os.read(fd if isinstance(fd, int) else fd.fileno(), 4096)
            except OSError:
                data = b''
            (terminal if fd == master else output).extend(data)
        if not keyboard and b'\x1b[?u\x1b[c' in terminal:
            os.write(master, b'\x1b[?1u\x1b[?1;2c')
            keyboard = True
        if not stale and b'READY' in output:
            os.write(master, b'\x1b[2;3Rx')
            stale = True
        if not cursor and b'\x1b[6n' in output:
            os.write(master, b'\x1b[7;9R')
            cursor = True
    rest, errors = child.communicate(timeout=2)
    output.extend(rest)
    assert child.returncode == 0, (output, terminal, errors)
    assert keyboard and stale and cursor, (output, terminal)
    assert b'queries checked' in output, output
    assert b'\x1b[?u' not in output, output
finally:
    if child.poll() is None:
        child.kill()
        child.wait()
    os.close(master)
"#;
        let output = std::process::Command::new("python3")
            .args(["-c", script])
            .arg(std::env::current_exe().unwrap())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "launched by the PTY parent with a private controlling terminal"]
    fn query_child() {
        use crossterm::{event, terminal};
        terminal::enable_raw_mode().unwrap();
        assert!(terminal::supports_keyboard_enhancement().unwrap());
        println!("READY");
        std::io::stdout().flush().unwrap();
        assert_eq!(
            event::read().unwrap(),
            event::Event::Key(event::KeyEvent::new(
                event::KeyCode::Char('x'),
                event::KeyModifiers::NONE
            ))
        );
        assert_eq!(crossterm::cursor::position().unwrap(), (8, 6));
        terminal::disable_raw_mode().unwrap();
        println!("queries checked");
    }
}
