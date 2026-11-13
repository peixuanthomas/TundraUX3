# UI design and acceptance requirements

This is the shared design and acceptance contract for the complete interface: startup and account flows, Home, Launcher, Settings, system management, files, editor and Vim, scripts, Command Line, Tasks, clock, weather, dialogs, menus, and AutoAdmin (AA). Apply it in stages across shared foundations, global bars, workspaces, tools, and motion; a page is complete only when its rendering, input, automation, and tests agree.

## Visual language and density

The default appearance uses dark blue backgrounds and one accent. Theme overrides and explicit saved appearance choices remain supported.

| Token | Default | Use |
| --- | --- | --- |
| Canvas | `#071116` | Full-frame background |
| Surface | `#0D1B22` | Navigation, lists, and content groups |
| Alternate row | `#162832` | Every other list or table row |
| Raised surface | `#192C36` | Detail panes, table headers, and dialogs |
| Text | `#E6F1F4` | Normal readable content |
| Muted text | `#8EA7B0` | Secondary information |
| Accent | `#63D3E5` | Focus and primary actions |
| Border | `#383838` | Inputs and necessary overlay boundaries |

- Separate ordinary pages, groups, lists, tables, and action areas with headings, whitespace, alignment, or a change of background. Do not draw complete boxes around them or nest decorative frames.
- A modal, menu, or input may use one boundary. Temporary messages use the shared surface and the corresponding success, warning, or danger color.
- Use a shaded backdrop and a raised panel for modal dialogs. Keep the title, message, and actions visually separate. In option lists, fill the active row across its full width and mute secondary text on the other rows.
- Ordinary buttons use plain action text without `[]`. Primary actions use an accent fill; destructive actions use the danger color. Selection and the current item use background emphasis independently of keyboard focus.
- Navigation entries and table rows each occupy one terminal row. Use one row between groups, two columns between adjacent groups, and one cell between grid items. Keep a complete row clickable, including its padding, for mouse and touch.
- The title bar and status bar each occupy one row. A 108 × 20 terminal therefore retains 18 content rows. Tasks, Help, Back, and the clock remain available; use compact labels when needed. The keyboard entry sits in the status row immediately before the clock.
- Use the same spacing, typography, and control states across pages. Text must remain readable with truecolor or ANSI color, and with Chinese, English, combining marks, and wide characters.

New defaults apply when a preference is absent. Preserve existing legacy-color migration and explicit saved border, color, theme, animation speed, and timing preferences; do not overwrite them merely because defaults changed.

## Text and action entry points

- Buttons state the action and its object directly, with any existing shortcut next to the action text. Complete keyboard instructions belong in F1 help.
- Common operations use visible buttons and options. Keep search, filters, refresh, and path navigation near the top, and page actions at the bottom right of the content area. Wrap actions at their full text width in narrow windows without fixed button-count or three-row limits.
- Preserve existing features while reducing decoration: multi-selection, sorting, search, expert areas, help, return paths, and task management must remain reachable.
- Keep service logs, file copy/cut/paste/rename, editor save/undo/find, and scheduled-task run-now directly visible. Home shows supported system tasks with compact readable entries in small windows.
- Complex pages provide an expert area that stays expanded when the selected object changes. Less frequent operations may also use menu shortcuts.
- Empty states, details, and status bars provide useful information. Errors explain the specific cause and next step. Update Chinese and English together and verify text width and clickable areas.
- First save and Save As provide directory browsing, a persistent filename field, Save, and Cancel in the same picker.
- Delete in Trash affects the selected items. Empty Trash has its own explicit action. Irreversible actions, clock management, and manual AA approval default to Cancel or Deny.
- Keep the primary action at the far right. Selection-dependent actions remain visible but disabled when no object is selected.
- Action models declare destructive risk independently of confirmation or authorization. Removing, uninstalling, and force-stopping use the danger color; viewing, starting, and resuming do not become dangerous merely because they need confirmation.
- Center More Actions in the content area and size it to its options. Allow scrolling and button wrapping in small windows, keeping the current option visible.

## Buttons and input methods

The shared Button, Theme, and RenderContext own control states. Pages pass focused, selected, pressed, and disabled state rather than drawing their own equivalents.

| State | Appearance and behavior |
| --- | --- |
| Normal | Readable text on a neutral background |
| Mouse hover | Accent feedback without changing layout |
| Mouse pressed | Stronger accent feedback; release within the same target activates it |
| Keyboard focus | Visible accent indicator independent of current-item and selection backgrounds |
| Disabled | `TundraTheme::disabled_style()`; ignore activation |
| Click completed | Clear pressed and hover feedback until actual pointer movement |

- Initially show keyboard focus. Valid keyboard navigation moves focus immediately; actual mouse movement, taps, drags, or scrolling hide its indicator while retaining logical focus.
- A stationary pointer must not highlight another button or reclaim emphasis after keyboard input. Repeated reports of the same coordinates do not switch input methods.
- Activate a button only after pressing and releasing within the same target. Dragging outside, losing focus, resizing, or changing pages cancels the pending action.
- A double click requires a released first click followed by a second press on the same target and cell within 500ms. Scrolling, dragging, keyboard input, focus loss, and resizing clear that record.
- Only the topmost dialog receives hover, press, text, or keyboard input. Covered pages retain their state. Toasts do not take focus.
- Hide keyboard emphasis on window focus loss while preserving logical focus. Current-page and record-selection emphasis remains independent of the input method.
- Selected text fields use accent feedback and a blinking underscore cursor at the actual position. Up/Down switches fields; Left/Right moves the cursor. Mask passwords and scroll long text with the cursor.
- Keyboard, mouse, and touch trigger the same transition behavior. Focus, activation, text input, scrolling, and cancellation take effect immediately, including during animation.

## Motion and scheduling

Use restrained transitions to explain state changes without delaying operation. Layout and content must be usable from the first frame.

| Transition | Default duration | Presentation |
| --- | --- | --- |
| Page | 160ms | Fixed final layout; background and color transition |
| Dialog | 180ms | Fixed final layout; background and color transition |
| Menu | 120ms | Fixed final layout; background and color transition |
| Details | 180ms | Clipped expansion or collapse |
| On-screen keyboard | 160ms | Clipped expansion or collapse from the bottom |
| Progress graphic | 200ms | Interpolation toward the real value, without overshoot |
| Busy indicator | 80ms per step | One terminal character, with a 1.2s white highlight sweep across the gray loading label |

- Page, dialog, and menu bodies are readable on the first frame. Do not show an empty moving outline or wait for the transition to reveal text. Opening and closing keep the intended final layout and blend only the presentation.
- Details expand rightward in wide windows and vertically in narrow windows. The list, details, and keyboard use the same current geometry for painting, clipping, PTY sizing, automation, and hit testing. Do not insert blank intermediate frames.
- The real progress value and its numeric label update immediately. Only the graphic interpolates; changing targets or reversing direction starts from its current displayed value and never overshoots.
- New navigation, reverse actions, repeated switching, input-method changes, and resize interrupt or retarget the active transition. Do not queue animations or require an extra key or click to finish one.
- Reduced motion renders the terminal state directly, including details, keyboard, progress, and busy feedback. No hidden animation continues afterward.
- Active UI transitions schedule frames at 60 FPS. Loading labels request a frame every 40ms while visible. Idle UI has no animation timer. Weather and continuous rainbow effects retain their separate 30 FPS policy. Cursor blink, clocks, and real data refresh retain their own deadlines.
- Slow output advances by elapsed time and skips missed frames. Never replay accumulated animation frames after the terminal stalls.
- Motion curves, durations, clipping, spacing, and pane geometry belong in `crates/ui`; Shell owns input routing, navigation, dialog stacking, and transition lifecycle.

Settings retains animation speed, reduced motion, and all nine phase values. The three default phase durations are:

| Kind | Acceleration | Constant speed | Deceleration |
| --- | --- | --- | --- |
| Page | 40ms | 80ms | 40ms |
| Dialog | 45ms | 90ms | 45ms |
| Menu | 30ms | 60ms | 30ms |

Use one three-column form with direct input and Tab navigation. Each phase accepts 1–2000ms. Validate all values before saving together. Reset changes the draft; Save commits 100% speed and the default phases after reset, while Cancel or Esc discards the draft and restores focus. Keep Reset, Save, and Cancel reachable at 108 × 20, including with reduced motion enabled.

## Pages, navigation, and global bars

- `ScreenCompositor` centrally draws the title bar, Back button, status bar, and clock. Logged-in non-guest users can open Tasks and see running or awaiting-approval status. Pages draw only `ShellFrameLayout.main` and their own overlays.
- Create an independent instance on each page opening. Check instance IDs for state, task results, and focus restoration so late results cannot enter a reopened page.
- Shell handles entry, overlays, and return navigation. Esc and Back first close dialogs or finish page-local actions, then return one level along the opening path.
- Holding Esc does not exit multiple levels. Keys in Command Line and the Vim editing region go to the child terminal; outer Back still offers close confirmation.
- Closing a page with running work offers Continue in background / Cancel task and close / Stay on page. Cancellation waits for process shutdown and cleanup.
- Vim displays the full native terminal. Ordinary editor menus and status lines must not cover its mode, selection, command line, or errors. Geometry changes update rendering, PTY size, and pointer translation together.
- Keep automation control IDs and the external-control protocol compatible. Capture actual visible controls, disabled states, text, focus, clipping, and coordinates; preserve password masking. Interaction must never target a stale location from an earlier layout or covered layer.

## Screen keyboard, forms, and selection dialogs

- Keep the screen-keyboard entry in the status row before the clock, without consuming a content row. On ordinary pages, opening it preserves the one-row title bar, hides the status bar, and makes room for content; AA adjusts only the topmost dialog.
- Keep forms and action buttons above the keyboard. If the terminal cannot fit them, ask the user to enlarge it instead of covering required fields.
- Place Paste at the left end of the keyboard's bottom row and Hide at the right. Physical-key feedback and mouse-latched modifiers use separate state; clear both on focus loss or resize.
- Keyboard keys use neutral boundaries and text contrast of at least 4.5:1. Pressed or latched keys use accent fill.
- Use the shared `FormDialog`. Enter advances fields and submits from the last field; Space or a tap toggles switches; Ctrl+Enter submits directly; F10 activates the default confirmation.
- Keep the current field, Confirm, and Cancel reachable while scrolling long forms. Passwords remain masked in both rendering and snapshots.
- Packages uses an Installed, Updates, Search sidebar; Settings reuses that workspace structure. Tab/Shift+Tab moves between regions and controls. Arrows change categories only in the focused sidebar and otherwise operate content. Packages retains F7/F8/F6 for Installed/Updates/Search.
- In application settings, Left/Right moves between the application list and its controls. Enter/Space activates controls; arrow navigation does not save options.
- Numeric options support direct entry or minus/plus controls. AA policy and screen timeout use selection dialogs.
- Once required account information is valid, first-time setup can finish with the default appearance. Distinguish Tundra application administrators from operating-system administrators.

## Lists and sorting

Packages, services, processes, scheduled tasks, and files share multi-selection behavior. See [list selection](list-selection.md) for scope.

- Ctrl/Cmd+click toggles individual records; Shift+click selects a range from a stable anchor; Shift+Enter toggles the current row. Explicit multi-selection mode also supports touch checkboxes.
- Clicking a column header sorts by it; clicking again reverses direction. Show the active sort direction.
- Compare text case-insensitively and counts, percentages, capacities, and rates numerically. Files keep directories first.
- Sort complete records, preserve selected objects, and retain sorting after refresh.
- Ctrl+F6 changes sort columns; Ctrl+Shift+F6 reverses direction. Files and Launcher retain their own shortcuts.
- Manually sorting logs pauses following. Returning to the latest records restores reverse chronological order.

## Acceptance checks

Run the matrix at 108 × 20, 120 × 36, and 180 × 50, in Chinese and English, using truecolor and ANSI color. Include wide and combining characters in text, paths, fields, and labels. Complete each workflow separately with keyboard, mouse, and touch events; touch checks use presses, releases, and drags without hover.

| Workflow | Keyboard | Mouse | Touchscreen |
| --- | --- | --- | --- |
| Enter, act, and return | Tab/arrows, Enter/Space, shortcuts | Visible actions, full rows, Back | Tap the same targets |
| Long lists and forms | Arrows, page keys, Home/End | Wheel, scrollbar dragging | Scrollbar dragging |
| Text editing and submission | Typing, deletion, focus, submit | On-screen tools and Submit/Cancel | On-screen tools and tappable actions |
| Menus and destructive actions | Open, cancel, confirm, restore focus | Visible entry, release activation, cancellation | No hover or right-click dependency |
| Focus loss, resize, navigation | Preserve logical focus | Cancel stale presses and drags | Cancel stale gestures and coordinates |

Verify these cases on shared controls and representative complete pages:

1. The minimum window retains readable content and essential actions. Ordinary areas have no full frames; bars and rows use the specified density.
2. Hover, press, release, exit, disabled state, current item, multi-selection, and keyboard focus remain distinct. A resting mouse does not reclaim keyboard emphasis.
3. A dialog intercepts every input method; closing it restores the correct page and focus. Drag-out and focus loss cancel pending actions.
4. Both keyboard and pointer transitions show readable first-frame content and respond during continuous switching, reversal, resize, and nested modal changes.
5. Details and the keyboard clip without blank frames. Vim and child-terminal painting, PTY sizes, and pointer targets agree throughout geometry changes.
6. Reduced motion reaches the final state immediately. Active transitions stop scheduling frames at completion; long slow output does not cause frame catch-up.
7. Semantic PTY checks use control IDs, snapshots, or business content. Do not depend on decorative border glyphs, the old `╭Status` title, or fixed screen coordinates.
8. Release measurements at 120 × 36 target draw P95 ≤ 16.7ms and input-to-output P95 ≤ 50ms. Report actual measured metrics, sample counts, build, and environment. PTY output or control-round-trip timing alone is not a draw-time measurement.

See [Testing guide](testing.md) for commands and the difference between automated, real-terminal, and device acceptance checks.
