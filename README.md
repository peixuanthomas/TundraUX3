# TundraUX3

TundraUX3 是一个使用 Rust 编写的终端桌面环境实验项目。它以完整的 TUI 会话整合主页、文件管理、应用启动、纯文本编辑、设置、诊断和通知；Windows/macOS 还保留应用内本地账户与锁屏。

项目基于 [Ratatui](https://ratatui.rs/) 与 [crossterm](https://github.com/crossterm-rs/crossterm)，面向 Windows 11、macOS 和 Linux 桌面环境。当前仍处于实验阶段，建议在兼容 crossterm 的真实终端中体验。

## 功能概览

- 首次启动外观设置；Linux 附着当前系统用户，Windows/macOS 使用本地账户登录与角色
- Settings 提供声音、显示与亮度、Wi-Fi 和蓝牙的界面框架；目前显示不可用提示，尚未接入系统控制或设备查询
- 独立天气应用、时钟、通知中心和可定制外观
- Spring 卡片界面、柔和入场和弹簧进度动画；支持减少动态效果
- 文件管理器、应用启动器与内嵌命令行
- 从 Launcher 打开的纯文本编辑器；Markdown 文件按原文编辑，不做预览或格式解析
- 跨平台存储、系统集成、诊断和故障恢复
- 后台任务监督、异常报告与终端安全恢复

## 快速开始

需要支持 Rust 2024 edition 的稳定版 Rust 与 Cargo。默认资源建议终端至少为 `108 × 20`。

```console
cargo build --locked -p shell -p cli
cargo run --locked -p shell --bin tundra-shell
```

查看命令行工具：

```console
cargo run --locked -p cli --bin tundra-cli -- --help
cargo run --locked -p cli --bin tundra-cli -- debug doctor
```

## 详细文档

架构、crate 分工、运行流程、平台适配、数据存储、测试和打包说明请阅读：

**[TundraUX3 技术说明](README-TECHNICAL.md)**

## 许可证

项目根目录代码采用 [MIT License](LICENSE)。Weathr 组件另带 [GNU GPL v3 许可文本](crates/weathr/LICENSE.weathr)；分发或再使用时请同时检查对应组件及第三方资源的许可要求。

### Linux 普通用户会话

先通过 Fedora 正常登录，再在该用户的终端中启动 Tundra。Linux 身份唯一来源是
当前进程 UID 和 NSS；Shell 和 CLI 拒绝 UID/EUID 不一致及 GID/EGID 不一致的启动。
以 root 运行时先显示权限风险警告，必须在交互终端按小写 `y` 才继续（无需回车）；
其他按键或无交互终端均退出。建议日常使用普通用户运行。
首次运行进入 Appearance，完成后到 Home；之后直接进入 Home。Exit 只退出 Tundra。
账号、密码、系统登录和系统授权由 Fedora 管理，Tundra 不提供内部 Linux 登录、锁屏、
切换用户或注销系统会话，也不自动执行 sudo。

Fedora RPM 安装版在设置中通过 PackageKit 检查并更新已安装的 `tundraux3` 及必要依赖；
安装前展示事务预览。正式的用户可写便携版使用独立的用户级更新流程。
源码构建、无法确认归属的安装，以及本阶段的 Debian 系统安装版显示更新不可用。
依赖、数据目录和验证说明见 [Linux 运行说明](packaging/linux/README-LINUX.txt)。

运行日志与诊断导出请参阅 [Logs APP 使用与存储说明](LOGS.md)。
