# Bundled bashdb

Upstream: https://github.com/Trepan-Debuggers/bashdb/releases/tag/5.2-1.2.0

Original release archive: `bashdb-5.2-1.2.0.tar.gz`

SHA-256: `67516c0a739bc189f9d6b14057eda66e7fa491a2bce6460a4d7173a0eede1ccb`

The original source and GPL-2.0-or-later license are retained here. Runtime files are embedded by `script-runtime/build.rs`; no source is fetched during ordinary builds.

Local fixes:

- Command input uses `read -r`, and eval/print/examine receive the complete argument string, preserving backslashes and spaces.
- Print and examine keep expression text unchanged and use bashdb's existing verbatim output helper instead of interpreting backslash escapes again.
- Eval writes its command with `printf` instead of `echo -e`, and captures errors in the debugger output channel.
- The trace loader quotes its library path.
- Quiet automatic exit preserves the debugged script's actual exit code.

Regression coverage: `crates/script-runtime/tests/real_bash.rs` and variable parser tests. `crates/script-runtime/src/bridge.sh` connects existing bashdb command-loop hooks to the application; bashdb continues to own breakpoints and stepping.
