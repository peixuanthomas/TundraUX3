# Editor and script workspace

The built-in editor provides multiple documents, syntax highlighting, search and replace, soft wrapping, JSON formatting, and a real Vim mode. The Linux script workspace reuses the same editor and provides Bash execution and debugging.

## Standard editor

The interface is modeled on [Microsoft Edit](https://github.com/microsoft/edit/tree/cb6de0a8966aecd01d19a3541dab1829d0e679ca/crates/edit/src/bin/edit), with File, Edit, View, and Help menus. Find and Replace bars appear above the document; controls for language, line endings, encoding, indentation, line/column position, and document switching appear at the bottom.

- Ctrl+N creates a document, Ctrl+O opens a file, and Ctrl+P shows the document list.
- Ctrl+Tab / Ctrl+Shift+Tab switches documents, Ctrl+W closes the current document, and Ctrl+Q closes the entire group one document at a time.
- Each document keeps its own undo history, cursor, search options, file tasks, and recovery draft.
- F10 focuses the menu, Alt+F/E/V/H opens the corresponding menu, and Esc returns to the document.
- Ctrl+F10 opens Linux system configuration files; saving goes through a diff preview, configuration validation, authorization, and backup.
- Green, yellow, and red marks beside line numbers indicate additions, changes, and deletions, and clear after saving.

Text is stored in `ropey::Rope`. UI positions and editing boundaries use Unicode grapheme clusters, while search and syntax tokens use UTF-8 byte ranges.

## Common operations

| Operation | Entry point | Notes |
| --- | --- | --- |
| Find | Ctrl+F / Edit → Find | Supports plain strings, regex, case sensitivity, whole words, and backward search |
| Next / previous match | F3 / Shift+F3 | Cycles through matches, including zero-width regex matches |
| Replace | Ctrl+R, Ctrl+H / Edit → Replace | Enter replaces one match; Ctrl+Enter replaces all. An empty replacement deletes the matched text |
| Close Find bar | Esc / Close button | Preserves the query, replacement text, and options |
| Go to line | Ctrl+G / View → Go to Line | Enter a 1-based line number or `line:column` |
| Line endings and encoding | Bottom buttons | Supports LF/CRLF/CR and UTF-8, UTF-8 BOM, UTF-16LE, UTF-16BE |
| Indentation | Bottom button / Tab, Shift+Tab | Sets spaces or tabs and their width; supports adjusting indentation on multiple lines |
| Soft wrap | Alt+Z / View menu | Wraps to the display width, marks continuation lines with `↪`, and does not change the file |
| Format JSON | Ctrl+Shift+J | Uses two-space indentation and can be reverted with one undo |

Regular expressions use Rust `regex` syntax with multiline anchors enabled by default; use `(?s)` for matches across lines. Replacements support `$0`, `$1`, `${name}`, and `$$`. Matches that would split combining characters, emoji, or CRLF are skipped.

Replacements are computed in the background. Before applying them, the document ID, revision, and original text are checked; the whole batch is a single undo operation. One replacement operation handles at most 10,000 matches or 8 MiB of data. System configuration replacement previews show the first 40 matches and apply the full result after confirmation.

JSON formatting validates the entire document first, then adjusts whitespace outside strings while preserving field order, duplicate keys, number notation, string escapes, and BOM. Invalid JSON reports its location and leaves the original text intact.

## Real Vim mode

F8 or the Vim button switches to the bundled real Vim. The editing area directly displays Vim's mode, selections, registers, macros, windows, command line, search, errors, and `:help`; Vim handles the keys.

| Operation | Behavior |
| --- | --- |
| F8 / VIM mode enabled | Synchronizes unsaved content and returns to the standard editor without automatically writing the file |
| Press F8 again | Synchronizes changes from the standard editor and resumes the same Vim process |
| `:w`, `:write filename` | Writes through the host save task and confirms after success |
| `:wq` | Exits after a successful save; retains changes on failure |
| `:q`, `:q!`, `:qa!` | Follows Vim's window, buffer, and discard rules |

Vim writes reuse the host's asynchronous saving, external-change checks, and permission workflow. System configuration still goes through diff review, validation, authorization, and backup; protected configuration accepts full-document UTF-8 writes without a BOM.

The standard editor supports UTF-8 and UTF-16. After using another `fileencoding` in Vim, convert to a host-supported encoding before returning to the standard editor. Normal operation loads the user's Vim configuration and plugins.

Portable builds place the complete engine in `vim/` beside Shell. If the engine is missing, the installation dialog can download a prebuilt package from the project's Release. For development builds, use:

```sh
python3 scripts/build-vim.py --output target/debug/vim
```

`TUNDRA_VIM` selects another full Vim; `TUNDRA_VIM_CLEAN=1` isolates personal configuration and plugins. See [bundled Vim](packaging/vim.md) for build and release details, and [real Vim regression tests](testing.md#real-vim-regression-tests) for test entry points.

## Linux script workspace

Open Script Editor from Launcher, or choose Script → Script Workspace in a script document. New scripts use `#!/usr/bin/env bash`.

Run settings can specify the interpreter, arguments, and working directory. When the interpreter is blank, it is read from the script's first line; `.bash` defaults to Bash, and other scripts without a declaration default to `/bin/sh`. Argument parsing handles quotes and escapes without command substitution or variable expansion. Breakpoint debugging requires Bash 5.2 or newer and uses the bundled bashdb debugger.

| Operation | Shortcut |
| --- | --- |
| Run / debug or continue | Ctrl+F5 / F5 |
| Step over / into / out | F7 / F11 / Shift+F11 |
| Stop / toggle breakpoint | Shift+F5 / F9 |
| Next / previous region | F6 / Shift+F6 |
| Actions within a region / editor menu | Tab, Enter / F10 |
| Evaluate expression / execute Bash command | Enter / Ctrl+Enter |

- Modified scripts start only after saving successfully; the current document is read-only during execution.
- The interface provides source, output, variables, call stack, expressions, and breakpoints regions, with clickable buttons for every operation.
- Expressions are evaluated while paused; Execute Command can change variables in the debugged program. The output region directly receives script key and paste input.
- Function-call and breakpoint lists can navigate to files included with `source`. Hitting a breakpoint in the background leaves execution paused, and a notification provides a way back.
- Scrollable output remains after execution ends. Closing the page offers continuing in the background, stopping and closing, or staying on the page.
- Breakpoints and expression history belong to the current document page. Invalid breakpoints appear as Not active; validity is confirmed when they are actually hit.

## Implementation structure

- `crates/app/src/editor/syntax.rs` feeds line-by-line text to LSH, maintains state across lines, and caches token types and byte ranges.
- `TundraTheme::syntax_style` in `crates/ui/src/theme/mod.rs` maps tokens to theme colors.
- `crates/app/src/editor/search.rs` implements an independent search engine; replacements are committed together through a Source transaction.
- `crates/app/src/editor/soft_wrap.rs` generates mappings between logical and display lines, shared by the cursor, mouse, scrolling, and line numbers.
- LSH uses the MIT-licensed component from [Microsoft Edit](https://github.com/microsoft/edit/tree/1be69c85a63bcd475a0b397196d0be3dbed60023/crates/lsh), with source in `third_party/lsh`.

## Validation

```sh
cargo test --locked -p app -p ui -p shell --test editor --test editor_advanced
cargo test --locked -p shell --lib controller::management::config_editor::tests
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored
python3 scripts/tests/editor-vim-smoke.py target/debug/tundra-shell target/debug/vim/vim
```
