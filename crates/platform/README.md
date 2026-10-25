# platform

`platform` 提供各操作系统的实际读写和查询：路径、文件、进程、终端能力、系统账户、服务、网络和磁盘。UI 展示结果，不能自行拼接提权命令或调用系统接口。

`src/linux/`、`windows/`、`macos/` 放各系统实现；`management/` 放固定管理操作，`command_session/` 放命令会话。共享文件、路径、进程和终端类型保留在对应根模块。

文档读取、尾部读取和指纹计算在打开文件前拒绝 FIFO、设备等非普通文件，避免等待管道写入方；仍拒绝路径中的符号链接，并在打开后继续核对文件类型。

| 内容 | 文档 |
| --- | --- |
| 数据目录、Linux 启动身份、桌面和文件集成 | [系统集成](docs/integration.md) |
| 服务、进程、软件包和授权助手 | [Linux 系统管理](docs/linux-management.md) |
| macOS Homebrew Formula 与 Cask | [Homebrew 软件包](docs/homebrew.md) |
| WinGet 查询、安装、卸载和升级 | [Windows 软件包](docs/windows-packages.md) |
| 私有授权连接、撤销和任务控制接口 | [授权与助手连接](docs/authorization.md) |
| Wi-Fi、网络恢复、磁盘健康与目录扫描 | [网络和磁盘](docs/network-disks.md) |
| 系统用户、用户组和 SSH 公钥 | [用户管理](docs/user-management.md) |
| 多选文件的权限、归属和递归范围预览 | [文件权限](docs/file-permissions.md) |
| 主机名、系统时区、自动校时与服务器配置 | [操作系统设置](docs/system-settings.md) |
| 差异预览、私有检查、授权保存和恢复 | [系统配置文件](docs/system-config.md) |
| 按操作分组的配置、账户和用户组恢复 | [配置备份与恢复](docs/config-backups.md) |
| 监听端口、UFW 与 firewalld 放行规则 | [防火墙与端口](docs/firewall.md) |
| SSH 登录设置、端口与新连接确认 | [SSH 服务管理](docs/ssh-management.md) |
| systemd timer 与已有 cron 任务 | [定时任务](docs/scheduled-tasks.md) |
| 脚本调用、任务编号与退出码 | [运维 CLI](../cli/docs/operations.md) |
| 进程、日志、文档、目录性能及复测方式 | [性能优化](docs/performance.md) |

查询使用当前进程权限。需要额外权限的操作先经过 AA，再由系统授权；保存的 Tundra 角色不会授予 Linux 权限。后端拒绝或无法确认写入结果时，先查询实际状态，不能换后端重复写入。

软件包、服务、进程和计划任务共用 `management::selection` 传递每个目标的编号和查询时的信息，并限制可批量执行的操作。服务先检查全部单元，再交给一次 systemctl；进程先核对全部身份并打开绑定原进程的句柄，再逐项发信号或设置同一主线程优先级。同一 cron 来源的多条任务先一起检查，再一次保存、生成一份备份；timer 保留原有逐项检查与备份。失败即停止，已完成的修改保留。

APT、DNF 和 pacman 一次处理全部已选包，pacman 安装和更新继续包含完整系统升级。Homebrew 按 Formula/Cask 分组，WinGet 按准确编号和版本依次处理；Homebrew 的命令和 PTY 夹具需在 macOS 运行。

macOS 软件包后端位于 `management/homebrew/`，与 Linux 后端共用查询、操作、AA 事件和子进程监督接口。通过 brew JSON v2 读取 Formula/Cask，类型与完整名称共同组成目标标识；提交前重新核对选中版本。写操作以当前用户运行，Cask 密码等原生提示通过 PTY 交互，关闭页面可继续后台运行，显式停止和强制结束沿用 AA。查询和普通操作不自动刷新、不清理其他包；刷新按钮显式执行 `brew update`。

Windows 的 Packages 使用独立 WinGet 后端，复用管理页的数据和动作接口；其他 Linux 管理后端不在 Windows 启用。只调用现有的 `winget.exe` 和 `winget` 软件源，不自动安装 WinGet，也不接入 `msstore`。安装器所需的系统权限由 Windows/UAC 处理，软件包修改期间应保持 Tundra 运行。

`shell_control` 为外部 CLI 提供本机 Shell 连接和会话发现。登记文件保存在当前用户缓存目录的 `shell-control/` 中；Unix 使用用户独有的 `0700` 目录和 `0600` socket、登记文件，连接后核对对方用户编号和 Shell 进程编号。Windows 命名管道和登记目录只允许当前用户 SID，明确拒绝网络用户，子文件继承相同权限。退出时删除登记文件并关闭连接，Unix socket 随监听器删除；崩溃留下的无效登记不会出现在会话列表中。

Shell 在主线程调用 `poll`，每次最多处理 16 个连接、每个连接读写 64 KiB；请求最多 1 MiB、响应最多 32 MiB，每条连接只接受一个请求并在 30 秒后清理。客户端连接和收发共用调用方给出的超时。输入内容只留在内存中。Windows 先用 `PeekNamedPipe` 检查可读字节，避免 interprocess 2.4.6 把空闲管道的 `ERROR_NO_DATA` 当成断线；繁忙管道使用库已有的零等待连接选项。

管理查询区分缺依赖、环境不支持、权限不足和执行失败，保留完整失败详情；systemd 查询先检查宿主环境，避免把容器限制误报为需要授权。无温度传感器或电池按不可用数据处理，不作为反复采集错误。

操作系统设置依赖 systemd、系统 D-Bus 和主机名、时间服务接口；环境缺失时明确说明受限操作。时区列表或校时配置查询单独失败时保留已读数据，并禁用受影响的操作，原生工具诊断保留在详情中。

`SystemSettings` 默认查询只返回主机名，`scope = "time"` 返回系统时区和校时配置，供“区域与时间”使用；写入后按同一范围刷新，授权、版本检查与备份规则不变。

权限修改按 Linux 原始文件名传递目标；预览明确显示文件名中的换行等特殊字符，设置成原有归属时不重复修改元数据。

新增管理功能的标签、提示与错误使用英文、简体中文语言资源；原生工具参数、规则身份和机器字段保留原值。后台助手在内存中使用调用方的语言副本。授权查询保留完整查询参数，详情和筛选刷新沿用同一次授权读取方式。

验证：`cargo test --locked -p platform`。Linux 检查在 Linux/WSL 运行；真实授权窗口、服务变更、无线设备、挂载和物理盘行为需要对应测试主机。

固定端口与进程测试使用离线数据核对解析和归属，不创建真实 TCP 监听端口。

连接测试：`cargo test --locked -p platform --test shell_control`，覆盖独立进程发现与大请求收发、慢客户端、断线、超时、错误进程编号和退出清理。Windows 需在正常用户进程运行，附加沙箱身份不能通过当前用户 SID 专用权限。

Linux 日志的共享子进程采集测试同时在 Linux 和 macOS 编译运行，覆盖超时、取消和输出限额；macOS 的公开日志查询仍返回不支持。

macOS 子进程清理遇到 `EPERM` 时会检查原进程组是否已全部退出或成为僵尸；确认没有存活成员后才接受清理完成，避免正常查询被误报为权限失败，真正的权限错误继续保留。
