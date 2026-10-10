# platform

`platform` 提供各操作系统的实际读写和查询：路径、文件、进程、终端能力、系统账户、服务、网络和磁盘。UI 展示结果，不能自行拼接提权命令或调用系统接口。

`src/linux/`、`windows/`、`macos/` 放各系统实现；`management/` 放固定管理操作，`command_session/` 放命令会话。共享文件、路径、进程和终端类型保留在对应根模块。

| 内容 | 文档 |
| --- | --- |
| 数据目录、Linux 启动身份、桌面和文件集成 | [系统集成](docs/integration.md) |
| 服务、进程、软件包和授权助手 | [Linux 系统管理](docs/linux-management.md) |
| 私有授权连接、撤销和任务控制接口 | [授权与助手连接](docs/authorization.md) |
| Wi-Fi、网络恢复、磁盘健康与目录扫描 | [网络和磁盘](docs/network-disks.md) |
| 系统用户、用户组和 SSH 公钥 | [用户管理](docs/user-management.md) |
| 差异预览、私有检查、授权保存和恢复 | [系统配置文件](docs/system-config.md) |
| 脚本调用、任务编号与退出码 | [运维 CLI](../cli/docs/operations.md) |

查询使用当前进程权限。需要额外权限的操作先经过 AA，再由系统授权；保存的 Tundra 角色不会授予 Linux 权限。后端拒绝或无法确认写入结果时，先查询实际状态，不能换后端重复写入。

验证：`cargo test --locked -p platform`。Linux 检查在 Linux/WSL 运行；真实授权窗口、服务变更、无线设备、挂载和物理盘行为需要对应测试主机。
