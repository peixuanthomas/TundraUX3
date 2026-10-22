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

当前固定 **Vim 9.2.1225**，上游提交和源码 SHA-256 写在 [`packaging/vim.json`](../../packaging/vim.json)。脚本只构建这份未修改的源码，保留完整 runtime 和原始 Vim 许可证；不依赖系统 `vim`、`vi` 或功能裁剪的 `vim-tiny`。缺少或损坏的引擎应显示错误，不回退到旧的按键模拟。

## 本地构建

普通 `cargo build` 不下载或编译 Vim。启用 Vim 模式前，显式运行一次：

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

如已有固定源码归档，可离线运行；仍会核对相同的 SHA-256：

```sh
python3 scripts/build-vim.py --source-archive /path/to/vim.tar.gz --output target/debug/vim
python3 scripts/build-vim.py --verify-only --output target/debug/vim
```

校验包括固定版本、`+eval`、`+channel`、`+autocmd`、`+timers`、`+multi_byte`，以及 runtime、许可证和上游元数据。构建启用 Huge 功能集和终端支持，禁用 GUI、X11、可选外部语言解释器及可选动态库，避免便携包依赖未随包提供的组件。

## 发布

最新发布工作流在 Windows、macOS、Linux 的打包步骤显式构建 Vim。Linux 本地打包先准备 release 引擎，再运行原打包脚本：

```sh
python3 scripts/build-vim.py --output target/release/vim
bash scripts/package-linux.sh
```

`scripts/package-linux.sh` 校验完整引擎后把 `vim/` 放入归档；也可通过 `TUNDRAUX3_VIM_DIR` 指向已经构建的同版本引擎目录。缺少引擎或校验失败时停止打包。升级上游版本需要同时更新固定提交、源码哈希，重跑脚本和编辑器的真实 Vim 验收；不能只改变版本文字。
