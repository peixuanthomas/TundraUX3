use std::io::Write;

pub(crate) fn write_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "TundraUX3 CLI")?;
    writeln!(
        output,
        "Usage: tundra-cli <config|launcher|services|processes|packages|network|disks|users|system-config|operations|logs|debug|cls|new|repl|help>"
    )?;
    writeln!(
        output,
        "  cls     Clear terminal history and screen, then move the cursor home"
    )?;
    writeln!(
        output,
        "  config  Read, set, reset, or list values for UX settings; run config help"
    )?;
    writeln!(
        output,
        "  debug   Diagnostics and test commands; run debug help for details"
    )?;
    writeln!(
        output,
        "  new     Clear saved TundraUX3 data and recreate initial storage"
    )?;
    writeln!(
        output,
        "  repl    Enter the interactive command loop; system commands run by default, /<command> runs a UX command, exit leaves"
    )?;
    writeln!(
        output,
        "          System commands keep exported environment variables and the working directory until you leave this REPL."
    )?;
    writeln!(
        output,
        "  logs    Query runtime logs and incidents, or export diagnostics; run logs help"
    )?;
    writeln!(
        output,
        "  launcher  List, pin, or unpin applications; run launcher help"
    )?;
    writeln!(
        output,
        "  help [command]  Show help, for example: help config or help debug doctor"
    )?;
    writeln!(
        output,
        "\nUse <command> --help (or -h) for syntax, values, and examples."
    )?;
    writeln!(
        output,
        "In Command Line / repl, use / for UX commands: /config set motion reduced; /help shows UX help."
    )?;
    writeln!(
        output,
        "System commands need no prefix there: pwd, ls -la (Linux/macOS), dir (Windows). External CLI calls need no /: tundra-cli config set motion reduced."
    )?;
    writeln!(
        output,
        "Exit status: 0 success, 1 operation failed, 2 invalid arguments; logs may also return 3/130 (see logs help)."
    )
}

pub(crate) fn write_debug_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "Usage: tundra-cli debug <command>")?;
    writeln!(
        output,
        "  screen-keyboard  Try an English QWERTY keyboard with temporary text"
    )?;
    writeln!(
        output,
        "  test-aa-style1  AA preview: dimmed page + centered caution frame"
    )?;
    writeln!(
        output,
        "  test-aa-style2  AA preview: dimmed page + wide hazard banner"
    )?;
    writeln!(
        output,
        "  test-aa-style3  AA preview: dimmed page + split AA authorization panel"
    )?;
    writeln!(
        output,
        "  clear-logs <all|TYPE|--file PATH> [--yes]  Preview or clear runtime, incidents, snapshots, or legacy logs"
    )?;
    writeln!(
        output,
        "  view-ui-style [1|2|3]  Compare interactive UI styles and animations"
    )?;
    writeln!(
        output,
        "  asset   Print test assets or their original source files"
    )?;
    writeln!(
        output,
        "  doctor  Diagnose terminal, storage, and Linux runtime/tools; warnings are optional features"
    )?;
    writeln!(output, "  paths   Print configured and resolved app paths")?;
    writeln!(
        output,
        "  explain Show CLI startup flow and kernel/UI boundaries"
    )?;
    writeln!(
        output,
        "  test-frost  Play only the startup frost banner animation"
    )?;
    writeln!(
        output,
        "  test-matrix Play only the first-run Matrix banner animation"
    )?;
    writeln!(
        output,
        "  test-watchdog-error    Write an intentional error report"
    )?;
    writeln!(
        output,
        "  test-watchdog-critical Write an intentional critical error report"
    )?;
    writeln!(
        output,
        "  test-watchdog-panic    Trigger a real panic; the current session is discarded"
    )?;
    writeln!(
        output,
        "Error tests print JSON/text report paths. Panic tests enter the normal critical-error flow; embedded tests panic the Shell session."
    )
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
    writeln!(
        output,
        "  2. tundra-cli handles config, launcher pins, logs, cls, new, and repl; diagnostics and tests are under debug."
    )?;
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
        | "debug doctor"
        | "debug paths"
        | "debug explain"
        | "debug test-frost"
        | "debug test-aa-style1"
        | "debug test-aa-style2"
        | "debug test-aa-style3"
        | "debug screen-keyboard"
        | "debug test-matrix"
        | "debug test-watchdog-error"
        | "debug test-watchdog-critical"
        | "debug test-watchdog-panic" => CliCommand::TopicHelp(topic),
        _ => return Err(crate::CliError::UnknownCommand(topic)),
    })
}

pub(crate) fn write_config_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(
        output,
        "Usage: tundra-cli config [get [field] | set <field> <value> | reset <field> | options [field]]"
    )?;
    writeln!(
        output,
        "No arguments prints settings. reset restores only the named field. theme is a read-only color summary."
    )?;
    writeln!(output, "Fields and values:")?;
    writeln!(output, "  border-shape       rounded | square")?;
    writeln!(
        output,
        "  border-color       default | named color (e.g. light-cyan) | #RRGGBB"
    )?;
    writeln!(
        output,
        "  accent-color       default | named color | #RRGGBB"
    )?;
    writeln!(
        output,
        "  icon-mode          ascii | image (image needs terminal graphics support)"
    )?;
    writeln!(output, "  motion             full | reduced")?;
    writeln!(
        output,
        "  animation-speed    50..200 percent, in steps of 25; default 100"
    )?;
    writeln!(
        output,
        "  language           installed locale code; config options language lists choices"
    )?;
    writeln!(
        output,
        "  timezone / address timezone ID or city label; config options timezone lists choices"
    )?;
    writeln!(
        output,
        "  weather-location   English address in quotes (max 120 chars) | auto (follow timezone location)"
    )?;
    writeln!(
        output,
        "  update-mode        release | beta (Linux only; saves choice, does not start an update)"
    )?;
    writeln!(
        output,
        "address changes the timezone. weather-location changes only weather; it does not change system time."
    )?;
    writeln!(
        output,
        "Examples:\n  tundra-cli config set motion reduced\n  tundra-cli config set animation-speed 125\n  tundra-cli config set weather-location \"Shanghai, China\"\n  tundra-cli config reset weather-location\n  tundra-cli config set accent-color \"#38bdf8\""
    )?;
    writeln!(
        output,
        "Changes use the current OS user's config. Restart a running TundraUX3 UI to apply them. Identity/password fields are not exposed."
    )
}

pub(crate) fn write_topic_help(output: &mut impl Write, topic: &str) -> std::io::Result<()> {
    if topic == "config" || topic.starts_with("config ") {
        return write_config_help(output);
    }
    if topic == "launcher" || topic.starts_with("launcher ") {
        return writeln!(
            output,
            "Usage: tundra-cli launcher [list | pin <path> | unpin <id>]\n  list          Show saved IDs, target paths, and availability\n  pin <path>    Add a checked executable; quote paths with spaces; does not run it\n  unpin <id>    Remove only the pin, using an ID from list; keeps the file\nExamples:\n  tundra-cli launcher pin \"/home/user/My App/run.sh\"\n  tundra-cli launcher list\nPaths may be relative to the current command directory. Duplicate pins are ignored.\nUses the current OS user's config. Restart a running TundraUX3 UI to refresh its Launcher."
        );
    }
    writeln!(output, "Usage: tundra-cli {topic}")?;
    let detail = match topic {
        "debug doctor" => {
            "Check runtime prerequisites, paths, storage, and assets. Linux checks include /bin/sh, /proc, PTY access, service/network/disk tools and optional desktop integration.\nWARN means a feature may be unavailable; FAIL means a required check failed. Exit: 0 no failures, 1 failure.\nDirectory write probes create/remove temporary files; storage may be initialized or recovered. No packages are installed and no services are changed. Use debug paths for path templates."
        }
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
        "repl" => {
            "Start interactive Command Line. Use /help for UX commands and exit or EOF to leave.\nSystem commands run by default: ls -la or dir. UX commands require a leading /: /config set motion reduced. External CLI calls stay unchanged: tundra-cli config set motion reduced.\nFailed system input that looks like a UX command gets a / hint. Unknown / commands that look like system commands get a hint to remove /. Hints never execute another command.\nExported environment and working directory persist for this REPL session. cd changes the system command directory; /launcher pin uses that directory too.\nExamples: /config set motion reduced; pwd (Linux/macOS); cd (Windows)."
        }
        "debug test-frost" | "debug test-matrix" => {
            "Play an animation preview in the current terminal; settings are not changed."
        }
        "debug test-aa-style1" | "debug test-aa-style2" | "debug test-aa-style3" => {
            "Preview an AutoAdmin (AA) warning style without running operations or saving settings. Requires an interactive terminal. C: confirmation; R: running; F: finished; B: compare the background before/after dimming. Tab/arrow keys and mouse select buttons; Enter activates; Esc/Ctrl+C returns."
        }
        "debug screen-keyboard" => {
            "Open a standalone English QWERTY screen keyboard in the lower half of the terminal. Key widths adapt to the terminal size. Includes digits, symbols, F1-F12, Shift, Tab, CapsLock, Ctrl, Alt and right Ctrl. Click Shift/Ctrl/Alt for the next key; CapsLock stays on until toggled. Backspace removes the last character; on-screen Enter/Tab insert a newline/tab. Function keys and modified combinations appear in the last-key display. Copy/Ctrl+C copies all text; Paste/Ctrl+V appends clipboard text. Hide/Show collapses or expands the keyboard; Clear empties the text. Physical printable keys type directly. Tab/arrows select a button, then Enter/Space activates it; after direct typing Enter/Space inserts a newline/space. Exit or Esc returns. Requires an interactive external terminal; unavailable in embedded Command Line. Text is temporary unless copied; keys are not sent to other applications.\nRun from an external terminal: tundra-cli debug screen-keyboard."
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
        Some("launcher") => write_topic_help(output, "launcher"),
        Some("debug") => match args.get(1).map(String::as_str) {
            Some("asset") => write_asset_help(output),
            Some("view-ui-style") => write_ui_style_help(output),
            Some("clear-logs") => crate::clear_logs_command::help(output),
            Some(
                "doctor"
                | "paths"
                | "explain"
                | "test-frost"
                | "test-aa-style1"
                | "test-aa-style2"
                | "test-aa-style3"
                | "screen-keyboard"
                | "test-matrix"
                | "test-watchdog-error"
                | "test-watchdog-critical"
                | "test-watchdog-panic",
            ) => write_topic_help(output, &format!("debug {}", args[1])),
            _ => write_debug_help(output),
        },
        Some("logs") => crate::logs_command::write_logs_help(output),
        _ => write_help(output),
    }
}
