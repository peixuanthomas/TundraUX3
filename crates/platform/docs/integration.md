# 平台路径、身份检查与系统集成

## 应用数据路径

| 用途 | Windows | macOS | Linux |
| --- | --- | --- | --- |
| 配置 | `%APPDATA%\TundraUX3\config.toml` | `~/Library/Application Support/TundraUX3/config.toml` | `$XDG_CONFIG_HOME/TundraUX3/config.toml`，默认 `~/.config/TundraUX3/config.toml` |
| 状态 | `%LOCALAPPDATA%\TundraUX3\state` | `~/Library/Application Support/TundraUX3/state` | `$XDG_DATA_HOME/TundraUX3/state`，默认 `~/.local/share/TundraUX3/state` |
| 缓存 | `%LOCALAPPDATA%\TundraUX3\cache` | `~/Library/Caches/TundraUX3` | `$XDG_CACHE_HOME/TundraUX3`，默认 `~/.cache/TundraUX3` |
| 日志 | `%LOCALAPPDATA%\TundraUX3\logs` | `~/Library/Logs/TundraUX3` | `$XDG_STATE_HOME/TundraUX3/logs`，默认 `~/.local/state/TundraUX3/logs` |
| 临时文件 | `%TEMP%\TundraUX3` | 系统临时目录下的 `TundraUX3` | `$XDG_RUNTIME_DIR/TundraUX3`；缺失时为带 UID 的私有 `/tmp` 目录 |

使用 `tundra-cli debug paths` 可同时查看路径模板和解析后的绝对路径。

## Linux 启动身份

Shell 和 CLI 在读存储、恢复更新、启动 watchdog 或进入全屏前，先检查 UID/EUID、GID/EGID 一致。不一致直接拒绝，不自动修复身份或执行 sudo/su。

| 启动身份 | 行为 |
| --- | --- |
| 普通用户 | 直接查询当前 UID 的 NSS 记录，使用其 HOME、Shell、附加组和有效 XDG 环境 |
| root | 警告文件操作和子程序具有 root 权限；stdin/stderr 都是终端时按小写 `y` 才继续，其他输入退出 |
| set-ID 不一致 | 拒绝启动 |

root 确认不需要回车，不保存豁免，管道不能代替；读取后恢复原终端模式。正式运维 CLI 的非交互确认规则见 [CLI 授权](../../cli/docs/operations.md#confirmation-and-authorization--确认与授权)。身份来自 NSS，不依赖环境用户名、用户枚举、UID 范围或 Tundra 角色。

`AuthSession::source` 区分 `LinuxCurrentProcess` 和 `LocalAccount`。Linux 不接受应用内密码登录，不创建 logind 会话，也不提供锁屏、解锁、切换用户或注销系统会话；退出只结束本程序。Windows/macOS 保留本地账户流程。

`linux-uid-<UID>` 只保存个人外观、仪表板和时钟等偏好，历史密码、角色和锁定信息不参与 Linux 权限判断。首次外观设置连同完成标记保存成功后才能进入主页；失败或中断后继续设置。天气为独立应用。

个人设置只核对当前进程用户并读写本地配置，不依赖 AccountsService。系统账户变更、当前账户保护和后端选择集中说明于[用户管理](user-management.md)。Windows/macOS 本地账户写入也经过 AA，批准不能替代 UserService 角色检查；删除后清理时钟资料，删除或停用当前账户后退出登录。

普通用户不读取或迁移 `/root` 的旧数据。确认以 root 运行时使用 root 的 NSS HOME/XDG 路径及 `linux-uid-0`。文件管理和 Command Line 的个人目录读取 NSS HOME 与 `user-dirs.dirs`，支持绝对路径、中文目录和 HOME 禁用约定，不执行配置中的 Shell 命令。缺少 XDG_RUNTIME_DIR、logind 或 D-Bus 不阻止本地界面，相应功能单独报告不可用。

## 系统服务和授权

Linux 的 D-Bus、电源和诊断模块返回明确结果：权限拒绝、授权取消、服务不可用、忙碌、网络错误、连接中断、不支持或结果未知。普通调用和授权等待各有超时，UI 不解析原始 D-Bus 错误字符串。

Shell 通过私有 sudo 授权连接执行固定的管理、账户和电源请求，不接受任意命令。账户后端使用 AccountsService 或固定系统工具，电源使用 logind。其他调用方可通过绑定进程及启动时间的 `pkttyagent` 在独立终端处理 polkit；不接管主终端。

logind 按基础、多会话或抑制器策略授权。系统已允许的电源操作、普通用户修改本人资料与密码保留原有授权及 `passwd` 流程，不额外要求 sudo。管理员提权时复用 Shell 授权连接。通信失败撤销连接；正常返回的服务错误或过期任务不撤销。取消、超时、断线都不自动重放写操作；日志读取不回退为 root。

授权连接、root 控制接口与重新连接要求见 [Linux 管理](linux-management.md#后台任务与网络回退)。便携包不安装系统账户、PAM 配置或系统服务；诊断只检查条件，不自动提权。[程序自更新](../../app/docs/update.md)只替换用户拥有且可写的便携程序，与系统软件包管理分开。

## 桌面与文件集成

| 功能 | Windows | Linux |
| --- | --- | --- |
| 默认程序打开文件/URI | 原生接口 | `xdg-open`，后台回收，不阻塞界面 |
| Launcher | 原生程序和快捷方式 | ELF、AppImage、脚本和已验证的 `.desktop` |
| 剪贴板 | 原生后端 | Wayland data-control 或 X11/XWayland；失败可重连，编辑器仍可接收粘贴 |
| 本地卷和回收站 | 原生后端 | mountinfo/statvfs/sysfs 和 Freedesktop Trash |
| 严重错误 | 平台提示与日志 | 桌面通知、watchdog 文本报告与 stderr |
| 重启/关机 | 原生 API 和系统权限 | logind + polkit，分别检查 CanReboot/CanPowerOff |

Explorer 的卷列表只包含本地固定盘、可移动盘上已挂载的文件系统，过滤网络及伪文件系统。Btrfs 匿名设备号会回溯实际块设备，根目录、home 和其他子卷可分别访问；标签显示设备名及挂载路径。未挂载分区、交换空间和裸物理盘不作为目录入口。

回收站只使用当前用户权限，保留私有权限和防符号链接检查，不存在 root 跨用户特例。失败显示对象与原因，不自动提权，也不退化为永久删除。

普通文本或图片即使有可执行位，仍按文件打开配置处理。ELF、带 `#!` 的脚本、具有可执行位的已知脚本类型、AppImage、EXE 和 `.desktop` 交给 Launcher。缺少桌面助手不会改用 Shell 字符串或 sudo 兜底。

`debug doctor` 检查 Shell、伪终端、`/proc`、`/sys` 及管理命令；D-Bus/polkit/logind 各报告一次，没有图形会话则跳过桌面专用检查。支持范围不含 aarch64、系统镜像或系统会话切换。依赖见 [Linux 运行说明](../../../docs/packaging/linux/README-LINUX.txt)。

## 从 Windows 迁移到 Linux

1. 关闭两端程序，在 Windows 运行 `tundra-cli debug paths` 并备份配置和 state。
2. 在 Linux 运行同一命令，把备份分别复制到其显示的 config 和 state 路径；保留原备份，不合并两个 state 目录。
3. 全局设置可复用，但旧本地账户的外观和时钟不会自动绑定到系统 UID。
4. Windows 绝对路径不会自动转换；Launcher 和最近文件显示 Missing 后，重新选择目标。
