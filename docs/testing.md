# Testing guide

Run all commands from the repository root. Automated tests cover core workflows, data safety, and permission boundaries; real terminals, touchscreens, system authorization, and device writes require separate acceptance checks on the corresponding platform.

## Routine checks

```sh
cargo fmt --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --locked -p shell -p cli -p weathr
python3 scripts/check-localization.py
bash scripts/check-weathr-dependency-boundary.sh
```

The fixed test suite makes no HTTP requests and starts no HTTP test servers. Add `--offline` when dependencies are cached; add `-- --test-threads=1` when investigating concurrency issues.

When resources are limited, use:

```sh
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo test --workspace --locked
```

## Targeted tests

```sh
cargo test --locked -p app
cargo test --locked -p ui
cargo test --locked -p shell
cargo test --locked -p storage
cargo test --locked -p identity
cargo test --locked -p platform
cargo test --locked -p system-services --no-default-features
cargo test --locked -p system-services --features runtime
```

The minimum supported version is Rust 1.95. After changing the toolchain or dependencies, check that minimum version:

```sh
rustup toolchain install 1.95.0 --profile minimal
cargo +1.95.0 check --workspace --all-targets --all-features --locked
cargo +1.95.0 test --workspace --locked
```

CI configuration is in [cross-platform.yml](../.github/workflows/cross-platform.yml), [linux.yml](../.github/workflows/linux.yml), and the corresponding release workflows. Run Linux-related checks in WSL or native Linux, and Windows and macOS checks on their respective platforms.

## Linux PTY and system operations

Build Shell, CLI, and Vim before running real-terminal checks:

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
TUNDRA_PTY_CHECK_CRASH=1 python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
python3 scripts/tests/shell-control.py target/debug/tundra-shell
```

These scripts use isolated XDG directories and real PTYs to check initial setup, file workflows, mouse and keyboard input, Command Line, external UI control, crash reports, and terminal restoration. `TUNDRA_PTY_MOUSE_EVENT_COUNT` adjusts the number of events in the mouse-event flood test.

Readiness and global-button targeting use automation snapshots and control IDs. Page assertions use business content and painted screens, so changes to decorative frames or bar coordinates do not change the workflow contract. Shell and its companion CLI must come from the same build.

The native Trash round-trip test creates only temporary items:

```sh
cargo test --locked -p platform --test native_trash_smoke -- --ignored --nocapture
```

Isolated service and scheduled-task tests use private systemd, D-Bus, mount, PID, network, and cgroup environments:

```sh
scripts/tests/isolated-systemd.sh /absolute/path/to/platform-test-binary ORIGINAL_USER_UID selection
```

Run root-startup checks only in disposable Linux test environments:

```sh
sudo python3 scripts/tests/linux-root-startup.py target/debug/tundra-shell target/debug/tundra-cli
sudo python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
```

Package queries use fixed fixtures and read-only smoke tests. Do not run destructive installation, removal, or system-upgrade tests on the development host; verify real writes in an isolated environment.

## Compact UI and motion acceptance

Run the semantic matrix after building Shell and CLI:

```sh
python3 scripts/tests/ui-refresh-smoke.py target/debug/tundra-shell
```

The script checks 108 × 20, 120 × 36, and 180 × 50 in English and Chinese, with truecolor and ANSI terminal capabilities, and full and reduced motion. It checks one-row global controls, the keyboard entry before the clock, tap activation without hover, modal interception, resize during a dialog, wide and combining characters, and return navigation using current control rectangles. For a focused run, select `--language en-US --color truecolor --motion full`.

Also complete the keyboard-only, mouse-only, and touchscreen workflows in [UI requirements](UI-requirements.md), including rapid page changes, reversal, continuous resize, details and keyboard expansion, sorting, multi-selection, and expert areas. Ordinary pages and groups use whitespace instead of full frames. At 108 × 20, the title and status rows leave 18 content rows. Check that text remains readable on the first transition frame and that Vim/child-terminal sizing and pointer targets follow the same geometry.

For performance measurements, use a Release build at 120 × 36:

```sh
cargo build --release --locked -p shell -p cli
python3 scripts/tests/ui-refresh-smoke.py target/release/tundra-shell --samples 50 --output /tmp/tundra-ui-timings.json
```

The report includes the executable path, size and modification time, platform, Python version, sample count, and P95/max timing for a raw PTY search keystroke to its matching UTF-8 output. Snapshot verification happens after timing. The acceptance budgets at 120 × 36 are input-to-output P95 ≤ 50ms and draw P95 ≤ 16.7ms. The PTY echo series includes host and terminal scheduling; it does not isolate application input-to-render time or internal draw duration, and the JSON leaves draw P95 unset. Certify the draw budget only with actual draw instrumentation, recording the method and build. Report the observed echo metric under its own name and state any unmeasured budgets.

Scheduler unit tests cover completed transitions, reduced motion, idle deadlines, and skipped frames after slow output. PTY silence alone does not prove that the runtime has no idle animation timer. Weather/rainbow retain 30 FPS; active UI transitions use 60 FPS, and one-character busy indicators advance every 80ms.

## Real Vim regression tests

Ordinary workspace tests skip ignored cases that require a complete Vim. After building the engine, run them explicitly:

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored --nocapture
python3 scripts/tests/editor-vim-smoke.py target/debug/tundra-shell target/debug/vim/vim
```

Runtime tests check buffer synchronization, undo, encoding, save failures, `:wq`, forced discard, and read-only protection. Shell smoke tests check the native display, Visual mode, registers, macros, multiple buffers, resizing, F8 synchronization, and terminal restoration.

On Windows, use `python scripts/build-vim.py --output target/debug/vim` and set the following in PowerShell:

```powershell
$env:TUNDRA_TEST_VIM = (Resolve-Path target/debug/vim/vim.exe).Path
```

`editor-vim-smoke.py` uses Unix PTYs; Windows ConPTY regressions run through the corresponding Rust tests and CI. `TUNDRA_TEST_VIM` is for Rust regressions, `TUNDRA_VIM` overrides Shell's Vim path, and `TUNDRA_VIM_CLEAN=1` isolates user configuration.

## Child terminals, scripts, and Launcher

Real PTY checks for external TUIs:

```sh
python3 scripts/tests/embedded-tui-smoke.py target/debug/tundra-shell --nvim /absolute/path/to/nvim --yazi /absolute/path/to/yazi
```

This script covers startup through Command Line and Launcher, input, paste, mouse, resizing, exit, and terminal restoration. Image protocols use protocol fixtures; native images and physical touchscreens require acceptance checks on the corresponding devices.

Linux script workspace:

```sh
cargo test --locked -p app --test script
cargo test --locked -p script-runtime
cargo test --locked -p ui --test script
cargo test --locked -p shell --lib controller::script::tests
python3 scripts/tests/editor-script-smoke.py target/debug/tundra-shell
```

Add `--skip-vim` to the smoke script when only script execution and debugging need checking. For multi-document switching between Vim and scripts, also run:

```sh
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p shell --lib vim_host::script_tests -- --ignored
```

Launcher organize mode:

```sh
cargo test --locked -p shell --lib controller::launcher::settings::tests
cargo test --locked -p app --test launcher
python3 scripts/tests/launcher-organize-smoke.py target/debug/tundra-shell
```

## Test organization

```text
crates/<crate>/src/                 Implementation, test module declarations, and necessary test hooks
crates/<crate>/tests/unit/          Unit tests, fixtures, and helpers for private implementation
crates/<crate>/tests/*.rs           Integration tests through public APIs
crates/<crate>/tests/support/       Shared integration-test helpers
scripts/tests/                     Packaging, authorization, PTY, and system-service tests
```

Implementation modules include unit tests with `#[cfg(test)]` and `#[path = ".../tests/unit/..."] mod tests;`, preserving private-module scope. Do not give `tests/unit/` a separate `main.rs`, to avoid duplicate discovery by Cargo.

New tests should prioritize core workflows, data safety, authorization boundaries, error recovery, and user-visible regressions. Shared controls test common states and colors; page tests check hit detection, modal interception, password masking, wide characters, and resizing. When deleting cases, also remove unused fixtures and test entry points.

See [large-file editor support](editor-scalability.md) for reproduction steps and measurement definitions. Historical acceptance results remain in `docs/*validation*.md` and the corresponding specialized records.
