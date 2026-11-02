# CLI 命令与交互

## 程序入口

```sh
tundra-shell
tundra-cli <command> [options]
```

`tundra-shell` 不带参数进入全屏 UI，`--help` 和 `--version` 输出查询结果。`tundra-cli` 提供独立命令，也可通过 `ui` 向已开启外部控制的 Shell 发送正常输入。

Shell/CLI 的帮助和版本查询不启动 watchdog、不读取配置，也不进入全屏界面。

## 公开命令

| 命令 | 作用 |
| --- | --- |
| `doctor` | 检查系统、终端、权限、路径、存储和资源 |
| `config` / `config get [field]` | 查看全部或指定公开配置 |
| `config set <field> <value>` | 修改公开配置 |
| `config reset <field>` | 恢复一个字段的默认值 |
| `config options [field]` | 列出字段的可选值 |
| `launcher [list]` | 列出固定应用及状态 |
| `launcher pin <path>` | 检查并固定可执行目标 |
| `launcher unpin <id>` | 移除固定项，保留应用文件 |
| `ui …` | Shell 会话、快照和输入控制，见 [UI 控制](ui-control.md) |
| `services` / `processes` / `packages` / `network` / `disks` | Linux 管理命令 |
| `users` / `system-config` / `operations` | 账户、配置和任务命令 |
| `firewall` / `ssh` / `backups` / `scheduled-tasks` | 防火墙、SSH、备份和计划任务 |
| `logs …` | 日志查询、跟随和导出 |
| `repl` | 启动交互命令行，`exit` 或 EOF 退出 |
| `debug asset …` | 查看、渲染或原样输出嵌入资源 |
| `debug paths` / `debug explain` | 查看解析路径或启动边界说明 |
| `debug ui-test` / `debug screen-keyboard` | 共享控件和屏幕键盘预览 |
| `debug view-ui-style [1\|2\|3]` | 界面与动画样式预览 |
| `debug test-rainbow-text` | 彩虹文字预览 |
| `debug test-frost` / `debug test-matrix` | 启动画面预览 |
| `debug test-aa-style1/2/3` | AA 样式演示 |
| `debug test-watchdog-error/critical/panic` | 主动生成故障测试 |
| `cls` | 清空终端历史和可见内容 |
| `help [command …]` | 查看命令帮助 |
| `new` | 重置 Tundra 用户配置和状态 |

使用 `tundra-cli <command> --help` 或 `tundra-cli help <command>` 查看准确参数。调试预览见 [Shell 调试预览](../../shell/docs/previews.md)。

## 配置

`config set` 支持：

- `automation.enabled`：`true` / `false`，控制外部 CLI 输入。
- `border-shape`、`border-color`、`accent-color`、`icon-mode`：外观。
- `motion`：`full` / `reduced`。
- `animation-speed`：50–200 之间 25 的倍数，默认 100。
- `language`、`timezone`、`address`、`weather-location`：语言、时区和天气地点。
- `update-mode`：Linux 的 `release` / `beta`。

`theme` 是只读摘要。通过 CLI 修改设置后重启现有 Shell；设置页中的外部控制开关会立即生效。`weather-location` 使用 `auto` 或 `config reset weather-location` 恢复按时区选择。

常用示例：

```sh
tundra-cli config set timezone Asia/Shanghai
tundra-cli config set border-shape rounded
tundra-cli config set accent-color "#38bdf8"
tundra-cli config set motion reduced
tundra-cli config set animation-speed 125
tundra-cli config set weather-location "Shanghai, China"
tundra-cli launcher pin "/home/user/My App/run.sh"
```

Launcher 的 CLI 与 UI 共用目标检查，拒绝缺失文件、符号链接和不支持的目标。REPL 中的相对路径跟随当前会话目录。

## 命令识别与补全

Command Line 和独立 `repl` 默认执行系统命令，Tundra UX 命令使用 `/`：

```sh
ls -la
/help
/config set motion reduced
```

普通输入与 UX 命令同名时仍按系统命令执行。失败后可提示适用的 `/` 前缀，但不会自动改执行为另一命令。Unix 绝对路径可直接输入；根目录下的单个可执行路径可用引号包住。

Tab 查看或接受补全，右方向键接受光标后的候选。拼写纠错只显示建议，不改变输入。系统候选来自常见命令、Git/Cargo 子命令和后台 PATH 扫描，扫描只读取文件名与元数据。

`export PATH`、`set PATH`、`set PATHEXT` 或 `cd` 后刷新查找位置，旧扫描结果不会覆盖新位置。PATH 扫描在后台执行，输入线程只读取缓存。

## 会话目录与环境

系统命令会话保留工作目录和已导出的环境变量：

```sh
export PROJECT_MODE=dev
cd "/path/with spaces"
echo "$PROJECT_MODE"
```

Windows 对应 `set PROJECT_MODE=dev`、`cd /d "C:\path with spaces"`。UX 命令不会清空这些状态，Tundra 主进程和系统环境配置也不会改变。

Linux/macOS 首次执行时选择有效 Shell，后续命令沿用会话状态；Windows 使用 cmd.exe。未导出的局部变量、函数、别名和 Shell 选项不跨命令保存，脚本需要修改当前环境时使用 `. script` 或 `call script.cmd`。

每个嵌入 Command Line 页面拥有独立子终端和目录。提示符显示当前绝对路径，命令完成标记保留在滚动历史中；正常 `exit` 关闭当前页面，返回按钮只清理当前实例。

## 诊断与故障测试

`doctor` 检查必需运行条件，并报告可选桌面、系统命令和图形协议能力。可选功能缺失显示 WARN，必需条件失败返回 1；路径模板通过 `debug paths` 查看。

`debug test-watchdog-error` 和 `critical` 生成主动测试报告。`debug test-watchdog-panic` 触发真实故障流程，独立 CLI 由最外层 watchdog 收尾，嵌入 Command Line 通过内部退出码请求 Shell 显示 panic 页面。

## 数据重置

`new` 会删除用户配置和状态，执行前先用 `debug paths` 确认并备份目录，确认时精确输入 `RESET`。

`repl --embedded` 是 Command Line 专用入口。嵌入 CLI 通过退出码 75 请求 Shell 完成终端恢复、任务收尾、存储重置和重启。
