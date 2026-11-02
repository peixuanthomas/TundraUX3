# 测试指南

所有命令从仓库根目录运行。自动测试覆盖核心工作流、数据安全和权限边界；真实终端、触屏、系统授权和设备写操作在对应平台单独验收。

## 常规检查

```sh
cargo fmt --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --locked -p shell -p cli -p weathr
python3 scripts/check-localization.py
bash scripts/check-weathr-dependency-boundary.sh
```

固定测试不发起 HTTP 请求，也不启动 HTTP 测试服务器。依赖已缓存时，可添加 `--offline`；排查并发问题时可添加 `-- --test-threads=1`。

资源有限时可使用：

```sh
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo test --workspace --locked
```

## 定向测试

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

最低支持 Rust 1.95。修改工具链或依赖后，检查最低版本：

```sh
rustup toolchain install 1.95.0 --profile minimal
cargo +1.95.0 check --workspace --all-targets --all-features --locked
cargo +1.95.0 test --workspace --locked
```

CI 配置见 [cross-platform.yml](../.github/workflows/cross-platform.yml)、[linux.yml](../.github/workflows/linux.yml) 和对应发布工作流。Linux 相关检查在 WSL 或原生 Linux 执行，Windows 和 macOS 检查在各自平台执行。

## Linux PTY 与系统操作

先构建 Shell、CLI 和 Vim，再运行真实终端检查：

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
TUNDRA_PTY_CHECK_CRASH=1 python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
python3 scripts/tests/shell-control.py target/debug/tundra-shell
```

这些脚本使用隔离的 XDG 目录和真实 PTY，检查首次设置、文件工作流、鼠标与键盘输入、命令行、外部 UI 控制、崩溃报告和终端恢复。`TUNDRA_PTY_MOUSE_EVENT_COUNT` 可调整鼠标事件洪峰测试的事件数量。

原生回收站往返测试只创建临时项：

```sh
cargo test --locked -p platform --test native_trash_smoke -- --ignored --nocapture
```

服务和计划任务的隔离测试使用私有 systemd、D-Bus、挂载、PID、网络和 cgroup：

```sh
scripts/tests/isolated-systemd.sh /absolute/path/to/platform-test-binary 原用户UID selection
```

root 启动检查仅放在可丢弃的 Linux 测试环境运行：

```sh
sudo python3 scripts/tests/linux-root-startup.py target/debug/tundra-shell target/debug/tundra-cli
sudo python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
```

软件包查询使用固定夹具和只读 smoke。安装、卸载与系统升级不在开发主机上执行破坏性测试；真实写操作放在隔离环境验收。

## 真实 Vim 回归

普通 workspace 测试会跳过需要完整 Vim 的 ignored 用例。构建引擎后显式运行：

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored --nocapture
python3 scripts/tests/editor-vim-smoke.py target/debug/tundra-shell target/debug/vim/vim
```

Runtime 测试检查缓冲同步、撤销、编码、保存失败、`:wq`、强制放弃和只读保护。Shell smoke 检查原生画面、Visual 模式、寄存器、宏、多缓冲、缩放、F8 同步和终端恢复。

Windows 使用 `python scripts/build-vim.py --output target/debug/vim`，并在 PowerShell 中设置：

```powershell
$env:TUNDRA_TEST_VIM = (Resolve-Path target/debug/vim/vim.exe).Path
```

`editor-vim-smoke.py` 使用 Unix PTY；Windows 的 ConPTY 回归由对应 Rust 测试和 CI 执行。`TUNDRA_TEST_VIM` 用于 Rust 回归，`TUNDRA_VIM` 用于覆盖 Shell 的 Vim 路径，`TUNDRA_VIM_CLEAN=1` 用于隔离用户配置。

## 子终端、脚本与启动器

运行外部 TUI 的真实 PTY 检查：

```sh
python3 scripts/tests/embedded-tui-smoke.py target/debug/tundra-shell --nvim /absolute/path/to/nvim --yazi /absolute/path/to/yazi
```

该脚本覆盖命令行和 Launcher 启动、输入、粘贴、鼠标、缩放、退出及终端恢复。图片协议使用协议夹具测试，原生图片和物理触屏在对应设备上验收。

Linux 脚本工作区：

```sh
cargo test --locked -p app --test script
cargo test --locked -p script-runtime
cargo test --locked -p ui --test script
cargo test --locked -p shell --lib controller::script::tests
python3 scripts/tests/editor-script-smoke.py target/debug/tundra-shell
```

仅需检查脚本运行和调试时可给 smoke 脚本添加 `--skip-vim`。多文档 Vim 与脚本切换另运行：

```sh
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p shell --lib vim_host::script_tests -- --ignored
```

启动器整理模式：

```sh
cargo test --locked -p shell --lib controller::launcher::settings::tests
cargo test --locked -p app --test launcher
python3 scripts/tests/launcher-organize-smoke.py target/debug/tundra-shell
```

## 测试组织

```text
crates/<crate>/src/                 业务实现、测试模块声明和必要的测试钩子
crates/<crate>/tests/unit/          私有实现的单元测试、夹具和辅助模块
crates/<crate>/tests/*.rs           通过公开 API 验证的集成测试
crates/<crate>/tests/support/       集成测试共享辅助代码
scripts/tests/                     打包、授权、PTY 和系统服务测试
```

业务模块通过 `#[cfg(test)]` 和 `#[path = ".../tests/unit/..."] mod tests;` 引入单元测试，保留私有模块作用域。`tests/unit/` 不设置独立 `main.rs`，避免 Cargo 重复发现。

新增测试优先覆盖核心工作流、数据安全、授权边界、错误恢复和用户可见回归。共享控件验证通用状态与颜色，页面测试验证命中、模态拦截、密码遮罩、宽字符和缩放；删除用例时同步清理无用夹具与测试入口。

编辑器大文件的复现方法和测量口径见[编辑器大文件支持](editor-scalability.md)。历史验收结果保留在 `docs/*validation*.md` 和对应专项记录中。
