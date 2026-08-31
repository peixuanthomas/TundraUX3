# TundraUX3

TundraUX3 是一个使用 Rust 编写的终端桌面环境实验项目。它以完整的 TUI 会话整合锁屏、登录、主页、文件管理、应用启动、纯文本编辑、设置、诊断和通知等功能。

项目基于 [Ratatui](https://ratatui.rs/) 与 [crossterm](https://github.com/crossterm-rs/crossterm)，面向 Windows 11、macOS 和 Linux 桌面环境。当前仍处于实验阶段，建议在兼容 crossterm 的真实终端中体验。

## 功能概览

- 首次启动配置、账户登录、角色权限与登录保护
- 天气锁屏、时钟、通知中心和可定制外观
- 文件管理器、应用启动器与内嵌命令行
- 从 Launcher 打开的纯文本编辑器；Markdown 文件按原文编辑，不做预览或格式解析
- 跨平台存储、系统集成、诊断和故障恢复
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
cargo run -p cli --bin tundra-cli -- doctor
```

## 详细文档

架构、crate 分工、运行流程、平台适配、数据存储、测试和打包说明请阅读：

**[TundraUX3 技术说明](README-TECHNICAL.md)**

## 许可证

项目根目录代码采用 [MIT License](LICENSE)。Weathr 组件另带 [GNU GPL v3 许可文本](crates/weathr/LICENSE.weathr)；分发或再使用时请同时检查对应组件及第三方资源的许可要求。
