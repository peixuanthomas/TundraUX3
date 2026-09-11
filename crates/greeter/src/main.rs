#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux::run() {
        // Never log channel payloads or PAM responses.
        eprintln!("tundra-greeter: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("tundra-greeter requires Linux and a protected sessiond channel");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod linux {
    use crossterm::{
        event::{
            self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste,
            EnableMouseCapture,
        },
        execute,
        terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use std::{
        io::{self, BufReader},
        os::unix::fs::MetadataExt,
        path::Path,
        sync::{Arc, mpsc},
        time::Duration,
    };
    use tundra_greeter::{
        channel::{self, ClientMessage, ServerMessage},
        input,
        model::Greeter,
    };
    use zeroize::Zeroize;

    const TRUSTED_LOCALES: &str = "/usr/share/tundra/greeter/locales";

    pub fn run() -> io::Result<()> {
        let mut args = std::env::args().skip(1);
        if args.next().as_deref() != Some("--channel-fd") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: tundra-greeter --channel-fd N [--locale en-US|zh-CN]",
            ));
        }
        let fd = args
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid channel fd"))?;
        let locale = match args.next().as_deref() {
            None => "en-US".to_owned(),
            Some("--locale") => args
                .next()
                .filter(|v| matches!(v.as_str(), "en-US" | "zh-CN"))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "unsupported trusted locale")
                })?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unknown argument",
                ));
            }
        };
        if args.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unexpected argument",
            ));
        }
        // Greeter itself never needs root, and must not leave core dumps containing secrets.
        if unsafe { libc::getuid() } == 0 || unsafe { libc::geteuid() } == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "greeter must run as its dedicated unprivileged account",
            ));
        }
        if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) } < 0
            || unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut channel = channel::inherited_root_channel(fd)?;
        let locale_snapshot = if locale == "en-US" {
            i18n::LanguageSnapshot::embedded(0)
        } else {
            validate_trusted_tree(Path::new(TRUSTED_LOCALES))?;
            i18n::LanguageSnapshot::load(TRUSTED_LOCALES, &locale, 0)
                .map_err(io::Error::other)?
                .snapshot
        };
        let _locale_guard = i18n::enter_snapshot(Arc::new(locale_snapshot));
        let reader = channel.try_clone()?;
        let (sender, messages) = mpsc::sync_channel(8);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            loop {
                let message = channel::read_frame::<ServerMessage>(&mut reader);
                let failed = message.is_err();
                if sender.send(message).is_err() || failed {
                    break;
                }
            }
        });
        let mut guard = TerminalGuard(false);
        terminal::enable_raw_mode()?;
        guard.0 = true;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
        let mut greeter = Greeter::default();
        terminal.draw(|f| greeter.render(f))?;
        channel::write_frame(&mut channel, &ClientMessage::Ready {})?;
        loop {
            while let Ok(message) = messages.try_recv() {
                let message = message?;
                // Remove terminal input queued before each new authentication/consent view.
                while event::poll(Duration::ZERO)? {
                    let _ = event::read()?;
                }
                greeter.receive(message);
            }
            terminal.draw(|f| greeter.render(f))?;
            if greeter.is_complete() {
                return Ok(());
            }
            if event::poll(Duration::from_millis(40))? {
                if let Some(event) = input::translate(event::read()?) {
                    let (columns, rows) = terminal::size()?;
                    if let Some(mut response) =
                        greeter.handle(event, ratatui::layout::Rect::new(0, 0, columns, rows))
                    {
                        let result = channel::write_frame(&mut channel, &response);
                        if let ClientMessage::PamResponse { response, .. } = &mut response {
                            response.zeroize();
                        }
                        result?;
                    }
                }
            }
        }
    }

    struct TerminalGuard(bool);
    impl Drop for TerminalGuard {
        fn drop(&mut self) {
            if self.0 {
                let _ = execute!(
                    io::stdout(),
                    DisableBracketedPaste,
                    DisableMouseCapture,
                    LeaveAlternateScreen
                );
                let _ = terminal::disable_raw_mode();
            }
        }
    }

    fn validate_trusted_tree(path: &Path) -> io::Result<()> {
        for ancestor in path.ancestors() {
            validate_metadata(ancestor)?;
        }
        fn visit(path: &Path) -> io::Result<()> {
            let metadata = validate_metadata(path)?;
            if metadata.is_dir() {
                for entry in std::fs::read_dir(path)? {
                    visit(&entry?.path())?;
                }
            } else if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "trusted resource is not a regular file",
                ));
            }
            Ok(())
        }
        visit(path)
    }
    fn validate_metadata(path: &Path) -> io::Result<std::fs::Metadata> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 || metadata.file_type().is_symlink()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "greeter resources must be root owned and not writable by other users",
            ));
        }
        Ok(metadata)
    }
}
