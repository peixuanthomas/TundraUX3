# macOS Homebrew 软件包管理

macOS 的“启动器 → 软件包”支持 Homebrew Formula（命令行软件与库）和 Cask（macOS 应用），复用已安装、更新、搜索和详情页面。

使用前需要安装并初始化 Homebrew。程序依次查找 `PATH`、`/opt/homebrew/bin/brew` 和 `/usr/local/bin/brew` 中的可执行文件。

## 操作与 brew 接口

| 页面操作 | brew 命令 |
| --- | --- |
| 已安装 | `brew info --json=v2 --installed` |
| 更新 | `brew outdated --json=v2` |
| 搜索 | `brew search --formula` / `brew search --cask` |
| 详情 | `brew info --json=v2 --formula/--cask -- <name>` |
| 安装、卸载、升级所选包 | `brew install/uninstall/upgrade --formula/--cask -- <name>` |
| 全部升级 | `brew upgrade` |
| 刷新元数据 | `brew update` |

Formula 与 Cask 使用不同的目标标识，同名包不会混淆。提交前重新读取版本，仅接受包名和 `owner/tap/name`，锁定版本的包不提供所选升级。

查询和普通操作设置 `HOMEBREW_NO_AUTO_UPDATE`，元数据只通过刷新按钮更新。自动清理和孤立依赖移除默认关闭，搜索最多显示 2,000 项。

## 确认与任务

写操作经过确认和 AA，以当前 macOS 用户运行，使用 Homebrew 原生确认。Cask 安装程序的系统密码通过独立 PTY 输入，输入内容不写入历史。

任务运行时可切换页面或继续后台运行，停止和强制结束沿用 AA 与子进程监督。Homebrew 页面不提供 Linux 的软件源编辑、配置冲突或数据库修复操作，`tundra-cli packages` 仍只面向 Linux。

实现使用 [Homebrew JSON v2](https://docs.brew.sh/Querying-Brew) 和[原生命令](https://docs.brew.sh/Manpage)。

## 验证

```sh
TMPDIR=/private/tmp cargo test --locked --offline -p platform management::packages --lib
```

测试使用临时 brew 夹具，覆盖类型与版本身份、固定命令、失败结果、原生确认、隐藏密码和停止流程。真实 Cask 安装与系统授权在 macOS 测试环境验收。
