# 日志与诊断

Logs 是独立应用，集中显示 UX 运行事件、事故报告、Linux 日志和指定日志文件。System Status 保留系统指标与健康检查。

## 来源与权限

| 来源 | 内容与访问 |
| --- | --- |
| UX log | 结构化运行事件与事故报告，事故按最高等级 Critical 显示；普通用户只看自己的记录，完整事故报告需要管理员 |
| Linux log | 按需读取 journal，不可用时回退 dmesg；TUI 需要管理员，操作系统权限仍然有效 |
| 指定文件 | 读取用户选择的绝对路径，使用当前操作系统权限并有界读取尾部 |

UX 支持 Linux、Windows 和 macOS，Guest 没有 Logs 入口。Windows/macOS 的 Linux 来源显示 Unsupported。

来源与级别分开：Explorer 错误属于 `ux.explorer`，内核消息属于 `linux.kernel`，服务消息属于 `linux.service`。journal priority 映射到日志级别，原始来源和时间备注保留。

权限失败、部分输出、取消和来源不可用分别显示，不作为空结果。Linux 记录不添加虚构的 UX run/task ID。

## CLI 查询与导出

```sh
tundra-cli logs query
tundra-cli logs query --source ux --module ux.explorer --level error --format jsonl
tundra-cli logs query --since 2026-09-01T00:00:00Z --until 2026-09-02T00:00:00Z
tundra-cli logs query --run-id RUN_ID --task-id TASK_ID --limit 500
tundra-cli logs query --source linux --level warning
tundra-cli logs incidents --incident-id INCIDENT_ID
tundra-cli logs export --run-id RUN_ID --output ./diagnostics-new
```

查询默认返回最新 200 条，最多 10,000 条。筛选包括来源、UTC 时间、最低级别、模块、run、operation、task 和 incident，输出支持文本与 JSONL。

独立 CLI 使用当前操作系统身份和文件权限，不接受调用方声明的 Tundra 角色或所有者。

退出码：0 成功（包括空结果），1 失败或结果截断，2 参数错误，3 无权限，4 不支持，130 取消。

导出目录必须不存在，程序会创建私有目录并写入 manifest，记录来源状态、截断、损坏记录和 writer 健康。部分导出保留可用记录并报告遗漏，导出文件不占用受管理日志配额。

## TUI 操作

从 Launcher 打开 Logs，默认显示全部级别。

| 输入 | 行为 |
| --- | --- |
| Left / Right | 切换 UX / Linux 来源 |
| L | 循环最低级别，Warning 会包含 Error 和 Critical |
| M / T / C | 模块筛选 / 时间筛选 / 清除筛选 |
| R 或 F5 | 刷新 |
| Enter 或 O | 在只读 Editor 打开脱敏查询文档 |
| I / E | 打开完整事故报告 / 查看关联事件 |
| G / Open folder | 在内置 Explorer 打开 UX 日志根目录 |
| Esc | 返回并保留筛选与选择 |

方向键、PageUp/PageDown、Home/End、鼠标和滚动条用于浏览。Tab 不切换日志来源。

打开文件时生成按当前用户过滤的新快照，不绕过所有者检查。完整事故报告使用白名单和脱敏副本，不直接暴露原始 crash 文件路径。

## 日志跟随与集中排查 / Following and filtering logs

日志应用持续收集新记录。手动上滚、选择旧记录或拖动滚动条会暂停自动跟随，后台继续收集并显示新增数量；“回到最新”恢复跟随。当前缓冲最多保留 1,000 条。

服务、启动批次、单次运行和文件选择位于筛选表单。服务、进程和配置任务可携带服务名称、system/user 范围、启动 ID、运行 ID 和时间打开日志，并返回原页面。

```sh
tundra-cli logs follow --unit ssh.service --scope system --boot 0 --json
tundra-cli logs query --unit worker.service --boot -1
tundra-cli logs follow --file /var/log/syslog --format jsonl
```

内建 Command Line 使用 `/logs follow …`。JSONL 每行一个事件，状态和截断提示写入标准错误，Ctrl+C 停止并返回 130。

指定文件使用有界尾部读取，报告更换、截断、删除和权限变化。文件轮转后继续跟随新文件，暂时不存在时继续等待。原始文件的未知时间和级别保持未知，查询与跟随都不修改日志源。

## 清理日志

使用 `/debug clear-logs` 或外部 `tundra-cli debug clear-logs`。不带 `--yes` 时只预览文件与大小，`--yes` 确认清理。

```sh
tundra-cli debug clear-logs all
tundra-cli debug clear-logs incidents --yes
tundra-cli debug clear-logs runtime --yes
tundra-cli debug clear-logs snapshots --yes
tundra-cli debug clear-logs legacy --yes
tundra-cli debug clear-logs --file crashes/crash-EXAMPLE.json --yes
```

| 目标 | 文件范围 |
| --- | --- |
| `all` / `--all` | 以下全部类别 |
| `runtime` | `runtime/runtime-*.jsonl` |
| `incidents` | `crashes/` 下的 JSON、TXT、LOG 报告 |
| `snapshots` | `runtime/snapshots/snapshot-*.json` 和 `*.jsonl` |
| `legacy` | 日志根目录下的 `*.log` |
| `--file PATH` | 日志根目录内一个受支持的文件 |

每次只选择一个目标，`--type TYPE` 是类别别名。清理单个 incident JSON 不删除 TXT 副本，按类别清理会同时处理两种格式；对应 UX 事件需要另行选择 `runtime` 或 `all`。

清理只使用当前操作系统权限，拒绝路径穿越和符号链接。活动 runtime segment 在与 writer 相同的锁下清空，writer 从实际文件末尾继续写入。正在运行的进程可在清理后继续产生新事件。

导出目录、Linux journal/dmesg、配置、账户、watchdog 状态与运行标记不在清理范围内。

清理退出码：0 全部完成或没有匹配项，1 没有可清理文件或输出失败，2 参数错误，3 部分失败。

## 存储与配额

runtime 文件位于平台日志目录的 `runtime/`。事件携带 schema、UTC 时间、事件 ID、process/run/app/module/operation/task ID、所有者、阶段、级别和可选 incident ID。

默认配置：

```toml
[runtime_logs]
max_age_days = 30
max_total_mib = 200
segment_mib = 10
```

segment 按 UTC 日期或配置大小轮转，保留清理使用跨进程锁，不删除其他活动 writer 锁定的 segment。配置在进程启动时读取。

Editor 快照共享总容量，另限制为 20 MiB、30 分钟 TTL 和每文档 5 MiB。watchdog crash 报告使用独立保留策略。

writer 使用 4,096 事件有界队列、非阻塞提交、一秒刷新和最多两秒退出等待。队列饱和与写入失败通过健康计数报告，不递归写日志。重复告警按身份合并，计数摘要每 60 秒生成一次。

## 验证

```sh
cargo test --locked -p runtime-log
cargo test --locked -p cli --test logs
cargo test --locked -p shell --lib logs
```

测试覆盖核心存储、Linux 解析与回退、权限隔离、CLI/导出、TUI、文件轮转和清理。Linux 数据源在 Ubuntu/Fedora 验证，UX 与 Unsupported 来源在 Windows/macOS 验证。
