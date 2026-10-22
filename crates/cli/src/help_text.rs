use std::io::Write;

pub(crate) fn write_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "{}", i18n::tr!("startup-cli-help"))
}

pub(crate) fn write_debug_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "{}", i18n::tr!("startup-cli-debug-help"))
}

pub(crate) fn write_ui_style_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "Usage: tundra-cli debug view-ui-style <1|2|3>")?;
    for style in shell::UiStyleVersion::ALL {
        writeln!(
            output,
            "  {}  {}: {}",
            style.number(),
            style.title(),
            style.description()
        )?;
    }
    writeln!(
        output,
        "All versions use Rust/Ratatui and existing Tundra components; Tea is inspired by Bubble Tea, not a Go integration."
    )?;
    writeln!(
        output,
        "F1-F3 switch styles; F4 toggles motion; F5 replays; Tab moves focus; Esc closes the dialog or exits; Ctrl-C exits."
    )?;
    writeln!(
        output,
        "Use arrows or mouse to select, type in the text field, and activate the buttons. Demo progress is simulated; preferences are not saved."
    )
}

pub(crate) fn write_explain(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "TundraUX3 startup and boundary model")?;
    writeln!(output)?;
    writeln!(output, "Startup flow:")?;
    writeln!(
        output,
        "  1. User starts tundra-cli or tundra-shell from a crossterm-compatible terminal."
    )?;
    writeln!(output, "{}", i18n::tr!("startup-cli-explain-commands"))?;
    writeln!(
        output,
        "  3. tundra-shell shows the banner, initializes the UX shell, then enters the main loop."
    )?;
    writeln!(
        output,
        "  4. The main loop reads keyboard/mouse input, runs the selected action, and redraws the UI."
    )?;
    writeln!(output)?;
    writeln!(output, "Kernel boundary:")?;
    writeln!(
        output,
        "  - platform reads OS facts and paths, checks the terminal, and calls OS services."
    )?;
    writeln!(
        output,
        "  - storage reads and writes TOML configuration and versioned JSON state."
    )?;
    writeln!(
        output,
        "  - UI and app code must call these crates instead of touching platform APIs or storage paths directly."
    )?;
    writeln!(output)?;
    writeln!(output, "UI boundary:")?;
    writeln!(
        output,
        "  - tundra-shell owns startup visuals, application sessions, and the event/render loop."
    )?;
    writeln!(
        output,
        "  - UI code consumes view state and commands; it should not create platform-specific paths or call platform APIs directly."
    )
}

pub(crate) fn write_asset_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "TundraUX3 asset test command")?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  tundra-cli debug asset <name>")?;
    writeln!(output, "  tundra-cli debug asset <name> -a")?;
    writeln!(output, "  tundra-cli debug asset <name> --<item>")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(
        output,
        "  -a          Print the complete original asset file"
    )?;
    writeln!(
        output,
        "  --<item>    Print one item from a TOML art set, such as --launcher"
    )?;
    writeln!(output)?;
    writeln!(output, "Examples:")?;
    writeln!(output, "  tundra-cli debug asset banner")?;
    writeln!(output, "  tundra-cli debug asset explorer_icons -a")?;
    writeln!(output, "  tundra-cli debug asset explorer_icons --folder")?;
    writeln!(output, "  tundra-cli debug asset home_icons --launcher")?;
    writeln!(
        output,
        "  tundra-cli debug asset launcher_icons --builtin.command-line"
    )?;
    writeln!(output)?;
    writeln!(output, "Available assets:")?;
    for asset in ascii_assets::required_assets() {
        writeln!(output, "  {}", asset.key)?;
    }
    Ok(())
}

// Recognize command paths, not arbitrary option values, so a help request can
// never accidentally perform a reset, start a preview, or edit configuration.
pub(crate) fn parse_help_topic(args: &[String]) -> Result<crate::CliCommand, crate::CliError> {
    use crate::{AssetAction, ClearLogsAction, CliCommand, LogsAction};
    let topic = args.join(" ");
    if args.first().is_some_and(|group| group == "ui")
        && (args.len() == 1
            || (args.len() == 2 && crate::ui_command::VERBS.contains(&args[1].as_str())))
    {
        return Ok(CliCommand::Ui(crate::UiCommand::Help));
    }
    if args.first().is_some_and(|group| {
        crate::management_command::kind(group).is_some() || group == "operations"
    }) {
        if args.len() == 1
            || (args.len() == 2
                && crate::management_command::verbs(&args[0]).contains(&args[1].as_str()))
        {
            return Ok(CliCommand::Management(crate::ManagementCli::Help(
                args[0].clone(),
            )));
        }
    }
    Ok(match topic.as_str() {
        "" => CliCommand::Help,
        "debug" => CliCommand::DebugHelp,
        "debug asset" => CliCommand::Asset(AssetAction::Help),
        "debug view-ui-style" => CliCommand::UiStyleHelp,
        "debug clear-logs" => CliCommand::ClearLogs(ClearLogsAction::Help),
        "logs" | "logs query" | "logs incidents" | "logs export" | "logs follow" => {
            CliCommand::Logs(LogsAction::Help)
        }
        "config"
        | "config get"
        | "config set"
        | "config reset"
        | "config options"
        | "launcher"
        | "launcher list"
        | "launcher pin"
        | "launcher unpin"
        | "new"
        | "cls"
        | "repl"
        | "doctor"
        | "debug paths"
        | "debug explain"
        | "debug test-frost"
        | "debug test-aa-style1"
        | "debug test-aa-style2"
        | "debug test-aa-style3"
        | "debug screen-keyboard"
        | "debug ui-test"
        | "debug test-rainbow-text"
        | "debug test-matrix"
        | "debug test-watchdog-error"
        | "debug test-watchdog-critical"
        | "debug test-watchdog-panic" => CliCommand::TopicHelp(topic),
        _ => return Err(crate::CliError::UnknownCommand(topic)),
    })
}

pub(crate) fn write_config_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "{}", i18n::tr!("startup-cli-config-help"))
}

pub(crate) fn write_topic_help(output: &mut impl Write, topic: &str) -> std::io::Result<()> {
    if topic == "debug test-rainbow-text" {
        return writeln!(output, "{}", i18n::tr!("rainbow-text-help"));
    }
    if topic == "debug ui-test" {
        return writeln!(output, "{}", i18n::tr!("ui-test-help"));
    }
    if topic == "config" || topic.starts_with("config ") {
        return write_config_help(output);
    }
    let localized = match topic {
        "doctor" => Some("startup-cli-doctor-help"),
        "repl" => Some("startup-cli-repl-help"),
        "launcher" | "launcher list" | "launcher pin" | "launcher unpin" => {
            Some("startup-cli-launcher-help")
        }
        "debug screen-keyboard" => Some("startup-cli-screen-keyboard-help"),
        _ => None,
    };
    if let Some(message) = localized {
        return writeln!(output, "{}", i18n::tr!(message));
    }
    writeln!(output, "Usage: tundra-cli {topic}")?;
    let detail = match topic {
        "debug paths" => {
            "Show path templates and the resolved config, data, state, logs, cache, and temporary paths."
        }
        "debug explain" => {
            "Describe CLI startup and which parts handle storage, OS calls, and the UI."
        }
        "new" => {
            "Erase saved TundraUX3 configuration and state, then create defaults. Requires typing RESET.\nFirst use debug paths and back up the displayed config/state directories. To reset one setting, use config reset <field>."
        }
        "cls" => {
            "Clear terminal scrollback and screen, then move the cursor home. Saved logs are kept."
        }
        "debug test-frost" | "debug test-matrix" => {
            "Play an animation preview in the current terminal; settings are not changed."
        }
        "debug test-aa-style1" | "debug test-aa-style2" | "debug test-aa-style3" => {
            "Preview an AutoAdmin (AA) warning style without running operations or saving settings. Requires an interactive terminal. C: confirmation; R: running; F: finished; B: compare the background before/after dimming. Tab/arrow keys and mouse select buttons; Enter activates; Esc/Ctrl+C returns."
        }
        "debug test-watchdog-error" | "debug test-watchdog-critical" => {
            "Write an intentional diagnostic report and print its JSON/text paths. Requires the normal CLI watchdog runtime."
        }
        "debug test-watchdog-panic" => {
            "Trigger a real panic and discard the current session. Embedded Command Line asks Shell to panic. This is an intentional failure test."
        }
        _ => "Use help for the command list.",
    };
    writeln!(output, "{detail}")
}

pub(crate) fn write_error_help(output: &mut impl Write, args: &[String]) -> std::io::Result<()> {
    match args.first().map(String::as_str) {
        Some(group)
            if crate::management_command::kind(group).is_some() || group == "operations" =>
        {
            crate::management_command::help(output, group)
        }
        Some("config") => write_config_help(output),
        Some("ui") => crate::ui_command::help(output),
        Some("launcher") => write_topic_help(output, "launcher"),
        Some("debug") => match args.get(1).map(String::as_str) {
            Some("asset") => write_asset_help(output),
            Some("view-ui-style") => write_ui_style_help(output),
            Some("clear-logs") => crate::clear_logs_command::help(output),
            Some(
                "paths"
                | "explain"
                | "test-frost"
                | "test-aa-style1"
                | "test-aa-style2"
                | "test-aa-style3"
                | "screen-keyboard"
                | "ui-test"
                | "test-rainbow-text"
                | "test-matrix"
                | "test-watchdog-error"
                | "test-watchdog-critical"
                | "test-watchdog-panic",
            ) => write_topic_help(output, &format!("debug {}", args[1])),
            _ => write_debug_help(output),
        },
        Some("doctor") => write_topic_help(output, "doctor"),
        Some("logs") => crate::logs_command::write_logs_help(output),
        _ => write_help(output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_root_debug_config_and_packages_help_keep_command_syntax() {
        let language = i18n::LanguageSnapshotData {
            code: "zh-CN".into(),
            generation: 1,
            current: vec![include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../ascii-assets/assets/locales/zh-CN/modules/regression-startup.ftl")).into(), include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../ascii-assets/assets/locales/zh-CN/modules/management-cli-administration.ftl")).into(), include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../ascii-assets/assets/locales/zh-CN/modules/ui-test.ftl")).into()],
            english: vec![],
        }.restore().unwrap();
        i18n::with_snapshot(&std::sync::Arc::new(language), || {
            for (writer, expected, syntax) in [
                (
                    write_help as fn(&mut Vec<u8>) -> std::io::Result<()>,
                    "命令行",
                    "doctor",
                ),
                (write_debug_help, "资源", "paths"),
                (
                    write_config_help,
                    "字段与取值",
                    "config set animation-speed 125",
                ),
            ] {
                let mut output = Vec::new();
                writer(&mut output).unwrap();
                let text = String::from_utf8(output).unwrap();
                assert!(text.contains(expected), "{text}");
                assert!(text.contains(syntax), "{text}");
                assert!(!text.contains("[startup-cli"));
            }
            let mut output = Vec::new();
            crate::management_command::help(&mut output, "packages").unwrap();
            let text = String::from_utf8(output).unwrap();
            assert!(text.contains("软件源"));
            assert!(text.contains("--apply --expected-version"));
        });
    }
}
