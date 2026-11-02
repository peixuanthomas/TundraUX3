# 外部 CLI 控制

`tundra-cli ui` 读取运行中 Shell 的界面，并发送正常键盘、鼠标和粘贴输入。页面命令、确认弹窗、登录和系统授权仍由现有界面处理。

## 开启与选择会话

在 **设置 → 系统 → 外部 CLI 控制** 中开启，立即生效；也可先保存配置再启动 Shell：

```sh
tundra-cli config set automation.enabled true
tundra-shell
```

读取会话和快照：

```sh
tundra-cli ui sessions
tundra-cli ui snapshot
tundra-cli ui snapshot --session SESSION_ID
```

只有一个会话时自动选择，多个会话必须指定 `--session`。会话 ID 每次启动都会改变，只有同一操作系统用户可以连接。

控制命令不启动 watchdog，不加载或改写配置和存储，也不初始化日志。关闭控制或退出 Shell 后，连接会关闭。

## 输入命令

坐标使用终端字符格，左上角为 `(0, 0)`。先从快照读取目标 ID 或矩形，再发送输入；下列大写名称是需要替换的占位符。

| 命令 | 参数 |
| --- | --- |
| `sessions` | 列出会话 |
| `snapshot` | 读取控件树和字符画面 |
| `wake` | 关闭空闲天气或天气锁屏；普通页面只读取快照 |
| `click` | `--id TARGET_ID` 或 `--x COL --y ROW`，可加 `--button left\|right\|middle`、`--double` |
| `move` | `--id TARGET_ID` 或 `--x COL --y ROW` |
| `drag` | `--from-id/--from-x/--from-y` 指定起点，`--to-id/--to-x/--to-y` 指定终点；可加 `--duration MS`、`--button` |
| `scroll` | 目标加 `--direction up\|down\|left\|right`，`--count N` 默认 1 |
| `key KEY` | Unicode 字符或命名按键，可加 `--modifiers` 和 `--phase press\|repeat\|release` |
| `text TEXT` | 向当前焦点粘贴文字，以短横线开头时使用 `text -- TEXT` |
| `text --stdin` | 从标准输入读取 UTF-8 文字并保留换行 |
| `wait --id TARGET_ID` | 等待 `--state present\|absent\|actionable`，默认 `actionable` |

拖动默认 300ms，范围 16–10000ms；滚动数量范围 1–1000。修饰键为 `ctrl,alt,shift,super,hyper,meta`。

命名按键不区分大小写，包括 `enter`、`esc`、`tab`、`backtab`、`backspace`、`space`、`delete`、`insert`、方向键、`home`、`end`、`pageup`、`pagedown` 和 `F1`–`F24`。

```sh
tundra-cli ui click --id TARGET_ID
tundra-cli ui key s --modifiers ctrl
tundra-cli ui key tab --modifiers shift
tundra-cli ui drag --from-id FROM_ID --to-x 40 --to-y 18 --duration 400
tundra-cli ui scroll --x 40 --y 18 --direction down --count 3
tundra-cli ui text "中文与 emoji 😀"
tundra-cli ui text -- --literal-text
tundra-cli ui wait --id TARGET_ID --state actionable --timeout 10000
```

`present` 只要求存在，`absent` 要求消失，`actionable` 要求启用且可接收输入。等待只读取快照，不点击目标或重发输入。

## 快照编号与超时

除 `sessions` 外，命令支持：

- `--session ID`：选择 Shell 会话。
- `--snapshot NUMBER`：指定先前读取的快照编号。
- `--timeout MS`：连接、响应或等待的总时限，默认 5000ms。

真实键盘、点击、拖动、滚动、粘贴或窗口缩放后，后续控制返回 `stale_snapshot`，直到重新执行 `snapshot` 或 `wait`。未传 `--snapshot` 也会检查。

自动重绘、动画、进度、时钟、后台结果和单纯鼠标移动不会使快照失效。外部 CLI 自己发送的输入不算真实用户操作。超时后不会自动重发，应重新读取快照确认输入结果。

## JSON 输出

每条结果是一行 JSON：

```json
{"protocol":1,"ok":true,"snapshot":{"session":"SESSION_ID","number":42,"page":"home","width":120,"height":30,"animation_running":false,"background_tasks_running":false,"nodes":[],"lines":[]}}
```

`nodes` 包含 `id`、`parent`、`role`、`name`、`text`、`rect`、`focused`、`selected`、`enabled`、`actionable`、`sensitive` 和 `actions`。`rect` 使用 `x`、`y`、`width`、`height`。输入目标优先使用节点 ID，敏感输入只返回遮罩内容。

`lines` 是按终端行排列的文字画面，不包含图像像素。`animation_running` 和 `background_tasks_running` 只描述状态，是否可操作应读取节点字段。

会话列表：

```json
{"protocol":1,"ok":true,"sessions":[{"session":"SESSION_ID","pid":1234,"endpoint":"LOCAL_ENDPOINT"}]}
```

关闭控制或退出时先返回 `{"protocol":1,"ok":true,"closed":true}`，再断开连接。

错误结果：

```json
{"protocol":1,"ok":false,"error":{"code":"stale_snapshot","message":"Shell content changed after user input. Run tundra-cli ui snapshot again before acting."}}
```

脚本检查 `ok` 和 `error.code`，不依赖可翻译的 `message`。正常完成退出码为 0，参数错误为 2，连接、拒绝、标准输入或等待失败为 1。

| 错误代码 | 含义 |
| --- | --- |
| `invalid_arguments` | 参数无效或请求超限 |
| `no_session` | 没有可连接的 Shell |
| `session_required` | 多个会话需要 `--session` |
| `session_not_found` | 指定会话不可用 |
| `unavailable` | 本地连接或会话目录不可用 |
| `not_ready` | Shell 尚未发布画面 |
| `stale_snapshot` | 用户操作后尚未重新读取，或编号无效 |
| `busy` | 另一个外部输入尚未完成 |
| `target_unavailable` | 目标不存在或当前不可操作 |
| `unsupported_protocol` / `protocol_error` | 协议版本或响应不符合要求 |
| `timeout` | 响应或目标等待超时 |
| `stdin_error` | 标准输入失败或不是有效 UTF-8 |

请求和响应类型由 `shell-control` 定义，连接由 `platform::shell_control` 提供。`wait` 只发送 `snapshot`，不需要额外的服务端等待命令。

## 验证

```sh
cargo test --locked -p cli --test ui_commands --test command_info
cargo test --locked -p cli --lib ui_commands_complete
python3 scripts/tests/shell-control.py target/debug/tundra-shell
```

自动测试使用临时会话目录和模拟 Shell，覆盖连接、JSON、会话选择、输入只发送一次、等待和超时。真实画面与输入由 Linux PTY 测试验证。
