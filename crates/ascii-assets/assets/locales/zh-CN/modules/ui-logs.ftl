# UI logs messages.

ui-logs-ux-log = UX 日志
ui-logs-linux-log = Linux 日志
ui-logs-events = 事件
ui-logs-files = 文件
ui-logs-refresh = R/F5 刷新
ui-logs-open = Enter/O 打开
ui-logs-pause-scroll = P 暂停滚动
ui-logs-back-latest = P 回到最新（{ $count } 条新增）
ui-logs-more = F10 更多操作
ui-logs-source-filters = F 服务、启动批次或文件
ui-logs-unit = 服务名称（.service；留空为全部）
ui-logs-scope = 服务范围
ui-logs-boot = 启动批次（0、-1 或批次 ID；留空为全部）
ui-logs-invocation = 服务本次运行 ID（可选）
ui-logs-file = 日志文件的绝对路径（可选）
ui-logs-file-absolute = 文件路径不完整。请输入绝对路径。
ui-logs-invalid-service = 服务名称无效。请输入完整的 .service 名称。
ui-logs-invalid-boot = 启动批次无效。请输入 0、-1 或 32 位批次 ID。
ui-logs-invalid-invocation = 运行 ID 无效。请输入服务的 32 位运行 ID。
ui-logs-file-replaced = 日志已轮转或截断。正在跟随当前文件。
ui-logs-buffer-limited = 日志缓冲区已满。较早内容已移出，请缩小查询范围。
ui-logs-state-ready = 已就绪
ui-logs-state-partial = 部分日志不可用。请查看详情。
ui-logs-state-permission = 无权读取日志。请检查系统权限。
ui-logs-state-unavailable = 日志不可用。请检查来源后刷新。
ui-logs-state-unsupported = 不支持此来源。请选择其他日志。
ui-logs-state-cancelled = 查询已取消。刷新可继续。
ui-logs-file-missing = 文件不存在。请检查路径或等待轮转完成。
ui-logs-file-denied = 无权读取文件。请检查文件权限。
ui-logs-file-unreadable = 无法读取文件。请检查后刷新。
ui-logs-file-tail = 正在显示文件末尾。较早内容仍保留在原文件。
ui-logs-file-unparsed = 文件中的时间和等级未知。请清除事件筛选。
ui-logs-journal-denied = 无权读取系统日志。请检查日志访问权限。
ui-logs-incidents = 故障事件
ui-logs-r-refresh = R/F5 刷新
ui-logs-o-open = Enter/O 打开
ui-logs-l-level = L 级别
ui-logs-m-module = M 模块
ui-logs-t-time = T 时间
ui-logs-linux-log-is-unavailable-on-windows-and-macos-ux-log-remains-available = Windows 和 macOS 不支持 Linux 日志。仍可查看 UX 日志。
ui-logs-linux-log-requires-administrator-access-and-operating-system-log-permissions = 查看 Linux 日志需要管理员身份及操作系统日志权限。
ui-logs-sign-in-to-view-your-ux-log-guest-access-is-disabled = 请登录以查看您的 UX 日志。访客无法访问。
ui-logs-logs = 日志
ui-logs-loading-logs = 正在加载日志…
ui-logs-linux-events = Linux 事件
ui-logs-ux-events = UX 事件
ui-logs-log-files = 日志文件
ui-logs-loading-events = 正在加载事件…
ui-logs-no-events-match-the-current-query = 没有符合当前查询的事件
ui-logs-select-an-event-to-inspect-its-operation-and-correlation-identifiers = 选择事件以查看其操作和关联标识。
ui-logs-esc-back-category-tab-section-enter-o-open-read-only-r-refresh-i-e-link = ←/→ 类别 · Tab 分区 · Enter/O 打开 · R/F5 刷新 · Esc 返回

ui-logs-event-detail = 时间：{ $time }
    级别：{ $level }
    事件：{ $event }
    操作：{ $operation }
    { $detail }{ $incident }

ui-logs-incident-link = { "\u000A" }故障事件：{ $id }

ui-logs-clear-filters = C 清除过滤
ui-logs-show-incident = I 查看故障
ui-logs-show-events = E 关联事件
