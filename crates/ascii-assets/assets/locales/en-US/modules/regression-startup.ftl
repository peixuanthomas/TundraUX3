startup-time-sync-fallback = { $reason }. Using the last synchronized time, or the system clock if none is available. Retry in Settings → Date & Time.
startup-update-error-explanation = The update could not finish. Check network access, the installation directory permissions, and the requirements for the selected update mode. Original details: { $details }
startup-account-source = Account source
startup-check-pass = PASS
startup-check-warning = WARN
startup-check-fail = FAIL
startup-diagnostics-status-explanation = PASS: passed; WARN: limited or optional feature; FAIL: a required check failed. Unavailable means this feature or data source cannot be used in the current environment.
startup-temperature-unavailable = Temperature unavailable: no sensors detected
startup-battery-unavailable = Battery unavailable: no battery detected
startup-cli-help = TundraUX3 CLI
    { cli-administration-root-usage }
    { "  " }cls     Clear terminal history and screen, then move the cursor home
    { "  " }config  Read, set, reset, or list values for UX settings; run config help
    { "  " }debug   Diagnostics and test commands; run debug help for details
    { "  " }new     Clear saved TundraUX3 data and recreate initial storage
    { "  " }repl    Enter the interactive command loop; system commands run by default, /<command> runs a UX command, exit leaves
    { "          " }System commands keep exported environment variables and the working directory until you leave this REPL.
    { "  " }logs    Query runtime logs and incidents, or export diagnostics; run logs help
    { "  " }ui      Read and operate a running Shell; run ui --help
    { "  " }launcher  List, pin, or unpin applications; run launcher help
    { "  " }help [command]  Show help, for example: help config or help debug doctor
    { "  " }-h, --help      Show help
    { "  " }-V, --version   Show version

    Use <command> --help (or -h) for syntax, values, and examples.
    In Command Line / repl, use / for UX commands: /config set motion reduced; /help shows UX help.
    System commands need no prefix there: pwd, ls -la (Linux/macOS), dir (Windows). External CLI calls need no /: tundra-cli config set motion reduced.
    Exit status: 0 success, 1 operation failed, 2 invalid arguments; logs may also return 3/130 (see logs help).
startup-cli-debug-help = Usage: tundra-cli debug <command>
    { "  " }ui-test  { ui-test-summary }
    { "  " }test-rainbow-text  { rainbow-text-summary }
    { "  " }screen-keyboard  Try an English QWERTY keyboard with temporary text
    { "  " }test-aa-style1  AA preview: dimmed page + centered caution frame
    { "  " }test-aa-style2  AA preview: dimmed page + wide hazard banner
    { "  " }test-aa-style3  AA preview: dimmed page + split AA authorization panel
    { "  " }clear-logs <all|TYPE|--file PATH> [--yes]  Preview or clear runtime, incidents, snapshots, or legacy logs
    { "  " }view-ui-style [1|2|3]  Compare interactive UI styles and animations
    { "  " }asset   Print test assets or their original source files
    { "  " }doctor  Diagnose terminal, storage, and Linux runtime/tools; warnings are optional features
    { "  " }paths   Print configured and resolved app paths
    { "  " }explain Show CLI startup flow and kernel/UI boundaries
    { "  " }test-frost  Play only the startup frost banner animation
    { "  " }test-matrix Play only the first-run Matrix banner animation
    { "  " }test-watchdog-error    Write an intentional error report
    { "  " }test-watchdog-critical Write an intentional critical error report
    { "  " }test-watchdog-panic    Trigger a real panic; the current session is discarded
    Error tests print JSON/text report paths. Panic tests enter the normal critical-error flow; embedded tests panic the Shell session.
startup-cli-config-help = Usage: tundra-cli config [get [field] | set <field> <value> | reset <field> | options [field]]
    No arguments prints settings. reset restores only the named field. theme is a read-only color summary.
    Fields and values:
    { "  " }automation.enabled true | false (default false; local Shell control)
    { "  " }border-shape       rounded | square
    { "  " }border-color       default | named color (e.g. light-cyan) | #RRGGBB
    { "  " }accent-color       default | named color | #RRGGBB
    { "  " }icon-mode          ascii | image (image needs terminal graphics support)
    { "  " }motion             full | reduced
    { "  " }animation-speed    50..200 percent, in steps of 25; default 100
    { "  " }language           installed locale code; config options language lists choices
    { "  " }timezone / address timezone ID or city label; config options timezone lists choices
    { "  " }weather-location   English address in quotes (max 120 chars) | auto (follow timezone location)
    { "  " }update-mode        release | beta (Linux only; saves choice, does not start an update)
    address changes the timezone. weather-location changes only weather; it does not change system time.
    Examples:
    { "  " }tundra-cli config set motion reduced
    { "  " }tundra-cli config set animation-speed 125
    { "  " }tundra-cli config set weather-location "Shanghai, China"
    { "  " }tundra-cli config reset weather-location
    { "  " }tundra-cli config set accent-color "#38bdf8"
    Changes use the current OS user's config. Restart a running TundraUX3 UI to apply them. Identity/password fields are not exposed.
startup-cli-packages-help = list [--scope installed|search|updates]; search TEXT; source-add --name NAME --url URL [APT: --suite SUITE --components main --signed-by /key; DNF: --gpgkey URL]. sources add/enable/disable/remove are aliases. Source changes output drafts; --apply --expected-version saves. Script changes retain current config; --config-policy replace is APT-only. Arch install/upgrade always upgrades the full system.
