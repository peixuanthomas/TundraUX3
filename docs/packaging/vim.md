# 随包 Vim

编辑器的 Vim 模式使用真实的 Vim 进程。便携包在 Shell 同目录保留 `vim/`：

```text
tundra-shell[.exe]
tundra-cli[.exe]
vim/
  vim[.exe]
  runtime/
  LICENSE
  UPSTREAM.json
```

当前固定 **Vim 9.2.1225**，上游提交和源码 SHA-256 写在 [`packaging/vim.json`](../../packaging/vim.json)。脚本只构建这份未修改的源码，保留完整 runtime 和原始 Vim 许可证；不依赖系统 `vim`、`vi` 或功能裁剪的 `vim-tiny`。缺少引擎时尝试下载对应预编译包；损坏或安装失败时显示错误，不回退到旧的按键模拟。

## 本地构建

用户开启 Vim 模式时，编辑器先查找随包引擎，再查找当前用户数据目录的 `vim/`。两处均缺少引擎时，安装弹窗从本项目 `vim-v<版本>` Release 下载当前平台的 ZIP，显示下载百分比及解压、校验阶段，并支持取消。成功后自动进入 Vim；失败或取消不改动文档。此流程不需要 Python 或编译器；`TUNDRA_VIM` 指定的路径无效时仍直接报错。

`.github/workflows/vim-engine-release.yml` 使用下述构建脚本，为 Windows x64、Linux x86_64（Ubuntu 24.04 构建）和 macOS Universal 发布同版本的独立引擎包。触发方式为手动运行或推送与固定版本一致的 `vim-v<版本>` 标签。各 ZIP 包含 `vim/`、runtime、LICENSE 和 UPSTREAM.json，并附带 `.zip.sha256` 校验文件；安装时检查 SHA-256、下载大小及固定上游清单。引擎 Release 不标为最新应用版本。尚未发布对应包时，编辑器明确提示尚未发布。

已有构建可直接校验并打包：`python scripts/build-vim.py --verify-only --output target/debug/vim --package target/vim-engine.zip`。

Linux 实机验收使用本机 WSL：`python3 scripts/tests/editor-vim-ui.py /path/to/tundra-shell`，所有操作经 `tundra-cli ui` 发给真实程序，配置及文档均使用临时目录。增加 `--missing-engine` 会从不带引擎的隔离副本启动，验证安装弹窗、进度条和鼠标取消；对应 Release 未发布时还会验证失败提示。原有 `scripts/tests/editor-vim-smoke.py` 继续检查更多真实 Vim 工作流。

普通 `cargo build` 不下载或编译 Vim。开发者也可以直接运行脚本准备随包引擎：

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
```

发布模式使用 `--output target/release/vim`；设置 `CARGO_TARGET_DIR` 或使用 Cargo 的 `--target` 时，将输出放到相应 Shell 可执行文件旁的 `vim/`。下载文件缓存在 Cargo target 目录下的 `vim-build/`，不会安装到系统或写入用户 Vim 配置。

构建需要 Python 3.12 或更新版本，以及下列本机构建工具：

| 平台 | 需要的工具 |
| --- | --- |
| Linux | C 编译器、make、ncurses 开发库；Debian/Ubuntu 为 `build-essential libncurses-dev`，Fedora 为 `gcc make ncurses-devel`，Arch 为 `base-devel ncurses` |
| macOS | Xcode Command Line Tools；通用发布包还使用 `lipo` |
| Windows x64 | Visual Studio C++ Build Tools 与 Windows SDK；脚本通过开发者命令行或 `vswhere` 查找 MSVC/NMake，控制台版本静态链接 C runtime |

Windows 可用 `python` 运行同一个脚本。macOS 发布包用 `--macos-universal` 同时构建 arm64 和 x86_64，并用 `lipo` 检查两个架构。常规本地构建只编译当前架构。

Windows 启用动态 iconv，随包提供固定的 `libiconv-2.dll`，用于 UTF-16 等编码转换。DLL 来自 [Vim 官方构建采用的来源](https://github.com/vim/vim-win32-installer/blob/master/.github/workflows/build-vim.yml)，具体版本、下载地址和 SHA-256 保存在 `packaging/vim.json` 的 `windows_iconv` 中。只提取这个 DLL，同时保留 `licenses/libiconv/` 下的 LGPL 及构建包许可证，以及 `sources/libiconv.tar.gz` 中的对应源码。校验脚本实际执行一次 UTF-16LE 转换，避免只检查功能名称却漏掉运行时 DLL。

如已有固定源码归档，可离线运行；仍会核对相同的 SHA-256：

```sh
python3 scripts/build-vim.py --source-archive /path/to/vim.tar.gz --output target/debug/vim
python3 scripts/build-vim.py --verify-only --output target/debug/vim
```

Windows 离线构建还需提供已下载的 `--iconv-archive` 和 `--iconv-source-archive`；也可以提前把这两份固定归档放入 `target/vim-build/` 缓存。脚本同样检查其 SHA-256。

校验包括固定版本、`+eval`、`+channel`、`+autocmd`、`+timers`、`+multi_byte`，以及 runtime、许可证和上游元数据。构建启用 Huge 功能集和终端支持，禁用 GUI、X11、可选外部语言解释器及其他可选动态库，避免便携包依赖未随包提供的组件。

Windows Vim 将多字节支持显示为 `+multi_byte_ime` 或 `+multi_byte_ime/dyn`，脚本同样接受这两个上游名称。校验回归：`python -B -m unittest discover -s scripts/tests -p test_build_vim.py`。

## 发布

最新发布工作流在 Windows、macOS、Linux 的打包步骤显式构建 Vim。Linux 本地打包先准备 release 引擎，再运行原打包脚本：

```sh
python3 scripts/build-vim.py --output target/release/vim
bash scripts/package-linux.sh
```

`scripts/package-linux.sh` 校验完整引擎后把 `vim/` 放入归档；也可通过 `TUNDRAUX3_VIM_DIR` 指向已经构建的同版本引擎目录。缺少引擎或校验失败时停止打包。升级上游版本需要同时更新固定提交、源码哈希，重跑脚本和编辑器的真实 Vim 验收；不能只改变版本文字。
