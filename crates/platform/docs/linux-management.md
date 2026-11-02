# Linux 系统管理

从启动器打开服务、进程、软件包、网络、磁盘、用户管理、防火墙、SSH 和计划任务。程序通常以普通用户运行，需要系统权限的操作通过 AutoAdmin（AA）和固定授权助手执行。

## 页面操作

管理页顶部用于搜索、筛选和刷新，主要操作位于右下方，低频操作放入“更多操作”。所有动作都有可见按钮，按钮标注实际快捷键。

| 按键 | 行为 |
| --- | --- |
| S 或 `/` | 输入搜索，Enter 应用，Esc 结束输入 |
| R 或 F5 | 刷新当前列表 |
| F4 | 展开或收起详情 |
| Tab / Shift+Tab | 在导航、列表、详情和操作区之间移动 |
| Ctrl+Enter / Ctrl+U | 提交搜索或表单 / 清空当前文字字段 |
| Ctrl+T | 打开该操作的 AA 输出 |
| F12 | 打开当前页面的任务关闭确认 |
| Esc | 关闭表单、详情或返回 |

表单中 Enter 移到下一栏，选择字段打开选项列表，Ctrl+Enter 直接提交。长列表、详情和表单支持键盘、滚轮和滚动条。

多选行为见[列表多选](../../../docs/list-selection.md)，AA 输入和停止规则见[AutoAdmin 操作流程](../../auto-admin/docs/workflow.md)。

## 支持范围

| 入口 | 主要操作 | 后端与条件 |
| --- | --- | --- |
| 服务 | 启停、重启、重载、启停用、依赖、日志和单元配置 | systemd |
| 进程 | 进程树、信号、优先级、文件、端口和读写指标 | 核对 PID 与启动时间 |
| 软件包 | 查询、安装、卸载、升级、软件源和配置冲突 | APT/dpkg、DNF、pacman |
| 网络 | 地址、路由、DNS、Wi-Fi 和诊断 | Wi-Fi 使用 NetworkManager |
| 磁盘 | 挂载、fstab、健康、inode 和目录扫描 | UDisks/smartctl 与系统文件接口 |
| 用户 | 账户、用户组、Shell、有效期和 SSH 公钥 | NSS、AccountsService 和固定系统工具 |
| 防火墙与端口 | 监听端口、放行规则增删 | UFW 或 firewalld |
| SSH | 有效设置、端口、公钥和登录策略 | OpenSSH 与 systemd |
| 配置备份 | 差异、原件、结果、恢复和清理 | 共享恢复记录 |
| 计划任务 | systemd timer、crontab、暂停、恢复和立即执行 | systemd 与本机 cron |

详细条件见[网络与磁盘](network-disks.md)、[用户管理](user-management.md)、[防火墙](firewall.md)、[SSH](ssh-management.md) 和[计划任务](scheduled-tasks.md)。

## 软件包页面

左侧提供已安装、更新和搜索，默认打开已安装。详情使用本机数据库或仓库查询，主要操作位于详情底部，高级区分别处理软件源、状态修复和配置冲突。

- 查询使用已有仓库信息，需要最新数据时显式刷新。
- 搜索最多显示 2,000 行，可缩小查询范围。
- 写入前重新核对已安装版本、候选版本和完整目标列表。
- 原生依赖确认、配置冲突和安装脚本问题在 AA 终端中显示并接收输入。
- 普通卸载不自动 purge 或 autoremove；APT/DNF 升级不等同于发行版升级。

### Arch / pacman

已安装、搜索、详情和更新查询使用 `/usr/bin/pacman`。仓库查询还需要 `/usr/bin/pacman-conf` 和可用的同步数据库，仓库必须支持相应 Search、Install、Upgrade 用途。

安装、选中升级和全部升级都执行 `pacman -Syu --needed`，指定目标时追加包名。这些操作会升级整个系统，最终版本、依赖和事务由 pacman 在 AA 终端中确认。

普通卸载使用 `pacman -R -- 软件包名`，保留 `.pacsave` 行为。升级后按原生输出检查 `.pacnew`。仓库外软件包可查看和卸载，AUR 构建与助手不在后端范围内。

## 后台任务与授权

关闭仍有操作的页面时，可继续后台运行、取消并关闭或留在页面。需要输入时 AA 会重新显示。退出 Shell 会请求停止当前会话任务并等待清理。

首次内置提权通过 sudo 建立本次 Shell 私有的授权连接，密码验证后清除。退出、注销或连接失效会撤销授权，重启 Shell 后重新授权。

仍在助手中运行的任务可通过“重新连接系统任务”恢复输出和交互。结果未知时先查询实际状态，不重复提交同一写操作。

网络配置和 SSH 加载使用独立恢复任务，需要在 120 秒内确认。网络通过 `network confirm` 确认，SSH 使用 `tundra-cli ssh confirm ID` 并验证真实新连接。

## 验证

```sh
cargo test --locked -p platform management::packages --lib
cargo test --locked -p platform management::packages::query::tests::live_apt_installed_search_updates_and_details --lib -- --ignored
cargo test --locked -p platform management::packages::pacman::tests::live_pacman_installed_search_updates_and_details --lib -- --ignored
```

APT/pacman 的 ignored 用例只读查询本机数据库。真实软件包写入、Wi-Fi、挂载和授权流程在可恢复的隔离环境或测试主机中验收。
