# Linux 用户会话与提权验证记录

验证时间：2026-09-11 至 2026-09-12，Asia/Shanghai。
测试主机：`ssh x240s-test`，Fedora 43 x86_64，systemd/logind，SELinux Enforcing。
本机开发环境：macOS；仓库在 `master` 上开发并按职责提交。

## 已验证的运行行为

| 范围 | 实际结果 |
| --- | --- |
| 普通终端启动 | SSH 用户 UID/GID 1001 的 UX 四种进程身份均一致；13 个隔离配置/状态文件属于该用户；配置未引用 `/root/`。 |
| root 启动拒绝 | ptrace 观察到一次 shell 自身 exec，零 fork/vfork/clone；在创建 UX 后台任务或终端前退出，未调用 sudo。 |
| PAM 登录 | 使用发行版真实 PAM 栈完成认证、账户检查、凭据建立、会话打开及 pam_systemd/logind 注册；实际桌面 UID/GID 1002，能力集为空，NoNewPrivs 为 1。 |
| 会话环境 | HOME、个人 XDG 目录、`/run/user/UID` 和用户 D-Bus 与登录 UID 一致；显式用户 XDG 配置与 NSS HOME 共同决定个人目录。 |
| 锁屏隔离 | 切换至可信 VT 后，五个预先复制的用户 evdev 句柄均返回 ENODEV，原 DRM 句柄失去 master；不能通过 VT_ACTIVATE、非特权 VT_UNLOCK、TIOCSTI 或原始输入设备绕过。 |
| 解锁 | 错误密码保持 Locked；正确密码恢复 Active，UID 和 logind session ID 不变。 |
| 按需授权 | 普通托管用户 CanRequest 为 false；显式加入 `tundra-admin` 的测试用户为 true。通过真实 libseat/kmscon 鼠标事件链路，取消得到 Cancelled，确认后受限日志请求完成，无第二次密码提示。 |
| D-Bus 请求归属 | root 和独立 SSH 发送者均不能读取或取消另一个真实发送者的待确认操作；非 root 不能拥有生产服务名。 |
| 注销与用户切换 | 旧桌面进程与 PAM/logind 会话清理后才回到 greeter；同 UID 的另一条独立真实 PAM/logind 会话及其进程继续存活。 |
| 正常停止与重启 | systemctl stop 完成用户及 greeter 清理并恢复原 tty1；再次启动成功。密码提示过程中停止，也在约 3 秒内完成取消和恢复。 |
| 维护状态 | 维护标记存在时拒绝新登录，受保护标记移除后恢复登录；崩溃发生在更新日志创建前时，恢复工具清除遗留维护标记。 |
| 旧数据迁移 | 已安装的稳定 helper 通过 22 项实机断言：dry-run 不写入；按目标 UID/GID 导入；排除旧凭据/角色/启动命令；重复导入保留冲突文件和原始来源；拒绝源及目标中间目录符号链接。 |

鼠标测试使用 root 创建的测试 uinput 设备，验证了完整设备、终端、控件事件链路，
不是人工手动点击的证据。桌面登录及迁移使用本次创建的临时账户；普通终端检查使用
已有的 SSH 测试账户 UID 1001。

## 构建与自动测试

- Linux `cargo check --workspace --all-targets --locked -j1` 通过。
- Linux 六个 Rust 可执行文件串行构建通过；私有 kmscon 和静态 libtsm 从固定提交构建。
- Linux sessiond、greeter、privileged、session-protocol、个人目录以及维护工具测试通过。
- 本机 `cargo fmt --all -- --check` 通过；本地化检查验证 1892 个字面调用及 1880 对中英文消息。
- 打包布局的 Python 回归测试通过，打包脚本语法与 CI YAML 解析通过。
- macOS 完整工作区测试命令完成 988 项通过、4 项失败、2 项忽略后，在既有
  system-services 时间同步测试处停止。失败都是等待回环 HTTP 请求的五秒超时。
  其余后续组件独立运行 417 项全部通过。不能将这一结果表述成一次完整测试全绿。
- 相同 system-services 工作区测试二进制在沙箱外，清空测试进程的代理变量并设置
  `NO_PROXY=*` / `no_proxy=*` 后，独立连续两次 29/29 通过；但相同设置的完整 cargo
  工作区运行仍出现上述四项超时。现有证据说明结果对执行环境敏感，尚未确定具体机制。
  没有改动时间同步生产代码、放宽测试断言或修改用户的系统代理设置。

## 安装验证

最终开发 RPM 来自代码提交 `38f28fe45c7d4478e1118870023fbee0469a9c72`，
文件名为 `tundraux3-1.3.0-1.x86_64.rpm`，SHA-256 为：

```text
61f3b75bd799148f1a181bded10c399abc7bbe071bbaef98b1d3c36e8aa4afd6
```

这是未签名的开发验证包，Rust 可执行文件使用剥离调试信息的 debug 构建；
不是已发布、可供受信任在线更新使用的正式制品。本轮本地产物位于
`/tmp/tundra-linux-session-artifacts/`。

- 移除本任务此前手工装入的 SELinux 模块后，RPM 自身成功安装持久文件标签策略。
  六个版本化可执行文件为 `bin_t`，私有 Pango 模块为 `lib_t`，元数据保持数据标签。
  全程保持 Enforcing，没有增加允许规则或切换宽松模式。
- 已安装的稳定维护 helper 在正式特权服务相同的沙箱和能力约束下，修复了新版本
  测试文件的标签；恢复单元清除了事务日志写入之前遗留的受保护维护标记。
  正式特权服务随后成功启动。
- 已安装负载全部属于 root:root，没有组/其他用户可写或 set-ID 文件；终端及模块
  摘要匹配能力清单，已有公共 assets 目录得到保留。
- 使用正式 systemd 单元和版本化可执行文件完成真实管理员登录，创建 UID 1003 的
  logind session 592；greeter 的 NSS HOME 为 `/nonexistent`。通过实际鼠标事件链路
  点击确认后，五条日志请求经过 `AwaitingConfirmation → Running → Completed`，
  随后恢复原用户会话。正常停止移除 session 592 并恢复 tty1。

收尾后主代理通过 SSH 独立回读：SELinux 为 Enforcing，前台 VT 为 1；SDDM 为
active/enabled，`tundra-sessiond` 与 `tundra-privileged` 均为 inactive/disabled。
一次性 UID 1002、1003、1004 账户及 HOME、root-only 测试凭据和
`/run/tundra-integration` 夹具已清理，临时输入及座席服务已停止。
真实用户 UID 1000、1001、已安装 RPM、生产 greeter 账户和 `tundra-admin` 组保留。

## 验证边界

此次未发布官方签名更新，也未伪造证明来跳过验证。因此，完整的在线官方更新成功路径
及跨版本回滚仍需在正式工作流产出可验证制品后验证；当前已验证提取/元数据约束、
事务恢复、真实维护状态解除和服务停止/重新启动。没有实际执行关机或重启。

已提供 Ubuntu/Fedora 打包与 CI，以及 Windows/macOS 的编译和普通 UX 边界检查；
此次物理会话验证限于 Fedora 主机，没有将它推断为 Ubuntu 启动验证或 Windows 实机验证。
独立会话后端目前为 seat0 的可信 tty8 与用户 tty9，尚不是 Wayland compositor。

详细记录：

- [会话架构](session-architecture.md)
- [PAM 与物理座席验证](../../crates/sessiond/README.md)
- [D-Bus 权限负向验证](../../crates/privileged/tests/README.md)
- [普通用户 UX 验证](standalone-session-test.md)
- [更新和迁移](system-maintenance.md)
