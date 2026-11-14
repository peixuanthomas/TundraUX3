# TundraUX3

**在终端里管理设备的集成工作区。**

[English](../README.md) | 简体中文

TundraUX3 把系统工具、文件、编辑器和日常实用程序整合到一个终端界面中，主要面向没有图形桌面的 Linux 设备，支持键盘、鼠标和触摸屏。项目使用 Rust、Ratatui 和 crossterm 构建。

## 主要功能

- **Linux 系统管理：**管理服务、进程、软件包、网络、磁盘、用户、防火墙、SSH 和计划任务，查看配置历史并恢复更改。
- **跨平台软件包：**使用 Linux 原生包管理工具、macOS 的 Homebrew 或 Windows 的 WinGet 搜索、安装、卸载和升级软件。
- **文件与编辑：**浏览目录、跨页面复制与剪切文件、撤销同一文件系统内的移动和重命名、管理权限，使用语法着色、查找替换、软换行和 JSON 格式化。按 F8 在内置编辑器和 Vim 之间切换。
- **开发工具：**在内嵌终端中运行程序，使用断点、单步、变量和调用栈调试 Bash 脚本。
- **系统动态：**查看资源占用、日志和通知，跟踪后台任务及其输出。
- **个人工作区：**按名称或命令搜索启动器、整理自定义项目，选择主题和语言，使用时钟与天气应用。
- **独立应用窗口：**每个窗口都有边框和最小化、全屏、分屏、关闭按钮，从底部窗口栏恢复后台应用，按平分或网格显示多个窗口。独立 watchdog 监测主界面和应用进程，应用无响应时仍能直接终止。
- **终端界面：**导航、列表选择、弹窗和加载反馈参照 OpenCode，沿用 TundraUX3 的 Glacier Night 深蓝配色。

| 平台 | 主要支持 |
| --- | --- |
| Linux | 设备操作与系统管理 |
| macOS | 终端工作区与 Homebrew 软件包管理 |
| Windows | 终端工作区与 WinGet 软件包管理 |

## 快速开始

使用至少 **108 列 × 20 行**的交互式终端。鼠标和触摸输入需要终端转发指针事件。

窗口上边缘提供 **最小化、全屏、分屏、关闭** 按钮。点击底部窗口栏中的名称即可切换或恢复应用；分屏菜单可以选择布局，再加入已有窗口或新建应用。普通应用按 Esc 先关闭弹窗或菜单，再最小化。原生终端和 Vim 保留 Esc，使用 **Ctrl+B，再按 B** 最小化。同一前缀后按 **W** 打开高级窗口管理、**Q** 正常关闭，按大写 **X** 立即强杀。可显示的窗口数量随终端尺寸和所有运行中应用的最大最小尺寸变化，后台应用也参与计算。后台运行只持续到本次 TundraUX3 退出，退出时统一清理。

### 使用 Linux 便携包

1. 从 [GitHub Releases](https://github.com/peixuanthomas/TundraUX3/releases) 下载并解压 Linux x86_64 压缩包。
2. 保持解压后的文件在同一目录，在该目录打开终端。
3. 启动程序：

```sh
./tundra-shell
```

检查运行环境：

```sh
./tundra-cli doctor
```

日常使用普通用户运行。需要管理员权限的操作会在执行前请求授权。

### 从源码构建

安装 **Rust 1.95 或更新版本**。构建随包 Vim 还需要 **Python 3.12 或更新版本**和本机 C 构建工具链。

在仓库根目录执行：

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
cargo run --locked -p shell --bin tundra-shell
```

Windows 没有 `python3` 命令时可使用 `python`。更新源码后同时重新构建 Shell 和 CLI；`packaging/vim.json` 变化时重新构建 Vim。

检查源码构建的运行环境：

```sh
cargo run --locked -p cli --bin tundra-cli -- doctor
```

## 配套 CLI

`tundra-cli` 提供诊断、配置、日志、系统管理命令和运行中界面的控制功能：

```sh
tundra-cli --help
tundra-cli doctor
tundra-cli watchdog list
```

需要从另一个终端控制界面时，先在**设置 → 系统 → 外部 CLI 控制**中开启该功能，然后使用：

```sh
tundra-cli ui snapshot
tundra-cli ui wake
```

点击、文字输入、滚动和自动化示例见 [CLI 控制指南](../crates/cli/docs/ui-control.md)。

## 文档

- [English README](../README.md)
- [构建、架构、测试与打包](README-TECHNICAL.md)
- [Linux 系统管理](../crates/platform/docs/linux-management.md)
- [编辑器与脚本调试](source-editor.md)
- [CLI 命令与运维操作](../crates/cli/docs/operations.md)
- [文档索引](README.md)

## 许可证

项目代码采用 [GPL-3.0-only](../LICENSE)。随包第三方组件保留各自的许可证。
