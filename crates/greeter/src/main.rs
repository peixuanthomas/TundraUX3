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
    use std::{io, os::unix::fs::MetadataExt, path::Path, sync::Arc, time::Duration};
    use tundra_greeter::{
        channel::{self, ClientMessage},
        input,
        model::Greeter,
        reader::ChannelReader,
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
            let executable = std::env::current_exe()?.canonicalize()?;
            let locales = locale_root(&executable);
            validate_trusted_tree(&locales)?;
            i18n::LanguageSnapshot::load(&locales, &locale, 0)
                .map_err(io::Error::other)?
                .snapshot
        };
        let _locale_guard = i18n::enter_snapshot(Arc::new(locale_snapshot));
        // sessiond owns lifecycle. Keep diagnostics private and ephemeral; do
        // not inherit user-selected paths or record conversation breadcrumbs.
        let diagnostics = tempfile::Builder::new()
            .prefix("tundra-greeter-")
            .tempdir_in("/tmp")?;
        let root = diagnostics.path();
        let config = watchdog::WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("state"),
            "tundra-greeter",
            env!("CARGO_PKG_VERSION"),
        )
        .with_unclean_exit_tracking(false);
        let (_runtime, process) =
            watchdog::WatchdogRuntime::start(config).map_err(io::Error::other)?;
        let app = process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("greeter"),
                "Trusted greeter",
                env!("CARGO_PKG_VERSION"),
                watchdog::AppCriticality::SessionCritical,
            ))
            .map_err(io::Error::other)?;
        let reader =
            ChannelReader::new(channel.try_clone()?, &app.task_group("protected-channel"))?;
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
            while let Some(message) = reader.try_message()? {
                // Remove terminal input queued before each new authentication/consent view.
                while event::poll(Duration::ZERO)? {
                    let _ = event::read()?;
                }
                greeter.receive(message);
                if greeter.is_complete() {
                    return Ok(());
                }
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

    fn locale_root(executable: &Path) -> std::path::PathBuf {
        let versions = Path::new("/var/lib/tundra/runtime/versions");
        if let Ok(relative) = executable.strip_prefix(versions) {
            let parts: Vec<_> = relative.components().collect();
            if let [
                std::path::Component::Normal(version),
                std::path::Component::Normal(bin),
                std::path::Component::Normal(binary),
            ] = parts.as_slice()
            {
                if *bin == "bin" && *binary == "tundra-greeter" {
                    return versions.join(version).join("share/tundra/greeter/locales");
                }
            }
        }
        Path::new(TRUSTED_LOCALES).to_path_buf()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn locale_source_tracks_exact_installed_version_without_user_paths() {
            assert_eq!(
                locale_root(Path::new(
                    "/var/lib/tundra/runtime/versions/v1.3.0/bin/tundra-greeter"
                )),
                Path::new("/var/lib/tundra/runtime/versions/v1.3.0/share/tundra/greeter/locales")
            );
            for executable in [
                "/usr/libexec/tundra/tundra-greeter",
                "/tmp/version/bin/tundra-greeter",
                "/var/lib/tundra/runtime/versions/../user/bin/tundra-greeter",
                "/var/lib/tundra/runtime/versions/v1.3.0/bin/other",
                "/var/lib/tundra/runtime/versions/v1.3.0/nested/bin/tundra-greeter",
            ] {
                assert_eq!(
                    locale_root(Path::new(executable)),
                    Path::new(TRUSTED_LOCALES)
                );
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
