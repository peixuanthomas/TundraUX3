# 测试指南

所有命令从仓库根目录运行。通过的检查应对应当前代码，汇报时区分自动检查和真实平台验收。

本地推荐检查：

```console
cargo fmt --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --locked -p shell -p cli -p weathr
```

本地回环 HTTP 测试应直接连接测试服务器。受系统代理影响时，可仅为测试进程设置 `NO_PROXY=127.0.0.1,localhost,::1` 和小写 `no_proxy`，无需修改系统代理。资源紧张或排查并发时可使用 `cargo test --workspace --locked -- --test-threads=1`；串行仍执行全部用例，交付时应注明运行方式及默认并发结果。

定向测试：

```console
cargo test --locked -p app
cargo test --locked -p ui
cargo test --locked -p shell
cargo test --locked -p storage
cargo test --locked -p identity
cargo test --locked -p platform
cargo test --locked -p system-services --no-default-features
cargo test --locked -p system-services --features runtime
bash scripts/check-weathr-dependency-boundary.sh
```

CI 配置以 [cross-platform.yml](../.github/workflows/cross-platform.yml)、[linux.yml](../.github/workflows/linux.yml)及对应发布工作流为准。

| 常规工作流 | 当前检查 |
| --- | --- |
| Windows / macOS | workspace 构建；文件安全、语言资源、日志和系统指标定向测试 |
| Ubuntu | workspace 构建；身份、平台、语言资源、日志、系统指标与本地软件包构建测试 |
| Fedora | 普通用户 workspace 构建；身份、平台、日志与系统指标定向测试 |
| Arch | 普通用户软件包参数/查询测试、显式只读 pacman smoke 和本地软件包构建测试 |

发布工作流另有自己的打包和测试步骤，不能把常规 CI 表视为每次都运行了完整 workspace 测试。

软件包查询和固定操作测试包含在 Linux workspace 测试中。Arch CI 额外以普通用户执行 `cargo test --locked -p platform management::packages --lib` 和 `cargo test --locked -p platform management::packages::pacman::tests::live_pacman_installed_search_updates_and_details --lib -- --ignored`，后者只读取已有数据库。隔离的合成数据库查询只执行只读命令，并核对数据库内容未变。缓存夹具与只读 smoke 不等于实际 Arch 写事务验收；安装、完整系统升级和卸载不得在开发主机进行破坏性测试。

Linux 的自动测试不触碰用户真实 Trash。发布候选可在 GNOME/KDE 普通用户会话运行原生往返 smoke；它只创建临时项，并在成功后恢复和清理：

```console
cargo test --locked -p platform --test native_trash_smoke -- --ignored --nocapture
cargo build --locked -p shell -p cli
python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
```

PTY smoke 使用隔离的 XDG 目录和 140 × 40 的真实 PTY 进入 Shell。它依次经过语言、时区页面（跳过创建用户），打开已有 Appearance 颜色输入框，默认注入 64 个 SGR 全移动鼠标事件（可通过 `TUNDRA_PTY_MOUSE_EVENT_COUNT` 调整），随后发送单字符哨兵，在 250 毫秒门限内验证鼠标洪峰后的普通字符输入响应；Ratatui 增量绘制不保证重发完整多字符字符串。测试再取消临时颜色、完成当前 Linux 用户的 Appearance 设置并进入 Home，从 Launcher 打开 Command Line，验证初始绝对路径以及切换到含空格和中文的目录后的提示符。最后发送 `SIGTERM`，检查终端属性、raw mode、鼠标捕获、备用屏幕和光标均得到恢复。

root 启动确认在可丢弃的 Linux 测试环境验证；下列脚本使用临时 XDG 目录，不修改系统配置。
第一项覆盖 Shell/CLI 的 `y` 确认、其他按键取消、终端模式恢复、管道拒绝、普通用户免确认和 set-ID 拒绝；
第二项确认警告后继续执行上述完整界面 smoke。

```console
sudo python3 scripts/tests/linux-root-startup.py target/debug/tundra-shell target/debug/tundra-cli
sudo python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
```

固定测试重点包括输入阶段/修饰键/paste/focus/双击/拖拽/滚动及高频鼠标事件合并、模态命中和焦点恢复、通知、Editor grapheme 与异步保存、Explorer/Launcher 后台操作、登录锁定与授权、时钟和 DST、storage schema/迁移/原子写入/损坏恢复，以及 watchdog 的 panic 边界、任务回收和事故报告。

预览动画、可行性 POC、无断言在线探针、平凡 getter/cache，以及与上层工作流重复的逐字符或逐像素断言不进入固定 workspace 测试；只有对应明确用户可见回归时才应加入。

业务实现与测试文件统一按以下目录分离：

```text
crates/<crate>/src/                 业务实现、测试模块声明和必要的测试钩子
crates/<crate>/tests/unit/          私有实现的单元测试、夹具和测试辅助模块
crates/<crate>/tests/*.rs           通过公开 API 验证的集成测试
crates/<crate>/tests/support/       集成测试共享辅助代码（按需设置）
scripts/tests/                     打包、授权 PTY 和系统服务测试夹具
```

单元测试由原业务模块通过 `#[cfg(test)]` 与 `#[path = ".../tests/unit/..."] mod tests;` 引入，仍位于原来的 Rust 模块作用域，可访问私有实现，无需为了测试扩大生产 API。`tests/unit/` 不设置独立的 `main.rs` 测试入口，避免 Cargo 重复发现和执行；`cargo test --workspace --locked` 同时运行这些单元测试及集成测试。二进制测试保留原来的平台条件，共享构建脚本仍放在原位置。不要在 `src/` 内重新添加测试函数或整段测试夹具。

测试优先保留核心工作流、数据安全、授权边界、错误恢复及明确的用户可见回归。新增测试前先检查已有下层测试和上层工作流；相同操作的不同输入优先用表格循环检查。界面集成测试共用的输出读取函数放在 `crates/ui/tests/support`。
