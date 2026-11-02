# 平台路径、身份与系统集成

## 应用数据路径

| 用途 | Windows | macOS | Linux |
| --- | --- | --- | --- |
| 配置 | `%APPDATA%\TundraUX3\config.toml` | `~/Library/Application Support/TundraUX3/config.toml` | `$XDG_CONFIG_HOME/TundraUX3/config.toml`，默认 `~/.config/TundraUX3/config.toml` |
| 状态 | `%LOCALAPPDATA%\TundraUX3\state` | `~/Library/Application Support/TundraUX3/state` | `$XDG_DATA_HOME/TundraUX3/state`，默认 `~/.local/share/TundraUX3/state` |
| 缓存 | `%LOCALAPPDATA%\TundraUX3\cache` | `~/Library/Caches/TundraUX3` | `$XDG_CACHE_HOME/TundraUX3`，默认 `~/.cache/TundraUX3` |
| 日志 | `%LOCALAPPDATA%\TundraUX3\logs` | `~/Library/Logs/TundraUX3` | `$XDG_STATE_HOME/TundraUX3/logs`，默认 `~/.local/state/TundraUX3/logs` |
| 临时文件 | `%TEMP%\TundraUX3` | 系统临时目录下的 `TundraUX3` | `$XDG_RUNTIME_DIR/TundraUX3`，缺失时使用带 UID 的私有 `/tmp` 目录 |

运行 `tundra-cli debug paths` 查看路径模板和解析后的绝对路径。

## Linux 启动身份

Shell 和 CLI 在访问存储或进入全屏前检查 UID/EUID 和 GID/EGID。

| 启动身份 | 行为 |
| --- | --- |
| 普通用户 | 读取当前 UID 的 NSS 记录，使用其 HOME、Shell、附加组和 XDG 环境 |
| root | 显示权限警告，stdin/stderr 都是终端时按小写 `y` 继续 |
| set-ID 不一致 | 拒绝启动 |

身份来自 NSS，不由 USER、LOGNAME、Tundra 角色或账户枚举决定。Linux 使用 `LinuxCurrentProcess`，Windows/macOS 使用 `LocalAccount`。

Linux 的 `linux-uid-<UID>` 保存个人外观、仪表盘和时钟等偏好。首次外观设置保存完成后进入主页，中断后下次继续。普通用户不读取 `/root` 的旧数据；确认 root 运行时使用 root 的 NSS HOME 和 `linux-uid-0`。

个人目录读取 NSS HOME 与 `user-dirs.dirs`，支持中文、空格和 HOME 禁用约定。缺少 XDG_RUNTIME_DIR、logind 或 D-Bus 时，本地界面仍可打开，相关系统功能单独报告不可用。

## 系统服务与授权

Linux 管理、账户和电源请求通过固定接口执行：

- 管理写入使用 Shell 私有的授权连接，接口不接受任意命令。
- 账户使用 AccountsService 或固定系统工具，电源使用 logind。
- 其他调用方可通过绑定进程和启动时间的 `pkttyagent` 处理 polkit。
- 普通用户修改本人资料与密码沿用系统授权和 `passwd`。
- 取消、超时和断线不会自动重放写操作。

D-Bus 操作区分权限拒绝、授权取消、服务不可用、忙碌、网络错误、连接中断和结果未知。详细连接规则见[授权与助手连接](authorization.md)，程序更新见[便携更新](../../app/docs/update.md)。

## 桌面与文件集成

| 功能 | Windows | Linux |
| --- | --- | --- |
| 默认程序打开文件或 URI | 原生接口 | `xdg-open`，后台执行 |
| Launcher | 原生程序和快捷方式 | ELF、AppImage、脚本和已验证的 `.desktop` |
| 剪贴板 | 原生后端 | Wayland data-control 或 X11/XWayland |
| 本地卷与回收站 | 原生后端 | mountinfo/statvfs/sysfs 和 Freedesktop Trash |
| 严重错误 | 平台提示与日志 | 桌面通知、watchdog 报告与 stderr |
| 重启/关机 | 原生 API | logind + polkit |

卷列表包含本地固定盘和已挂载的可移动文件系统，过滤网络与伪文件系统。Btrfs 子卷回溯实际块设备，标签显示设备名和挂载路径。

回收站使用当前用户权限并检查私有目录和符号链接。普通文本或图片即使带可执行位，也按文件打开配置处理；ELF、脚本、AppImage、EXE 和 `.desktop` 交给 Launcher。

运行 `tundra-cli doctor` 检查终端、`/proc`、`/sys`、管理命令、D-Bus、polkit 和桌面集成。依赖见 [Linux 运行说明](../../../docs/packaging/linux/README-LINUX.txt)。

## 从 Windows 迁移到 Linux

1. 关闭两端程序，在 Windows 运行 `tundra-cli debug paths` 并备份配置和 state。
2. 在 Linux 运行同一命令，将备份分别复制到显示的 config 和 state 路径，保留原备份。
3. 全局设置可复用；旧本地账户的外观和时钟不会自动绑定到 Linux UID。
4. Windows 绝对路径需要重新选择，Launcher 和最近文件会先显示 Missing。
