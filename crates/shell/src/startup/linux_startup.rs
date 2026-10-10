//! Linux executable entry checks, before storage, recovery or worker startup.
use std::io::{self, IsTerminal, Write};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use platform::linux::identity::{LinuxUserContext, ProcessIdentity};

use crate::terminal_session::TemporaryRawMode;

/// Call once at executable entry. Library identity lookups must not prompt.
pub fn confirm_linux_startup() -> io::Result<LinuxUserContext> {
    let process = ProcessIdentity::current().validate()?;
    if process.uid == 0 {
        confirm_root_execution()?;
    }
    LinuxUserContext::current()
}

fn confirm_root_execution() -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(
        stderr,
        "WARNING: Tundra is running as root. File operations and launched programs will have root privileges and may modify or delete system files."
    )?;
    if !io::stdin().is_terminal() || !stderr.is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Root startup requires an interactive terminal and explicit confirmation.",
        ));
    }

    // Reuse the terminal guard so cancellation, I/O errors and unwinding restore
    // the original mode. No alternate screen or application state is opened.
    let mut raw_mode = TemporaryRawMode::enter().map_err(io::Error::other)?;
    write!(
        stderr,
        "Press y to continue as root; any other key cancels: "
    )?;
    stderr.flush()?;
    let answer = (|| -> io::Result<bool> {
        loop {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    return Ok(
                        key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::NONE
                    );
                }
                Event::Paste(_) => return Ok(false),
                _ => {}
            }
        }
    })();
    raw_mode.restore()?;
    writeln!(stderr)?;
    if answer? {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Root startup cancelled.",
        ))
    }
}
