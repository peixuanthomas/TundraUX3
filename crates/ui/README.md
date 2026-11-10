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
| `assets/` | Icons and timezone maps; see [Asset notes](assets/README.md) for sources |

Pages render only the content in `ShellFrameLayout.main` and their own overlays. Shell handles the global title bar, status bar, Toast, and modal dialogs, using the same layout for rendering and clicks.

The global title bar exposes Tasks for logged-in non-guest users, including running and awaiting-approval status. Tasks, Help, and Back keep separate borders.

## Shared controls

### Forms

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

- `DataTable` provides table rendering, selected rows, sorting, and column-width calculation.
- `SidebarWorkspaceLayout` provides navigation, list, and detail columns, switching to top navigation and a vertical layout in narrow windows.
- `ManagementWorkspace` organizes search, details, action bars, and expert areas that stay expanded when the selected object changes.
- `PageActionBar` supports primary and danger colors, disabled states, and wrapping at the full button-text width in narrow windows, without fixed button-count or three-row limits. Log expert tools reuse the same layout without covering the list.
- Home exposes an Edit icons / Done action, an edit-mode summary, and insertion indicators. Home and Launcher share grid insertion geometry, including row gaps, empty tail slots, and scrollbar exclusion.
- `ManagementViewModel::selected_rows` represents all selected rows; `selected` represents the current position.
- `ScrollbarOrientation::hit_test` extends pointer hit detection by one cell on either side of a vertical bar or above and below a horizontal bar, without changing the visual layout.

Selection dialogs keep this extended scrollbar hit area clear of option rows and the Close button, including the last row of long paths.

### Page controls

- Home shows application names without letter-shortcut prefixes and uses the same system-application artwork and images as Launcher, while preserving Home-specific theme overrides. Cards that cannot fit the complete four-row icon and application name show only the name, for both ASCII and image themes.
- Explorer keeps Undo Move/Rename visible and disabled when no history is available; the context menu and platform-specific Ctrl/Cmd+Z shortcut use the same command. Save pickers keep the filename and Save/Cancel controls beside directory browsing; Tab moves between them, F10 saves, and Esc cancels.
- The ordinary editor keeps Save, Undo, and Find visible. Its context menu reuses edit actions, enabled states, and keyboard navigation, stays inside the window, and preserves the selection; Rich Markdown keeps its selection-formatting menu.
- Launcher search uses `TextInput` and shares its region between rendering and hit testing. Empty results show a search message; built-in and blank-area context menus reuse page actions while retaining restrictions on built-in entries.
- Settings uses Tab/Shift+Tab across categories, the operating-system entry, and controls. Arrows switch categories when that region is focused and operate content otherwise; ordinary numeric options open the shared input dialog.
- Update activity shows download progress for Release and adds compilation progress and live output for source builds. Hidden build panels take no space, keeping release notes close to the download meter.
- The shared terminal minimum is 108×22, with all Home entries and the Command Line available at that size. Login keeps the password and submit controls visible. First-time setup keeps completion with default appearance at the bottom of the visible area while its body scrolls; the administrator label identifies the role as belonging to Tundra.

### Input and themes

`TextInput` provides a cursor, selection, scrolling, and password masking. Button hover, pressed, focused, current-item, and disabled styles are calculated centrally by `TundraTheme` and `RenderContext`. See [UI requirements](../../docs/UI-requirements.md) for keyboard, mouse, and touchscreen rules.

## Animations

Shared animations include:

- `ease_in_out_cruise` and `MotionPhases`: three phase durations and a continuous velocity curve.
- `reveal_rect`, `morph_surface`: expand or collapse pages, dialogs, and menus from their entry points.
- `PanelMotion`, `expand_trailing_pane`: detail panes, the on-screen keyboard, and reversal during a transition.
- `ANIMATION_FRAME_INTERVAL`: animation scheduling at 30 FPS.
- `RainbowText`: continuous rainbow text colored by terminal character width.

`ui::animate` exports `Tween`, `Spring`, `Clock`, and macros from animate 0.8, with support for Ratatui types such as `Rect`, `Color`, and `Style`:

```rust
use ui::animate::{self, Clock};
```

Callers retain animation state and advance the clock with the same `MotionFrame` for each frame; keyboard actions or reduced motion go directly to the target.

Advanced animation settings reuse `FormDialog` for nine numeric fields in three columns, Tab navigation, range checks, and small-window scrolling. Reset changes the draft; Save validates and commits all values together, while Cancel leaves configuration unchanged. Reset, Save, and Cancel remain visible.

## Child terminals and images

`TerminalImageFragment` describes RGBA data, source cropping, content-area coordinates, pixel margins, and layers. `TerminalImageCache` clips and caches Kitty, Sixel, and iTerm2 images by viewport.

Text terminals, dialog occlusion, layout transitions, and transparent or overlapping images use character-block compositing. Snapshots preserve wide characters, italics, and native cursor shapes; the editor's Vim region shares rendering, PTY sizing, and mouse coordinates through `editor_vim_terminal_area`.

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
