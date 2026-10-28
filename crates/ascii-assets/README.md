# ascii-assets

中英文 `modules/terminal-launcher.ftl` 提供 Launcher 运行设置、参数和目录检查，以及嵌入终端的启动、输入和关闭提示。

中英文 `modules/selection.ftl` 提供可点击多选入口、已选数量和整页操作限制，沿用现有语言包自动加载。

`modules/tasks.ftl` 提供统一任务页、运行状态和清理说明。

`modules/management-schedule-workflow.ftl` 提供定时任务向导、原生编辑和补执行说明。

`modules/backup-filters.ftl` 和 `backup-history.ftl` 提供日期、来源筛选、分页、保留和清理预览说明。

中英文 `modules/script.ftl` 提供脚本编辑器入口、运行与调试操作、六区工作区、表达式和执行状态文字。语言资源沿用目录自动收集，不新增单独的加载路径。
脚本编辑器复用当前主题的编辑器图片和 ASCII 图案，已有自定义主题也使用同一图案。

`management-winget.ftl` 提供 Windows 软件包的中英文标签、缺少依赖、输出解析及执行结果提示，沿用现有语言包加载流程。

中英文 `modules/editor-vim.ftl` 也提供引擎安装弹窗、下载进度、取消及尚未发布的提示。

中英文 `modules/settings.ftl` 保存屏幕超时时间及说明，`modules/weathr.ftl` 保存空闲天气画面的返回提示。

`modules/regression-startup.ftl` 保存完整 CLI 命令列表及精简后的 doctor、REPL、应用入口和屏幕键盘帮助；doctor 使用顶层命令。它也保存 `tundra-shell --help` 中供 agent 使用的 UI automation 介绍与命令示例。

`ascii-assets` 加载和校验主题、ASCII 图案、图标及字体，统计绘制所需尺寸，并提供内置资源用于修复。语言源码也随资源分发；加载和翻译由 `i18n` 负责。

中英文 `modules/screen-keyboard.ftl` 包含 QWERTY 键盘的导航键与字母区／功能区切换标签。

启动器的任务模块使用 `builtin.tasks`，同时提供四行 ASCII 图案与 `launcher_icons/tasks.png`。默认图标会嵌入程序，旧主题升级时补齐缺少的任务图标并保留已有自定义内容。图片由内置 imagegen 根据现有青色发光图标风格生成，内容为两项已完成、一项运行中的任务清单，不含文字。

AA 自动策略的显示名称统一为“一律放行” / “Always allow”，帮助文字继续说明其只跳过 AA 确认。

编辑器 Vim 开关、“VIM模式已启用／未启用”状态按钮和按需打开的使用帮助位于中英文 `modules/editor-vim.ftl`；模式、命令行和错误提示保留原生 Vim 的显示。

软件包分栏页的主导航和高级区文字位于中英文 `modules/management-services-process-packages.ftl`；键盘操作统一放在 F1 帮助。

软件包、服务、进程和计划任务的 Shift+点击、Shift+Enter、多选保留和批量操作说明位于中英文 `modules/help.ftl`；无效选择、部分项目不支持操作及批量进度提示位于 `modules/management-services-process-packages.ftl`。

中英文 `modules/management-homebrew.ftl` 提供 macOS Formula/Cask 类型、查询要求、原生交互和执行结果提示。

文件权限与归属的中英文提示集中在 `modules/file-permissions.ftl`，包括空格与 Enter 的勾选操作说明。

操作系统设置和校时配置的中英文提示集中在 `modules/system-settings.ftl`，区分环境能力、工具和查询失败原因，并保留原始诊断详情。

`modules/settings.ftl` 和 `modules/ui-settings.ftl` 提供“区域与时间”直接修改 Linux 系统时区、自动校时和服务器的中英文提示，包含地图、刷新和服务器保存说明。

这两份资源也保存“应用设置”的分类名称、应用列表标题及恢复当前应用默认值的文字。

默认资源表使用一份静态数据，校验、修复和内存回退共用其中的图片字节，避免在同一个可执行文件中重复嵌入 PNG。

语言资源包含防火墙、SSH、配置恢复和定时任务的中英文界面文字；新增启动器入口复用现有图片，并提供各自的 ASCII 图标。旧图标目录会补入四个新入口，保留已有的自定义图案。

定时任务后端的表单、确认和错误提示集中在中英文 `modules/management-scheduled-tasks.ftl`，由 `i18n` 语言快照传给后台线程和授权助手。

用户管理的账户/用户组视图字段使用 `modules/management-users.ftl` 中的专用标签，与服务的系统/当前用户作用域区分。

中英文资源也包含运行期尺寸提示、可移植表单快捷键、管理能力与备份空状态说明；账户来源和任务名称使用各自的业务标签。

管理页和文件管理的中英文搜索入口统一提示 `[/]`；管理搜索输入时提示 `[Enter]` 提交。

动画高级选项弹窗的打开、关闭、阶段名称、毫秒说明和恢复提示位于中英文 `modules/settings.ftl`。

同一资源提供设置数值弹窗的范围校验、调整、输入、确认和取消提示；`modules/help.ftl` 同步说明 Tab/Shift+Tab 切换大类、方向键移动内容区焦点与弹窗内的数值操作。

重建发布程序后，可运行 `python scripts/check-embedded-assets.py target/release/tundra-shell.exe target/release/tundra-cli.exe` 检查每张默认图片是否恰好嵌入一次；Linux/macOS 使用不带 `.exe` 的程序路径。脚本逐字节匹配源码中的完整 PNG，同时检查遗漏和重复。

若某个平台的程序没有用到内置图片，编译器可以完全移除这些字节。此时添加 `--allow-missing`，只检查有无重复；脚本仍会打印实际图片数量和字节数。

## 资源校验

资源清单固定要求 25 项：20 个文本资源、4 个 TOML art set 和 1 个时钟字体。文本资源只能包含可打印 ASCII；TOML art set 使用 schema v1。图片路径不得是绝对路径或包含路径穿越，并且只允许 GIF、JPEG、PNG、WebP。

资源根可由 `TUNDRA_ASCII_ASSETS_DIR` 指定，也会从二进制同目录查找；Linux 在前两者未命中时再回退至 `/usr/share/tundraux3/assets`。所有资源都会参与尺寸统计，因此资源更新可能改变启动时的最小终端要求。

中英文 `modules/ui-test.ftl` 保存共享控件展示页的文字；`debug ui-test` 固定使用编译进程序的英文，不读取磁盘上的语言资源。

同一资源文件也保存 `debug test-rainbow-text` 的帮助、标题、示例和退出提示，中英文命令目录均包含此入口。

`modules/help.ftl` 保存各页 F1 帮助；启动器空列表使用简短说明和文件浏览器按钮，中英文同步提供。

外部 Shell 控制的开关、CLI 帮助与错误提示位于中英文 `modules/settings.ftl`；CLI 根帮助及公开配置字段列表同步列出 `ui` 和 `automation.enabled`。

同一帮助列出 `tundra-cli ui wake`，用于关闭空闲天气或天气锁屏并读取后续 Shell 画面。

外部控制的中英文错误提示明确要求用户操作后重新执行 `tundra-cli ui snapshot`；CLI 和 Shell 帮助同步说明自动更新、单纯移动鼠标与天气画面的例外。
