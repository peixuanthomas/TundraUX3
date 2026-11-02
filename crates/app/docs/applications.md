# 应用状态与内置应用

## AppState

`app::AppState` 保存时钟、退出状态、通知、认证会话、用户、配置、外观，以及可选的 Explorer、Launcher、Diagnostics 和 Editor 状态。

```rust
AppState::dispatch_at(AppCommand, Instant) -> AppAction
AppState::snapshot() -> AppSnapshot<'_>
```

显式传入单调时钟 `Instant`，便于确定地测试通知超时和登录锁定。`AppSnapshot` 提供借用的只读视图，状态变化通过新的 `AppCommand` 完成。`AppAction` 只描述 `Redraw`、`Exit`、`Reboot` 或 `PowerOff`，实际终端和系统操作由 Shell 执行。

通知包括 4 秒 toast、有键告警、FIFO 模态和可抢占的 critical 模态，响应队列使用有界容量。

## Explorer

Explorer 维护过滤、排序、多选、历史、剪贴板、拖放、冲突处理和进度。平台层完成目录枚举、复制、移动、重命名、打开和系统回收站操作。

`ExplorerTaskEngine` 为复制、移动和回收站提供单 worker、取消、暂存、journal/checkpoint 与崩溃恢复。部分轻量文件与存储操作仍在命令 apply 路径执行。

常用选择操作：

- Ctrl/Cmd+点击或空格逐项选择，Shift+点击或方向键连续选择。
- Ctrl/Cmd+A 全选，Ctrl/Cmd+I 反选，Ctrl/Cmd+Shift+A 取消选择。
- C/X/V 复制、剪切、粘贴，D 或 Delete 移入回收站。
- N/T 新建文件夹或文本文件，F2 重命名，Alt+Up 返回上级。
- 右键或 Shift+F10 打开菜单，菜单外点击或 Esc 关闭。

字母快捷键只在浏览列表时生效，路径、搜索和文件名输入中正常输入文字。批量操作只处理当前可见列表中的已选项。

路径栏通过 Ctrl/Cmd+L 编辑，支持选区、粘贴和自动转换路径分隔符。普通目录的空白处右键可在当前目录打开 Command Line，工作目录直接传给子进程。

## Launcher

Launcher 保存可执行目标和固定顺序，支持图标与列表视图。记录包含绝对非链接目标、类型、批准者和批准时间；启动前重新检查路径与执行批准，脚本和安装包保留二次确认。

内置应用由 `BUILT_IN_LAUNCHER_APPLICATIONS` 按平台声明，不写入可删除的 Launcher 配置。软件包入口在 Linux、Windows 和 macOS 分别使用发行版工具、WinGet 和 Homebrew。

外部程序可通过“运行设置 F4”保存系统启动或嵌入终端、参数和工作目录。参数解析为数组，不展开变量。自定义命令可选择退出后立即返回或保留输出直到按回车。

页面快捷键见 [Shell 会话](../../shell/docs/session.md#常用快捷键)，Command Line 状态见 [CLI 命令](../../cli/docs/commands.md) 和 [terminal-runtime](../../terminal-runtime/README.md)。

## 纯文本编辑器

编辑器使用 `Rope` 和 grapheme 光标，直接读写纯文本。Markdown 文件按原文编辑，不生成富文本或预览。

- 打开与保存使用后台任务，保存快照绑定精确 revision，旧任务不会清除较新修改。
- 每份文档有独立恢复编号、进程文件锁和节流写入，异常退出后逐份认领草稿。
- Explorer 默认交给编辑器的后缀由 `storage::DEFAULT_EDITOR_EXPLORER_OPEN_EXTENSIONS` 定义，自定义列表和空列表优先。
- LSH 提供语法着色，选区样式优先，文件内容不受显示影响。
- 行变更标记比较当前文本与最近成功保存的文本，不依赖 Git。

完整操作见[编辑器与脚本工作区](../../../docs/source-editor.md)。

## 时钟、系统状态与诊断

时钟调度位于 `app::clock`，保存于 `clock.v1.json`。Linux 跟随系统时间和时区；Windows/macOS 的演示时间设置沿用各自配置。时间同步细节见 [time](../../time/README.md)。

System Status 显示系统指标、进程、网络和存储。仪表盘支持添加、编辑和排列组件，系统概览独立展示详细指标。

`app::system_settings` 定义 `SystemSettingsBackend`、快照、请求和操作状态。后端通过受监督任务执行 I/O，Shell 将可用性和失败原因转换为界面状态。

`DiagnosticsTaskRuntime` 由 watchdog 管理，各页面可独立扫描。修复先显示预览并由用户确认；存储修复完成后锁存“需要重启”，重启后使用修复后的存储。日志与事故报告统一从 Logs 应用查看，详见[日志指南](../../runtime-log/docs/logs.md)。
