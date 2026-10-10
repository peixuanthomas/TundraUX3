# CLI 命令与交互

## 入口、配置与 REPL

```console
tundra-shell
```

`tundra-shell` 不接收任何命令行参数，包括 `--help`；传入任何参数都会以参数错误退出。它固定进入全屏 UI，应用选择与 Editor 文件打开只能从 UI 发起。

`tundra-cli` 是独立的运维工具，可读取和修改公开配置，但不能向 Shell 传参或绕过 UI 打开 Editor：

```console
tundra-cli <command> [options]
```

| 命令 | 作用 |
| --- | --- |
| `debug asset` / `debug asset <name>` | 显示资源帮助或渲染指定资源；TOML art set 会输出全部图案。 |
| `debug asset <name> -a` | 原样输出完整资源文件，包括 TOML 元数据。 |
| `debug asset <name> --<item>` | 只输出 TOML 资源中的项目，例如 `home_icons --launcher`。 |
| `cls` | 清空终端历史和可见内容，并将光标移到左上角。 |
| `config` | 查看全部公开配置。 |
| `config get [field]` | 查看外观、动画、语言、时区、天气地点和 Linux 更新模式；省略字段列出全部公开设置。 |
| `config set <field> <value>` | 设置 `border-shape`、`border-color`、`accent-color`、`icon-mode`、`motion`、`animation-speed`、`language`、`timezone`、`address`、`weather-location` 或 `update-mode`；`theme` 仅为只读摘要。 |
| `config reset <field>` | 只恢复指定字段的默认值，保留其他配置。 |
| `config options [field]` | 列出取值说明；`language` 列出已安装语言，`timezone` / `address` 列出支持的时区和城市。 |
| `launcher [list]` | 列出固定应用的 ID、状态和文件路径。 |
| `launcher pin <path>` | 检查并固定可执行文件，支持带引号的相对或绝对路径；不会启动应用，重复路径不重复添加。 |
| `launcher unpin <id>` | 按 `launcher list` 中的 ID 移除固定项，保留应用文件。 |
| `debug doctor` | 检查系统、终端、权限、应用路径、存储和资源；实际探测 Kitty、Sixel、iTerm2 图形协议。 |
| `debug explain` / `debug paths` | 输出启动/边界说明，或输出路径模板和解析路径。 |
| `repl` | 交互命令循环；`exit` 或 EOF 退出，普通输入交给固定系统命令解释器并显示退出码，`/<command>` 执行 UX 命令。外部 CLI 调用不加 `/`。 |
| `debug test-frost` / `debug test-matrix` | 仅播放启动 frost banner 或首次运行 Matrix banner。 |
| `debug view-ui-style [1\|2\|3]` | 不带数字时列出样式；带数字时进入交互 UI / 动画对比预览，不保存设置。 |
| `debug screen-keyboard` | 外部终端中的英文屏幕键盘与 AA 输入演示；不连接应用表单。 |
| `debug test-aa-style1` / `debug test-aa-style2` / `debug test-aa-style3` | AA 弹窗样式演示：三种方案都会压暗背景，分别使用居中双线警戒框、宽幅警戒条、带 AA 标识的分栏授权面板；按 B 对比背景遮罩前后。C/R/F 查看确认、执行中、结束状态；Tab/方向键与鼠标选择按钮，Enter 使用，Esc 退出。仅模拟，不执行命令、不保存样式；正式 AA 已采用 style3 的视觉效果，并保留真实操作提示和随内容收缩的布局。 |
| `debug` / `debug help` | 查看所有调试命令。 |
| `debug test-watchdog-error` | 主动生成普通错误报告。 |
| `debug test-watchdog-critical` | 主动生成严重错误报告。 |
| `debug test-watchdog-panic` | 触发真实 panic，进入正常故障处理流程；当前命令行会话终止。 |
| `services` / `processes` / `packages` / `network` / `disks` / `users` / `system-config` / `operations` | 正式管理命令，见 [运维 CLI](operations.md)。 |
| `help [command ...]` | 显示对应命令帮助，例如 `help config`、`help debug doctor`；也支持 `config --help`、`config set -h`、`launcher help`、`logs export --help`。 |
| `new` | 清除已保存的 TundraUX3 数据，重新创建初始存储。 |

调试命令统一使用 `debug` 前缀，不支持 `sudo` 前缀；原顶层调试命令和 `weathr` 命令已移除。Command Line 中输入 `/debug test-frost`；外部终端使用 `tundra-cli debug test-frost`。

## 配置与固定应用

设置通过当前操作系统用户的配置文件保存，不修改系统时间或系统账号。`icon-mode` 接受 `ascii` / `image`，`motion` 接受 `full` / `reduced`，`animation-speed` 接受 50–200 的 25 倍数，默认 100。`address` 沿用旧行为，会改变 Tundra 时区；`weather-location` 单独保存最多 120 字符的英文地址，`auto` 或 `config reset weather-location` 恢复按时区选择天气地点。`update-mode` 仅在 Linux 接受 `release` / `beta`，只保存更新选择，不立即下载或安装。已运行的 UI 需要重启以加载 CLI 保存的设置和 Launcher 固定项。

Launcher 的 CLI 和 UI 共用目标检查：拒绝缺失文件、不支持的目标、符号链接和重解析点；CLI 的添加记录标记为 `cli`，遵循当前操作系统用户对配置文件的写权限，不伪造 UI 登录状态。`/launcher pin` 的相对路径在 REPL 中跟随 `cd` 后的目录。

## 命令识别

Command Line 和独立 `repl` 默认执行系统命令，例如 `ls -la`、`dir`。UX 命令需要 `/` 前缀，例如 `/help`、`/config set motion reduced`；外部调用仍使用 `tundra-cli config set motion reduced`。普通输入即使与 UX 命令同名，也先按系统命令执行；失败后如果名称像 UX 命令，提示加 `/`，不自动执行 UX 命令。带 `/` 的未知 UX 命令如果像系统命令，提示去掉 `/`；识别常见解释器内置命令、Unix 变量赋值、当前会话 `PATH` 中的可执行文件和相对可执行路径，Windows 同时使用 `PATHEXT`，只检查名称和文件，不尝试执行猜测的命令。拼错的 UX 子命令继续显示对应帮助。Unix 绝对可执行路径可直接输入 `/usr/bin/ls`；开头的路径包含后续 `/` 时按系统命令执行，只有一个 `/` 的根目录可执行文件需用引号包住路径，如 `'/my-tool'`。

## 诊断

doctor 不再逐条输出固定的平台能力声明、重复的 Linux 架构/授权检查和各系统路径模板；模板仍可通过 `debug paths` 查看。Linux 增加伪终端读写、进程/内存数据可读性，以及 `systemctl`、`journalctl`、`ip`、`nmcli`、`lsblk`、`findmnt`、`df`、`pkexec`、`gio` 检查。命令存在仅说明已安装，不保证对应服务运行或用户获准执行操作。可选功能缺失显示 `WARN`，不让整体失败；必需运行条件、目录或存储检查失败返回 1。目录检查会创建并移除探测文件，原有存储检查可能初始化或恢复文档；不会安装软件或修改服务。

## 会话目录、环境与提示符

Command Line 和独立 `repl` 在当前会话内保留系统命令的**已导出环境变量和工作目录**。提示符使用 `user@绝对路径 >> command`；每次读取下一条输入前更新路径，执行 `cd` 后立即显示新目录，切换失败时保留实际目录。尚未执行系统命令时显示子 CLI 的启动工作目录；无法读取目录时以 `?` 标明未知。路径中的中文和空格原样显示，换行及终端控制字符转为可见的转义文字。Linux/macOS 例如先执行 `export PROJECT_MODE=dev`、`cd "/path/with spaces"`，后续 `echo "$PROJECT_MODE"`、`pwd` 和相对路径操作沿用修改后的状态；`unset PROJECT_MODE` 会移除变量。Windows 对应 `set PROJECT_MODE=dev`、`cd /d "C:\path with spaces"`、`echo %PROJECT_MODE%`，用 `set PROJECT_MODE=` 删除变量。中间执行 `/help` 等 UX 命令不会清空这些状态。

嵌入式 Command Line 在每条提示符左侧显示命令状态：输入中或等待执行结果时为 `○`，退出码为 0 时为主题强调色 `●`，非零退出码、命令解析失败或取消输入时为主题错误色 `×`。标记随命令保留在终端滚动历史中；重绘和换行不会将结果移到下一条命令。普通命令文字及输出继续使用原有颜色，`/cls` 同时清除历史标记。独立 `repl` 不输出这套嵌入式状态协议。

嵌入式 Command Line 的用户名和路径使用打开页面时的主题强调色，`>>` 和用户输入保持普通文字颜色，系统退出码使用灰色。每条命令完成或取消输入后，下一条提示符前增加一行空白；空输入不额外增加间距。Command Line 的启动说明、命令前缀提示及页面提示只显示英文，路径和系统命令输出仍保留原文。

系统命令的环境和目录状态由 `platform::SystemCommandSession` 持有，只传递给本会话的下一条系统命令，不修改 Tundra 主进程的环境或目录，也不更改系统环境变量配置；离开 Command Line、退出 REPL 或重启应用后释放。Unix 首次执行仍读取 `/bin/sh` 的登录默认设置，后续执行不重新加载登录配置，避免覆盖用户修改的 `PATH`。Unix 用私有匿名文件采集 NUL 分隔的环境和独立目录数据；Windows 保留 cmd 命令提示符语法，通过临时脚本和 UTF-16 快照采集 `set`、`cd` 的结果，临时文件随调用结束清理。命令输出继续使用原有 PTY。

Windows 输入在 cmd 命令组中执行，使条件命令的采集始终位于条件体之外；包含括号、`&` 等特殊字符的值应加引号，例如 `set "PATH=C:\my tools;%PATH%"`。`for` 仍使用命令提示符的 `%i` 写法。

普通失败也会保留失败前已经完成的环境和目录修改；失败的 `cd` 保持原目录。若强制终止、Unix `exec`/覆盖 `EXIT` trap，或 Windows `exit` 等操作跳过采集，则显示警告，下一条命令使用上一次完整状态。该功能保存的是环境和目录，未导出的 shell 局部变量、函数、别名及 shell 选项不跨命令保存；独立子进程本来也不能改变父 shell 的环境，脚本如需修改当前命令环境应使用 Unix 的 `. script` 或 Windows 的 `call script.cmd`。

## 调试预览与主动故障测试

样式和屏幕键盘演示见 [Shell 调试预览](../../shell/docs/previews.md)。

两个错误报告测试在 CLI 进程中生成报告，内容明确标注为主动测试，并输出 JSON 和文本报告路径；成功返回 0，写入失败或等待超时返回非零状态。`debug test-watchdog-panic` 不在命令内部捕获：独立 CLI 由最外层 watchdog 捕获、显示严重错误提示并退出；嵌入 Command Line 则以内部退出码 76 请求 Shell 主循环真正触发 panic，恢复终端后直接显示全屏 panic 页面，不经过天气锁屏或登录页面，也不弹出 critical 提示框。

## 命令示例

资源与配置示例：

```console
tundra-cli debug asset banner
tundra-cli debug asset explorer_icons
tundra-cli debug asset explorer_icons -a
tundra-cli debug asset explorer_icons --folder
tundra-cli debug asset home_icons --launcher
tundra-cli debug asset launcher_icons --builtin.command-line
tundra-cli debug asset house

tundra-cli config
tundra-cli config get timezone
tundra-cli config set timezone Asia/Shanghai
tundra-cli config set border-shape rounded
tundra-cli config set border-color light-cyan
tundra-cli config set accent-color "#38bdf8"
tundra-cli config set motion reduced
tundra-cli config set animation-speed 125
tundra-cli config set weather-location "Shanghai, China"
tundra-cli config reset weather-location
tundra-cli config set update-mode release
tundra-cli launcher pin "/home/user/My App/run.sh"
tundra-cli launcher list
tundra-cli help config
```

资源名可使用 `debug asset` 帮助列出的完整键，也可用唯一文件名，例如 `house`、`clock_font`。资源、文件或 TOML 条目不存在时会写入 stderr 并返回非零状态。

## 数据重置

`config` 不暴露身份字段，`theme` 为只读摘要。`new` 会删除用户配置和状态，执行前应先运行 `tundra-cli debug paths` 并备份。执行 `new` 必须精确输入 `RESET`；`repl --embedded` 是仅供 Command Line 使用的内部入口。嵌入 CLI 不会自行删除正在使用的数据，而是以退出码 `75` 通知 Shell；Shell 统一恢复终端、释放子进程与后台任务、关闭 watchdog、重置存储并重启，再回到首次设置。
