# platform

`platform` 提供操作系统查询与读写接口，包括应用路径、文件、进程、终端能力、系统指标、账户、服务、软件包、网络和磁盘。Shell 与 CLI 通过这些接口执行操作，不在页面层拼接提权命令。

## 模块结构

| 模块 | 职责 |
| --- | --- |
| `paths` | 各平台配置、状态、缓存、日志和临时目录 |
| `document` | 普通文件检查、分块读取、文档指纹和原子写入 |
| `platform` | 目录、卷、回收站、文件打开和平台能力 |
| `process`、`managed_process` | 进程参数、输出、取消和子进程管理 |
| `terminal` | 宿主终端能力、颜色与全屏序列 |
| `system_monitor` | CPU、内存、电池、网络和进程指标 |
| `management` | 类型化的系统管理查询、动作与固定后端 |
| `command_session` | 系统命令会话与可执行文件查找 |
| `shell_control` | 本机 Shell 控制连接与会话发现 |
| `task_history` | 页面、助手和定时任务的历史与输出 |
| `diagnostics` | `doctor` 环境和路径检查 |

平台专用实现位于 `src/linux/`、`src/windows/` 和 `src/macos/`。文档读取拒绝 FIFO、设备、符号链接和其他非普通文件，打开后再次核对文件身份。

## 管理接口

`ManagementQuery`、`ManagementSnapshot`、`ManagementAction` 和 `ManagementCommand` 描述查询、页面数据、可用操作和执行请求。动作 ID、字段 ID 和目标身份保持稳定，显示文字使用调用方语言。

当前管理类型包括服务、进程、软件包、网络、磁盘、用户、系统配置、配置备份、防火墙、SSH、计划任务、任务历史、文件权限和操作系统设置。

- 查询使用当前进程权限；需要额外权限的操作先经过 AA 和系统授权。
- 批量操作先核对全部目标。服务使用一次 systemctl，进程绑定原进程身份，同一 cron 来源合并保存。
- APT、DNF 和 pacman 支持批量软件包操作；pacman 安装和升级包含完整系统升级。
- Homebrew 位于 `management/homebrew/`，按 Formula/Cask 区分目标；WinGet 使用准确包编号和版本。
- 后端拒绝或无法确认结果时先读取实际状态，避免换后端重复写入。

## Shell 控制连接

`shell_control` 为外部 CLI 提供本机连接。Unix 使用当前用户私有的 socket 和登记文件并核对对端 UID/PID；Windows 命名管道和目录只允许当前用户 SID。

Shell 每次轮询最多处理 16 个连接，每连接读写 64 KiB；请求上限 1 MiB，响应上限 32 MiB。连接在 30 秒后清理，退出时删除登记和 socket。协议类型由 [shell-control](../shell-control/README.md) 定义。

## 文档导航

| 主题 | 文档 |
| --- | --- |
| 路径、启动身份、桌面和文件集成 | [系统集成](docs/integration.md) |
| 服务、进程、软件包与任务 | [Linux 系统管理](docs/linux-management.md) |
| 授权连接、撤销和固定操作 | [授权与助手连接](docs/authorization.md) |
| 网络、Wi-Fi、磁盘和目录扫描 | [网络和磁盘](docs/network-disks.md) |
| 用户、用户组和 SSH 公钥 | [用户管理](docs/user-management.md) |
| 文件权限、归属和递归范围 | [文件权限](docs/file-permissions.md) |
| 主机名、时区和自动校时 | [操作系统设置](docs/system-settings.md) |
| 配置检查、保存和恢复 | [系统配置文件](docs/system-config.md) |
| 配置历史、保留和清理 | [配置备份与恢复](docs/config-backups.md) |
| 监听端口、UFW 和 firewalld | [防火墙与端口](docs/firewall.md) |
| SSH 设置与新连接确认 | [SSH 服务管理](docs/ssh-management.md) |
| systemd timer 与 cron | [定时任务](docs/scheduled-tasks.md) |
| 任务摘要、输出和清理 | [任务历史](docs/task-history.md) |
| Homebrew Formula 与 Cask | [Homebrew 软件包](docs/homebrew.md) |
| WinGet 软件包 | [Windows 软件包](docs/windows-packages.md) |
| 进程、日志和目录性能 | [性能与复测](docs/performance.md) |
| 运维命令、任务编号与退出码 | [运维 CLI](../cli/docs/operations.md) |

## 验证

```sh
cargo test --locked -p platform
cargo test --locked -p platform --test shell_control
```

Linux 管理测试在 Linux 或 WSL 运行。真实授权、服务写入、无线设备、挂载和物理盘操作使用对应的隔离环境或测试主机；具体入口见各专题文档。
