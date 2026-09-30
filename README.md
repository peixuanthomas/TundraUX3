# TundraUX3

TundraUX3 是一个使用 Rust 编写的终端桌面环境实验项目。它以完整的 TUI 会话整合主页、文件管理、应用启动、纯文本编辑、设置、诊断和通知；Windows/macOS 还保留应用内本地账户与锁屏。

项目基于 [Ratatui](https://ratatui.rs/) 与 [crossterm](https://github.com/crossterm-rs/crossterm)，面向 Windows 11、macOS 和 Linux 桌面环境。当前仍处于实验阶段，建议在兼容 crossterm 的真实终端中体验。

## 功能概览

- 首次启动语言、时区与外观设置；Linux 附着当前系统用户，Windows/macOS 使用本地账户登录与角色
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

**[TundraUX3 技术说明](docs/README-TECHNICAL.md)**

项目说明文档统一存放在 [docs](docs/)；组件说明、测试指南和发布记录保留原目录层级：

- [语言资源与恢复](docs/LOCALIZATION.md)与 [i18n crate](docs/crates/i18n/README.md)
- [Watchdog 运行与恢复约定](docs/crates/watchdog/README.md)
- [UI 图形资源说明](docs/crates/ui/assets/README.md)
- [Linux 更新测试指南](docs/scripts/tests/README.md)
- [Linux 验证记录](docs/packaging/linux/)与[发布记录](docs/packaging/releases/)

## 许可证

Copyright (c) 2026 HanPeixuan。项目自身代码采用 [GNU GPL v3](LICENSE)（仅第 3 版，`GPL-3.0-only`）。Weathr 组件保留 [GPL-3.0-or-later](crates/weathr/LICENSE.weathr)；分发或再使用时请同时检查对应组件及第三方资源的许可要求。

### Linux 普通用户会话

先通过 Fedora 正常登录，再在该用户的终端中启动 Tundra。Linux 身份唯一来源是
当前进程 UID 和 NSS；Shell 和 CLI 拒绝 UID/EUID 不一致及 GID/EGID 不一致的启动。
以 root 运行时先显示权限风险警告，必须在交互终端按小写 `y` 才继续（无需回车）；
其他按键或无交互终端均退出。建议日常使用普通用户运行。
首次运行进入 Appearance，完成后到 Home；之后直接进入 Home。Exit 只退出 Tundra。
账号、密码、系统登录和系统授权由 Fedora 管理，Tundra 不提供内部 Linux 登录、锁屏、
切换用户或注销系统会话，也不自动执行 sudo。

Linux 后续只提供便携包，不再通过包管理器更新。在“设置 → 更新”中切换更新模式：
默认正式版下载 GitHub Releases 上的 Linux 便携包并校验后替换；测试版按 master 最新提交哈希
检查更新，下载源码并用本机 Rust 编译后替换。两种模式均保留首次启动失败回退和中断恢复。
模式会保存，个人数据和主题不参与替换。更新目录必须属于当前用户、可写且保留便携标记；
旧系统包安装不会被自动改写。正式版不需要 Rust，测试版需要先安装 Rust 和项目构建依赖。
依赖、数据目录和验证说明见 [Linux 运行说明](docs/packaging/linux/README-LINUX.txt)。

运行日志与诊断导出请参阅 [Logs APP 使用与存储说明](docs/LOGS.md)。
