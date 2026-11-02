# app

`app` 保存应用状态并处理领域命令，不依赖 UI、Ratatui 或 crossterm。窗口尺寸、按键、焦点和点击位置由 Shell/UI 处理，平台读写通过 `platform` 与 `storage` 完成。

## 模块

| 模块 | 职责 |
| --- | --- |
| `application/` | 全局状态、应用目录、通知、命令和只读快照 |
| `editor/` | 文本缓冲、多文档、撤销、保存、恢复和 Vim 镜像 |
| `script/` | 脚本解释器、参数、断点和每份文档的运行状态 |
| `explorer/` | 文件列表、选择、操作任务与恢复 |
| `launcher/` | 内置入口、自定义命令、外部程序和启动工作流 |
| `diagnostics/`、`runtime_logs/` | 诊断、日志查询和故障报告合并 |
| `system_settings` | 系统设置后端接口与可用性状态 |
| `clock.rs` | 闹钟和计时任务调度 |
| `update/` | 更新检查、下载、构建、Vim 安装、替换与回退 |

调用方通过 `AppCommand` 修改状态，通过 `AppSnapshot` 读取一致视图。`AppAction` 只表达重绘、退出、重启或关机请求，终端恢复与进程操作由 Shell 执行。

## 主要状态与工作流

### Explorer 与 Launcher

Explorer 管理目录、筛选、排序、多选、剪贴板和操作进度。`ExplorerTaskEngine` 为耗时文件任务提供取消、检查点和恢复，平台层执行实际复制、移动、回收站和目录操作。

Launcher 保存内置应用、自定义 Shell 命令和已批准的外部程序。自定义条目支持名称、5×2 终端格文字图标、参数、工作目录和退出后保留输出；内置入口不进入可删除配置。Unix 自定义命令使用 `/bin/sh -c`，Windows 使用 `cmd.exe /C`。

### Editor 与脚本

编辑器按 grapheme 处理用户可见字符，使用精确 revision 的保存快照和独立恢复记录。Vim 的外部写入字节通过 `SaveSnapshot::external_bytes` 进入同一原子保存流程，不重新编码或改变行尾。

脚本状态按文档保存解释器、参数、断点、运行编号和暂停编号。编辑、撤销和重做会移动断点，旧运行或旧暂停位置的结果不会覆盖当前数据。

### 更新与日志

更新同时准备 Shell、CLI 和完整 `vim/`，校验资源、许可和上游元数据，失败或启动超时一起回退。Vim 安装包验证 SHA-256、目录内容和固定版本，Windows 同时检查 iconv DLL、许可和源码。

运行日志将已授权的故障报告合并为最高等级 `Critical`，按时间排序并遵守报告访问限制。

## 文档

- [应用状态与内置应用](docs/applications.md)
- [程序自更新](docs/update.md)
- [更新测试](docs/update-tests.md)
- [编辑器与脚本工作区](../../docs/source-editor.md)
- [系统配置保存](../platform/docs/system-config.md)

## 验证

```sh
cargo test --locked -p app
```

修改跨 crate 命令或共享类型后运行 workspace 测试。更新与 Vim 安装测试使用离线数据，覆盖校验、取消、文件安全和回退。
