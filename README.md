# TundraUX3

TundraUX3 是一个使用 Rust 编写的终端桌面环境实验项目。它在同一个 TUI 会话中提供首次启动配置、账户登录、天气锁屏、主页、时钟、文件管理器、应用启动器、Markdown 编辑器、设置、用户管理、诊断和通知中心。

项目以 **crossterm** 负责终端输入输出适配，以 **Ratatui** 负责布局和绘制，并将应用状态、UI 基础设施、操作系统能力、持久化和进程监督拆分为独立 crate。workspace 内部 crate 使用功能名称；面向用户的二进制名称仍然是 **tundra-shell** 和 **tundra-cli**。

> 当前主要支持 Windows 11 和 macOS。运行环境需要兼容 crossterm 的真实终端；默认资源集要求至少 108 × 20 个终端单元格，程序会根据实际加载的 ASCII 资源自动提高这个下限。

## 目录

- [主要功能](#主要功能)
- [快速开始](#快速开始)
- [程序如何工作](#程序如何工作)
- [架构和 crate 分工](#架构和-crate-分工)
- [核心状态与事件流](#核心状态与事件流)
- [内置应用的实现](#内置应用的实现)
- [持久化数据](#持久化数据)
- [CLI 和 Shell 参数](#cli-和-shell-参数)
- [开发与验证](#开发与验证)
- [故障排查](#故障排查)
- [许可证](#许可证)

## 主要功能

- 首次启动向导：选择语言、时区和外观，并创建首个管理员账户。
- 身份与权限：Argon2 密码哈希、登录失败锁定、会话身份和基于角色的管理操作。
- Weathr 锁屏：天气数据、地理位置、时区时钟、ASCII 世界和天气动画。
- 主页与 Shell：全屏终端会话、顶部栏、状态栏、弹窗、通知和焦点导航。
- 时钟：网络时间同步、IANA 时区与 DST 投影、闹钟和计时项目持久化。
- Explorer：目录浏览、排序、选择、重命名、复制/移动、系统回收站和冲突确认。
- Launcher：发现和启动平台应用，管理固定项目及图标/列表视图。
- Markdown 编辑器：源码与富文本模型、grapheme 级光标语义、异步打开/保存、外部修改检测和恢复数据。
- Settings：外观、时区、天气位置、Explorer、Editor、Launcher 和安全选项。
- Diagnostics：平台能力、存储健康、日志、watchdog 事故和可确认的修复操作。
- Watchdog：统一捕获可恢复 panic、监督后台任务、记录运行标记和生成 JSON/文本事故报告。

## 快速开始

### 环境要求

1. 安装支持 Rust 2024 edition 的稳定版 Rust 和 Cargo。
2. 使用 Windows 11 或 macOS。
3. 在 Windows Terminal、WezTerm、iTerm2 等兼容 crossterm 的终端中运行。
4. 将终端窗口调整到至少 108 × 20；更大的自定义 ASCII 资源可能需要更多空间。

### 构建

~~~console
cargo build -p shell -p cli
~~~

发布构建：

~~~console
cargo build --release -p shell -p cli
~~~

生成的用户入口是：

- target/debug/tundra-shell（Windows 为 tundra-shell.exe）
- target/debug/tundra-cli（Windows 为 tundra-cli.exe）

### 启动完整 Shell

~~~console
cargo run -p shell --bin tundra-shell
~~~

首次运行时，Shell 会创建存储文件并进入设置向导；完成管理员创建后，后续运行会先显示 Weathr 锁屏和登录界面。

### 查看 CLI

~~~console
cargo run -p cli --bin tundra-cli -- --help
cargo run -p cli --bin tundra-cli -- doctor
cargo run -p cli --bin tundra-cli -- paths
~~~

第一个双横线用于结束 Cargo 自己的参数，后面的内容才会传给 tundra-cli。

## 程序如何工作

### 启动生命周期

tundra-shell 的一次正常启动大致经过以下阶段：

1. **建立监督边界**：二进制创建一个进程级 WatchdogRuntime，安装全局 ProcessWatchdog，注册终端紧急恢复函数，并检查上次未正常结束的运行标记。
2. **加载资源并检查终端**：从 ascii-assets 加载默认主题，计算所有资源、Shell 布局和 Weathr 的共同最小尺寸；尺寸不足会在进入全屏模式前返回可操作的错误。
3. **准备平台与存储**：platform 确认系统权限和能力，storage 解析系统路径、创建目录、验证 schema、迁移旧用户文件，并恢复损坏但可重建的文档。
4. **启动视觉与天气预取**：Shell 播放启动 banner，同时可以在受监督后台任务中预取天气数据。
5. **决定身份入口**：如果用户列表为空，进入首次设置与管理员创建；否则进入 Weathr 锁屏，然后进入登录界面。
6. **创建会话**：ShellSession 同时持有 UI 无关的 AppState 和终端会话专用的 UiSessionState，并根据启动配置构建初始屏幕、焦点和命中表。
7. **进入事件循环**：Shell 获取终端事件、更新时间与后台任务、分发命令、构造 ViewModel，再由 ui 计算布局并绘制一帧。
8. **退出或锁屏**：退出会先恢复 raw mode、备用屏幕、鼠标捕获和光标；注销则销毁当前 Shell UI 会话并重新进入 Weathr 锁屏。Windows 的关机操作只在终端恢复后由 Shell runtime 调用平台接口。

### 一帧是怎样产生的

~~~mermaid
flowchart LR
    A["crossterm 终端事件"] --> B["ui::InputEvent"]
    B --> C["Shell 路由、焦点与命中测试"]
    C --> D["ShellCommand / UiIntent"]
    D --> E["app::AppCommand"]
    E --> F["AppState::dispatch_at"]
    F --> G["AppAction"]
    F --> H["AppState::snapshot"]
    H --> I["Shell presentation"]
    I --> J["ui ViewModel"]
    J --> K["Ratatui 布局与渲染"]
    K --> L["终端帧"]
~~~

Shell 把 crossterm 的按键、鼠标、resize、paste 和 focus 事件规范化成 UI 层的类型。按键保留 Press、Repeat、Release 阶段和 Shift、Control、Alt、Super、Hyper、Meta 修饰键；鼠标保留移动、按下、释放、点击、双击、拖拽和四向滚动。

键盘事件优先交给当前模态界面和焦点组件；鼠标事件通过命中表选择目标。重叠目标的优先级固定为：

1. Shell modal
2. Shell chrome
3. App overlay
4. App content

同层目标先比较 z_index，仍相同时后注册的目标优先。这使退出确认、通知模态等界面不会把输入泄漏给底层应用。

### 后台任务如何回到主线程

Explorer 文件操作、Launcher 扫描、Editor 文件 I/O、诊断扫描、时间同步和天气请求可能在后台执行。这些任务由 watchdog 的 managed task/thread API 追踪，通过 channel 或完成事件把结果送回 Shell；状态变更和最终绘制仍发生在会话控制路径上。

耗时操作不会直接持有 Ratatui frame，也不会从工作线程修改焦点或命中表。任务结果先变成领域结果或 Shell follow-up 命令，再驱动下一次 redraw。

## 架构和 crate 分工

### 依赖方向

~~~mermaid
flowchart TD
    CLI["cli<br/>运维入口"] --> SHELL["shell<br/>组合与运行时"]
    CLI --> STORAGE["storage"]
    CLI --> PLATFORM["platform"]
    CLI --> WEATHR["weathr"]

    SHELL --> UI["ui<br/>布局、渲染、输入基础设施"]
    SHELL --> APP["app<br/>领域状态与工作流"]
    SHELL --> IDENTITY["identity"]
    SHELL --> STORAGE
    SHELL --> PLATFORM
    SHELL --> WEATHR
    SHELL --> WATCHDOG["watchdog"]

    UI --> APP
    APP --> IDENTITY
    APP --> STORAGE
    APP --> PLATFORM
    APP --> TIME["time"]
    APP --> WATCHDOG

    IDENTITY --> STORAGE
    STORAGE --> PLATFORM
    WEATHR --> TIME
    WEATHR --> ASCII["ascii-assets"]
    WEATHR --> WATCHDOG
    UI --> ASCII
~~~

最重要的边界是不允许 app 依赖 ui、ratatui 或 crossterm。UI 可以读取 APP 的领域类型，但 APP 命令不携带坐标、Rect、UiId 或终端按键。shell 是组合根，负责把终端世界和领域世界连接起来。

### Workspace crate

| Crate | 职责 |
| --- | --- |
| **app** | AppState、AppCommand、AppAction、只读快照，以及 Editor、Explorer、Launcher、Diagnostics、通知和配置目录等领域类型。 |
| **ui** | 输入事件、焦点、命中测试、类型化 UiIntent、通用组件、主题、各屏幕的 ViewModel、布局和渲染。它不拥有终端生命周期。 |
| **shell** | ShellSession、控制器、presentation、终端事件转换、全屏会话、锁屏/APP 组合和 tundra-shell 入口。 |
| **cli** | tundra-cli 参数解析、诊断、路径查看、配置读写、存储重置和 Editor/Weathr 启动命令。CLI 不依赖 ui。 |
| **identity** | 用户、会话、授权、密码验证和登录锁定；身份记录由 storage 持久化。 |
| **storage** | 平台路径上的 TOML/版本化 JSON 文档、原子写入、schema 校验、迁移与损坏文件恢复。 |
| **platform** | Windows/macOS 的路径、终端能力、文件系统、回收站、程序启动、系统诊断和关机等 OS 边界。 |
| **time** | NetworkClock、ClockDisplay、ClockSnapshot、时间同步和 TIME_SYNC_INTERVAL；供 APP 与 Weathr 共用。 |
| **weathr** | 天气提供方、缓存、地理位置、动画、ASCII 场景和锁屏运行时。它作为库被 CLI 与 Shell 托管。 |
| **ascii-assets** | 主题清单、banner、图标、天气世界和时钟字体的加载、校验与尺寸统计。 |
| **watchdog** | 进程级 panic 边界、受管理任务、恢复策略、运行 journal 和事故报告。 |

### 源码布局

~~~text
crates/
├── app/
│   └── src/{application,editor,explorer,launcher,diagnostics}/
├── ui/
│   └── src/{foundation,screens,components,assets,theme}/
├── shell/
│   └── src/session/{controller,presentation,runtime.rs,ui_state.rs}
├── identity/
├── storage/
├── platform/
├── time/
├── weathr/
├── ascii-assets/
├── watchdog/
└── cli/
~~~

application 放置跨应用的全局状态；APP 下的领域模块不包含终端渲染。UI 的 screens 按屏幕拆分 model、layout 和 render。Shell 的 controller 处理交互与工作流编排，presentation 将 APP 快照和 UI 会话状态转换成 UI ViewModel，runtime 只负责生命周期、事件循环和外部副作用。

## 核心状态与事件流

### AppState：可测试的领域状态

app::AppState 是 UI 无关的状态核心，当前集中保存：

- 网络时钟、时区和同步结果；
- 通知队列、超时、去重与语义动作；
- 登录会话与可管理用户；
- 持久化配置和当前用户外观；
- Explorer、Launcher、Editor 和 Diagnostics 的领域状态；
- 退出确认状态。

核心接口是：

~~~rust
AppState::dispatch_at(AppCommand, Instant) -> AppAction
AppState::snapshot() -> AppSnapshot<'_>
~~~

显式传入单调时钟 Instant，让通知过期、登录超时等行为可以用确定的时间测试。AppAction 只表达 Redraw、Exit 或 PowerOff；真正恢复终端、结束进程或调用操作系统关机接口的是 Shell。

AppSnapshot 借用内部状态并只读暴露给 presentation。UI 不通过快照修改 APP；修改必须重新形成 AppCommand，因此状态转换仍有单一入口。

### UiSessionState：当前终端会话

UiSessionState 保存只对这一轮 TUI 会话有意义的内容，例如：

- 屏幕栈、当前焦点、悬停目标、弹窗和模态焦点恢复上下文；
- 终端尺寸、命中表、拖拽/滚动捕获和最近输入；
- 列表窗口、编辑器菜单、设置 picker 等显示层选择；
- 文件/扫描任务句柄、加载保存进度和其他运行时资源；
- presentation 所需但不应写入领域快照的短暂状态。

ShellSession { app, ui } 把二者统一管理。它接收规范化的 InputEvent，完成路由与 Shell 工作流编排，并把需要修改领域状态的部分交给 AppState。

UI foundation 还提供 UiIntent::App(AppCommand) 和纯 UI intent（焦点、overlay、命中、布局、redraw），使快捷键表可以保存类型化意图。当前 Shell 控制器仍保留一层 ShellCommand，用于需要平台服务、存储 I/O 或多步确认的组合工作流。

### ViewModel 与渲染

UI 不直接读取整个 ShellSession。Shell presentation 根据 AppSnapshot 和必要的 UI 会话状态构造每个屏幕的 ViewModel，UI 再执行：

1. 依据终端 Rect 计算布局；
2. 根据 ViewModel 生成文本、列表、边框和状态样式；
3. 注册与布局一致的命中区域；
4. 交给 Ratatui 写入当前终端帧。

这种方式让领域逻辑不依赖终端尺寸，也让布局/渲染测试可以使用固定 ViewModel 和固定 Rect。

## 内置应用的实现

### Weathr 锁屏

Weathr 使用 Open-Meteo 获取天气，并通过地址搜索、配置位置或时区对应城市决定坐标。请求结果有缓存；Shell 启动时可以先预取，锁屏随后复用缓存。场景由天气标准化结果、昼夜、季节和动画系统共同驱动，最终绘制 ASCII 房屋、树木、云、雨雪、月相等元素。

独立 CLI 模式底部提示退出；Shell 锁屏模式底部提示进入系统。两种模式共享渲染和天气逻辑，但由宿主创建 watchdog 并负责终端恢复。

### Explorer

Explorer 的领域层描述目录条目、选择、排序、冲突策略和操作结果；平台层执行实际的枚举、复制、移动、重命名、打开和回收站操作。长时间文件操作通过受管理后台任务执行，Shell 显示阶段进度，并在名称冲突、删除或清空回收站前建立确认流程。

Windows 和 macOS 的回收站实现位于 platform，因此 APP 不拼接系统 Trash 路径，也不直接调用平台命令。

### Launcher

Launcher 保存平台可执行项目和固定顺序，支持图标/列表等视图状态。扫描与启动由平台适配器完成，Shell 将异步结果更新回 APP。目录固定项仍能被旧配置读取，但 Launcher 只把可执行条目当作可启动项目。

### Markdown 编辑器

编辑器领域层维护 Markdown、富文档节点、选择范围、编辑命令和副作用。光标位置按 grapheme 处理，而不是按 UTF-8 字节处理，因此 CJK、emoji 和组合字符不会让光标落在字符内部。

UI 分别实现文档视图、源码视图、布局和渲染。打开/保存由后台任务执行；Shell 用文档 fingerprint 检测外部修改，在未保存内容关闭、打开另一个文件或退出时先给出保存/丢弃/取消选择。恢复文件按节流策略写入，避免每次按键都同步落盘。

### 时钟、设置和诊断

共享 time crate 以最近一次同步 UTC 和单调时钟为锚点生成快照，再通过 chrono-tz 投影到配置时区，因此 DST 跳变不会被硬编码成固定偏移。时钟的用户项目写入 clock.v1.json。

Settings 修改 StorageConfig，保存成功后同步更新 APP 和当前主题。Diagnostics 汇总平台能力、存储文档健康、watchdog 报告和日志；修复操作先生成预览，再由用户确认执行。

## 持久化数据

### 平台路径

| 用途 | Windows | macOS |
| --- | --- | --- |
| 配置 | %APPDATA%\TundraUX3\config.toml | ~/Library/Application Support/TundraUX3/config.toml |
| 状态 | %LOCALAPPDATA%\TundraUX3\state | ~/Library/Application Support/TundraUX3/state |
| 缓存 | %LOCALAPPDATA%\TundraUX3\cache | ~/Library/Caches/TundraUX3 |
| 日志 | %LOCALAPPDATA%\TundraUX3\logs | ~/Library/Logs/TundraUX3 |
| 临时文件 | %TEMP%\TundraUX3 | 系统临时目录下的 TundraUX3 |

可使用 tundra-cli paths 查看模板路径和当前机器解析后的绝对路径。

### 文档与 schema

storage 会管理下列主要文件：

| 文件 | 格式 | 内容 |
| --- | --- | --- |
| **config.toml** | TOML，schema 1 | 语言、时区、天气位置、快捷键、外观及各应用设置。 |
| **users.v2.json** | 版本化 JSON，schema 2 | 用户、角色、密码哈希、登录失败与锁定信息。 |
| **state.v1.json** | 版本化 JSON，schema 1 | 一般应用状态。 |
| **recent-files.v1.json** | 版本化 JSON，schema 1 | 最近文件。 |
| **sessions.v1.json** | 版本化 JSON，schema 1 | 可恢复会话数据。 |
| **clock.v1.json** | 版本化 JSON，schema 1 | 时钟、闹钟和计时项目。 |
| **trash/trash.v1.json** | 版本化 JSON，schema 1 | 应用回收站清单。 |

写入使用临时文件和替换步骤，尽量避免部分写入。启动时先检查 schema：比当前程序更新的 schema 会被拒绝，防止旧程序覆盖新格式；无法解析的当前/旧格式文档会被移到恢复文件并用默认文档重建，同时在 Shell 中显示恢复提示。旧的 users.v1.json 会迁移到 users.v2.json。

密码不会以明文写入存储，而是通过带随机 salt 的 Argon2 哈希保存。CLI 明确禁止直接读取或修改用户名、密码等身份字段；这些操作必须通过经过授权的用户管理工作流。

### Watchdog 数据

事故报告写入日志目录下的 crashes，每个事故包含配对的 JSON 和文本报告。持久化操作 journal 位于：

~~~text
<data>/watchdog/operations/<app-id>/
~~~

活动运行标记位于 <data>/watchdog/runs/，用于在下次启动时报告进程来不及在当前进程中观察到的异常退出。watchdog 会集中脱敏并限制报告大小，但调用方也禁止把密码、token、剪贴板内容或原始用户输入写入事故上下文。

## CLI 和 Shell 参数

### tundra-shell

~~~console
tundra-shell [-notfullscreen] [-debug] [-editor]
~~~

| 参数 | 作用 |
| --- | --- |
| **-notfullscreen** | 不使用默认全屏终端模式。 |
| **-debug** | 请求调试主页；发布构建仍受 DebugPolicy 限制。 |
| **-editor** | 将启动目标设为 Markdown 编辑器；存在身份门禁时仍需先完成登录。 |

未知参数和重复参数都会使进程以参数错误退出。

### tundra-cli

~~~console
tundra-cli <config|doctor|editor|explain|new|paths|test-frost|test-matrix|weathr>
~~~

| 命令 | 作用 |
| --- | --- |
| **config** | 查看全部可公开配置。 |
| **config get [field]** | 查看 theme、border-shape、border-color、accent-color、language、timezone 或 address。 |
| **config set <field> <value>** | 修改边框形状/颜色、强调色、语言、时区或天气地址。theme 是只读摘要。 |
| **doctor** | 检查系统、终端、权限、应用路径、存储和资源是否就绪。 |
| **editor** | 直接启动 Shell 的 Markdown 编辑器目标。 |
| **explain** | 输出简短的启动和边界说明。 |
| **paths** | 输出配置模板路径和当前解析路径。 |
| **test-frost** | 仅播放启动 frost banner。 |
| **test-matrix** | 仅播放首次运行 Matrix banner。 |
| **weathr** | 以独立 CLI 模式运行天气场景。 |
| **new** | 清除已保存的 TundraUX3 数据并重新创建初始存储。 |

**new 会删除用户配置和状态。** 应先确认 tundra-cli paths 输出并备份需要保留的文件。

配置示例：

~~~console
tundra-cli config
tundra-cli config get timezone
tundra-cli config set timezone Asia/Shanghai
tundra-cli config set border-shape rounded
tundra-cli config set border-color light-cyan
tundra-cli config set accent-color "#38bdf8"
~~~

## 常用交互

Shell 支持键盘和鼠标；具体按键会随当前屏幕、焦点和弹窗改变。全局与入口级行为包括：

| 输入 | 行为 |
| --- | --- |
| Tab / Shift+Tab | 在当前焦点顺序中向前/向后移动。 |
| Ctrl+C | 请求关闭当前终端会话；Editor 内会优先保留编辑器自己的命令语义。 |
| q 或 Esc（主页） | 打开退出确认。 |
| L（主页） | 注销并返回 Weathr 锁屏。 |
| F2（登录） | 临时切换密码可见性。 |
| y / Enter（退出确认） | 确认退出。 |
| n / Esc（退出确认） | 取消退出。 |

Editor、Explorer、Settings 等屏幕还会根据工具栏、列表、对话框和输入模式处理方向键、Home/End、PageUp/PageDown、Enter、Space、Backspace、鼠标双击、拖拽和滚动。

## 开发与验证

### 推荐检查

~~~console
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo build -p shell -p cli -p weathr
~~~

weathr 是库 crate，因此最后一条命令验证它能被构建，但用户通过 tundra-cli weathr 或 Shell 锁屏运行它。

### 定向测试

~~~console
cargo test -p app
cargo test -p ui
cargo test -p shell
cargo test -p storage
cargo test -p identity
cargo test -p platform
~~~

测试重点包括：

- 输入阶段、修饰键、paste、focus、双击、拖拽和滚动；
- 模态命中优先级与焦点恢复；
- 通知 follow-up、超时和去重；
- Editor grapheme 位置、Markdown 往返、异步保存和退出保护；
- Explorer/Launcher 后台任务和平台文件操作；
- 登录锁定、用户权限和密码存储；
- 时钟同步、时区与 DST；
- storage schema、迁移、原子写入和损坏恢复；
- watchdog panic 边界、任务回收和事故报告。

### 修改代码时应保持的边界

- 不要从 app 引入 ui、ratatui 或 crossterm。
- APP 命令使用领域含义，不携带终端坐标或组件 ID。
- 平台路径和 OS API 只通过 platform；文档格式和写入只通过 storage。
- 终端 raw mode、备用屏幕和进程退出只由 Shell/Weathr runtime 管理。
- ViewModel 在 Shell presentation 组装；UI 只负责布局、绘制和通用交互基础设施。
- Editor 位置继续使用 grapheme 语义，不能退化为 UTF-8 字节偏移。
- 任何可能 panic 的生产后台任务都应进入 watchdog managed task group，并声明重放安全性。

## 故障排查

### 终端太小

如果看到 terminal is too small，按错误中给出的尺寸扩大窗口。默认资源目前需要至少 108 × 20，不是固定写死的常量：更换主题或加入更大的 ASCII 资源后要求会随之变化。

### 终端显示异常或程序异常退出

正常退出和大多数 panic 会自动恢复 raw mode、鼠标捕获、备用屏幕、颜色和光标。如果宿主被强制终止，可先重置当前终端，随后查看日志目录下的 crashes 报告。下一次启动还会读取未关闭的运行标记并生成“原因未知”的事故记录。

### 路径或权限失败

先运行：

~~~console
tundra-cli doctor
tundra-cli paths
~~~

macOS 上 Explorer 的 Trash 操作可能需要 Full Disk Access；程序会在启动/诊断中给出系统设置提示。Windows 平台检查要求 Windows 11 build 22000 或更高。

### 配置损坏

不要立即执行 tundra-cli new。先备份 paths 输出中的配置和状态目录，再查看 Shell 的恢复提示以及日志。Storage 通常会保留损坏原件并创建默认文档；new 适合明确需要彻底重置的情况。

## 许可证

项目根目录代码按 [MIT License](LICENSE) 授权。Weathr 组件另带 [GNU GPL v3 许可文本](crates/weathr/LICENSE.weathr)；分发或再使用时请同时检查对应组件和第三方资源的许可要求。
