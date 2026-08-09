# TundraUX3

TundraUX3 是一个使用 Rust 编写的终端桌面环境实验项目。它以完整的 TUI 会话整合锁屏、登录、主页、文件管理、应用启动、Markdown 编辑、设置、诊断和通知等功能。

项目基于 [Ratatui](https://ratatui.rs/) 与 [crossterm](https://github.com/crossterm-rs/crossterm)，面向 Windows 11、macOS 和 Linux 桌面环境。当前仍处于实验阶段。实验性 bundle 会自带受控 WezTerm；源码开发则仍需要在兼容 crossterm 的真实终端中运行。

## 功能概览

- 首次启动配置、账户登录、角色权限与登录保护
- 天气锁屏、时钟、通知中心和可定制外观
- 文件管理器、应用启动器与内嵌命令行
- 支持源码/富文本视图的 Markdown 编辑器
- 跨平台存储、系统集成、诊断和故障恢复
- 后台任务监督、异常报告与终端安全恢复

## 使用与快速开始

安装或解压实验性发行物后，双击 `tundra.exe`（Windows）、`TundraUX3.app`（macOS）或 Linux 应用图标/`tundra`。它会直接打开包内 WezTerm 并进入 Tundra Shell；不需要先打开终端或输入 `./tundra`。

从源码开发需要支持 Rust 2024 edition 的稳定版 Rust 与 Cargo。默认资源建议终端至少为 `108 × 20`：

```console
cargo build -p launcher -p shell -p cli -p recovery
cargo run -p shell --bin tundra-shell
```

`tundra-shell` 是开发入口；它不是 bundle 的用户入口。实验性 bundle 的构建、私有运行时布局与发行限制见 [bundled runtime 文档](docs/bundled-runtime.md)。

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
