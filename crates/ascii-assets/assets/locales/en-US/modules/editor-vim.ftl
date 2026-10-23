ui-editor-vim-toggle = VIM_mode
ui-editor-vim-return = F8 Return to editor
ui-editor-vim-off = { " F8 VIM_mode: Off " }
ui-editor-vim-normal = { " F8 VIM_mode: Normal " }
ui-editor-vim-insert = { " F8 VIM_mode: Insert " }
ui-editor-vim-help = Vim help
vim-install-title = Install Vim engine
vim-install-checking = Looking for the matching prebuilt engine…
vim-install-downloading = Downloading the Vim engine…
vim-install-extracting = Download complete. Extracting the engine…
vim-install-verifying = Checking the engine version and runtime…
vim-install-percent = { $percent }%
vim-install-cancel = Esc Cancel
vim-install-cancelling = Cancelling. Waiting for temporary files to be removed…
vim-install-cancelled = Vim installation cancelled. Your document is unchanged.
vim-install-not-published = The matching Vim engine package has not been published yet. Please try again after the project publishes it.
vim-install-unsupported = No prebuilt Vim engine is available for this system and architecture.
vim-install-worker-failed = The Vim installer stopped before it finished.
vim-install-invalid-checksum = The Vim package checksum file does not contain a valid SHA-256 checksum.
vim-install-damaged-download = The Vim download failed its SHA-256 or size check. Please try again.
vim-install-unsafe-archive = The Vim package contains an unsafe file path or symbolic link.
vim-install-mismatched-bundle = The Vim package does not match this application's pinned version.
ui-editor-vim-help-text =
    Vim mode runs the bundled Vim editor. Use :help or F1 for Vim's complete help.
    Normal, Insert, Replace, Visual and command-line modes, motions, operators, text objects, registers, macros and undo follow Vim.
    :write saves; :wq saves and quits; :quit quits; :quit! discards changes. Ctrl+W operates Vim windows.
    F8 returns to the regular editor. F8 again resumes Vim with its registers and undo history. The preference is saved for next time.
shell-vim-runtime-error = Vim could not continue: { $reason }. Your synchronized documents remain in the editor.
shell-vim-startup-failed = Vim did not connect to the editor. A full Vim build with channel, eval, autocmd and timer support is required.
shell-vim-exited-unexpectedly = Vim stopped unexpectedly (exit { $code }). Synchronized unsaved text is retained.
shell-vim-buffer-unavailable = The document for this Vim buffer is no longer available.
shell-vim-file-name-required = E32: No file name. Use :write with a path.
shell-vim-save-failed = The editor could not start this write. The Vim buffer remains modified.
shell-vim-save-cancelled = The configuration write was cancelled. The Vim buffer remains modified.
shell-vim-engine-missing = The bundled Vim engine is missing at { $path }. Build it with scripts/build-vim.py or use a complete portable package.
shell-vim-config-whole-write = Configuration files require a whole-buffer write through preview, validation and authorization.
shell-vim-unsupported-editor-encoding = The regular editor cannot preserve { $encoding }. Continue in Vim or use :set fileencoding=utf-8 before F8.
shell-vim-save-suspended = Vim writes are paused while the regular editor owns the documents. Press F8 to resume Vim before writing.
shell-vim-config-text-write = Configuration validation requires the current UTF-8 text without a BOM or write-time byte conversion. Use a regular file for an encoded copy.
shell-vim-unsupported-editor-bom = The regular editor requires a BOM for UTF-16. Continue in Vim or use :set bomb before F8.
