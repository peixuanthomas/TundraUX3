# app

`app` 保存应用状态并处理命令。它不依赖 UI、Ratatui 或 crossterm；窗口大小、按键、焦点和点击位置由 Shell/UI 处理。

| 模块 | 职责 |
| --- | --- |
| `application/` | 全局状态、应用命令、通知和只读快照 |
| `editor/` | 文本、选择、撤销、保存快照和恢复 |
| `explorer/` | 文件列表、选择、操作任务与恢复 |
| `launcher/` | 内置入口、固定应用和启动工作流 |
| `diagnostics/`、`runtime_logs/` | 诊断状态、日志查询和查看 |
| `clock.rs` | 闹钟和计时任务的调度 |
| `update/` | 更新检查、下载、构建、替换与回退 |

`editor/` 将文本缓冲、文档、编辑状态和命令处理分别放在 `source_buffer.rs`、`document.rs`、`state.rs`、`controller.rs`；`explorer/` 将状态、命令处理和文件操作分别放在 `state.rs`、`controller.rs`、`files.rs`。`update/` 的 Git 查询、工具链和 Linux 正式包处理分别位于 `git.rs`、`toolchain.rs`、`release.rs`。

调用方通过 `AppCommand` 修改状态，通过 `AppSnapshot` 读取状态。`AppAction` 返回重绘、退出或电源请求，Shell 执行终端恢复与进程操作。

部分文件和启动器操作仍在 `apply` 中调用平台或存储服务；不能假定每条命令都没有文件读写。后台任务使用 watchdog，写操作不能未经判断自动重放。

- [应用行为与保存约束](docs/applications.md)
- [程序自更新](docs/update.md)与[更新测试](docs/update-tests.md)
- [系统配置保存](../platform/docs/system-config.md)

验证：`cargo test --locked -p app`。修改跨 crate 的命令或共享类型后运行 workspace 测试。
