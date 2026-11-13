# Debug interface previews

Previews are launched by the CLI, with interfaces provided by Shell and UI. They use demonstration data, do not perform real management tasks, and restore the terminal when closed.

## All shared controls

```sh
tundra-cli debug ui-test
# Or run from source
cargo run --locked -p cli --bin tundra-cli -- debug ui-test
```

The preview uses the built-in dark theme and English, without reading or saving user configuration, language files, or logs. The terminal must be at least 88 columns × 30 rows; 120 × 36 is recommended.

Shared controls demonstrate the Glacier Night blue palette, distinct control surfaces, filled primary actions, danger color, disabled states, and focus independent of selection. The dialog page uses the same padded header, close action, and footer as production. Use the actual Shell at 108 × 20 to validate minimum-size page workflows; this control gallery has its own larger layout.

F1–F6, PageUp/PageDown, or the bottom buttons switch pages. Tab/Shift+Tab moves focus. Esc, Ctrl+C, or the Exit button ends the preview.

| Page | Controls |
| --- | --- |
| Basics | Button states, TextInput, Surface, Panel |
| Selection | List, DataTable, Tabs, Picker, horizontal and vertical Scrollbar |
| Navigation | NavRail, SidebarWorkspaceLayout |
| Feedback | EmptyState, Skeleton, CommandStatus, Toast |
| Dialogs | Dialog, ContextMenu, CommandPalette |
| Progress | ProgressGauge, MetricCard, UpdateActivity |

The entry point is `crates/shell/src/previews/ui_test.rs`, and the control compositions are in `crates/ui/src/ui_test.rs`.

## Rainbow text

```sh
tundra-cli debug test-rainbow-text
```

This preview shows continuous rainbow coloring of Chinese text, combining characters, and emoji. Esc, Ctrl+C, Enter, or the Exit button ends it.

## On-screen keyboard

```sh
tundra-cli debug screen-keyboard
```

This demo only edits sample text for the current session. Test AA only displays an input dialog; it does not execute commands or request authorization. See [The on-screen keyboard for AA and Command Line](session.md#on-screen-keyboard-for-aa-and-command-line) for access from actual pages.

| Part | Behavior |
| --- | --- |
| Layout | QWERTY in the lower half of the screen, with function keys and a navigation area in wide windows |
| Modifiers | Click to latch, click again to release; multiple keys can be combined |
| Editing | Backspace deletes; Enter/Tab inserts a newline or tab |
| Clipboard | Copy all text; paste appends content |
| Toolbar | Collapse, expand, clear, and exit |
| Settings | Read the current theme, animation speed, and reduced-motion setting |

Physical-key highlighting and mouse-latched states are stored separately and cleared on focus loss, collapse, or resize. Dialogs and the keyboard use the same layout calculation for rendering and click positions.

## Interface and animation styles

```sh
tundra-cli debug view-ui-style 2
# Use in Command Line
/debug view-ui-style 2
```

Tea requires at least 60 × 20; Glacier and Spring require 60 × 24. A 100 × 35 terminal or larger is recommended for comparing all styles.

| Version | Style |
| --- | --- |
| `1` Glacier | Bordered components, direct progress updates, and sweep-in effects |
| `2` Tea | Neutral surfaces, compact borderless lists and forms, clear focus, and one-character task status |
| `3` Spring | Cards, spring progress, and gentle entrances for comparison |

F1–F3 switches versions, F4 toggles reduced motion, F5 replays, Tab moves focus, and Esc closes a dialog before exiting. The preview only changes demonstration state for the current session. Entry points are `crates/shell/src/previews/style.rs` and `crates/ui/src/style_preview.rs`.

The Spring comparison retains its demonstration spring model. Production progress uses a 200ms tween without overshoot and shows the real numeric value immediately. Production page/dialog/menu transitions use fixed layout and readable content from the first frame, with defaults of 160/180/120ms; details and the keyboard use clipped expansion. See [UI requirements](../../../docs/UI-requirements.md) and the [testing matrix](../../../docs/testing.md#compact-ui-and-motion-acceptance) for acceptance criteria.
