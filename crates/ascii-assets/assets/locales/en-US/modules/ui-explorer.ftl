# UI explorer messages.

ui-explorer-explorer = Explorer
ui-explorer-explorer-ascii-assets-are-unavailable = Explorer ASCII assets are unavailable
ui-explorer-edit-button = { "[" }Edit]
ui-explorer-quick-access = Quick access
ui-explorer-trash-is-empty = (Trash is empty)
ui-explorer-empty-directory = (empty directory)
ui-explorer-no-entry-selected = No entry selected
ui-explorer-shown = shown
ui-explorer-hidden = hidden
ui-explorer-an-item-with-this-name-already-exists = An item with this name already exists.
ui-explorer-on = On
ui-explorer-off = Off
ui-explorer-search = Search: /
ui-explorer-empty = <empty>
ui-explorer-search-padded = Search:{ " " }
ui-explorer-entries = Entries
ui-explorer-active = active
ui-explorer-inactive = inactive
ui-explorer-none = none
ui-explorer-name = Name
ui-explorer-type = Type
ui-explorer-size = Size
ui-explorer-modified = Modified
ui-explorer-back = Back
ui-explorer-forward = Forward
ui-explorer-up = Up
ui-explorer-refresh = Refresh
ui-explorer-new = New
ui-explorer-cut = Cut
ui-explorer-copy = Copy
ui-explorer-paste = Paste
ui-explorer-rename = Rename
ui-explorer-delete = Delete
ui-explorer-restore = Restore
ui-explorer-dump-trash = Dump Trash
ui-explorer-sort = Sort
ui-explorer-options = Options
ui-explorer-scanning = Scanning
ui-explorer-checking-conflicts = Checking conflicts
ui-explorer-copying = Copying
ui-explorer-moving = Moving
ui-explorer-deleting = Deleting
ui-explorer-finishing = Finishing
ui-explorer-keep-both = Keep both
ui-explorer-replace = Replace
ui-explorer-skip = Skip
ui-explorer-cancel = Cancel

ui-explorer-selected-count = { $count } selected

ui-explorer-selected-names = Selected: { $names }

ui-explorer-entry-details = Name: { $name } | Type: { $kind } | Size: { $size }

ui-explorer-entry-metadata = Modified: { $modified } | Attributes: { $attributes }

ui-explorer-error = Error: { $error }

ui-explorer-operation-items = { $count ->
    [one] { $operation }: { $count } item
   *[other] { $operation }: { $count } items
    }

ui-explorer-metadata-warnings = { $count ->
    [one] { $count } metadata warning
   *[other] { $count } metadata warnings
    }

ui-explorer-conflict-source = Source: { $source }

ui-explorer-conflict-destination = Destination: { $destination }

ui-explorer-apply-remaining = Apply to remaining items: { $state }

ui-explorer-path = Path: { $path }

ui-explorer-hidden-files = Hidden files: { $state }

ui-explorer-search-query = Search: { $query }{ $suffix }

ui-explorer-search-matches = { " " }({ $count ->
        [one] { $count } match
       *[other] { $count } matches
    }, { $mode })

ui-explorer-help = Enter: open    Left/Right: back/forward    Backspace: parent    N: folder    T: text file    F2: rename    Del: delete    X: cut    C: copy    V: paste    F5: refresh    S: sort    O: options    /: search    H: hidden    Tab/Shift+Tab: quick access    Esc: back

ui-explorer-compact-help = Enter: open | Backspace: parent | /: search | Hidden files: { $hidden }{ $quick }

ui-explorer-quick-access-help = { " " }| Tab/Shift+Tab: quick access
