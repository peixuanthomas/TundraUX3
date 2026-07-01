# TundraUX 3 Phase 1 Shell Design

Date: 2026-07-01

## Context

`docs/Initial Plan.md` defines Phase 1 as the foundation for the full-screen TundraUX shell: terminal setup, unified event loop, global state, screen stack, basic layout, theme, status messages, error display, exit confirmation, and panic restore.

The current repository is a Phase 0 baseline. It has a Rust workspace with `tundra-shell`, `tundra-platform`, `tundra-storage`, and `tundra-cli`. `tundra-shell` currently renders a banner, writes manual alternate-screen escape sequences, and runs a blocking smoke loop. It does not yet use `ratatui` or `crossterm`, and it has no real input event loop or UI layout crate.

Phase 1 should replace the smoke shell with the smallest reliable full-screen shell framework while keeping future Phase 2 input routing and component work out of scope.

## Goals

- Start `tundra-shell` in a ratatui full-screen terminal by default.
- Use crossterm for raw mode, alternate screen, mouse capture, keyboard input, mouse input, resize events, and controlled terminal restore.
- Add a new `tundra-ui` crate for theme, layout, Home rendering, status/toast/error rendering, and debug/user Home presentation.
- Keep all raw input reading in `tundra-shell`; UI code receives view models and renders them.
- Maintain a lightweight screen stack for `Home` and `ExitConfirm`.
- Support two Home modes:
  - debug builds default to diagnostics-oriented Home.
  - release builds default to user-oriented Home.
- Support `-debug` to force diagnostics Home from the command line.
- Preserve `-notfullscreen` as a non-ratatui smoke and diagnostics path.
- Restore terminal state on normal exit, `Ctrl+C`, terminal close events when possible, and panic.

## Non-Goals

- No full component system, focus model, context menus, command palette, or hit testing beyond recording mouse coordinates.
- No real Explorer, Launcher, Editor, Settings, or Diagnostics app behavior.
- No platform file opening, storage persistence, login, audit, or permissions work.
- No Windows API calls from UI code.
- No long-running background job system.

## Architecture

Phase 1 uses two primary crates.

### `tundra-shell`

`tundra-shell` owns the runtime shell:

- command-line parsing
- terminal initialization
- raw mode, alternate screen, mouse capture, and cursor visibility
- ratatui terminal creation
- event polling and tick scheduling
- global `ShellState`
- lightweight screen stack
- shutdown and exit confirmation
- panic hook and terminal restore
- conversion from raw crossterm events into shell-level state updates

All `crossterm::event::read` or `poll` calls must stay in `tundra-shell`.

### `tundra-ui`

`tundra-ui` owns pure UI rendering:

- theme tokens
- top bar, main area, and status bar layout
- debug Home rendering
- user Home rendering
- status, toast, and error panel rendering
- exit confirmation dialog rendering
- small view-model structs shared with `tundra-shell`

`tundra-ui` must not read terminal events, touch the filesystem, access environment variables, call Windows APIs, or mutate shell state.

## Dependency Direction

The Phase 1 dependency direction is:

```text
tundra-shell -> tundra-ui
tundra-shell -> tundra-storage
tundra-shell -> tundra-platform only for existing Phase 0 diagnostics if needed
tundra-ui    -> ratatui
```

`tundra-ui` must not depend on `tundra-shell`, `tundra-platform`, or `tundra-storage`.

## Launch Modes

The existing `ShellLaunchMode` should evolve into a launch configuration that captures both terminal mode and Home display mode.

Terminal mode:

- default: full-screen ratatui shell
- `-notfullscreen`: non-fullscreen smoke output, no raw mode, no alternate screen, no mouse capture, no ratatui event loop

Home display mode:

- debug build: default to diagnostics Home
- release build: default to user Home
- `-debug`: force diagnostics Home in any build mode

Accepted examples:

```text
tundra-shell
tundra-shell -debug
tundra-shell -notfullscreen
tundra-shell -notfullscreen -debug
```

Unknown arguments should fail with a clear parse error. Duplicate known arguments should fail rather than silently overriding each other.

## Shell State

`ShellState` should be small and explicit:

- active screen stack: `Home` or `ExitConfirm` on top
- Home display mode: `Debug` or `User`
- terminal size
- tick count
- shell start time
- last key event summary
- last mouse event summary
- last resize event summary
- last status message
- last toast message
- last recoverable error message
- terminal capability flags known to the shell guard
- shutdown requested flag

State updates should be testable without a terminal. The event loop should delegate to pure update functions for key, mouse, resize, and tick handling.

## Event Flow

The event flow is:

```text
crossterm::event::Event
  -> shell event update
  -> ShellState
  -> HomeViewModel or ExitConfirmViewModel
  -> tundra-ui render
```

Phase 1 shell events:

- key events
- mouse move, click, scroll, drag events when crossterm reports them
- resize events
- tick events generated by the event loop
- shutdown events from `Ctrl+C` or console control handling

Shell-level actions:

- `q` or `Esc` on Home opens exit confirmation.
- `y` or `Enter` on exit confirmation exits.
- `n` or `Esc` on exit confirmation returns to Home.
- `Ctrl+C` requests shutdown through the same terminal restore path.
- Mouse events update diagnostics state only.
- Resize events update terminal size and trigger redraw.

No app-specific command dispatch is added in Phase 1.

## Layout

Both Home modes use the same high-level layout:

```text
top bar
main area
status bar
```

The layout must tolerate small terminal sizes. When the terminal is too small for the full view, the UI should render a compact fallback message instead of panicking or overflowing.

## Debug Home

Debug Home is the default for debug builds and when `-debug` is provided.

It should show diagnostics useful for validating Phase 1:

- app name and build mode
- Home display mode
- launch arguments summary
- terminal size
- tick count
- screen stack
- last key event
- last mouse event
- last resize event
- mouse coordinates
- scroll direction when available
- raw mode, alternate screen, mouse capture, and cursor restore state
- current status, toast, and error messages

Debug Home may expose raw event summaries because it is a developer view.

## User Home

User Home is the default for release builds.

It should show the product-facing shell surface:

- TundraUX 3 identity
- current time
- current user static label: `Guest`
- status bar
- primary non-functional entries:
  - Explorer
  - Launcher
  - Editor
  - Settings
  - Diagnostics

These entries do not open real apps in Phase 1. If the implementation makes them keyboard or mouse selectable, selection may only show a status or toast such as "Explorer is planned for a later phase". The user view must not show raw input logs, raw terminal state, or debug-only diagnostics by default.

## Status, Toast, and Errors

Phase 1 needs three user-visible message surfaces:

- status line: persistent current shell status
- toast: short-lived notification driven by shell state
- error panel or status variant: recoverable errors

Recoverable errors should update state and keep the event loop running. Initialization errors before terminal takeover should print a readable error to stderr. Errors after terminal takeover should restore the terminal before printing process-level failure details.

## Terminal Restore and Panic Handling

Terminal lifecycle must be guarded by a drop-based terminal guard.

On enter:

- enable raw mode
- enter alternate screen
- enable mouse capture
- hide cursor when appropriate
- create ratatui terminal

On restore:

- show cursor
- disable mouse capture
- leave alternate screen
- disable raw mode
- flush output

The restore path must be used for:

- normal confirmed exit
- `Ctrl+C`
- supported console control events
- initialization failure after partial terminal setup
- panic hook

The panic hook should attempt terminal restore before printing panic details.

## Testing

Phase 1 test coverage should focus on pure logic where possible:

- argument parsing for default, `-debug`, `-notfullscreen`, combined valid flags, unknown arguments, and duplicate flags
- Home display mode selection for debug and release build defaults
- `ShellState` update functions for key, mouse, resize, tick, exit confirmation, and shutdown
- `tundra-ui` view-model rendering decisions for debug Home and user Home
- layout fallback behavior for very small terminal sizes
- status, toast, and error text propagation
- existing Phase 0 smoke behavior for `-notfullscreen`

If ratatui buffer assertions are stable, add focused buffer tests for key labels and fallback rendering. Avoid broad snapshot tests that break on harmless spacing changes.

Verification command:

```text
cargo test
```

Manual verification after implementation:

```text
cargo run -p tundra-shell
cargo run -p tundra-shell -- -debug
cargo run -p tundra-shell -- -notfullscreen
cargo run --release -p tundra-shell
cargo run --release -p tundra-shell -- -debug
```

## Deliverables

- `tundra-ui` crate added to the workspace.
- `tundra-shell` uses ratatui and crossterm for the full-screen shell path.
- Existing `-notfullscreen` path remains available for smoke output.
- Debug Home and User Home both render through `tundra-ui`.
- Shell event loop handles keyboard, mouse, resize, tick, shutdown, and redraw.
- Exit confirmation screen works.
- Terminal restore works on normal exit and tested error paths.
- Tests cover argument parsing, state updates, and UI view-model/layout behavior.

## Acceptance Criteria

- `cargo test` passes.
- Starting `tundra-shell` in a debug build opens the full-screen diagnostics Home.
- Starting `tundra-shell -debug` opens diagnostics Home in any build mode.
- Starting `tundra-shell` in a release build opens the user Home.
- Starting `tundra-shell -notfullscreen` does not enter raw mode, alternate screen, mouse capture, or the ratatui event loop.
- Mouse movement, clicks, scrolls, keyboard input, ticks, and resize events are recorded in Debug Home.
- User Home hides raw diagnostics and shows product-facing non-functional entries.
- `q` or `Esc` opens exit confirmation from Home.
- `y` or `Enter` exits from exit confirmation.
- `n` or `Esc` cancels exit confirmation.
- After exit or panic, terminal state is restored as far as the platform allows.

## Implementation Boundaries for the Next Plan

The implementation plan should sequence work so the terminal guard, argument parsing, and pure state transitions are built before the interactive render loop. That keeps early tests deterministic and reduces the risk of being unable to recover the terminal during development.

The plan should not implement Phase 2 focus routing, component abstractions, app-specific controllers, Explorer behavior, persistent settings, or platform file opening.
