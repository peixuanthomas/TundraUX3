use std::io::Write;

pub(crate) fn write_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "TundraUX3 CLI")?;
    writeln!(
        output,
        "Usage: tundra-cli <config|doctor|editor|explain|new|paths|test-frost|test-matrix|weathr>"
    )?;
    writeln!(
        output,
        "  config  View or update user config: get [field], set <theme|language|timezone|address> <value>"
    )?;
    writeln!(
        output,
        "  doctor  Check Windows/macOS, terminal, and app path readiness"
    )?;
    writeln!(
        output,
        "  editor  Launch the shell directly into the Markdown editor"
    )?;
    writeln!(
        output,
        "  explain Show CLI startup flow and kernel/UI boundaries"
    )?;
    writeln!(
        output,
        "  new     Clear saved TundraUX3 data and recreate initial storage"
    )?;
    writeln!(output, "  paths   Print configured and resolved app paths")?;
    writeln!(
        output,
        "  test-frost  Play only the startup frost banner animation"
    )?;
    writeln!(
        output,
        "  test-matrix Play only the first-run Matrix banner animation"
    )?;
    writeln!(output, "  weathr  Launch the terminal weather scene")
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
        "  2. tundra-cli handles diagnostics, operator commands, config, and launchers: doctor, editor, paths, explain, new, test-frost, test-matrix, weathr."
    )?;
    writeln!(
        output,
        "  3. tundra-shell shows the banner, initializes the UX shell, then enters the main loop."
    )?;
    writeln!(
        output,
        "  4. The main loop will route input to UI controllers; Phase 0 uses a placeholder loop."
    )?;
    writeln!(output)?;
    writeln!(output, "Kernel boundary:")?;
    writeln!(
        output,
        "  - tundra-platform is the platform boundary for OS facts, paths, terminal checks, and future platform API calls."
    )?;
    writeln!(
        output,
        "  - tundra-storage owns config/state format boundaries: TOML config and schema-v1 JSON state."
    )?;
    writeln!(
        output,
        "  - UI and app code must call these crates instead of touching platform APIs or storage paths directly."
    )?;
    writeln!(output)?;
    writeln!(output, "UI boundary:")?;
    writeln!(
        output,
        "  - tundra-shell owns startup visuals, shell lifecycle, and the future event/render loop."
    )?;
    writeln!(
        output,
        "  - UI code consumes view state and commands; it should not create platform-specific paths or call platform APIs directly."
    )
}
