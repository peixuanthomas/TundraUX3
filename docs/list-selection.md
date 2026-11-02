# List multi-selection

Multi-selection is for lists where the same operation can be applied to multiple objects. Lists used only to view details, enter individual parameters, or configure items one at a time remain single-selection.

## Common operations

Packages, services, processes, scheduled tasks, and file management provide Select Multiple / Finish Selection controls, checkboxes, and a selection count.

- Tapping a row toggles its checkbox; arrow keys move the current position; Enter or Space toggles the current row.
- Exiting selection mode clears the batch selection and restores single selection of the current row. Batch operations cannot run with an empty selection.
- Sorting and refreshing retain targets that still exist; changing filters or query criteria clears the selection.
- Confirmation and authorization dialogs list every target, and background processing checks the objects again before execution.
- File management retains Shift+click for range selection and Ctrl/Command+click for individual selection; other management pages use Shift+click or Shift+Enter for individual selection.
- Mouse and touchscreen gestures are canceled after dragging out, scrolling, focus loss, or resizing, so releasing cannot act on a new target.

Package selections are stored by package ID. Installation, removal, or upgrade is allowed only when all selected packages support the same operation. APT, DNF, and pacman use batch commands; pacman installation and upgrade also perform a full system upgrade. Homebrew groups by Formula/Cask, while WinGet processes packages individually using exact IDs and versions.

Services support batch start, stop, restart, reload, enable, and disable. Processes support batch suspend, resume, termination, and assignment of the same priority, with process identity checked before execution. Scheduled tasks support batch pause, resume, and run-now; changes from the same cron source are saved together.

## Selection behavior by page

| Page or list | Selection | Notes |
| --- | --- | --- |
| Packages: Installed, Updates, Search | Multiple | Batch installation, removal, and upgrades |
| Packages: sources, configuration conflicts, check results | Single | Inspect, compare, or repair individually |
| File Explorer: ordinary directories and Trash | Multiple | Copy, cut, delete, restore, and change permissions |
| File Explorer: quick access, history, and options | Single | Open one directory or change one option at a time |
| Service management | Multiple | Log viewing and configuration editing remain individual |
| Process management | Multiple | Send signals or set priorities in batches |
| Scheduled tasks | Multiple | Creation, editing, and time previews remain individual |
| Firewall rules and listening ports | Single | Review rules individually; ports are primarily for inspection |
| Users, groups, and in-app accounts | Single | Passwords, membership, ownership, and deletion concern one account |
| Networks, disks, SSH, system settings | Single | Each configuration uses separate parameters and checks |
| Configuration files and configuration backups | Single | Edit or restore one configuration at a time |
| Launcher applications and shortcuts | Single | Opening, ordering, and editing concern one item |
| Settings categories, application lists, and field options | Single | Checkboxes can still be toggled individually |
| Logs, log files, and incident reports | Single | Open details, follow, and inspect |
| Diagnostic checks and repair previews | Single | Supports repairing the current item or all items |
| System status cards and selection dialogs | Single | Inspect data and edit cards individually |
| Clock alarms and timers | Single | Management dialogs handle one item at a time |
| Editor documents and menus | Single | Text selections and Vim Visual mode follow their own rules |
| Login, initial setup, and timezone map | Single | Select one account, option, or input field at a time |
| Home, Help, notifications, Command Line, AA, weather | Single | Primarily entry points, instructions, messages, or the current task |

When adding a batch operation, first make the backend accept and validate multiple targets, then connect page selection, confirmation, and authorization. See F1 help for each page's complete shortcuts.
