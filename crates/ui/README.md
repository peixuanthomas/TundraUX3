# ui

`ui` converts interface data into Ratatui layouts and frames, providing shared controls, themes, focus, hit testing, animations, and automation snapshots. It does not manage the host terminal, perform system operations, or modify application state while rendering.

## Module structure

| Location | Contents |
| --- | --- |
| `src/foundation/` | Input events, focus, hit testing, and shared basic types |
| `src/components/` | Buttons, lists, tables, forms, dialogs, and scrollbars |
| `src/screens/` | Page data models, layouts, and rendering |
| `src/theme/` | Themes, colors, control states, and animation scheduling |
| `src/motion.rs`, `surface_motion.rs` | Transition calculations for pages, dialogs, details, and the keyboard |
| `src/automation.rs` | Capture control trees and text from actual rendered frames |
| `src/terminal_images.rs` | Child-terminal image clipping, caching, and fallbacks |
| `src/workspace/` | Split geometry, window controls, and composition of app-host frames |
| `assets/` | Icons and timezone maps; see [Asset notes](assets/README.md) for sources |

Pages render only the content in `ShellFrameLayout.main` and their own overlays. Shell handles the global title bar, status bar, Toast, and modal dialogs, using the same layout for rendering and clicks.

The global title bar exposes Tasks for logged-in non-guest users, including running and awaiting-approval status. Title and status bars each use one row; Tasks, Help, Back, the clock, and the screen-keyboard entry share their actual render and hit-test geometry.

The Windows panel and its title-bar entry use the same rectangles for painting and interaction. The panel shows foreground and background windows, their content minimums, and windows that are closing or unresponsive. Closing and force-killing remain available when the terminal cannot fit the workspace. F1 help lists the workspace shortcuts.

A single pane uses all available content rows below the global title. Split panes add a title row and a one-cell gap between panes. Left/right and top/bottom layouts fit two panes when space permits. Grid capacity grows with the terminal, and the last row distributes its width among the windows it holds. All live windows contribute to the minimum content size, including those in the background.

App-host frames carry content in pane-local coordinates. Composition translates and clips cells, controls, and sensitive areas together. Only the focused pane keeps the existing automation IDs. The composed snapshot masks sensitive text in every visible pane.

## Shared controls

### Forms

`DialogLayout` gives shared and page-specific dialogs the same padded header, top-right close action, body, and raised footer. Its geometry also drives hit testing and automation. Shell dims covered content; only the topmost dialog receives input. Selection lists keep a fixed marker column and a full-row background, while supporting text stays muted.

`components::FormDialog` handles form-dialog layout, input, options, switches, scrolling, and buttons in one place. The caller supplies fields and buttons; the component returns a submit, cancel, or custom action. The caller performs application checks and saving.

```rust
let mut form = FormDialog::new("create", title, vec![
    FormField::text("name", name_label, ""),
    FormField::toggle("keep", keep_label, false),
]);
form.buttons = vec![
    FormButton::new("save", save_label, FormAction::Submit),
    FormButton::new("cancel", cancel_label, FormAction::Cancel),
];
let event = form.handle_event(input, area);
form.render_frame(frame, area, context);
```

Enter moves to the next field and submits from the last field; Space or a tap toggles a switch; Ctrl+Enter submits directly; F10 activates the default submit button. When rebuilding a form, use `interaction_state()` and `with_interaction()` to preserve scroll and drag positions. `ManagementForm` is a compatibility name.

### Lists and page layouts

- `DataTable` provides a separate header band, full-row selection and pointer feedback, sorting, and column-width calculation.
- `SidebarWorkspaceLayout` provides a padded navigation surface, list, and detail columns, switching to top navigation and a vertical layout in narrow windows.
- `ManagementWorkspace` organizes search, details, action bars, and expert areas that stay expanded when the selected object changes.
- `PageActionBar` supports primary and danger colors, disabled states, and wrapping at the full button-text width in narrow windows, without fixed button-count or three-row limits. Log expert tools reuse the same layout without covering the list.
- Home exposes an Edit icons / Done action, an edit-mode summary, and insertion indicators. Home and Launcher share grid insertion geometry, including row gaps, empty tail slots, and scrollbar exclusion.
- `ManagementViewModel::selected_rows` represents all selected rows; `selected` represents the current position.
- `ScrollbarOrientation::hit_test` extends pointer hit detection by one cell on either side of a vertical bar or above and below a horizontal bar, without changing the visual layout.

Selection dialogs keep this extended scrollbar hit area clear of option rows and the Close button, including the last row of long paths.

Ordinary workspaces use headings, backgrounds, and whitespace instead of complete frames. Navigation entries and table rows use one line. Shared spacing is one row between groups, two columns between groups, and one cell between grid items; full rows remain clickable. A modal, menu, or input may retain one boundary.

### Page controls

- Home and Launcher show entries on distinct tile surfaces without permanent frames, with one-cell gaps and current-item emphasis separate from keyboard focus. Home bounds its workspace to 112 columns and centers the title, account, and grid together when vertical space permits; its action footer stays at the bottom. Keyboard focus underlines only the card's application name. Home shows application names without letter-shortcut prefixes and uses name-only entries when four icon rows plus a label do not fit. Larger layouts retain artwork and theme overrides.
- Explorer uses a distinct background for context menus in truecolor and ANSI terminals. It keeps Undo Move/Rename visible and disabled when no history is available. The context menu and platform-specific Ctrl/Cmd+Z shortcut use the same command. Save pickers keep the filename and Save/Cancel controls beside directory browsing; Tab moves between them, F10 saves, and Esc cancels.
- The ordinary editor keeps Save, Undo, and Find visible. Its context menu reuses edit actions, enabled states, and keyboard navigation, stays inside the window, and preserves the selection; Rich Markdown keeps its selection-formatting menu.
- Launcher search uses `TextInput` and shares its region between rendering and hit testing. Empty results show a search message; built-in and blank-area context menus reuse page actions while retaining restrictions on built-in entries.
- Settings uses Tab/Shift+Tab across categories, the operating-system entry, and controls. Arrows switch categories when that region is focused and operate content otherwise; ordinary numeric options open the shared input dialog.
- Update activity shows download progress for Release and adds compilation progress and live output for source builds. Hidden build panels take no space, keeping release notes close to the download meter.
- The shared terminal minimum is 108×20, with all Home entries and the Command Line available at that size. Login keeps the password and submit controls visible. First-time setup keeps completion with default appearance at the bottom of the visible area while its body scrolls; the administrator label identifies the role as belonging to Tundra.

### Input and themes

`TextInput` provides a cursor, selection, scrolling, and password masking. Button hover, pressed, focused, current-item, and disabled styles are calculated centrally by `TundraTheme` and `RenderContext`. See [UI requirements](../../docs/UI-requirements.md) for keyboard, mouse, and touchscreen rules.

The default palette restores v1.5.1 Glacier Night: canvas `#071116`, surface `#0D1B22`, raised surface `#13262F`, text `#E6F1F4`, muted text `#8EA7B0`, accent `#63D3E5`, and border `#29434E`. Ordinary buttons have a visible control surface and plain labels; primary actions use accent fill, while danger actions retain semantic color. Current-item and multi-selection backgrounds remain independent of keyboard focus. Explicit saved appearance settings remain supported.

Compatibility themes retain resolved surface colors and the frame's terminal color capability, so page renderers use the same layered palette and ANSI output remains free of RGB colors. `Surface`, `Button`, and `List` accept an explicit background when embedded in a sidebar, dialog, or action band.

## Animations

Shared animations include:

- `ease_in_out_cruise` and `MotionPhases`: three phase durations and a continuous velocity curve.
- Pages, dialogs, and menus keep their final layout and readable body from the first frame, using short color transitions.
- `PanelMotion`, `expand_trailing_pane`: clipped details and on-screen keyboard expansion, with shared geometry during reversal.
- Active UI transitions schedule at 60 FPS; weather and continuous rainbow effects retain their separate 30 FPS cadence.
- `RainbowText`: continuous rainbow text colored by terminal character width.
- `BusyIndicator`: OpenCode's ten-frame Braille spinner and 1200ms loading-text sweep, using its coast and smootherstep curves. Wide and combining characters are colored by grapheme and terminal column.

`ui::animate` exports `Tween`, `Spring`, `Clock`, and macros from animate 0.8, with support for Ratatui types such as `Rect`, `Color`, and `Style`:

```rust
use ui::animate::{self, Clock};
```

Callers retain animation state and advance the clock with the same `MotionFrame` for each frame. Keyboard and pointer actions share transitions and take effect immediately. New navigation, reversal, or resize interrupts active motion; reduced motion goes directly to the target. Idle UI has no animation timer, and slow output skips missed frames instead of catching up.

Defaults are 160ms for pages, 180ms for dialogs and details, and 120ms for menus. The keyboard expands over 160ms. Progress graphics interpolate for 200ms without overshoot while numeric values update immediately. Busy indicators occupy one character with an 80ms step; visible truecolor loading labels refresh at 20Hz for the sweep. ANSI keeps the spinner without RGB interpolation. Animation speed applies to both, and reduced motion uses a static ellipsis with no animation timer.

OpenCode's dialogs mount directly; the configurable page/dialog/menu transitions above remain TundraUX3 behavior. The inspected upstream revision, source links, and MIT attribution are documented in [UI requirements](../../docs/UI-requirements.md#opencode-source-reference) and [OpenCode reference](../../third_party/opencode/README.md).

Advanced animation settings reuse `FormDialog` for nine numeric fields in three columns, Tab navigation, range checks, and small-window scrolling. Reset changes the draft; Save validates and commits all values together, while Cancel leaves configuration unchanged. Reset, Save, and Cancel remain visible.

Default acceleration/constant-speed/deceleration phases are page `[40, 80, 40]`, dialog `[45, 90, 45]`, and menu `[30, 60, 30]` milliseconds. Explicit saved timing values and speed settings remain effective.

## Child terminals and images

`TerminalImageFragment` describes RGBA data, source cropping, content-area coordinates, pixel margins, and layers. `TerminalImageCache` clips and caches Kitty, Sixel, and iTerm2 images by viewport. An app host captures the visible image placements alongside its rasterized cells. Shell rebuilds native graphics in the pane and retires native resources when the pane moves, closes, or becomes obscured.

Launcher icons use the terminal's measured cell size for layout. App hosts send icon pixels and pane-local placement to Shell, with half-block cells for frames where an overlay covers the pane. Transparent pixels blend with the tile's current background before both representations are generated.

Text terminals, dialog occlusion, layout transitions, and transparent or overlapping images use character-block compositing. Snapshots preserve wide characters, italics, and native cursor shapes; the editor's Vim region shares rendering, PTY sizing, and mouse coordinates through `editor_vim_terminal_area`.

Inactive rich Markdown headings use the terminal's text-sizing protocol when available. App hosts send heading text, level, style, and location as structured data. Shell generates OSC 66 only when the complete heading fits in the visible pane.

Vim reserves one row for its editor-mode switch and gives the rest to the native terminal. Command Line uses the entire main region. Script output reuses Command Line terminal geometry, reserving a scrollbar column only when history is present.

## Automation snapshots

When external control is enabled, `automation::FrameCapture` collects the control tree during actual rendering:

1. Shell starts the frame with `begin` and marks pages and overlays with `set_group`.
2. Controls record their IDs, names, regions, focus, selection, disabled status, and sensitive status using the actual layout.
3. `finish` reads the final Buffer and adds visible text not covered by controls.

Password regions are consistently masked in both controls and full-screen text; child terminals export parsed visible text. Capture only describes the screen; application operations still run through Shell's normal input flow.

## Validation

```sh
cargo test --locked -p ui
cargo test --locked -p ui --test automation
tundra-cli debug ui-test
```

Shared-control tests cover button states, forms, modal interception, password masking, wide characters, and resizing. Page hit testing and complete workflows are covered by Shell tests and real-terminal acceptance checks.

Related documentation: [Debug previews](../shell/docs/previews.md), [Shell sessions](../shell/docs/session.md), [UI requirements](../../docs/UI-requirements.md).
