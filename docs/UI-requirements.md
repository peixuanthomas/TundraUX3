# UI design and acceptance requirements

This document applies to Home, Settings, accounts, file management, Launcher, system management, toolbars, and dialogs, including AutoAdmin (AA). Check both new and modified pages against these requirements.

## Text and action entry points

- Buttons state the action and its object directly, with any existing shortcut displayed next to the action text.
- Common operations use visible buttons and options; complete keyboard instructions belong in F1 help.
- Empty states, details, and status bars each provide useful information; error messages explain the specific cause and next step.
- Update Chinese and English text together, then check text width, layout, and clickable areas.
- Place page actions at the bottom right of the content area, with search, filters, refresh, and path navigation at the top. Show at most five actions directly on each page; put less frequent actions under More or Advanced.
- Place the primary action at the far right and use the danger color for destructive actions. Buttons that depend on a selection remain visible but disabled when nothing is selected.
- Center More Actions in the content area and size it to the longest option and the number of options. Allow scrolling and button wrapping in small windows, keeping the current option visible.

## Buttons and input methods

The shared Button, Theme, and RenderContext handle button states; pages only pass states such as focused, selected, and disabled.

| State | Appearance and behavior |
| --- | --- |
| Normal | Use the theme's normal text and border |
| Mouse hover | Use the theme's accent color |
| Mouse pressed | Use a lighter accent color; the shared implementation blends 35% toward white, while ANSI uses the corresponding bright color |
| Keyboard focus | Keep the accent color active; also accent the border on bordered buttons |
| Disabled | Use `TundraTheme::disabled_style()` and ignore actions |
| Click completed | Clear pressed and hover feedback until the next actual mouse movement |

Follow these rules when switching input methods:

- Initially show the keyboard selection. Valid keyboard navigation moves focus immediately; actual mouse movement, taps, drags, or scrolling hide the keyboard focus indicator.
- Preserve logical focus when switching back to the keyboard. A stationary mouse must not highlight another button; repeated reports of the same coordinates must not switch the input method.
- Activate a button only after pressing and releasing within the same target. Dragging outside, losing focus, resizing, or changing pages cancels the pending action.
- A double click requires the first click to have been released and the second press to occur on the same target and cell within 500 milliseconds. Scrolling, dragging, keyboard input, focus loss, or resizing clears the consecutive-click record.
- Only controls in the topmost dialog respond. Covered pages receive no hover, press, or text input. Toasts do not take focus.
- Hide keyboard emphasis when the window loses focus, while preserving logical focus. Highlighting of the current page and selected records is independent of the input method.
- Keep selected text fields in the accent color and show a blinking underscore cursor at the actual position. Up/Down switches fields; Left/Right moves the cursor. Passwords remain masked, and long text scrolls with the cursor.

## Animation and page hierarchy

Animations show where pages come from and return to. Focus, scrolling, and text input always respond immediately.

- Keyboard actions display the final layout immediately. When a mouse or touchscreen opens a page, dialog, or menu, its border expands from the entry point and returns there on closing. A context menu without entry-point coordinates expands from its own center.
- In wide windows, details expand to the right and the list contracts to the left; narrow windows use vertically stacked panes. The screen keyboard opens and closes from the bottom, and related layouts use the same animation progress.
- Moving pages draw only borders and background colors; body content appears when opening or closing finishes. Closing an upper dialog leaves the underlying page unchanged.
- Default transitions take 220ms for pages, 180ms for dialogs, and 160ms for menus. The speed curve accelerates for the first 25%, remains constant for the middle 50%, and decelerates for the final 25%.
- Settings provides animation speed, nine phase durations, reset to defaults, and reduced motion. Each phase ranges from 1 to 2000ms; resetting restores both 100% speed and all phase durations.
- Switching to the keyboard, reversing an opening/closing transition, or resizing during an animation must immediately produce the correct state. Moved controls must not respond to clicks at old coordinates.
- Speed curves, timing, expansion directions, and pane-size calculations belong in `crates/ui`; Shell manages input sources, page order, and dialog origins.

## Pages, navigation, and global bars

- `ScreenCompositor` draws the title bar, Back button, status bar, and clock centrally. Pages draw only `ShellFrameLayout.main` and their own overlays.
- Create a separate instance each time a page opens. Check instance IDs for page state, task results, and focus restoration so that late results cannot enter a reopened page.
- The Shell navigation manager handles entry, overlays, and return navigation. Esc and the Back button first close dialogs or complete page-local actions, then return one level at a time along the opening path.
- Holding Esc must not exit multiple levels in succession. Keys in Command Line and the Vim editing area go to the child terminal; the outer Back button still offers close confirmation.
- Closing a page with running work offers Continue in background / Cancel task and close / Stay on page. After cancellation is chosen, wait for the process to stop and cleanup to finish.
- Vim mode displays the full Vim terminal. The ordinary editor's menus and status line must not cover Vim's mode, selection, command line, or error messages.
- Temporary messages use the shared `Surface`, with border shape and color following the theme. Success, warning, and danger messages use their respective message colors.

## Screen keyboard, forms, and selection dialogs

- Place the screen keyboard entry point at the bottom right. On ordinary pages, opening the keyboard collapses the title bar, hides the status bar, and makes room for content; AA only adjusts the position of the topmost dialog.
- Keep dialogs and action buttons above the keyboard. When the window is too small, ask the user to enlarge it instead of covering the form with the keyboard.
- Place Paste at the left end of the keyboard's bottom row and Hide at the right. Manage physical-key feedback separately from mouse modifier-key state, and clear both after focus loss or resizing.
- Keyboard keys use neutral borders and readable text with a contrast ratio of at least 4.5:1; pressed or latched keys use an accent-color fill.
- Input dialogs use the shared `FormDialog`. Enter moves to the next field and confirms from the last field; Space or a tap toggles switches; Ctrl+Enter confirms directly; F10 activates the default confirmation button.
- Keep the current field and the Confirm and Cancel buttons reachable when scrolling long forms. Passwords remain masked in both the rendered screen and control snapshots.
- The Packages page uses an Installed, Updates, Search sidebar, and Settings reuses the same layout. Tab/Shift+Tab switches main categories, arrow keys select items, and Enter/Space activates them.
- Numeric options use dialogs for direct entry or adjustment with minus/plus controls; AA policy and screen timeout use selection dialogs.

## Lists and sorting

Packages, services, processes, scheduled tasks, and file management use consistent multi-selection behavior. See [list selection](list-selection.md) for its scope.

- Clicking a column header sorts by it; clicking again toggles ascending/descending order, with an arrow showing the direction.
- Text comparison is case-insensitive; counts, percentages, capacities, and rates use numeric comparison. File management keeps folders first.
- Sorting moves whole records and preserves the selected objects; the current sort remains applied after refresh.
- Ctrl+F6 switches the sort column, and Ctrl+Shift+F6 reverses the current column. File management and Launcher keep their own sorting shortcuts.
- Manually sorting logs pauses automatic following; returning to the latest records restores reverse chronological order.

## Acceptance checks

Complete each workflow separately with each input method, without using another method to supply missing actions. Mouse and touch acceptance checks require a terminal that forwards the corresponding events; automated touch tests use presses, releases, and drags without hover.

| Workflow | Keyboard | Mouse | Touchscreen |
| --- | --- | --- | --- |
| Enter, act, and return | Tab/arrows, Enter/Space, and shortcuts | Visible buttons, list items, and Back button | Tap the same targets directly |
| Long lists and dialogs | Arrows, page navigation, Home/End | Wheel or scrollbar dragging | Scrollbar dragging |
| Text editing and submission | Typing, deletion, focus navigation, and submit keys | On-screen input tools and Submit/Cancel buttons | On-screen input tools and tappable buttons |
| Menus and destructive actions | Menus, confirmation, cancellation, and focus restoration | Visible entry points, confirmation, and cancellation | Tap entry points without relying on hover or right-click |
| Focus loss, resizing, and page changes | Preserve logical selection and restore focus | Cancel old presses and drags | Cancel old gestures and coordinates |

Check at least the following scenarios:

1. Complete a page or dialog workflow using only the keyboard, with focus clearly visible throughout.
2. Verify colors after mouse entry, press, release, exit, and subsequent movement, in that order.
3. With the mouse resting on one button, select another using the keyboard and confirm that the stationary mouse does not reclaim the highlight.
4. Verify dialog occlusion, disabled buttons, drag-out cancellation, and state cleanup after focus loss and resizing.
5. Check actual colors, hit locations, and terminal behavior for shared buttons and complete dialog workflows.
