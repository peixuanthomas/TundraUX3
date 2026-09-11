# TundraUX3

TundraUX3 是一个使用 Rust 编写的终端桌面环境实验项目。它以完整的 TUI 会话整合锁屏、登录、主页、文件管理、应用启动、纯文本编辑、设置、诊断和通知等功能。

项目基于 [Ratatui](https://ratatui.rs/) 与 [crossterm](https://github.com/crossterm-rs/crossterm)，向独立 Linux 桌面会话发展。Windows 11 和 macOS 保留 UX 展示及普通应用功能，不提供系统登录、提权或深层系统管理。当前仍处于实验阶段；直接运行 UX 时请使用普通用户和兼容 crossterm 的真实终端。

## 功能概览

- Linux 真实用户会话、独立可信登录界面与按需授权；其他平台保留本地 UX 账户展示
- 天气锁屏、时钟、通知中心和可定制外观
- Spring 卡片界面、柔和入场和弹簧进度动画；支持减少动态效果
- 文件管理器、应用启动器与内嵌命令行
- 从 Launcher 打开的纯文本编辑器；Markdown 文件按原文编辑，不做预览或格式解析
- 跨平台个人存储、普通文件与应用操作、诊断和故障恢复
- 后台任务监督、异常报告与终端安全恢复

## 快速开始

需要支持 Rust 2024 edition 的稳定版 Rust 与 Cargo。默认资源建议终端至少为 `108 × 20`。

```console
cargo build -p shell -p cli
cargo run -p shell --bin tundra-shell
```

查看命令行工具：

```console
cargo run -p cli --bin tundra-cli -- --help
cargo run -p cli --bin tundra-cli -- debug doctor
```

## 详细文档

架构、crate 分工、运行流程、平台适配、数据存储、测试和打包说明请阅读：

**[TundraUX3 技术说明](README-TECHNICAL.md)**

## 许可证

项目根目录代码采用 [MIT License](LICENSE)。Weathr 组件另带 [GNU GPL v3 许可文本](crates/weathr/LICENSE.weathr)；分发或再使用时请同时检查对应组件及第三方资源的许可要求。

### Linux 系统用户会话

直接启动 `tundra-shell` 时使用当前系统用户，拒绝 root 桌面；UX、文件操作、内置终端和普通应用共同使用该用户的 UID、用户组、HOME 与个人目录。UX 不调用 sudo，也不通过选择用户名模拟换用户。

独立桌面由显式启用的 `tundra-sessiond` 管理：完整 PAM 登录注册 logind 会话，随后降权启动用户桌面。锁屏与解锁保留原会话；注销关闭 PAM 并清理该会话，切换用户先注销。系统操作交给 root `tundra-privileged`，只有 root 策略允许的管理员组成员，经过独立可信界面的确认，才可执行列明的操作。

安装软件包不会替换或启动显示管理器，也不会自动授予管理员组成员资格。独立会话使用随包构建、固定源码提交且启用 libseat 的私有 kmscon；系统自带的 kmscon 不作为可信后端。架构与实际验收边界见 [Linux 会话架构](docs/linux/session-architecture.md)，依赖和启用说明见 [Linux 运行说明](packaging/linux/README-LINUX.txt)。

运行日志与诊断导出请参阅 [Logs APP 使用与存储说明](LOGS.md)。
