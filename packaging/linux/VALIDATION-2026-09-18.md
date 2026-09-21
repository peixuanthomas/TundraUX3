# Linux 编辑器崩溃与 RPM 更新重启验证

验证日期：2026-09-17 至 2026-09-18（Asia/Shanghai）。
初始主分支为 `master`，工作树干净，基线提交为
`bda731b8645f5c22e983dfdac2c620e23e445fcf`。
编辑器修复提交为 `eca6c4ef5ec14c96d39563d0dbd59a29d66c748c`；
RPM 重启与正常退出修复提交为 `0da9c5da772888f25440cc3d6a3e6391c8f57062`。
未创建开发分支，未推送。

## 环境和证据目录

- 本地 macOS，Rust/Cargo 1.97.1。
- `ssh x240s-test`：Fedora 43 KDE、x86_64、Rust/Cargo 1.98.0，普通用户 UID 1001。
- 独立测试根目录：
  `/home/x240s-test/agent-tasks/editor-crash-bda731b-20260917`，以下路径均相对此目录。
- `source/` 保存待测源码；`target/`、`release-target/` 保存独立构建产物。
  原服务器仓库和构建缓存没有覆盖。最终 `source/` 位于上述 RPM 修复提交，工作树干净，
  `tux3-final-source-manifest.json` 校验全部 613 个跟踪文件 SHA-256 一致。
- 更新迁移前，RPM 数据库记录 `tundraux3-1.3.1-1.x86_64`，但 Shell/CLI 已被手动替换，
  实际报告提交 `8504a7a9962a6b5d291ba436fbdc62aaa8a5c369`，`rpm -V` 报告二进制不一致。
- `installation-backup/installed-payload.tar` 备份原二进制、资源、desktop entry、文档和 PAM 文件；
  同目录保存原 RPM 元数据、文件列表、依赖、二进制散列及构建身份。没有改写原用户数据。

## 编辑器根因与回归

原始证据是 root 用户状态目录中的
`crash-20260917T154621.855Z-run-5068-1789659960889098190-7.txt`，
panic 为 `byte_slice(): Invalid byte range 109..0: start must be <= end`。
调用链经过 `SourceBuffer::viewport_line`、编辑器视图转换和鼠标指针处理。

非 ASCII 文档横向滚动越过短行时，没有字素进入可视范围，起点回退到行末，
终点却仍在行首，最终把倒置字节区间交给 Rope。修复使不可见短行返回合法的空区间，
并维护对应的列坐标。

- 原始代码在本地和 Fedora 均复现精确的 `109..0` panic。
- Fedora GDB 捕获 `line={start:0,end:109}`、`left_column=108`、
  `visible_start=109`、`visible_end=0`；证据在 `baseline-gdb.log`。
- 新测试覆盖中文短行、其他行含 Unicode 的 ASCII 短行、emoji/组合字符、空行、
  LF/CRLF/CR、行末及超出行末滚动、`usize::MAX` 和非首行偏移。
- Shell 回归覆盖键盘横向滚动、Home 恢复以及滚动条鼠标拖动，检查磁盘内容不变。
- 真实 Linux PTY 输入中文短行和长行，执行横向滚动及鼠标操作，通过；
  64 个鼠标事件后的字符响应为 0.011 秒（门限 0.25 秒），SIGTERM 后终端恢复，
  无 watchdog 事故。证据：`update-editor-pty-assets.log`。

## 更新时发现并修复的问题

RPM 更新会解除旧可执行文件的目录项，Linux 的 `current_exe()` 随后返回带
` (deleted)` 的路径。旧实现点击重启后尝试执行该路径，报 ENOENT 并退出。
现在在启动时保存可执行文件原路径，更新后通过同一路径 exec 新文件，保留前台进程组。
Linux 回归测试实际重命名替换正在运行的测试程序，再验证 exec 成功。

另一个已存在的问题是 logind 监听线程阻塞等待信号，不能响应 watchdog 关闭。
现有托管线程 API 增加协作取消令牌，logind 连接、订阅、空闲等待与重连延迟均可取消。
单元测试覆盖 handle、任务组和整个进程三种取消入口；终端测试增加退出后事故检查。

Fedora rootless Podman 隔离环境使用真实 PackageKit、polkit、pkttyagent 和 `/bin/login`，
测试 RPM 使用容器内临时签名密钥。主机软件源和信任密钥没有修改。

| 测试 | 结果和证据 |
| --- | --- |
| 认证、安装、重启 | `verified-update-success.log`：真实认证完成 `1.0.0-1 → 2.0.0-1`，点击重启回到新 Home；运行进程的设备号/inode 等于已安装文件，正常退出且无 watchdog 事故。 |
| RPM 事务范围 | `verified-update-rpm.txt`：Tundra 2.0.0-1、必要依赖 tundra-runtime 1.0.0-1、无关 tundra-unrelated 保持 1.0.0-1。 |
| 取消认证 | `verified-update-cancel.log`：在真实系统密码提示处 Ctrl-C，保持 Tundra 1.0.0，终端恢复，无 watchdog 事故。 |
| 用户数据与凭据 | 更新脚本检查隔离状态文件属于普通测试用户；仅使用夹具凭据，并验证其不进入 Shell 输出或状态日志。 |

成功容器为 `tundra-editor-update-verified-20260918`，取消容器为
`tundra-editor-update-cancel-fixed-20260918`。验证使用最终 Rust 修复内容的原生 debug
程序，经 strip 后装入签名测试 RPM；它的编译身份仍为提交前的工作树，因此与最终
release RPM 的构建身份分别记录。`scripts/tests/authorization-pty.py` 已保存完整重启检查，
不能仅以安装提示或终端残留的 Home 文字判断重启成功。

## 真实主机迁移与安装后验证

用户明确选择保留 RPM 安装并验证 PackageKit 流程。通过
`scripts/package-linux.sh --rpm` 完成 release 构建，生成
`dist/tundraux3-1.3.1-1.x86_64.rpm`，SHA-256 为
`d35f4999f1056558556831765fb6ef91307c7234b1583bd939117dc65acb7c9c`。
这是本地构建的未签名 RPM，没有发布到远程源；上面的隔离更新夹具使用签名 RPM。

先用 DNF `--assumeno reinstall` 检查事务，确认只有 Tundra 一个包；随后以 `dnf -y reinstall`
执行迁移。项目包版本仍为 1.3.1-1，因此使用重装，实际代码版本通过构建提交确认。

- `host-rpm-install.log`：事务成功，仅重装 Tundra。
- `host-rpm-verify.log`：`rpm -V tundraux3` 返回 0、输出为空；Shell/CLI 均由该 RPM 管理。
- `host-shell-build.txt`、`host-cli-build.txt`：版本 1.3.1、协议 2、提交
  `0da9c5da772888f25440cc3d6a3e6391c8f57062`，分别报告 `dirty=false` 和 `state=clean`。
- `host-installed-smoke.log`：**原版** PTY smoke 通过，使用默认启动等待和 250 毫秒键盘门限，
  64 个鼠标事件后的字符响应 0.001 秒，终端完整恢复，正常退出不产生 watchdog 事故。
- `host-installed-editor-pty.log`：已安装 release 程序的中文短行、长行横向滚动及鼠标回归通过，
  键盘响应 0.001 秒，退出无 watchdog 事故。
- `host-installed-update-page.log`：通过真实 PTY 进入 Settings → Update，识别为 `SystemRpm`，
  显示已配置源不提供更新的 Tundra 包；`tux3-host-update-pty.frame.txt` 保存最终页面。
  此检查正常退出并恢复终端，没有产生 watchdog 事故。主机未配置 Tundra 发布源，
  不能把这个正常的无候选结果称为已从真实发布源更新成功。

## 最终 Fedora 工作区检查

- 原生 `cargo check --workspace --locked` 通过，证据：`update-check.log`。
- 最终提交的 `cargo test --workspace --locked -- --test-threads=1` 完整通过：
  **86 个测试目标，1459 通过、0 失败、2 项已有忽略**。忽略项是原有原生 Trash 往返测试，
  本次没有修改其实现或运行这些忽略项。
- 全量日志为 `final-tests-linux.log`，`final-tests-linux.exit` 为 `0`；
  包含 Unicode 视口、Shell 横向滚动、运行中程序被替换后的重启、三种线程取消入口等新回归。
- 服务器采用 `CARGO_BUILD_JOBS=1` 限制构建资源。早期并发链接带来高磁盘负载，
  已停止仅属于本次验证的旧构建；最终全量运行使用非 PTY 的 SSH 标准输入，
  避免 CLI doctor 测试在后台交互式任务中被 SIGTTIN 挂起。上述中止运行没有计为通过。
- 最终源码仍为干净的 `0da9c5d`；此文档提交不改变已测试代码或已安装 RPM 的构建身份。

## 验证范围和过程中保留的失败记录

- 图形认证和真实电源操作不在本次范围；容器的 polkit 服务隔离覆盖仅用于测试，
  不改变主机服务。没有重启、关机或改动主机网络。
- 早期默认并发 macOS 测试有四个已有 system-services 回环 HTTP 等待超时；
  编辑器修复后的串行全量运行曾通过 1398 项、0 失败、2 项已有忽略。
  RPM 修复后的首次串行运行又遇到 `active_metrics_preserve_pending_time_sync_request`
  的 5 秒请求等待超时，记录在 `/tmp/tux3-update-tests-macos.log`，没有修改该业务代码或测试。
  随后使用 `--no-fail-fast -- --test-threads=1` 完成所有 86 个测试目标，1398 通过、1 失败、
  2 项已有忽略；失败项为另一未改动的 `failed_time_server_validation_shows_error_and_does_not_save`
  后台任务等待超时，记录在 `/tmp/tux3-update-tests-macos-retry.log`。不能将本轮全量运行报告为零失败。
  该 `shell --test settings` 测试目标随后单独串行重跑，16 项全部通过，见
  `/tmp/tux3-final-settings-macos.log`。本地 `cargo fmt --check`、
  `cargo check --workspace --locked` 和差异检查通过。
- Fedora 未安装 rustfmt；格式检查在本地执行。
- 调试构建的早期标准 PTY smoke 超过启动等待上限；临时编辑器测试只将启动等待放宽至
  45 秒，保留 250 毫秒的键盘门限。另一次测试使用了未完成链接的旧产物，后续按完成的
  构建重新验证。独立 strip 程序最初读取主机旧语言资源而弹出资源兼容提示，复制同版
  资源后验证通过。上述失败日志保留，没有计为通过。
- 没有为主机配置 Tundra RPM 软件源。新版 SystemRpm 更新使用已配置源中版本更高的
  RPM，不会跟随 GitHub 提交编译安装；未来自动更新需要发布更高 RPM EVR 并提供可信源。
  隔离环境的完整更新成功不能代替真实发布源的可用性。
