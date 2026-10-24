# 外部 CLI 控制

`tundra-cli ui` 读取运行中 Shell 的界面，并发送与正常键盘、鼠标、粘贴相同的输入。它不会直接调用页面业务操作，确认弹窗、登录和系统授权仍由现有界面处理。

## 开启与选择会话

默认关闭。在 **设置 → 系统 → 外部 CLI 控制** 中开启，立即生效；也可以先保存配置，再启动 Shell：

```console
tundra-cli config set automation.enabled true
tundra-shell
```

从另一个终端读取会话与快照：

```console
tundra-cli ui sessions
tundra-cli ui snapshot
tundra-cli ui snapshot --session SESSION_ID
```

只有一个可连接会话时自动选择；存在多个会话时必须指定 `--session`，不会猜测当前活动窗口。会话 ID 每次启动都会改变。只有同一操作系统用户可以连接，登记文件放在该用户的 TundraUX3 缓存目录内。关闭设置或退出 Shell 后不再接受新的外部输入。

控制命令不启动 watchdog，不加载或改写配置和存储，不初始化日志。帮助也不读取目录或连接会话；外部 CLI 在没有语言快照时使用内置英文，在已有语言快照中使用当前语言。

## 输入命令

所有位置都是终端字符格，左上角为 `(0, 0)`。先从快照中读取目标 ID 或矩形，再发送输入。下列 `TARGET_ID`、`FROM_ID` 等只是占位符，请替换为实际快照中的 ID。

| 命令 | 参数 |
| --- | --- |
| `sessions` | 列出会话；不接受选项。 |
| `snapshot` | 读取界面树和字符画面。 |
| `wake` | 关闭空闲天气或天气锁屏，返回绘制后的 Shell 快照；已在普通页面时只返回当前快照。不会跳过登录。 |
| `click` | `--id TARGET_ID` 或 `--x COL --y ROW`；可加 `--button left\|right\|middle`、`--double`。 |
| `move` | `--id TARGET_ID` 或 `--x COL --y ROW`。 |
| `drag` | 起点 `--from-id FROM_ID` 或 `--from-x COL --from-y ROW`；终点 `--to-id TO_ID` 或 `--to-x COL --to-y ROW`；可加 `--duration MS`、`--button left\|right\|middle`。默认 300 ms，允许 16–10000 ms。 |
| `scroll` | `--id TARGET_ID` 或 `--x COL --y ROW`，加 `--direction up\|down\|left\|right`；`--count N` 默认 1，允许 1–1000。 |
| `key KEY` | 一个 Unicode 字符或命名按键；可加 `--modifiers ctrl,alt,shift,super,hyper,meta` 和 `--phase press\|repeat\|release`，默认 `press`。 |
| `text TEXT` | 向当前焦点粘贴文字；空格需由当前 shell 的引号包住。以短横线开头的正文使用 `text -- TEXT`。 |
| `text --stdin` | 从标准输入读取 UTF-8 文字，保留换行，不把正文写入日志或错误信息。 |
| `wait --id TARGET_ID` | 只读取快照，等待目标满足 `--state present\|absent\|actionable`，默认 `actionable`。 |

命名按键不区分大小写：`enter`、`esc` / `escape`、`tab`、`backtab`、`backspace`、`space`、`delete`、`insert`、`left`、`right`、`up`、`down`、`home`、`end`、`pageup`、`pagedown` 和 `F1`–`F24`。组合键通过 `--modifiers` 指定，例如：

```console
tundra-cli ui click --id TARGET_ID
tundra-cli ui wake
tundra-cli ui key s --modifiers ctrl
tundra-cli ui key tab --modifiers shift
tundra-cli ui drag --from-id FROM_ID --to-x 40 --to-y 18 --duration 400
tundra-cli ui scroll --x 40 --y 18 --direction down --count 3
tundra-cli ui text "中文与 emoji 😀"
tundra-cli ui text -- --literal-text
tundra-cli ui wait --id TARGET_ID --state actionable --timeout 10000
```

`present` 只要求目标存在，`absent` 要求目标消失，`actionable` 还要求目标启用且可接收输入。布局移动时 Shell 会把受影响的目标暂设为不可操作；通知和进度等动画不阻止已经可操作的目标。等待不会点击目标或重发上一个输入。

## 快照与输出

除了 `sessions`，各命令都支持 `--session ID`、`--snapshot NUMBER`、`--timeout MS`。`--timeout` 是正整数，默认 5000 ms，限制连接/响应等待或 `wait` 的总等待时间；读取 `--stdin` 先等待标准输入结束。参数重复、未知选项、缺少坐标、同时提供 ID 和坐标都以参数错误退出。

`--snapshot` 要求 Shell 的当前快照编号仍与指定值一致，否则返回 `stale_snapshot`，不执行输入。适用于读取位置后马上操作，防止误点已经变动的界面。超时后不会自动重发输入；此时输入可能已经发生，应重新读取快照确认结果。

每条结果都是一行 JSON。成功的快照及输入结果形如：

```json
{"protocol":1,"ok":true,"snapshot":{"session":"SESSION_ID","number":42,"page":"home","width":120,"height":30,"animation_running":false,"background_tasks_running":false,"nodes":[],"lines":[]}}
```

关闭控制开关、退出或重启时，执行该输入的连接会先收到 `{"protocol":1,"ok":true,"closed":true}`，随后断开；这类结果没有画面快照。

`nodes` 是界面树中的节点列表，`parent` 指向父节点 ID。每个节点提供 `id`、`role`、`name`、`text`、`rect`、`focused`、`selected`、`enabled`、`actionable`、`sensitive` 和 `actions`。`rect` 包含 `x`、`y`、`width`、`height`。输入目标应优先使用节点 ID；普通画面文字节点可能不可操作。敏感输入只提供遮蔽后的内容。

`lines` 保存按终端行排列的字符画面；它不包含真实图像像素。`animation_running` 表示仍有动画，`background_tasks_running` 表示还有后台任务，两者都不代表该页所有输入被禁用。是否可以操作应读取节点的 `enabled` 和 `actionable`。

会话列表形如：

```json
{"protocol":1,"ok":true,"sessions":[{"session":"SESSION_ID","pid":1234,"endpoint":"LOCAL_ENDPOINT"}]}
```

错误形如：

```json
{"protocol":1,"ok":false,"error":{"code":"stale_snapshot","message":"The interface changed. Read a new snapshot before acting."}}
```

脚本应检查 `ok` 和 `error.code`，不匹配会随语言改变的 `message`。正常完成退出码为 `0`，参数解析失败为 `2`，连接、服务端拒绝、读取标准输入和等待失败为 `1`。

| 错误代码 | 含义 |
| --- | --- |
| `invalid_arguments` | 参数无效或请求超出大小限制。 |
| `no_session` | 没有可连接的 Shell。 |
| `session_required` | 有多个会话，需要 `--session`。 |
| `session_not_found` | 指定会话不可用。 |
| `unavailable` | 本地连接或会话目录不可用。 |
| `not_ready` | Shell 尚未发布画面；`wait` 会继续读取。 |
| `stale_snapshot` | 快照编号已改变，输入未执行。 |
| `busy` | 另一个外部输入尚未完成。 |
| `target_unavailable` | 目标不存在或当前不能接收输入。 |
| `unsupported_protocol` / `protocol_error` | 服务端不支持请求版本，或客户端收到不符合协议的响应。 |
| `timeout` | 响应或目标等待超时；输入不会自动重发。 |
| `stdin_error` | 标准输入读取失败或不是有效 UTF-8。 |

请求和响应类型由 `shell-control` crate 定义；连接由 `platform::shell_control` 提供。CLI 的 `wait` 只发送 `snapshot`，不会引入额外的服务端输入命令。

## 验证

```console
cargo test --locked -p cli --test ui_commands --test command_info
cargo test --locked -p cli --lib ui_commands_complete
```

测试使用临时会话目录和模拟 Shell，检查真正的本地连接、JSON 请求、响应错误、会话选择、输入只发送一次、等待条件与超时。Linux 测试使用本机 WSL；真实画面交互另由 Shell 的终端测试验证。
