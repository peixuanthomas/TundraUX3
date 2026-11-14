# UI design and acceptance requirements

This is the shared design and acceptance contract for the complete interface: startup and account flows, Home, Launcher, Settings, system management, files, editor and Vim, scripts, Command Line, Tasks, clock, weather, dialogs, menus, and AutoAdmin (AA). Apply it in stages across shared foundations, global bars, workspaces, tools, and motion; a page is complete only when its rendering, input, automation, and tests agree.

## Visual language and density

The default dark appearance retains the v1.5.1 Glacier Night blue surfaces and one accent. Layout and control hierarchy follow the OpenCode terminal interface; its black palette is not used. Theme overrides and explicit saved appearance choices remain supported.

| Token | Default | Use |
| --- | --- | --- |
| Canvas | `#071116` | Full-frame background |
| Surface | `#0D1B22` | Grouped content |
| Raised surface | `#13262F` | Raised controls, sidebars, and overlays |
| Text | `#E6F1F4` | Normal readable content |
| Muted text | `#8EA7B0` | Secondary information |
| Accent | `#63D3E5` | Focus and primary actions |
| Border | `#29434E` | Inputs and necessary overlay boundaries |

- Each application window has one complete outer frame with its own title and controls. Separate the page's internal groups, lists, tables, and action areas with headings, whitespace, alignment, or a change of background; do not nest decorative frames.
- A modal, menu, or input may use one boundary. Temporary messages use the shared surface and the corresponding success, warning, or danger color.
- Dialogs share three-column side padding, a title and top-right close action, a readable body, and a raised footer when actions are present. Compact windows reduce vertical gaps before hiding required content. Dim covered content without changing its layout; Toasts remain in the upper-right and do not take focus.
- Ordinary buttons use plain action text without `[]` on a visible control surface, distinct from nearby static text. Primary actions use an accent fill; destructive actions use the danger color. Selection and the current item use background emphasis independently of keyboard focus.
- Navigation entries and table rows each occupy one terminal row. Use one row between groups, two columns between adjacent groups, and one cell between grid items. Keep a complete row clickable, including its padding, for mouse and touch.
- The global title and status bars each occupy one row. At 108 × 20, the application window occupies 108 × 18 cells and its outer frame leaves 106 × 16 content cells. Tasks, Help, Back, and the clock remain available. The bottom window bar shares the status row with the keyboard entry and clock; global errors keep a separate clickable status area.
- Use the same spacing, typography, and control states across pages. Text must remain readable with truecolor or ANSI color, and with Chinese, English, combining marks, and wide characters.

New defaults apply when a preference is absent. Preserve existing legacy-color migration and explicit saved border, color, theme, animation speed, and timing preferences; do not overwrite them merely because defaults changed.

### OpenCode source reference

The implementation reference is `anomalyco/opencode` at commit `5b131f5995a9f7fad458c59a7a5e39686237beb8`, inspected on 2026-11-13. Use its terminal components, not its desktop/web CSS:

- [Dialog, header, layout, buttons, and footer](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/ui/dialog.tsx): a single modal surface, padded title and body, distinct footer actions, and an obscured background.
- [Selection dialog](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/ui/dialog-select.tsx): full-row interaction, grouped options, muted supporting text, and separate active/current markers.
- [Session sidebar](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/routes/session/sidebar.tsx): a distinct surface with shared padding and spacing between groups.
- [Spinner](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/component/spinner.tsx), [shimmer text](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/component/shimmer-text.tsx), and [animation scheduling](https://github.com/anomalyco/opencode/blob/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src/ui/animation.ts): elapsed-time feedback, motion-disabled fallbacks, and scheduling only while motion remains active.

OpenCode's dialog mounts directly; it does not supply TundraUX3's page/dialog entrance transitions. The existing configurable phases below remain TundraUX3 behavior. Keep TundraUX3's release-inside-target activation, safety confirmations, localization, terminal compatibility, and application workflows when adapting these visual patterns.

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
- Only the topmost dialog receives hover, press, text, or keyboard input. Covered pages retain their state. Toasts do not take keyboard focus, but their painted area blocks pointer activation of covered controls until the Toast disappears.
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
| Busy indicator | 80ms per step | One terminal character |
| Loading label | 1200ms per sweep | Muted text with OpenCode's moving highlight |

- Page, dialog, and menu bodies are readable on the first frame. Do not show an empty moving outline or wait for the transition to reveal text. Opening and closing keep the intended final layout and blend only the presentation.
- Details expand rightward in wide windows and vertically in narrow windows. The list, details, and keyboard use the same current geometry for painting, clipping, PTY sizing, automation, and hit testing. Do not insert blank intermediate frames.
- The real progress value and its numeric label update immediately. Only the graphic interpolates; changing targets or reversing direction starts from its current displayed value and never overshoots.
- New navigation, reverse actions, repeated switching, input-method changes, and resize interrupt or retarget the active transition. Do not queue animations or require an extra key or click to finish one.
- Reduced motion renders the terminal state directly, including details, keyboard, progress, and busy feedback. No hidden animation continues afterward.
- Active UI transitions schedule frames at 60 FPS. Idle UI has no animation timer. Weather and continuous rainbow effects retain their separate 30 FPS policy. Cursor blink, clocks, and real data refresh retain their own deadlines.
- A visible loading label refreshes at 20Hz in truecolor. ANSI uses only the 80ms spinner cadence without RGB interpolation; reduced motion uses a static ellipsis and schedules neither effect. Respect animation speed and terminal-column width for Chinese and combining text.
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
- Create an independent instance when opening a subpage or explicitly starting a new application session. An ordinary application launch restores an existing window when available. Check instance IDs for state, task results, and focus restoration so late results cannot enter a reopened page.
- Shell handles entry and overlays. Esc first cancels the top dialog, menu, or page-local action. At an ordinary app root, Esc moves only the focused app to the background and retains its state and work. Command Line, native Vim, and custom terminal apps receive their own Esc.
- Every visible window, including a single window or a maximized window, has a complete frame. Its top edge contains the title, Minimize, Full screen/Restore, Split, and Close buttons. Clicking the frame focuses that window. Buttons act on their own window on release, even if another window previously had focus.
- The bottom window bar lists running apps. Click a label to focus or restore it. Click the separate minus button to request normal close. Scroll buttons keep overflow entries reachable. Right-click a taskbar entry for Open, New session, and Move. Delete appears only for user-added Launcher entries and removes the entry after the selected window closes successfully. Launcher icons use the same menu labels for the applicable actions. The split button opens a menu next to the window for choosing side-by-side, stacked, or grid layout and adding an app.
- The workspace prefix is Ctrl+B followed by B to background the focused app, W for windows, L for layout, arrows for focus, Z for zoom, Q for normal close, and uppercase X for immediate kill. Ctrl+B twice forwards one literal Ctrl+B; Esc cancels a pending prefix. Holding Esc must not dismiss several layers.
- Normal close retains existing dirty-document saving and warns before interrupting running work, with Cancel selected by default. After two seconds without a close response, offer waiting or immediate kill. Immediate kill is a distinct danger action and has no second confirmation; show the app as stopping until the guardian confirms it has exited.
- Keep the terminal minimum at 108 columns × 20 rows. All living apps, including background apps, contribute to the component-wise maximum content minimum. Default content minimum is 106 × 16. Reserve the global title and status rows, a one-cell frame on all four sides of every window, and one-cell gaps between windows. Equal horizontal/vertical layouts show at most two apps; a grid can show more as space permits.
- A custom PTY app may report `TUNDRA_MIN_SIZE` through OSC 1337 SetUserVar, using Base64-encoded positive `colsxrows`. Recompute capacity when its valid report changes. Invalid reports retain the previous minimum, empty reports withdraw it, and confirmed process exit removes it. If the global minimum cannot fit one content area, keep window management and kill actions available while showing the size requirement.
- When content cannot fit, retain the focused window's frame controls and the bottom window bar, but suspend its business viewport. If ordinary close needs a save or task confirmation that cannot fit, explain that the terminal must be enlarged and offer waiting or force kill; never silently discard work.
- Size-driven overflow retains the focused app and a stable window order, and restores automatically hidden panes when space grows. Manually backgrounded or replaced panes stay in the background. Zoom preserves the previous split layout. New launches replace the focused slot and background its former app; adding another visible pane is explicit.
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

1. The minimum window retains readable content and essential actions inside its complete outer frame. Interior groups do not add decorative frames; bars and rows use the specified density.
2. Hover, press, release, exit, disabled state, current item, multi-selection, and keyboard focus remain distinct. A resting mouse does not reclaim keyboard emphasis.
3. A dialog intercepts every input method; closing it restores the correct page and focus. Drag-out and focus loss cancel pending actions.
4. Both keyboard and pointer transitions show readable first-frame content and respond during continuous switching, reversal, resize, and nested modal changes.
5. Details and the keyboard clip without blank frames. Vim and child-terminal painting, PTY sizes, and pointer targets agree throughout geometry changes.
6. Reduced motion reaches the final state immediately. Active transitions stop scheduling frames at completion; long slow output does not cause frame catch-up.
7. Semantic PTY checks use control IDs, snapshots, or business content. Do not depend on decorative border glyphs, the old `╭Status` title, or fixed screen coordinates.
8. Release measurements at 120 × 36 target draw P95 ≤ 16.7ms and input-to-output P95 ≤ 50ms. Report actual measured metrics, sample counts, build, and environment. PTY output or control-round-trip timing alone is not a draw-time measurement.

See [Testing guide](testing.md) for commands and the difference between automated, real-terminal, and device acceptance checks.
