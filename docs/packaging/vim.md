# 随包 Vim

Vim 模式使用完整的真实 Vim 进程。便携包将引擎放在 Shell 同目录：

```text
tundra-shell[.exe]
tundra-cli[.exe]
vim/
  vim[.exe]
  runtime/
  LICENSE
  UPSTREAM.json
```

固定版本为 **Vim 9.2.1225**。上游提交和源码 SHA-256 记录在 [`packaging/vim.json`](../../packaging/vim.json)，构建脚本使用未修改的上游源码，并保留完整 runtime 和 Vim 许可证。

## 引擎获取

编辑器依次查找便携包的 `vim/` 和当前用户数据目录的 `vim/`。两处都没有引擎时，安装弹窗从本项目 `vim-v<版本>` Release 下载对应平台的 ZIP，显示下载、校验和解压进度，并支持取消。安装成功后直接进入 Vim。

`TUNDRA_VIM` 可指定其他完整 Vim 可执行文件，`TUNDRA_VIM_CLEAN=1` 可在测试中跳过个人配置和插件。

## 本地构建

普通 Cargo 构建不处理 Vim。开发时显式构建：

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
```

发布模式使用 `--output target/release/vim`。设置 `CARGO_TARGET_DIR` 或 Cargo `--target` 时，将输出放在对应 Shell 可执行文件旁的 `vim/`。下载缓存位于 Cargo target 目录下的 `vim-build/`。

构建需要 Python 3.12 或更新版本，以及：

| 平台 | 构建工具 |
| --- | --- |
| Linux | C 编译器、make、ncurses 开发库；Debian/Ubuntu 使用 `build-essential libncurses-dev`，Fedora 使用 `gcc make ncurses-devel`，Arch 使用 `base-devel ncurses` |
| macOS | Xcode Command Line Tools、Homebrew 的 `cairo` 与 `pixman` |
| Windows x64 | Visual Studio C++ Build Tools 和 Windows SDK；脚本通过开发者命令行或 `vswhere` 查找 MSVC/NMake |

Windows 可使用 `python` 运行同一脚本。macOS Universal 包分别在原生 ARM 和 Intel runner 构建，再用 `lipo` 合并；本地构建只编译当前架构。

Windows 包包含固定版本的 `libiconv-2.dll`、对应许可证和源码归档。版本、下载地址与 SHA-256 位于 `packaging/vim.json` 的 `windows_iconv`。

## 离线与校验

已有固定源码归档时可离线构建：

```sh
python3 scripts/build-vim.py --source-archive /path/to/vim.tar.gz --output target/debug/vim
python3 scripts/build-vim.py --verify-only --output target/debug/vim
```

Windows 离线构建还需通过 `--iconv-archive` 和 `--iconv-source-archive` 提供固定归档，或提前放入 `target/vim-build/` 缓存。

校验包括版本、`+eval`、`+channel`、`+autocmd`、`+timers`、`+multi_byte`、runtime、许可证和上游元数据。已有构建可直接校验并打包：

```sh
python scripts/build-vim.py --verify-only --output target/debug/vim --package target/vim-engine.zip
```

构建脚本回归：

```sh
python -B -m unittest discover -s scripts/tests -p test_build_vim.py
```

## 发布

`.github/workflows/vim-engine-release.yml` 为 Windows x64、Linux x86_64 和 macOS Universal 构建独立引擎包。工作流可手动触发，也可推送与固定版本一致的 `vim-v<版本>` 标签。每个 ZIP 包含引擎、runtime、许可证、上游元数据和 SHA-256 文件。

应用发布工作流在对应平台显式构建 Vim。Linux 本地打包：

```sh
python3 scripts/build-vim.py --output target/release/vim
bash scripts/package-linux.sh
```

`TUNDRAUX3_VIM_DIR` 可指向已构建的同版本引擎目录。升级 Vim 时，同时更新固定提交和源码哈希，并重新运行构建校验与真实 Vim 回归。

Linux 界面验收：

```sh
python3 scripts/tests/editor-vim-ui.py /path/to/tundra-shell
```

添加 `--missing-engine` 可检查安装弹窗、进度和取消流程；`scripts/tests/editor-vim-smoke.py` 覆盖更多编辑与保存工作流。
