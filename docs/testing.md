# 测试指南

所有命令从仓库根目录运行。通过的检查应对应当前代码，汇报时区分自动检查和真实平台验收。

新增 Linux 管理功能的真实工具、PTY、触屏事件与尚未实机验证的范围见 [2026-10-16 验收记录](linux-administration-validation.md)。

可见多选、全部内置文字输入、任务历史、脚本界面、定时维护和备份分页的本轮结果见 [2026-10-26 验收记录](operations-usability-validation.md)。设备点按与自动生成的点按事件分别记录。

脚本真实 PTY 检查使用 `python3 scripts/tests/editor-script-smoke.py target/debug/tundra-shell`，需要同目录 CLI 和配套 Vim。仅验核心运行与调试时可显式传 `--skip-vim`，该结果不包含真实 Vim 交接；默认仍要求完整引擎。

本地推荐检查：

```console
cargo fmt --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --locked -p shell -p cli -p weathr
```

固定测试不发起 HTTP 请求，也不启动本地 HTTP 模拟服务器，避免代理、连接失败或阻塞等待使测试卡住。更新和 Vim 安装保留离线校验、解压安全、取消前置检查与回退验证；网络管理保留配置解析和模拟后端测试，不创建真实监听端口。

软件包多选覆盖 Shift 鼠标事件、Shift+Enter、按键重复、取消全部、排序与刷新、混合选择的操作限制和 AA 完整包名。Linux 后端检查 APT/DNF/pacman 的多包命令；WinGet 检查逐包准确编号与版本；Homebrew 的分组与执行前检查需在 macOS 运行。构建 Shell 和 CLI 后，用 `python3 scripts/tests/shell-control.py target/debug/tundra-shell` 在真实 Linux PTY 中发送带 Shift 的 SGR 鼠标报告和 `CSI 13;2u`，读取实际已选项并检查 F1 帮助；不安装或卸载软件包。

服务、进程和计划任务使用同一组终端事件测试，并检查批量目标、操作限制、筛选清理、进程编号复用和 cron 条目替换。平台测试只对自己创建的临时子进程执行批量信号和优先级操作，验证后一个目标失效时前面的进程也不被修改；cron 用固定文本检查同来源多行的原版本校验、合并暂停与恢复。真实服务和计划任务的写入仍限于隔离 systemd 环境。

`isolated-systemd.sh /absolute/path/to/platform-test-binary 原用户UID selection` 复用私有挂载、PID、网络及 cgroup，并启动私有 D-Bus，验证两项服务的批量启停、重启、重载与启停用，两项 timer 的暂停、恢复、立即执行，以及同一 crontab 两条任务的合并保存、备份恢复和立即执行。`shell-control.py` 还按准确 PID 选择自己创建的两个临时进程，验证一次确认后全部暂停、继续；结束时清理子进程。

依赖下载完成后，可用 `cargo test --workspace --locked --offline` 避免 Cargo 访问网络。资源紧张或排查并发时可加 `-- --test-threads=1`；串行仍执行全部用例，交付时应注明运行方式及默认并发结果。

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
| Windows / macOS | workspace 与固定 Vim 构建；真实 Vim runtime、文件安全、语言资源、日志和系统指标定向测试 |
| Ubuntu | workspace 与固定 Vim 构建；真实 Vim runtime、身份、平台、语言资源、日志、系统指标与本地软件包构建测试 |
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

Vim 的真实进程、原生画面及 F8 同步流程由下文独立的 Vim 回归覆盖，测试使用隔离配置和临时文档。

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

共享控件集中验证按钮状态、颜色与基础交互；页面保留命中位置、模态拦截、密码遮罩、宽字符和缩放后可操作性，不重复验证固定文案、边框和装饰细节。删减测试时一并移除失去用途的夹具、辅助函数和测试专用入口，不使用 `ignore` 隐藏应当删除的用例。

## 真实 Vim 回归

普通 `cargo test --workspace` 会跳过需要完整 Vim 的 ignored 用例。先构建同目录引擎，再显式运行它们；需要真实 PTY 和本机回环连接，受限沙箱也须允许该回环连接：

```console
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored --nocapture
python3 scripts/tests/editor-vim-smoke.py target/debug/tundra-shell target/debug/vim/vim
```

Windows 可用 `python scripts/build-vim.py --output target/debug/vim`，并用 `$env:TUNDRA_TEST_VIM = (Resolve-Path target/debug/vim/vim.exe).Path` 设置绝对路径后运行同一个 Rust 集成测试。`editor-vim-smoke.py` 使用 Unix PTY，不能直接作为 Windows ConPTY 验收。构建依赖见[随包 Vim](packaging/vim.md)。

Runtime 集成测试隔离个人配置，检查导入不产生用户撤销记录、跨缓冲撤销、编码/换行、宿主确认前不写文件、保存失败、`:wq` 等待、强制放弃和只读保护。Shell smoke 使用独立 XDG 目录和临时文件，并设置 `TUNDRA_VIM_CLEAN=1`；它通过真实按键检查行末移动、次数和重复、text object、Visual 模式、插入会话撤销/重做、原生命令与错误显示、寄存器/宏、多缓冲、缩放、F8 未保存同步及 `:q`/`:q!`/`:wq`，退出后检查终端恢复。

`TUNDRA_VIM` 可覆盖 Shell 使用的完整 Vim 可执行路径；未设置时使用 Shell 同目录的随包引擎。`TUNDRA_VIM_CLEAN` 仅在需要隔离用户 Vim 配置/插件的测试中设置，正常运行不设置。`TUNDRA_TEST_VIM` 是 Rust 回归的可执行路径，不是 Shell 的配置项。

Linux 使用真实 PTY 验证；Windows/macOS 的构建和 runtime 回归已加入对应 CI，仍需在各自 runner 实跑。仅编译通过、系统 tiny Vim 或 Linux 检查不能代替这些运行时结果。

## 页面实例与任务关闭

任意 TUI 的协议夹具覆盖 Kitty 增强键盘、焦点、同步刷新、三种图片输入、像素边距与视口裁切；测试不依赖外部终端提供原生图片。外部程序退出码 75、76、失败后的回滚/重试/F12 返回，以及越界拖动释放另有真实 PTY 回归。

构建 Shell 和 CLI 后，可在隔离配置中运行实际 Neovim/Yazi smoke：

```console
python3 scripts/tests/embedded-tui-smoke.py target/debug/tundra-shell --nvim /absolute/path/to/nvim --yazi /absolute/path/to/yazi
```

该脚本检查命令行和 Launcher 启动、输入、粘贴、鼠标、缩放、退出及终端恢复。云 Linux PTY、WSL、原生图片终端与物理触屏分别记录结果；协议夹具或字符色块通过不代表真实设备图片显示已验收。当前渲染库的原生 API 会清除整格，透明、带像素边距或重叠图片使用彩色半格合成，须分别验收内容保留与原生图片精细度。

2026-10-28 云 Linux 验收使用 Rust 1.95.0、Neovim 0.12.6、Yazi 26.9.1 和随包 Vim 9.2.1225。真实 PTY smoke 验证了 Neovim 编辑/保存与实际行列数、Yazi 导航/筛选/图片字符回退及预览清除、Launcher 参数和含空格中文目录、外部程序退出码 75/76 留屏，以及退出后的终端恢复。真实 Vim 的两项 ignored runtime 回归、25 项编辑流程，以及脚本工作区的 11 项 bashdb/Vim 往返流程均通过。当前环境不是 WSL，未验收 Windows/macOS、物理触屏或宿主原生图片显示。

2026-10-29 启动器右键菜单的专项回归通过：app 启动器 13 项、storage 启动器 3 项、Shell 启动器 12 项和 UI 启动器 9 项。覆盖两种视图中的改名、改命令、固定字符图标、取消与删除、菜单外点按不穿透，以及修改后的旧后台结果拦截。`python3 scripts/tests/launcher-organize-smoke.py target/debug/tundra-shell` 在云 Linux 真实 PTY 中通过新增、右键编辑、中文多行与越界粘贴、实际命令启动、删除确认和内置项限制；需要允许本地控制 socket。当前环境不是 WSL，物理触屏仍需设备验收。

最终修复后，终端库 65 项、UI 库 77 项（图片 19 项）、Shell 命令行相关 31 项、UI 命令行集成 7 项及 Shell 多文档 Vim 的 ignored 回归 1 项均通过；`cargo check --workspace --all-targets --all-features --locked` 通过。改动的 Rust 文件及本地 WezTerm 补丁通过格式检查，中英文 4,093 对消息通过本地化校验。全项目格式检查仍会报告未修改的 `third_party/lsh` 既有格式差异。

本次全工作区运行记录为 2,520 项通过、9 项失败。失败已定位到现有模块的运行条件或检查误报：3 项依赖此容器缺少的 `/proc/<pid>/task/<tid>/children`，3 项 ACL 夹具受 `/tmp` 文件系统或 UID 映射限制，1 项旧 APT 故障复现预期在当前工具版本下不成立，1 项 watchdog 扫描器将任务历史的 `output(...)` 误当作创建进程，1 项彩色 PTY 测试继承了 `NO_COLOR`。彩色测试仅移除该环境变量后已单独通过；这些失败没有通过忽略测试或修改生产行为规避。

Linux 脚本工作区先运行 `cargo test --locked -p app --test script`、`cargo test --locked -p script-runtime`、`cargo test --locked -p ui --test script` 和 `cargo test --locked -p shell --lib controller::script::tests`。它们覆盖解释器选择、断点移动、过期结果、真实 bashdb、页面与输入。构建 Shell/CLI 后，在 WSL 执行 `python3 scripts/tests/editor-script-smoke.py target/debug/tundra-shell`，使用隔离目录和真实 PTY 验证六区、保存、运行输入、断点、步进、表达式、缩放和停止；真实触屏仍需独立设备验收。

多文档 Vim 切换另用 `TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p shell --lib vim_host::script_tests -- --ignored` 验证导出归属、原进程、只读恢复和撤销历史。

页面管理测试覆盖同类页面的独立数据、覆盖后返回、关闭后重开、迟到结果、旧登录会话结果、后台通知与前台焦点。编辑器测试额外检查独立恢复文件、进程文件锁、逐份认领草稿，以及保存或恢复记录清理失败时保留数据。

Shell 的 `page_lifecycle`、`auto_admin` 和 `update_cancellation_tests` 覆盖三种关闭选择、等待实际收尾、AA 从关闭确认返回、后台输入自动显示，以及更新准备成功与取消同时发生。更新取消还测试取消记录写入失败、修复后重试和撤回自动重启请求。

`command_line_runtime::tests` 在 Windows 使用真实 ConPTY，在 Linux 使用真实 PTY，检查两个页面的终端隔离、被覆盖时读取输出、Esc/Ctrl+Shift+X 字节转发、确认关闭以及子进程树退出。它们属于自动交互验证；桌面终端、触屏和真实系统授权仍需各自验收。

## 编辑器大文件回归与基准

运行方法、测量口径和 1/10/100 MiB 原始结果见 [Source 编辑器的大文件计算](editor-scalability.md)。

## 启动器整理模式

`cargo test --locked -p shell --lib controller::launcher::settings::tests` 覆盖图标和列表的整页滚动、首页末页边界、整理模式切换、新增草稿取消与必填检查、保存恢复、内置项不可删除、用户项删除确认、鼠标/无悬停拖动、Esc 取消拖动和键盘排序。应用层 `cargo test --locked -p app --test launcher` 验证自定义命令保存、权限与通用终端启动 effect；UI 和 storage 的 launcher 测试保留布局和旧配置兼容检查。

构建 Shell 与 CLI 后运行 `python3 scripts/tests/launcher-organize-smoke.py target/debug/tundra-shell`。脚本使用隔离配置及真实 Linux PTY，通过外部 CLI 完成首次设置、滚轮翻页、拖动内置图标、新增取消、自定义名称/文字图案/命令保存，并检查通用内嵌终端中的实际命令输出、保留输出后按回车返回、成功及失败退出后默认立即返回，以及退出后的终端模式恢复。

2026-10-29 云 Linux 验证中，上述定向回归、PTY 脚本、全工作区 all-targets/all-features 编译及 4,104 对中英文消息校验通过。环境不是 WSL，Windows/macOS、宿主原生图片显示与物理触屏未实机验证。完整 workspace 测试在 CLI 的 `embedded_children_linux` 两项守护子进程退出用例发生清理超时；该容器缺少 `/proc/<pid>/task/<tid>/children` 接口，不能把定向通过描述为全工作区测试全部通过。
