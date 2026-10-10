# 应用状态和内置应用

## AppState

`app::AppState` 是 UI 无关的状态聚合：它保存时钟与时区、退出状态、通知、认证会话与用户、配置与外观，以及可选的 Explorer、Launcher、Diagnostics、Editor 状态。

```rust
AppState::dispatch_at(AppCommand, Instant) -> AppAction
AppState::snapshot() -> AppSnapshot<'_>
```

显式传入单调时钟 `Instant`，使通知超时、登录锁定等时间行为可确定地测试。`AppAction` **仅**表示 `Redraw`、`Exit`、`Reboot` 或 `PowerOff`；终端恢复、进程结束与操作系统重启、关机由 Shell 执行。`AppSnapshot` 以借用方式给出一致的只读视图，UI 不能经由快照修改 APP，所有更改都必须形成新的 `AppCommand`。

通知系统包括 4 秒 toast、有键告警、FIFO 模态和可抢占的 critical 模态；响应队列有上限，避免异常输入或后台结果无限积压。

## Explorer

Explorer 维护过滤、排序、多选、历史、剪贴板、拖放、冲突处理与进度；领域层还描述目录条目和操作结果。平台层完成枚举、复制、移动、重命名、打开与移入系统回收站。`ExplorerTaskEngine` 为复制、移动和回收站操作提供单 worker、取消、暂存、journal/checkpoint 与崩溃恢复；它不是由 `AppState::dispatch_explorer_at` 直接接线。

耗时文件操作会显示阶段进度，名称冲突、删除和清空回收站都先进入确认工作流。与此同时，`ExplorerFileService` 仍会在 apply 路径执行一部分平台、文件系统或存储操作，不能将其描述为所有副作用都已异步抽离。

批量操作使用当前可见列表中的已选项：Ctrl/Cmd+点击或空格逐项勾选，Shift+点击/方向键连续选择，Ctrl/Cmd+Shift 扩大选择时保留原有勾选。Ctrl/Cmd+方向键只移动焦点；Home/End 和 PageUp/PageDown 跳到首尾或翻页，可配合 Shift 选择。Ctrl/Cmd+A 全选、Ctrl/Cmd+I 反选、Ctrl/Cmd+Shift+A 取消选择。C/X/V 复制、剪切和粘贴全部已选文件或文件夹，D 或 Delete 按删除确认设置将它们移入系统回收站。所有已选文件行持续显示选中底色，鼠标操作不会隐藏已选范围；普通点击恢复单选，Ctrl+点击已选文件会取消该项。右键或 Shift+F10 打开菜单，提供选择操作和粘贴入口；点击菜单外的页面空白处或按 Esc 关闭菜单，关闭点击不会同时操作下方文件；过滤掉的条目不参与批量操作。N/T 新建文件夹/文本文件、F2 重命名单个已选项、Alt+Up 返回上级目录；Ctrl/Cmd+C/X/V 保留为兼容快捷键。文件操作单键仅在浏览列表或菜单时生效；输入路径、搜索词、新文件名和重命名时，C/X/V/D 等字母用于正常输入。

在普通目录的列表空白处右键，可选择“在此处打开终端”，以当前目录启动内建 Command Line；路径直接作为子进程工作目录传入，中文、空格和特殊字符无需拼成命令。此入口沿用 Command Line 权限检查，无权限时禁用，回收站和文件选中菜单不提供此项。退出终端后返回原文件管理器目录；从 Launcher 打开终端仍使用默认的用户文档目录。

点击路径栏“编辑”或按 Ctrl/Cmd+L 后，光标位于现有路径末尾，直接输入会追加文字。左右键移动光标，Shift+左右键选择文字；Home/End 跳到首尾，也可配合 Shift 选择。Ctrl/Cmd+A 全选路径，输入或粘贴替换选中的部分；Backspace 删除前一个字符，Delete 删除后一个字符，有选中内容时都删除选中的部分。输入和粘贴的 `/`、`\` 自动转换为当前系统的路径分隔符；长路径编辑时随光标横向滚动。

Windows、macOS 和 Linux 的 Trash 实现均封装在 `platform`，APP 不拼接系统回收站路径，也不直接调用平台命令。

## Launcher

界面快捷键见 [Shell 会话](../../shell/docs/session.md#各应用快捷键)。

Launcher 存储平台可执行项目及固定顺序，支持图标/列表视图。持久化记录绝对的非链接目标、目标类型、批准者和批准时间；刷新列表和启动前仅检查目标是否仍位于记录的路径，不计算或比较内容指纹、大小、修改时间或类型变化。旧配置中的指纹字段兼容保留但不再使用。脚本、安装包和快捷方式还需要二次确认。扫描和启动由平台适配器与 `LauncherController` 协作，结果回流 APP；`LauncherController` 目前仍会在 apply 路径完成一部分平台、文件系统或存储操作。旧配置中的目录固定项仍可读，但只有可执行条目会被当作可启动项目。

主页入口和 Launcher 应用均单击打开，无需双击。共用按钮要求在同一按钮内按下左键，并在 500 毫秒内松开才执行，不设最短按住时长；超过 500 毫秒的长按和拖动均不算单击。Launcher 的图标和列表视图都支持此操作；图标拖动排序时不会启动应用。

Launcher 固定提供 **Editor**；Linux 当前用户及 Windows/macOS 本地管理员还会在第一项看到 **Command Line**。这些内建应用不写入 Launcher 配置，不能删除或拖动排序。图标由 `launcher_icons.toml` 中的 built-in application ID 定义。

Command Line 的 PTY、输入和环境状态见 [CLI 说明](../../cli/docs/commands.md)与 [terminal-runtime](../../terminal-runtime/README.md)。更新源码后需同时构建 Shell 和 CLI。

## 纯文本编辑器

编辑器以 `Rope` 保存文本，并按 grapheme（用户看到的一个完整字符）移动光标，因此 CJK、emoji 和组合字符不会被截断。它只有纯文本编辑界面，没有 Source/Rich 切换、Markdown 格式按钮或预览。`.md`、`.markdown` 等文件与 `.txt` 一样直接读取和写回原文，Markdown 标记不会被解析成标题、列表或其他富文本内容。

打开和保存由后台任务执行；保存使用精确 revision 的 `SaveSnapshot`，旧 revision 即使成功也不会清除较新 revision 的 dirty 标记。Shell 用文档 fingerprint 发现外部修改，面对未保存内容关闭、打开其他文件或退出时提供保存/丢弃/取消。恢复文件按节流策略写入，避免每次按键落盘。

Settings 的 Editor 分类可配置 Explorer 交给内置编辑器打开的后缀；匹配不区分大小写，支持 `.d.ts` 等复合后缀。清空列表会把所有文件交回系统默认应用。

默认列表包含 Markdown、`.txt`、`.log`、`.json`、`.jsonl`、`.toml`、`.yaml`、`.yml`、`.ini`、`.cfg`、`.conf`、`.xml`、`.csv`、`.tsv`、`.c` 和 `.h`。已有配置中保存的自定义列表继续生效，不会自动追加后缀。

Editor 仅对 `.c`、`.h` 文件启用 C 语法高亮，区分关键字、字符串和字符常量、数字、注释及预处理指令。高亮只改变显示颜色，不改动文件内容；选区颜色优先。程序从完整文本计算并缓存标记的字节范围，滚动时复用，因此跨行注释和横向滚动中的字符串仍能正确着色；编辑、撤销和重做后重新计算。其他后缀保持普通文本显示。

文本编辑器左侧固定显示行号，横向滚动只移动正文。程序与上次成功保存的文本逐行比较：新增行号为绿色，修改行号为黄色；删除行用行号与正文之间的红色上横线标出，文件末尾删除用红色下横线标出。无需 Git 仓库，也不执行 Git 命令。打开文件后没有标记，保存成功后以实际写入的快照重新比较；保存失败保留标记，撤销和重做同步更新。比较结果按当前版本和保存版本缓存；大段重复或完全重写的内容超过比较工作量上限时，整段保守地标为替换，避免阻塞输入。

## 时钟、设置与诊断

### 系统状态

System Status 的进程详情使用彩色 CPU、内存用量条，以及右对齐的 PID、CPU%、内存列；名称使用剩余宽度。PID 为青色，内存为紫色，CPU 占用低于 40% 为绿色、40% 起为黄色、80% 起为红色。保留 CPU 和内存两份排行榜及其榜单标识，选中行以背景、加粗和下划线提示，数值颜色继续保留。过期和不可用数据仍明确显示对应状态。矮窗口省去内层边框，并优先保留表头和进程行。

### 设置与可用性

Settings 已移除不可用的 Sound、Display、Wi-Fi、Bluetooth 四个分类。网络操作从启动器进入网络应用；Windows/macOS 保留各自现有功能，缺少平台支持的操作明确显示不可用。

后续系统集成接口位于 `app::system_settings`：`SystemSettingsBackend` 提供缓存 `snapshot`、非阻塞 `submit` 和 `poll`；`SystemSettingsSnapshot` 按类别携带可用性及可选状态，`SystemSettingsRequest` 使用稳定设备／网络标识表达操作，`OperationStatus` 区分不可用、执行中、完成和失败。`None` 表示未获取，`Some(Vec::new())` 才表示真实查询后的空列表。Shell 当前只持有 `UnavailableSystemSettingsBackend`，在 Shell 的设置设备控制器中将原因转换成界面提示并统一拦截操作。适配器需通过受监督任务执行 I/O，并明确设备选择、凭据确认和失败恢复；UI 不直接调用平台 API。

### 时间与诊断

时钟调度位于 `app::clock`，项目保存于 `clock.v1.json`；同步和时区规则见 [time](../../time/README.md)。

Settings 的时间设置可使用平台时钟、默认 HTTP(S) 时间服务器或自定义地址。自定义地址仅在返回有效 `Date` 响应头并确认可同步后保存。全局选项写入 `StorageConfig` 并同步 AppState；外观选项写入当前用户账户并即时应用主题。System Status 保留健康检查和系统指标；日志、事件和事故报告统一从独立 Logs 应用查看。旧日志组件在加载布局时移除，其他组件位置保持。空白处右键先打开添加菜单，确认添加新组件后才进入编辑模式。选中组件通过强调色边框提示，编辑时其余组件边框每两秒切换一次明暗，并显示固定的编辑提示；减少动画时保持静态强调边框。System Overview 详情独立展示 CPU、内存、存储、电池用量条、网络和温度趋势，以及运行时间和主要进程，不受已添加组件影响。日志中的 O/Enter 打开选中的文件，完整规则见 [日志指南](../../runtime-log/docs/logs.md)。`DiagnosticsTaskRuntime` 是由 watchdog 管理的单 worker，汇总平台能力、存储文档健康、watchdog 报告与日志，修复操作先给出预览再由用户确认。修复存储后会锁存“需要重启”的状态，必须重启才能继续使用已修复的存储。
