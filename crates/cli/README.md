# cli

`cli` 提供 `tundra-cli`：公开配置、诊断、日志、Linux 运维命令、运行中 Shell 的 UI 控制和交互命令行。它也承载更新及授权助手入口；这些内部入口不作为普通脚本接口。

`debug screen-keyboard` 复用 QWERTY 屏幕键盘：小窗口通过 Fn / ABC 切换区域，宽窗口同时显示功能键与导航区，实体导航键也提供按下反馈。

`src/commands/` 按命令分组；参数解析、REPL 和程序入口留在 crate 根模块，Linux 管理连接使用共享的 platform 接口。

运维命令包括 `firewall`（监听端口与放行规则）、`ssh`（生效设置、公钥和新连接确认）、`backups`（集中差异与恢复）和 `scheduled-tasks`（既有任务及持久 service/timer）。需要系统权限的只读查询也使用同一授权流程；查询及预览支持 JSON。

定时任务保留原命令和 `schedule` 表达式接口，Shell 向导使用同一后端。CLI 的只读查询和预览不生成修改任务历史；实际操作沿用授权助手的稳定任务编号与输出归档。

| 用法 | 示例 |
| --- | --- |
| 外部终端 | `tundra-cli doctor` |
| 内建 Command Line 或 `repl` 中的 UX 命令 | `/doctor` |
| 内建 Command Line 或 `repl` 中的系统命令 | `ls -la` 或 `dir` |
| 查询帮助 | `tundra-cli help`、`tundra-cli users --help` |
| 查询版本 | `tundra-cli --version` 或 `tundra-cli -V` |

`doctor` 是顶层命令，支持 `doctor --help` 和 REPL 补全。根帮助逐项列出全部公开命令，子命令通过各组 `--help` 查看。

帮助（含子命令帮助和无参数启动）与版本查询无需启动 watchdog 或读取配置，日志和数据目录不可写时也能使用。

`tundra-cli ui` 在当前操作系统用户的 Shell 会话中读取界面快照，并发送点击、移动、拖动、滚轮、按键和文字输入；`wait` 等待目标出现、消失或可操作。默认关闭，需先在设置中开启外部 CLI 控制。所有控制结果使用版本 1 的 JSON；多个会话必须指定 `--session`，输入超时后不自动重发。控制命令跳过配置、日志和 watchdog 初始化，文字可通过 `text --stdin` 输入。

交互帮助与软件包管理帮助沿当前语言快照显示；诊断结果提供中文状态说明，外部工具的原始输出仍保留供排查。

普通输入交给系统命令解释器，UX 命令必须加 `/`。命令行保留当前会话的目录和已导出环境变量，不改变主进程或系统配置。

PATH 补全在后台扫描，输入线程只读取缓存；扫描线程空闲时阻塞等待新请求，由取消通知唤醒退出，不定时轮询。

Linux 内建 Command Line 会继续管理脱离终端会话的后代进程；正常 `exit` 先请求它们结束并等清理完成，忽略 TERM 的进程需要通过右上角按钮确认终止。

- [命令、配置与 REPL](docs/commands.md)
- [外部 CLI 控制、快照与 JSON 协议](docs/ui-control.md)
- [运维命令、系统授权和退出码](docs/operations.md)
- [日志查询、跟随和导出](../runtime-log/docs/logs.md)
- [界面预览](../shell/docs/previews.md)

运行 `cargo build --locked -p shell -p cli` 后再验证内嵌命令行。CLI 使用 Shell 提供的界面预览，不能把它描述为完全不依赖界面代码。`firewall`、`ssh`、`backups`、`scheduled-tasks` 的帮助和参数提示使用当前语言，提供英文、简体中文资源；无语言快照时使用内置英文，帮助查询仍不读取配置。

验证：`cargo test --locked -p cli`。中文帮助测试同时加载启动、共享 UI 和管理命令语言资源，覆盖跨文件的文字引用。

运维命令固定测试保留参数校验、权限边界和文件安全，不创建真实 TCP 监听端口。

其他命令保留分发、参数拒绝、配置持久化和权限测试；共享帮助流程已覆盖的演示命令、固定路径标签和 REPL 提示颜色不再单独测试。

UI 控制测试覆盖参数、帮助、补全、会话选择、模拟 Shell 通信、旧快照拒绝与等待超时；输入命令在超时后只发送一次。Linux 验证使用本机 WSL。

`tundra-cli ui wake` 关闭空闲天气或天气锁屏，并返回绘制后的 Shell 快照；原有页面和弹窗保留，登录要求继续生效。普通页面执行时只读取当前快照。

用户操作后，控制命令会拒绝，需重新执行 `snapshot` 或 `wait` 获取信息；不传 `--snapshot` 也会检查。旧编号可跨自动重绘使用，动画、进度条、时钟、单纯移动鼠标和天气画面不会触发这项拒绝。

`tundra-cli debug ui-test` 在六页中展示全部公共共享控件，使用内置深色主题和英文，不读写配置、语言文件、日志或状态。F1–F6 或底部按钮翻页，Tab 切换焦点，Esc 退出；最小终端尺寸为 88 列 × 30 行。

`tundra-cli debug test-rainbow-text` 独立演示持续变色的彩虹文字，使用内置示例，跳过启动确认、配置和日志初始化；Esc、Ctrl+C、Enter 或退出按钮结束。命令支持帮助和 REPL 补全。
