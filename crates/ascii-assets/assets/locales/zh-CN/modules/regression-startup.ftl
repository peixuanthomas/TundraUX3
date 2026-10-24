startup-time-sync-fallback = { $reason }。正在使用上次校准的时间；尚未校准时使用系统时间。可在“设置 → 日期与时间”中重试。
startup-update-error-explanation = 更新未能完成。请检查网络连接、安装目录写入权限，以及所选更新模式的运行要求。原始详情：{ $details }
startup-account-source = 账户来源
startup-check-pass = 通过/PASS
startup-check-warning = 警告/WARN
startup-check-fail = 失败/FAIL
startup-diagnostics-status-explanation = 状态说明：PASS/Pass 表示通过；WARN 表示可选功能受限；FAIL 表示必要检查失败；Unavailable 表示当前环境无法使用该功能或数据源。
startup-temperature-unavailable = 温度不可用：未检测到传感器
startup-battery-unavailable = 电池不可用：未检测到电池
startup-shell-ui-automation-help = UI automation（供 agent 使用）：
    agent 可以通过 tundra-cli ui 查看运行中 Shell 的界面并操作内部控件。
    先在“设置 → 系统 → 外部 CLI 控制”中开启此功能（默认关闭）。
    也可在启动 Shell 前运行 tundra-cli config set automation.enabled true。
    在同一操作系统用户的另一个终端中运行：
    { "  " }tundra-cli ui snapshot
    { "  " }tundra-cli ui click --id TARGET_ID
    { "  " }tundra-cli ui text "TEXT"
    将 TARGET_ID 替换为 snapshot 中的控件 ID；文字会输入到当前焦点控件。
    用户操作后需重新执行 snapshot 或 wait 获取信息；动画和控件自动更新不影响此检查。
    用 tundra-cli ui wake 关闭空闲天气或天气锁屏。
    点击、拖动、滚动、按键、文字输入和等待的完整用法见 tundra-cli ui --help。
startup-cli-help = TundraUX3 命令行
    { cli-administration-root-usage }
    { "  " }config [get|set|reset|options]  设置
    { "  " }doctor                        检查运行环境
    { "  " }launcher [list|pin|unpin]       应用快捷入口
    { "  " }services                      系统服务
    { "  " }processes                     运行中的进程
    { "  " }packages                      软件包、更新和软件源
    { "  " }network                       网络连接
    { "  " }disks                         磁盘和挂载点
    { "  " }users                         账户和用户组
    { "  " }firewall                      端口和防火墙规则
    { "  " }ssh                           SSH 设置和密钥
    { "  " }backups                       配置备份与恢复
    { "  " }scheduled-tasks               定时任务
    { "  " }system-config                 系统配置文件
    { "  " }operations                    操作状态与授权
    { "  " }logs [query|incidents|export|follow]  日志与诊断
    { "  " }ui [sessions|snapshot|click|move|drag|scroll|key|text|wait]  操作 Shell
    { "  " }debug <command>               调试工具，见 debug --help
    { "  " }cls                           清空终端
    { "  " }new                           重置已保存的 TundraUX3 数据
    { "  " }repl                          交互命令行
    { "  " }help [command ...]            命令帮助
    { "  " }-h, --help                    帮助
    { "  " }-V, --version                 版本
    使用 <command> --help 查看子命令和选项。
startup-cli-debug-help = 用法：tundra-cli debug <command>
    { "  " }ui-test  { ui-test-summary }
    { "  " }test-rainbow-text  { rainbow-text-summary }
    screen-keyboard：试用英文 QWERTY 屏幕键盘，文字仅临时保存。
    test-aa-style1 / test-aa-style2 / test-aa-style3：预览三种 AA 授权警告样式。
    clear-logs <all|TYPE|--file PATH> [--yes]：预览或清理运行、故障、快照及旧日志。
    view-ui-style [1|2|3]：对比可交互的界面样式及动画。
    asset：输出测试资源或原始资源文件。
    paths：输出已配置和已解析的应用路径。
    explain：说明 CLI 启动流程及系统/UI 分工。
    test-frost / test-matrix：仅播放启动或首次启动的横幅动画。
    test-watchdog-error / test-watchdog-critical：主动生成错误或严重错误报告。
    test-watchdog-panic：触发真实 panic，丢弃当前会话。
startup-cli-config-help = 用法：tundra-cli config [get [field] | set <field> <value> | reset <field> | options [field]]
    不带参数时输出设置；reset 只重置指定字段；theme 是只读颜色摘要。
    字段与取值：
    automation.enabled：true | false（默认 false；本机 Shell 控制）
    border-shape：rounded | square
    border-color：default | 颜色名（如 light-cyan）| #RRGGBB
    accent-color：default | 颜色名 | #RRGGBB
    icon-mode：ascii | image（image 需要终端图形支持）
    motion：full | reduced
    animation-speed：50..200，步长 25，默认 100（百分比）
    language：已安装的语言代码；config options language 列出选项。
    timezone / address：时区 ID 或城市名；config options timezone 列出选项。
    weather-location：带引号的英文地址（最多 120 字符）| auto（跟随时区位置）
    update-mode：release | beta（仅 Linux；保存选项，不启动更新）
    address 更改时区；weather-location 只更改天气位置，不更改系统时间。
    示例：
    tundra-cli config set motion reduced
    tundra-cli config set animation-speed 125
    tundra-cli config set weather-location "Shanghai, China"
    tundra-cli config reset weather-location
    tundra-cli config set accent-color "#38bdf8"
    修改当前操作系统用户的配置；运行中的 TundraUX3 需重启后生效。不提供身份或密码字段。
startup-cli-packages-help = list [--scope installed|search|updates] 列出已安装、搜索或可更新的软件包；search TEXT 搜索。
    source-add --name NAME --url URL 添加软件源；APT 另需 --suite SUITE --components main --signed-by /key，DNF 使用 --gpgkey URL。
    sources add/enable/disable/remove 为添加、启用、禁用、删除软件源的别名。修改软件源先生成草稿，经审阅后使用 --apply --expected-version 保存。
    脚本修改默认保留现有配置；--config-policy replace 仅支持 APT。Arch 的安装和升级始终升级整个系统。

startup-cli-doctor-help = 用法：tundra-cli doctor
    检查终端、权限、路径、存储和资源。
    存储检查可能创建或恢复应用文件。退出码：0 通过，1 失败。
startup-cli-explain-commands = { "  " }2. tundra-cli 提供配置、应用入口和日志；doctor 检查环境；debug 提供测试工具。
startup-cli-repl-help = 用法：tundra-cli repl
    /help  命令列表
    exit   退出（或 EOF）
startup-cli-launcher-help = 用法：tundra-cli launcher [list | pin <path> | unpin <id>]
    list          列出已保存的应用
    pin <path>    保存可执行文件的快捷入口
    unpin <id>    移除快捷入口，保留文件
startup-cli-screen-keyboard-help = 用法：tundra-cli debug screen-keyboard
    在外部交互终端中预览屏幕键盘和 AA 输入。
    Tab/方向键选择，Enter/空格执行，Esc 退出。
