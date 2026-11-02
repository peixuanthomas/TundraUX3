# TundraUX3

**在终端中管理设备的工作空间。**

[English](../README.md) | 简体中文

TundraUX3 将系统工具、文件、编辑器和日常实用工具整合到一个终端界面中。它面向没有图形桌面的 Linux 设备，支持键盘、鼠标和触摸屏操作，使用 Rust 编写，基于 Ratatui 和 crossterm。

## 功能

- **Linux 系统管理：**管理服务、进程、软件包、网络、磁盘、用户、防火墙规则、SSH 和定时任务。查看配置历史并恢复之前的更改。
- **跨平台软件包：**使用 Linux 原生软件包工具、macOS 上的 Homebrew 或 Windows 上的 WinGet 搜索、安装、卸载和升级软件。
- **文件与编辑：**浏览目录、管理权限，使用语法高亮、查找与替换、软换行和 JSON 格式化功能编辑文本。按 F8 在内置编辑器和 Vim 之间切换。
- **开发工具：**在内嵌终端中运行程序，使用断点、单步执行、变量和调用栈调试 Bash 脚本。
- **系统运行情况：**监控资源使用，查看日志和通知，跟踪后台任务及其输出。
- **个人工作空间：**整理自定义应用启动器，选择主题和语言，使用内置时钟和天气应用。

| 平台 | 主要支持 |
| --- | --- |
| Linux | 设备操作与系统管理 |
| macOS | 终端工作空间与 Homebrew 软件包管理 |
| Windows | 终端工作空间与 WinGet 软件包管理 |

## 快速开始

请使用至少 **108 列 × 20 行**的交互式终端。鼠标和触摸输入需要终端支持转发指针事件。

### 使用 Linux 便携包

1. 从 [GitHub Releases](https://github.com/peixuanthomas/TundraUX3/releases) 下载并解压 Linux x86_64 压缩包。
2. 将解压后的文件保留在一起，并在该目录打开终端。
3. 启动应用：

```sh
./tundra-shell
```

检查运行环境：

```sh
./tundra-cli doctor
```

请以普通用户身份运行 TundraUX3。管理操作会在需要时请求授权。

### 从源码构建

安装 **Rust 1.95 或更新版本**。构建随包提供的 Vim 还需要 **Python 3.12 或更新版本**及本机 C 构建工具链。

在仓库根目录执行：

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
cargo run --locked -p shell --bin tundra-shell
```

在 Windows 上，如果 `python3` 不可用，请使用 `python`。拉取更新后，重新构建 Shell 和 CLI；当 `packaging/vim.json` 变化时，重新构建 Vim。

从源码构建后运行环境检查：

```sh
cargo run --locked -p cli --bin tundra-cli -- doctor
```

## 使用界面

从**启动器**打开应用。常用操作以按钮形式显示在各页面底部，无需记住快捷键也能使用界面。

| 输入 | 操作 |
| --- | --- |
| 方向键 / Tab | 在项目和控件之间移动 |
| Enter / Space | 打开、确认或切换 |
| Esc / 返回按钮 | 返回或关闭当前弹窗 |
| F1 | 显示当前页面的帮助 |
| F2 | 整理启动器项目 |
| F4 | 配置启动器中外部程序的运行方式 |
| 编辑器中的 F8 | 在普通编辑器和 Vim 之间切换 |

在仪表盘中，方向键根据卡片的可见位置移动，支持大小不同和错位排列的布局。

鼠标和触摸屏用户可以点击或点按控件、滚动列表、拖动滚动条。文本输入框可使用屏幕键盘。

在启动器中打开**任务**，可查看正在运行的工作、读取已保存的输出、返回相关页面或取消任务。语言、外观、动画、天气和应用选项均可在**设置**中调整。

## 配套 CLI

`tundra-cli` 提供诊断、配置、日志、系统管理命令，以及对运行中界面的控制：

```sh
tundra-cli --help
tundra-cli doctor
```

如需从另一个终端控制界面，请启用**设置 → 系统 → 外部 CLI 控制**，然后使用以下命令：

```sh
tundra-cli ui snapshot
tundra-cli ui wake
```

点击、输入、滚动和自动化示例见 [UI 控制指南](../crates/cli/docs/ui-control.md)。

## 文档

- [English README](../README.md)
- [构建、架构、测试与打包](README-TECHNICAL.md)
- [Linux 系统管理](../crates/platform/docs/linux-management.md)
- [编辑器与脚本调试](source-editor.md)
- [CLI 命令与操作](../crates/cli/docs/operations.md)
- [文档索引](README.md)

## 许可证

项目代码采用 [GPL-3.0-only](../LICENSE) 许可证。随包提供的第三方组件保留各自的许可证。
