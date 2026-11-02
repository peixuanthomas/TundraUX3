# Bundled Vim

Vim mode uses a complete, real Vim process. Portable builds place the engine beside Shell:

```text
tundra-shell[.exe]
tundra-cli[.exe]
vim/
  vim[.exe]
  runtime/
  LICENSE
  UPSTREAM.json
```

The pinned version is **Vim 9.2.1225**. The upstream commit and source SHA-256 are recorded in [`packaging/vim.json`](../../packaging/vim.json). The build script uses unmodified upstream source and preserves the complete runtime and Vim license.

## Obtaining the engine

The editor checks the portable build's `vim/` first, then `vim/` in the current user's data directory. If neither contains an engine, the installation dialog downloads the platform's ZIP from this project's `vim-v<version>` Release, shows download, verification, and extraction progress, and supports cancellation. Successful installation enters Vim directly.

`TUNDRA_VIM` can select another complete Vim executable; `TUNDRA_VIM_CLEAN=1` skips personal configuration and plugins during tests.

## Local builds

Ordinary Cargo builds do not handle Vim. Build it explicitly during development:

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
```

For release mode, use `--output target/release/vim`. When setting `CARGO_TARGET_DIR` or Cargo's `--target`, put the output in `vim/` beside the corresponding Shell executable. Downloads are cached in `vim-build/` under the Cargo target directory.

Building requires Python 3.12 or newer, plus:

| Platform | Build tools |
| --- | --- |
| Linux | C compiler, make, and ncurses development libraries; `build-essential libncurses-dev` on Debian/Ubuntu, `gcc make ncurses-devel` on Fedora, `base-devel ncurses` on Arch |
| macOS | Xcode Command Line Tools, Homebrew `cairo` and `pixman` |
| Windows x64 | Visual Studio C++ Build Tools and Windows SDK; the script locates MSVC/NMake through a developer command prompt or `vswhere` |

Windows can run the same script with `python`. macOS Universal packages are built separately on native ARM and Intel runners, then combined with `lipo`; local builds compile only for the current architecture.

Windows packages include the pinned `libiconv-2.dll`, its license, and source archive. The version, download URLs, and SHA-256 are in `windows_iconv` in `packaging/vim.json`.

## Offline builds and verification

Build offline when the pinned source archive is already available:

```sh
python3 scripts/build-vim.py --source-archive /path/to/vim.tar.gz --output target/debug/vim
python3 scripts/build-vim.py --verify-only --output target/debug/vim
```

Offline Windows builds also require the pinned archives through `--iconv-archive` and `--iconv-source-archive`, or placed in the `target/vim-build/` cache beforehand.

Verification covers the version, `+eval`, `+channel`, `+autocmd`, `+timers`, `+multi_byte`, runtime, license, and upstream metadata. Existing builds can be verified and packaged directly:

```sh
python scripts/build-vim.py --verify-only --output target/debug/vim --package target/vim-engine.zip
```

Build-script regressions:

```sh
python -B -m unittest discover -s scripts/tests -p test_build_vim.py
```

## Releases

`.github/workflows/vim-engine-release.yml` builds standalone engine packages for Windows x64, Linux x86_64, and macOS Universal. Trigger it manually or by pushing a `vim-v<version>` tag matching the pinned version. Each ZIP contains the engine, runtime, license, upstream metadata, and a SHA-256 file.

Application release workflows explicitly build Vim on the corresponding platform. Local Linux packaging:

```sh
python3 scripts/build-vim.py --output target/release/vim
bash scripts/package-linux.sh
```

`TUNDRAUX3_VIM_DIR` can point to an already-built engine directory of the same version. When upgrading Vim, update both the pinned commit and source hash, then rerun build verification and real Vim regressions.

Linux UI acceptance:

```sh
python3 scripts/tests/editor-vim-ui.py /path/to/tundra-shell
```

Add `--missing-engine` to check the installation dialog, progress, and cancellation; `scripts/tests/editor-vim-smoke.py` covers more editing and saving workflows.
