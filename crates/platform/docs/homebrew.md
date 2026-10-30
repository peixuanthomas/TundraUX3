# macOS Homebrew 软件包管理

macOS 的“启动器 → 软件包”复用已有的已安装、更新、搜索和详情页面，支持 Homebrew Formula（命令行软件与库）及 Cask（macOS 应用）。需先安装 Homebrew；程序先查找 `PATH`，再检查 Apple Silicon 常用的 `/opt/homebrew/bin/brew` 和 Intel 常用的 `/usr/local/bin/brew`。

| 页面操作 | brew 接口 |
| --- | --- |
| 已安装 | `brew info --json=v2 --installed` |
| 更新 | `brew outdated --json=v2` |
| 搜索 | 分别使用 `brew search --formula`、`brew search --cask` |
| 详情 | `brew info --json=v2 --formula/--cask -- <name>` |
| 安装、卸载、升级所选包 | `brew install/uninstall/upgrade --formula/--cask -- <name>` |
| 全部升级 | `brew upgrade` |
| 高级功能中的刷新软件源 | `brew update` |

Formula 与 Cask 即使同名也使用独立标识，提交前再次读取版本，避免对旧选中项继续操作。仅接受包名及 `owner/tap/name`，不接受任意选项、路径、网址或 Ruby 文件。锁定版本的包禁用所选升级操作，全部升级遵循 brew 的原生规则。

查询和普通操作设置 `HOMEBREW_NO_AUTO_UPDATE`，元数据更新由刷新按钮单独执行；同时关闭操作附带的自动清理和自动移除孤立依赖。查询设有取消、时间及输出大小限制，搜索结果最多显示 2000 项。搜索空白时先提示输入名称，避免读取整个包目录。

所有写操作经过现有确认和 AA 策略，以当前 macOS 用户运行，拒绝 root、set-ID 和其他用户身份。启动写操作时移除已被 Homebrew 禁用的 `HOMEBREW_ASK`，并继续移除 `HOMEBREW_NO_ASK`，使用默认的原生确认行为。原生确认及 Cask 安装程序的系统密码通过独立 PTY 输入，不记录输入内容。可在程序运行期间切换页面或继续后台运行；停止和超时后的强制结束沿用 AA。退出整个程序时由现有子进程监督负责清理，不能通过 Linux 的后台任务 socket 重连。

macOS 不提供 Linux 的软件源文件编辑、配置冲突和数据库修复操作；这些按钮不会出现在 Homebrew 快照中。本次接入的是 Shell 软件包模块，公开 `tundra-cli packages` 运维入口仍只支持 Linux。

实现使用 [Homebrew 官方 JSON v2 接口](https://docs.brew.sh/Querying-Brew)及[原生命令](https://docs.brew.sh/Manpage)。离线回归：

```sh
TMPDIR=/private/tmp cargo test --locked --offline -p platform management::packages --lib
```

测试使用临时 brew 夹具，覆盖类型与版本身份、固定命令、失败与无结果、Formula/Cask 及全部升级时禁用旧变量并保留默认确认、隐藏密码、停止及强制停止，不安装或卸载主机软件。真实 Cask 安装程序的系统授权仍需在对应软件上验收。
