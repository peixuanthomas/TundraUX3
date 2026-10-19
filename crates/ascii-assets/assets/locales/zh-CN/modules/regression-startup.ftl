startup-time-sync-fallback = { $reason }。正在使用上次校准的时间；尚未校准时使用系统时间。可在“设置 → 日期与时间”中重试。
startup-update-error-explanation = 更新未能完成。请检查网络连接、安装目录写入权限，以及所选更新模式的运行要求。原始详情：{ $details }
startup-account-source = 账户来源
startup-check-pass = 通过/PASS
startup-check-warning = 警告/WARN
startup-check-fail = 失败/FAIL
startup-diagnostics-status-explanation = 状态说明：PASS/Pass 表示通过；WARN 表示可选功能受限；FAIL 表示必要检查失败；Unavailable 表示当前环境无法使用该功能或数据源。
startup-temperature-unavailable = 温度不可用：未检测到传感器
startup-battery-unavailable = 电池不可用：未检测到电池
startup-cli-help = TundraUX3 命令行
    { cli-administration-root-usage }
    cls：清空终端历史和屏幕，并将光标移到起点。
    config：读取、修改、重置或列出设置；详情见 config help。
    debug：诊断与测试命令；详情见 debug help。
    new：清除已保存的 TundraUX3 数据并重新初始化。
    repl：进入交互命令行。默认运行系统命令；/<command> 运行 UX 命令；exit 退出。
    系统命令保留当前交互会话中的已导出环境变量和工作目录。
    logs：查询运行日志、故障事件或导出诊断；详情见 logs help。
    launcher：列出、固定或取消固定应用；详情见 launcher help。
    help [command]：显示帮助，例如 help config 或 help debug doctor。
    -h, --help：显示帮助；-V, --version：显示版本。
    使用 <command> --help（或 -h）查看语法、取值和示例。
    在内置命令行或 repl 中，UX 命令加 /：/config set motion reduced；/help 显示 UX 帮助。
    系统命令无需前缀：pwd、ls -la（Linux/macOS）、dir（Windows）。外部 CLI 也无需 /：tundra-cli config set motion reduced。
    退出码：0 成功，1 操作失败，2 参数无效；logs 也可能返回 3/130（见 logs help）。
startup-cli-debug-help = 用法：tundra-cli debug <command>
    screen-keyboard：试用英文 QWERTY 屏幕键盘，文字仅临时保存。
    test-aa-style1 / test-aa-style2 / test-aa-style3：预览三种 AA 授权警告样式。
    clear-logs <all|TYPE|--file PATH> [--yes]：预览或清理运行、故障、快照及旧日志。
    view-ui-style [1|2|3]：对比可交互的界面样式及动画。
    asset：输出测试资源或原始资源文件。
    doctor：检查终端、存储和 Linux 运行工具；警告表示可选功能受限。
    paths：输出已配置和已解析的应用路径。
    explain：说明 CLI 启动流程及系统/UI 分工。
    test-frost / test-matrix：仅播放启动或首次启动的横幅动画。
    test-watchdog-error / test-watchdog-critical：主动生成错误或严重错误报告。
    test-watchdog-panic：触发真实 panic，丢弃当前会话。
    错误测试输出 JSON/文本报告路径；panic 测试进入正常严重错误流程，内置测试会使 Shell 会话 panic。
startup-cli-config-help = 用法：tundra-cli config [get [field] | set <field> <value> | reset <field> | options [field]]
    不带参数时输出设置；reset 只重置指定字段；theme 是只读颜色摘要。
    字段与取值：
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
