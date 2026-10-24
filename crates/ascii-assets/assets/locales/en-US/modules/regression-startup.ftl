startup-time-sync-fallback = { $reason }. Using the last synchronized time, or the system clock if none is available. Retry in Settings → Date & Time.
startup-update-error-explanation = The update could not finish. Check network access, the installation directory permissions, and the requirements for the selected update mode. Original details: { $details }
startup-account-source = Account source
startup-check-pass = PASS
startup-check-warning = WARN
startup-check-fail = FAIL
startup-diagnostics-status-explanation = PASS: passed; WARN: limited or optional feature; FAIL: a required check failed. Unavailable means this feature or data source cannot be used in the current environment.
startup-temperature-unavailable = Temperature unavailable: no sensors detected
startup-battery-unavailable = Battery unavailable: no battery detected
startup-shell-ui-automation-help = UI automation (for agents):
    Agents can use tundra-cli ui to inspect and operate controls in a running Shell.
    Enable Settings > System > External CLI control (off by default).
    Or run tundra-cli config set automation.enabled true before starting Shell.
    From another terminal under the same OS user:
    { "  " }tundra-cli ui snapshot
    { "  " }tundra-cli ui click --id TARGET_ID
    { "  " }tundra-cli ui text "TEXT"
    Replace TARGET_ID with an ID from snapshot; text goes to the focused control.
    After user operations, read snapshot or wait again; animations and automatic updates are ignored.
    Run tundra-cli ui wake to close idle weather or the weather lockscreen.
    See tundra-cli ui --help for click, drag, scroll, key, text, and wait commands.
startup-cli-help = TundraUX3 CLI
    { cli-administration-root-usage }
    { "  " }config [get|set|reset|options]  Settings
    { "  " }doctor                        Check the runtime environment
    { "  " }launcher [list|pin|unpin]       Application shortcuts
    { "  " }services                      System services
    { "  " }processes                     Running processes
    { "  " }packages                      Installed packages, updates and sources
    { "  " }network                       Network connections
    { "  " }disks                         Disks and mount points
    { "  " }users                         Accounts and groups
    { "  " }firewall                      Ports and firewall rules
    { "  " }ssh                           SSH settings and keys
    { "  " }backups                       Configuration backups and restore
    { "  " }scheduled-tasks               Scheduled tasks
    { "  " }system-config                 System configuration files
    { "  " }operations                    Operation status and authorization
    { "  " }logs [query|incidents|export|follow]  Logs and diagnostics
    { "  " }ui [sessions|snapshot|click|move|drag|scroll|key|text|wait]  Shell control
    { "  " }debug <command>               Debug tools; see debug --help
    { "  " }cls                           Clear the terminal
    { "  " }new                           Reset saved TundraUX3 data
    { "  " }repl                          Interactive command line
    { "  " }help [command ...]            Command help
    { "  " }-h, --help                    Help
    { "  " }-V, --version                 Version
    Use <command> --help for subcommands and options.
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
    { "  " }paths   Print configured and resolved app paths
    { "  " }explain Show CLI startup flow and kernel/UI boundaries
    { "  " }test-frost  Play only the startup frost banner animation
    { "  " }test-matrix Play only the first-run Matrix banner animation
    { "  " }test-watchdog-error    Write an intentional error report
    { "  " }test-watchdog-critical Write an intentional critical error report
    { "  " }test-watchdog-panic    Trigger a real panic; the current session is discarded
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

startup-cli-doctor-help = Usage: tundra-cli doctor
    Check terminal, permissions, paths, storage and assets.
    Storage checks may create or recover application files. Exit: 0 passed, 1 failed.
startup-cli-explain-commands = { "  " }2. tundra-cli provides configuration, applications and logs; doctor checks the environment; debug contains test tools.
startup-cli-repl-help = Usage: tundra-cli repl
    /help  Commands
    exit   Exit (or EOF)
startup-cli-launcher-help = Usage: tundra-cli launcher [list | pin <path> | unpin <id>]
    list          List saved applications
    pin <path>    Save an executable shortcut
    unpin <id>    Remove a shortcut; keep the file
startup-cli-screen-keyboard-help = Usage: tundra-cli debug screen-keyboard
    Keyboard and AA input preview in an external interactive terminal.
    Tab/arrow keys select; Enter/Space activate; Esc exits.
