use crate::CliCommand;
use std::io::Write;

pub(crate) fn run(command: CliCommand, out: &mut impl Write, err: &mut impl Write) -> i32 {
    #[cfg(target_os = "linux")]
    let result = execute(command).and_then(|value| writeln!(out, "{value}"));
    #[cfg(not(target_os = "linux"))]
    let result: std::io::Result<()> = {
        let _ = (command, out);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "System sessions and privileged operations are available only on Linux",
        ))
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(err, "ERROR: {error}");
            1
        }
    }
}

#[cfg(target_os = "linux")]
fn execute(command: CliCommand) -> std::io::Result<String> {
    use session_protocol::{SystemAction, linux};
    use std::io;
    match command {
        CliCommand::Session(action) if action == "status" => {
            let user = linux::current_user()?;
            let logind = linux::current_session().ok().map(|s| s.identity);
            let managed = linux::managed_snapshot().ok().flatten();
            serde_json::to_string_pretty(
                &serde_json::json!({"user": user, "logind": logind, "managed": managed}),
            )
            .map_err(io::Error::other)
        }
        CliCommand::Session(action) => {
            let method = match action.as_str() {
                "lock" => "Lock",
                "logout" => "Logout",
                "switch" => "SwitchUser",
                _ => return Err(io::Error::other("unknown session action")),
            };
            linux::session_action(method)?;
            Ok("Session request accepted".into())
        }
        CliCommand::System(args) => {
            let action = match args
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["poweroff"] => SystemAction::PowerOff,
                ["reboot"] => SystemAction::Reboot,
                ["logs"] => SystemAction::ReadSystemLogs {
                    max_records: 1000,
                    since_epoch_seconds: 0,
                },
                ["update", release] => SystemAction::InstallUpdate {
                    release_id: (*release).into(),
                },
                _ => {
                    return Err(io::Error::other(
                        "usage: system poweroff|reboot|logs|update <vX.Y.Z>",
                    ));
                }
            };
            linux::request_system_action(&action, &std::sync::atomic::AtomicBool::new(false))
        }
        CliCommand::MigrateLegacy(args) => {
            use std::os::unix::fs::MetadataExt;
            use std::os::unix::process::CommandExt;
            let path = "/usr/libexec/tundra/tundra-system-maintenance";
            // This package-owned helper is never resolved through PATH or the user installation.
            for ancestor in std::path::Path::new(path).ancestors() {
                let metadata = std::fs::symlink_metadata(ancestor)?;
                if metadata.uid() != 0
                    || metadata.mode() & 0o022 != 0
                    || metadata.file_type().is_symlink()
                {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "unsafe maintenance helper installation",
                    ));
                }
            }
            Err(std::process::Command::new(path)
                .arg("migrate-legacy")
                .args(args)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LANG", "C.UTF-8")
                .exec())
        }
        _ => Err(io::Error::other("unknown system command")),
    }
}
