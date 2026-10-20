# 测试指南

所有命令从仓库根目录运行。通过的检查应对应当前代码，汇报时区分自动检查和真实平台验收。

新增 Linux 管理功能的真实工具、PTY、触屏事件与尚未实机验证的范围见 [2026-10-16 验收记录](linux-administration-validation.md)。

本地推荐检查：

```console
cargo fmt --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --locked -p shell -p cli -p weathr
```

本地回环 HTTP 测试应直接连接测试服务器。受系统代理影响时，可仅为测试进程设置 `NO_PROXY=127.0.0.1,localhost,::1` 和小写 `no_proxy`，无需修改系统代理。资源紧张或排查并发时可使用 `cargo test --workspace --locked -- --test-threads=1`；串行仍执行全部用例，交付时应注明运行方式及默认并发结果。

最低支持版本为 Rust 1.95；修改工具链或依赖后，还需验证最低版本：

```console
rustup toolchain install 1.95.0 --profile minimal
cargo +1.95.0 check --workspace --all-targets --all-features --locked
cargo +1.95.0 test --workspace --locked
```

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
| 最低 Rust 版本（Linux / Windows / macOS） | 从根 `Cargo.toml` 读取最低版本，检查 workspace 全部 targets 和 features |
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
# 加测屏幕键盘粘贴失败、崩溃详情、报告保存和 Q 退出。
TUNDRA_PTY_CHECK_CRASH=1 python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
```

PTY smoke 使用隔离的 XDG 目录和 140 × 40 的真实 PTY 进入 Shell。Linux 从语言直接进入外观设置，时间和时区跟随系统。它打开已有 Appearance 颜色输入框，默认注入 64 个 SGR 全移动鼠标事件（可通过 `TUNDRA_PTY_MOUSE_EVENT_COUNT` 调整），随后发送单字符哨兵，在 250 毫秒门限内验证鼠标洪峰后的普通字符输入响应；Ratatui 增量绘制不保证重发完整多字符字符串。测试再取消临时颜色、完成当前 Linux 用户的 Appearance 设置并进入 Home，从 Launcher 打开 Command Line，验证初始绝对路径以及切换到含空格和中文的目录后的提示符。最后发送 `SIGTERM`，检查终端属性、raw mode、鼠标捕获、备用屏幕和光标均得到恢复。

首次外观设置用 F10 完成。额外的文件工作流在 116 × 41 下编辑临时文本并打开未保存确认，缩到 80 × 24 验证进程仍在、隐藏弹窗不接收输入；恢复尺寸后取消确认并保存，检查文件内容，再用 F2、Ctrl+A 替换完整名称并核对落盘结果。

页面标题和首次外观设置中的颜色弹窗按实际终端画面判断。键盘操作立即切换时，标题可能分成多次增量绘制，不能要求输出包含一段连续的完整标题。

文件工作流也通过真实 F8、gg、i、Esc 和 u 字节验证 Vim 开关、输入与撤销，再关闭 Vim 完成保存；测试始终使用隔离配置和临时文档。

同一临时文件从属性进入权限表单，验证真实空格字节和 Enter 切换权限格、Tab 到递归复选框后用空格切换，取消放弃时保留草稿。最后放弃退出，核对文件权限、归属和元数据变更时间均未改变；不提交权限变更或递归操作。

PID 1 不是 systemd 时，额外从设置 F8 进入操作系统设置，检查环境限制提示及 F4 中的具体原因、能力要求，并逐层返回；这不代表正常 systemd/系统 D-Bus 环境中的修改流程已经验收。

软件包搜索页切换完成后直接输入查询，验证自动聚焦；缩放后的鼠标操作等待实际按钮重绘后再获取命中位置。

设置 `TUNDRA_PTY_CHECK_CRASH=1` 时，测试子进程不连接桌面剪贴板，连续点击三次粘贴后检查提示并执行一条命令。最后主动触发一次崩溃，检查简短详情与保存的报告，再按 Q 退出并检查终端恢复；不会清空用户剪贴板。

root 启动确认在可丢弃的 Linux 测试环境验证；下列脚本使用临时 XDG 目录，不修改系统配置。
第一项覆盖 Shell/CLI 的 `y` 确认、其他按键取消、终端模式恢复、管道拒绝、普通用户免确认和 set-ID 拒绝；
第二项确认警告后继续执行上述完整界面 smoke。

```console
sudo python3 scripts/tests/linux-root-startup.py target/debug/tundra-shell target/debug/tundra-cli
sudo python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
```

固定测试重点包括输入阶段/修饰键/paste/focus/双击/拖拽/滚动及高频鼠标事件合并、模态命中和焦点恢复、通知、Editor grapheme 与异步保存、Explorer/Launcher 后台操作、登录锁定与授权、时钟和 DST、storage schema/迁移/原子写入/损坏恢复，以及 watchdog 的 panic 边界、任务回收和事故报告。

Shell 输入批处理回归位于 `tests/unit/input/terminal_batch.rs`；输入方式和快捷键重复保护位于 `tests/unit/session/controller/input_routing/interaction_tests.rs`。合成器的 `exit_dialog_round_trip_with_keyboard_mouse_or_touch_alone` 从 Crossterm 事件出发，分别用键盘、带移动的鼠标和无悬停点按完成打开/取消退出弹窗，并检查页面、焦点和捕获状态恢复。它验证终端事件模型中的三条独立路径；全部页面的设备验收仍按 [UI 要求](UI-requirements.md)执行。

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

## 页面实例与任务关闭

页面管理测试覆盖同类页面的独立数据、覆盖后返回、关闭后重开、迟到结果、旧登录会话结果、后台通知与前台焦点。编辑器测试额外检查独立恢复文件、进程文件锁、逐份认领草稿，以及保存或恢复记录清理失败时保留数据。

Shell 的 `page_lifecycle`、`auto_admin` 和 `update_cancellation_tests` 覆盖三种关闭选择、等待实际收尾、AA 从关闭确认返回、后台输入自动显示，以及更新准备成功与取消同时发生。更新取消还测试取消记录写入失败、修复后重试和撤回自动重启请求。

`command_line_runtime::tests` 在 Windows 使用真实 ConPTY，在 Linux 使用真实 PTY，检查两个页面的终端隔离、被覆盖时读取输出、Esc/Ctrl+Shift+X 字节转发、确认关闭以及子进程树退出。它们属于自动交互验证；桌面终端、触屏和真实系统授权仍需各自验收。

## 编辑器大文件回归与基准

运行方法、测量口径和 1/10/100 MiB 原始结果见 [Source 编辑器的大文件计算](editor-scalability.md)。
