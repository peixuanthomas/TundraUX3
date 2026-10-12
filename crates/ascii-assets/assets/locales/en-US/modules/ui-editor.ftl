# UI editor messages.

ui-editor-normal = Normal
ui-editor-new-padded = { " " }New{ " " }
ui-editor-open-padded = { " " }Open{ " " }
ui-editor-save-padded = { " " }Save{ " " }
ui-editor-undo-padded = { " " }Undo{ " " }
ui-editor-redo-padded = { " " }Redo{ " " }
ui-editor-normal-padded = { " " }Normal{ " " }
ui-editor-code-padded = { " " }Code{ " " }
ui-editor-bullets-padded = { " " }Bullets{ " " }
ui-editor-numbered-padded = { " " }Numbered{ " " }
ui-editor-quote-padded = { " " }Quote{ " " }
ui-editor-link-padded = { " " }Link{ " " }
ui-editor-image-padded = { " " }Image{ " " }
ui-editor-table-padded = { " " }Table{ " " }
ui-editor-find-padded = { " " }Find{ " " }
ui-editor-more-padded = { " " }More{ " " }
ui-editor-file = File
ui-editor-edit = Edit
ui-editor-insert = Insert
ui-editor-format = Format
ui-editor-view = View
ui-editor-settings = Settings
ui-editor-new = New
ui-editor-open = Open
ui-editor-save = Save
ui-editor-undo = Undo
ui-editor-redo = Redo
ui-editor-normal-text = Normal text
ui-editor-bold = Bold
ui-editor-italic = Italic
ui-editor-strikethrough = Strikethrough
ui-editor-inline-code = Inline code
ui-editor-bulleted-list = Bulleted list
ui-editor-numbered-list = Numbered list
ui-editor-quote = Quote
ui-editor-link = Link
ui-editor-image = Image
ui-editor-table = Table
ui-editor-find = Find
ui-editor-more = More
ui-editor-rich-view = Rich view
ui-editor-source-view = Source view
ui-editor-rich = Rich
ui-editor-source = Source
ui-editor-editor-settings-padded = { " " }Editor Settings{ " " }
ui-editor-hold-one-direction-to-accelerate-with-a-quadratic-curve = Hold one direction to accelerate with a quadratic curve.
ui-editor-read-only-administrator-permission-is-required-to-change-these-settings = Read-only: administrator permission is required to change these settings.
ui-editor-cursor-acceleration-padded = { " " }Cursor acceleration
ui-editor-start-delay-padded = { " " }Start delay
ui-editor-ramp-to-maximum-padded = { " " }Ramp to maximum
ui-editor-horizontal-maximum-padded = { " " }Horizontal maximum
ui-editor-vertical-maximum-padded = { " " }Vertical maximum
ui-editor-on-button = { "[" }T On]
ui-editor-off-button = { "[" }T Off]
ui-editor-restore-defaults-button = { "[" }R Reset]
ui-editor-save-button = { "[" }Ctrl+S Save]
ui-editor-cancel-button = { "[" }Esc Cancel]
ui-editor-tab-select-left-right-adjust-enter-activate-esc-cancel = Tab select · ←/→ or -/+ adjust · Enter activate
ui-editor-image-detecting = image:detecting
ui-editor-image-fallback = image:fallback
ui-editor-image-terminal = image:terminal
ui-editor-bytes-0-of-0 = Bytes 0 of 0
ui-editor-read-only = Read only
ui-editor-ready = Ready

ui-editor-milliseconds = { $count } ms

ui-editor-cells = { $count ->
    [one] { $count } cell
   *[other] { $count } cells
    }

ui-editor-lines = { $count ->
    [one] { $count } line
   *[other] { $count } lines
    }

ui-editor-byte-window = Bytes { $start }-{ $end } of { $total }

ui-editor-reload-help = { $left } · R Reload

ui-editor-status = { $mode }  Ln { $line }, Col { $column }  { $count ->
    [one] { $count } word
   *[other] { $count } words
    }  { $encoding }/{ $line_ending }  { $protocol }{ $window }

ui-editor-read-only-title = { $title } [read-only]

ui-editor-tail-title = { $title } [tail]

ui-editor-replace = Replace
ui-editor-wrap = Toggle soft wrap
ui-editor-format-json = Format JSON
editor-search-stale-preview = The document changed. Create a new replacement preview.
editor-search-invalid-range = Replacement ranges are invalid or no longer match the document.
config-editor-replace = Find and replace
config-editor-replace-apply = Apply replacement preview
editor-search-text = Find
editor-search-regex = Regular expression
editor-search-case_sensitive = Case sensitive
editor-search-whole_word = Whole word
editor-search-backwards = Search backwards
editor-search-replacement = Replace with (empty deletes matches)
editor-search-scope = Replace scope
editor-search-all = All matches
editor-search-current = Current / next match
editor-search-help = F3: next · Shift+F3: previous. Regex replacements support $1 and ${ "{" }name{ "}" }. Matches respect grapheme boundaries.
editor-search-preview-count = { $count } replacements. Submit to apply; Escape to cancel. One Undo restores the batch.
editor-search-preview-truncated = Showing the first 40 replacements. Apply includes all matches.
editor-search-count = Match { $current } / { $count } · F3 / Shift+F3
