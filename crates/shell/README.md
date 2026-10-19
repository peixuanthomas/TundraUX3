# shell

`shell` 把应用状态、界面、终端输入和后台任务连接起来，提供 `tundra-shell`。它决定当前页面、返回路径、焦点和弹窗，最后合成完整的一帧。

文件属性的“权限与归属”支持多选，并打开独立管理页，沿用 AA、页面任务和返回规则。权限与递归勾选项可用空格或 Enter 切换，文字字段的空格继续作为文本输入。修改表单退出前确认是否放弃，账户选择支持查找与屏幕键盘。

设置内区分 Tundra 选项与 Linux 操作系统设置。主机名、时区、自动校时和服务器配置沿用管理页的授权、任务和返回规则，不改 Tundra 自己的显示时区和时间来源。

`tundra-shell --help`（`-h`）显示帮助，`tundra-shell --version`（`-V`）显示版本。这些查询不启动 watchdog、不读写配置，也不进入全屏界面；不带参数时正常启动 UI。

## 分工

| 位置 | 做什么 |
| --- | --- |
| `src/session/controller/` | 按应用分组，处理操作、任务结果和输入路由 |
| `src/session/presentation/` | 把应用状态和会话状态转换成界面需要的数据 |
| `src/session/compositor/` | 按顺序绘制页面、标题栏、状态栏和弹窗 |
| `src/session/navigation.rs`、`page_state.rs`、`page_lifecycle.rs` | 保存独立页面实例，安排打开、覆盖、确认关闭和清理 |
| `src/session/tasks.rs`、`overlay*.rs` | 核对任务结果和弹窗的页面归属，恢复对应焦点 |
| `src/session/runtime.rs` | 事件循环、刷新和退出处理 |
| `src/startup/` | 启动检查、首次设置和启动画面 |
| `src/previews/` | 样式、AA 和屏幕键盘演示 |
| `src/input/` | Crossterm 事件批处理、终端事件转换和 Shell 输入类型 |

`controller/` 按 account、auto_admin、clock、command_line、diagnostics、editor、explorer、launcher、logs、management、notifications、settings、system_status 分组。页面专用输入放回对应分组，通常使用 `input.rs`；公共路由、焦点和命中逻辑留在 `controller/input/`。

输入入口统一为 `controller/input/preparation.rs::prepare_input`，运行时和无终端测试共用。`interaction.rs` 保存跨帧的输入方式、悬停抑制、按钮捕获和快捷键重复保护；`pointer.rs` 处理共享按钮的命中与松开确认；`input_routing.rs` 决定事件目标和语义命令。页面控制器只保留其业务操作、文本编辑和专用拖动语义。Crossterm 继续负责底层事件解析，Ratatui 继续作为唯一界面与渲染系统。

管理页搜索、共享管理文字表单、编辑器配置表单和 AA/Command Line 的屏幕键盘按钮统一由合成器放在右下角、时间栏上方。普通页面展开键盘时，标题栏逐渐收成一行，状态和时间栏退场，内容与表单缩到按键上方；粘贴和收起分别放在键盘最底排的左、右端，大小和其他按键一致；收起时恢复。AA 是顶层弹窗，只调整弹窗，保持灰色背景和标题栏布局。所有开合跟随动画速度和减少动态效果配置，绘制、点击及子终端行列数使用同一布局。键盘输入核对页面实例与表单实例，关闭搜索、切换表单或打开其他弹窗后，不会把旧按键送进新的输入框。

已包含完整详情的管理快照只切换详情显示，不重新查询。授权读取后的刷新和搜索沿快照提供的读取动作再次授权，保留当前查询条件、排序与所选记录；读取失败不会自动重复请求授权。

各管理页的查看详情沿用共享展开动画，跟随动画速度；减少动态效果时直接显示。动画绑定当前页面实例，缩放或离开页面时清除。

软件包页接入共享分栏：已安装、更新、搜索从上到下使用左侧大按钮，默认打开已安装；当前页保留强调色和 `->`，鼠标操作后列表选中条仍显示。主要操作位于详情底部；软件源、检查修复、配置冲突分别归入对应页的高级区。F7/F8/F6 对应上述主页面，F3 展开高级区；Tab/Shift+Tab 切换区域，方向键选择，Enter/空格执行。详情区可以单独滚动，F4 仍可放大详情。切换子页会清除旧列表，等查询返回后显示本页内容。

切换为鼠标操作时，首页按钮隐藏键盘焦点颜色；启动器的选中项目继续显示强调色。

新建闹钟弹窗可通过取消按钮、Esc，或 Tab 选中取消后按 Enter/空格关闭；鼠标与触摸沿用同一区域按下、松开才执行的规则。

管理结果记录生成时的语言版本。切换语言或重载自定义语言包后，正在显示或重新返回的管理页会等当前操作和表单结束，再沿原读取方式刷新一次；失败不循环请求。同一视图保留排序、筛选和所选记录。

较大的控制器再按实际工作拆分：editor 分为 view/input/settings/files/recovery/tasks；settings 分为 forms/background/view/devices/tasks；management 分为 actions/input/touch/config_editor。入口文件负责连接这些步骤。

终端模式和子终端由 [terminal-runtime](../terminal-runtime/README.md) 管理；批准、密码和任务执行由 [auto-admin](../auto-admin/README.md) 管理。Shell 保留界面控制，不另建任务监督系统。

AA 实时输出右侧提供可拖动滚动条，运行中和结束后均可查看历史；键盘翻页、滚轮和拖动共用同一输出位置。失焦、缩放或键盘输入会取消旧拖动。

编辑器顶部 `VIM_mode` 按钮和 F8 共用开关，首次默认关闭，切换后保存到配置。开启后支持普通/输入模式、h/j/k/l、w/b、0/$、gg/G、i/a/I/A、o/O、x、dd/yy、p/P、u、Ctrl+R 和 /、n/N；编辑命令仍由 app 处理。快捷键说明只在“帮助 → Vim 快捷键”中显示。实体 Esc 回到普通模式，Ctrl+W 和返回按钮继续关闭文档；恢复光标加速默认设置不会改变 Vim 开关。

运行中窗口小于布局下限时显示尺寸提示并暂停页面输入，保留编辑缓冲、弹窗和计时状态；恢复尺寸后继续。编辑器保存完成同步更新状态及相关文件列表。文件名输入复用共享选区控件，首次设置和管理表单提供 F10 完成，管理搜索明确提示 Enter 提交。

```mermaid
flowchart LR
    Input[终端输入] --> Route[路由与当前控制器]
    Route --> App[app 状态与命令]
    App --> Present[presentation]
    Present --> UI[ui 布局和绘制]
    UI --> Compose[最终合成器]
    Job[后台任务结果] --> Route
```

## 阅读入口

- [会话、导航与绘制](docs/session.md)：启动、输入、焦点、页面返回和异常处理。
- [调试预览](docs/previews.md)：屏幕键盘、AA 输入演示及样式预览。
- [统一 UI 要求](../../docs/UI-requirements.md)：所有页面必须遵守的交互规则。
- [内置应用](../app/docs/applications.md)、[CLI 与 Command Line](../cli/docs/commands.md)。

构建时同时执行 `cargo build --locked -p shell -p cli`，否则内嵌命令行可能继续使用旧 CLI。验证使用 `cargo test --locked -p shell`；终端输入和模式恢复还需对应系统的真实 PTY 检查。

`src/previews/ui_test.rs` 提供无状态的 `debug ui-test` 入口：使用内置主题和英文，不读写配置、资源文件或日志；输入及终端恢复沿用现有组件和 `TerminalGuard`。六页控件清单见[调试预览](docs/previews.md#全部共享控件)。
