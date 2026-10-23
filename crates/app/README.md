# app

`update/vim/install.rs` 从项目独立引擎 Release 下载固定版本的 ZIP，复用现有可取消的 HTTP 请求，报告下载字节及解压、校验阶段。验证 SHA-256、目录内容和上游清单后才替换用户数据目录的 `vim/`，失败或取消时清理本次临时目录。

Windows 引擎校验还要求 `libiconv-2.dll`、对应许可证和源码归档完整；下载及更新都保留这些文件，支持真实 Vim 的 UTF-16 保存。

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

外部 Vim 的原始保存字节通过 `SaveSnapshot::external_bytes` 交给现有异步原子保存流程，不重新编码、不改换行或 BOM；Vim 继续负责缓冲的修改标记、撤销和路径，快照不会认领普通编辑器的待保存记录。`synchronize_external_source` 仅镜像文本、元数据、光标和修改状态，保留只读保存边界。

更新同时准备 Shell、CLI 和完整 `vim/` 目录，校验 Vim 的运行资源、许可和上游构建信息；发布失败、启动失败或中断恢复时一并回退。旧版没有 Vim 目录时，回退会移除本次新增目录。Beta 在 Cargo 构建后显式运行 `scripts/build-vim.py`，需要 Python 3.12 及 Vim 的 C 构建依赖；普通 Cargo 构建不会下载 Vim。默认主题、自定义资源和用户文件继续保留。

调用方通过 `AppCommand` 修改状态，通过 `AppSnapshot` 读取状态。`AppAction` 返回重绘、退出或电源请求，Shell 执行终端恢复与进程操作。

`SetClockTimezone` 的 `timezone_id: None` 让时钟直接跟随系统时间和时区。此模式忽略旧的同步时间，保存其他设置也不会重新套用配置中的独立时区。

Explorer 刷新当前目录时保留搜索、选择和视口；切换目录才重置导航状态。编辑器保存后由 Shell 刷新所有指向该目录的已有页面。

Explorer 默认交给编辑器的 106 种后缀包含现有语法着色支持的全部 99 种后缀和原有文本后缀。测试直接核对语法定义，避免已有支持的类型遗漏；自定义列表仍按用户设置匹配。

Linux 启动器固定包含防火墙与端口、SSH 服务、配置备份与恢复、定时任务入口；这些入口和已有管理应用一样，由 Shell 打开独立页面，由 platform 执行实际操作。

部分文件和启动器操作仍在 `apply` 中调用平台或存储服务；不能假定每条命令都没有文件读写。后台任务使用 watchdog，写操作不能未经判断自动重放。

- [应用行为与保存约束](docs/applications.md)
- [程序自更新](docs/update.md)与[更新测试](docs/update-tests.md)
- [系统配置保存](../platform/docs/system-config.md)

验证：`cargo test --locked -p app`。修改跨 crate 的命令或共享类型后运行 workspace 测试。
