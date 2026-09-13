# Linux 普通用户与 Fedora 更新前端验证记录

验证日期：2026-09-13。工作基线为 `master` 的
`8504a7a9962a6b5d291ba436fbdc62aaa8a5c369`；未切换、合并或修改实验分支，未推送。
实施提交及最终工作树状态见本次交付记录。

## 环境与隔离范围

- 本地 macOS：通用代码、原有本地身份模型、UI 与工作区回归。
- `x240s-test`：实际确认 Fedora 43 KDE、x86_64、普通用户 UID/GID 1001，
  Rust/Cargo 1.98、PackageKit 1.3.6、polkit 126。
- 源码仅同步至 `/home/x240s-test/tundra-validation/ordinary-user-8504a7a`；
  记录待测提交、未提交差异和逐文件 SHA-256。编译与临时文件使用此验证目录。
- RPM 写入、可信测试源、系统认证及软件包安装均在显式命名的 rootless Podman
  容器内进行。没有替换主机日常 Tundra，没有执行主机关机、重启或网络修改。
- 图形认证使用容器中真实 KDE polkit agent、隔离 Xvfb 与 Fedora 登录会话；
  TTY 认证使用真实 PTY 与系统 pkttyagent。没有访问主机用户的图形认证窗口。

## 验证结果

| 范围 | 结果与证据 |
| --- | --- |
| Linux 身份边界 | 普通用户启动、首次 Appearance → Home、再次 Home；伪造 USER/HOME 不改变 NSS 身份；嵌入式 Terminal 的实际 UID/GID、HOME/USER 正确。UID/EUID/GID 不匹配的校验有自动测试；真实 UID 0、sudo 启动和缺失 NSS 记录均被拒绝。 |
| 缺失系统会话服务 | 无 system/session bus 的真实 PTY 仍能启动并完成个人设置；缺失服务仅影响相应能力。 |
| 更新后端隔离 | 未识别安装拒绝替换；正式便携夹具使用实际发布标记并运行新 CLI helper；SystemRpm 与 PortableUser 的禁止串线检查通过。 |
| 真实 PackageKit | 签名本地测试源，固定 Tundra 1.0.0 → 2.0.0；可信依赖预览；安装必要 runtime，保留 unrelated 旧版本。成功必须同时符合 Finished 成功与新 RPM 查询。无候选是正常结果。 |
| 取消与恢复 | 调用官方 Cancel；晚取消没有伪装成回滚。保存非秘密事务线索，模拟丢失最终日志写入并在事务结束后重启 PackageKit，使用新历史查询与 RPM 信息恢复结果，不重放安装。真实事务中途服务断线仍由模拟后端覆盖。 |
| 已有图形 agent | 真实 KDE 认证完成更新，无 TTY 密码提示，实际 RPM 版本符合预期；测试策略观察到两次图形认证。 |
| TTY agent | 成功、Ctrl-C、错误密码、agent 崩溃、Shell 中断、EOF、认证后 RPM 安装失败路径通过；恢复 raw/alternate screen/mouse/focus/cursor 和退出后的 canonical/echo。 |
| 密码与数据归属 | 仅使用隔离测试账户的一次性凭据；它未进入 Shell 输出、用户状态或日志。生成的个人数据属于 UID 1000。Tundra 没有读取系统密码或调用 PAM。 |
| 正式包定义 | 实际 RPM spec 构建、依赖解析、隔离安装、文件归属、pkttyagent 提供包及 root 拒绝检查通过；DEB control/内容与便携标记检查通过。无 Tundra PAM、sudo 运行依赖、系统账户或 session/seat 安装脚本。 |
| 已安装程序诊断 | 普通用户运行安装后的 doctor：UID/EUID/GID/EGID 1000、NSS/HOME/XDG、SystemRpm 和 tundraux3 1.3.1-1 正确，存储与资源检查通过。无 runtime/session bus 或电源授权时显示警告。 |
| UI 复用 | 更新卡片、可滚动预览确认和重启入口复用已有组件与 Theme；新增页面逻辑无 Buffer 绘制、硬编码颜色或同步系统调用。键盘、鼠标命中与焦点沿用共享布局。 |

通用与平台检查：

- `cargo fmt --check`、本地 `cargo check --workspace --locked`：通过。
- macOS 全工作区串行测试：86 个测试目标，1396 通过、0 失败、2 项已有忽略。
- Fedora 全工作区串行测试：86 个测试目标，1452 通过、0 失败、2 项已有忽略。
  两项原生文件／目录 Trash 往返测试随后在独立 XDG 回收站中显式执行，均通过。
- 全量 Fedora 回归后补充了小范围电源授权修正；最终增量通过 51 项 Linux 定向
  测试（含 4 项电源测试）和工作区 `cargo check`。保留全量回归与增量的分别记录。
- 最终程序的 KDE 图形授权、TTY 授权成功、Ctrl-C 取消和明确 polkit NO 规则下的电源拒绝路径通过；
  电源拒绝后 TUI 恢复，没有发生关机。
- 最终原生 Shell 构建通过。标准 PTY smoke 通过：64 个鼠标事件后的单字符哨兵
  在 0.011 秒内显示（门限仍为 0.25 秒），随后完成 Appearance → Home，并验证
  SIGTERM 后完整 termios、备用屏幕、鼠标和光标恢复。
- 本地化检查：1906 个调用点、1896 对 en-US／zh-CN 消息通过。
- workflow YAML、Unix shell 脚本语法、weathr 依赖边界和 `git diff --check`：通过。

## 可重复脚本与证据

- `scripts/tests/packagekit-fixture.py`：签名的固定 RPM、依赖与不相关包。
- `scripts/tests/authorization-pty.py`：真实 Shell、系统代理与 PTY 验证。
- `scripts/tests/package-artifacts.py`：正式包定义及已安装 RPM 验证。
- `scripts/tests/README.md`：独立容器、操作顺序、前置依赖和复位要求。
- `scripts/linux-shell-smoke.py`：64 个鼠标事件后的键盘优先级及终端退出恢复。

原始日志保留于测试机 `/home/x240s-test/tundra-validation`：
`final-native-workspace-check.log`、`final-native-workspace-tests.log`、
`authorization-final-success.log`、`authorization-final-gui.log`、
`auth-pty-{cancel,denied,crash,interrupt,eof,package_error}.log`、
`packagekit-real-{preview,execute,current,recovery}.log`、
`final-package-artifact-check.log`、`final-installed-rpm-doctor.log`、
`final-native-power-tests.log`、`final-native-delivery-check.log`、`final-native-trash.log`、
`final-native-pty.log`、`final-authorization-{power_denied,cancel,success,gui}.log`。
日志不包含真实用户凭据。

## 验证边界与已知情况

- 软件包内容验证使用原生构建后 strip 的 debug 程序，不能据此声称完成了
  release profile 构建；DEB 只做内容检查，没有在 Fedora 上冒充 Debian 运行验证。
  发布工作流经 YAML 与 shell 语法检查，没有触发远程发布或 CI。
- 本轮没有 Windows 实机，没有执行真实关机／重启。电源请求只通过固定 logind
  接口，测试覆盖授权挑战、拒绝、取消与不确定结果时不得重放的约束。
- macOS 默认并行测试曾出现现有 system-services 的四个本地 HTTP 等待超时；
  全量串行运行通过。单独设置 NO_PROXY 不能消除全部并行失败，不能把代理
  配置认定为唯一原因；未修改这些系统时间测试或业务代码。
- PTY 哨兵最初将首次保存与 Home 初始化计入输入门限；改为独立测量已有输入框中的
  普通字符。多字符原始输出匹配又会被 Ratatui 增量绘制误导，最终采用初始画面中
  不存在的单字符标记，保留 250 毫秒门限并另行断言首次设置完成。
- 原有 logind 信号监听线程在部分退出日志中仍记录 watchdog 关闭超时；
  进程退出与终端恢复检查通过。本轮未重构该既有后台线程生命周期。
- rootless 容器中为 polkit 配置了测试专用服务隔离覆盖；没有把该覆盖放入
  应用包，也没有把容器验证等价于主机 systemd 服务沙箱验证。

logind 按基础、多会话及抑制器状态选择自己的授权动作；前端仅依其明确的
`InteractiveAuthorizationRequired` 响应启动 fallback，最多重发同一请求一次。
依据为 systemd v258 的
[logind 授权选择](https://github.com/systemd/systemd/blob/v258/src/login/logind-dbus.c)及
[polkit 响应处理](https://github.com/systemd/systemd/blob/v258/src/shared/bus-polkit.c)。
