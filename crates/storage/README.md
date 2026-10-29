# storage

Launcher 的 order 保存内置与用户条目的统一 ID 顺序；条目的可选 custom 保存 name、icon、command，缺省字段兼容旧配置。

`storage` 读取和保存应用文档，检查格式版本，并保留损坏原件供排查。平台路径和文件操作由 `platform` 提供。

Launcher 条目的可选 `terminal` 设置保存参数数组和工作目录，用于嵌入终端启动。旧条目缺少此字段时继续使用系统启动方式，参数中的空格、中文和特殊字符原样保存。

`config.toml` 顶层的 `screen_timeout_seconds` 保存空闲后显示天气的秒数，`0` 表示永不；旧配置缺少此项时使用 300 秒。它与系统设置一起保存，不改变登录状态。

## 文档格式与安全保存

`storage` 管理下列主要文档：

| 文件 | 格式 | 内容 |
| --- | --- | --- |
| `config.toml` | TOML，schema 1 | 语言、时区、天气位置、快捷键、外观和各应用设置。 |
| `users.v2.json` | 版本化 JSON，schema 2 | Windows/macOS 本地账户、角色、密码与锁定信息；Linux 仅使用当前 UID 的个人偏好。 |
| `state.v1.json` | 版本化 JSON，schema 1 | 通用应用状态。 |
| `recent-files.v1.json` | 版本化 JSON，schema 1 | 最近文件。 |
| `sessions.v1.json` | 版本化 JSON，schema 1 | 可恢复会话数据。 |
| `clock.v1.json` | 版本化 JSON，schema 1 | 时钟、闹钟和计时项目。 |
| `trash/trash.v1.json` | 版本化 JSON，schema 1 | 应用回收站清单。 |

平台文档读取默认限制为 1 GiB，并在读取前后检查长度、修改时间和路径身份；路径逐级拒绝符号链接、junction 与 reparse point。条件写入还会核对文档 fingerprint，将外部修改作为独立冲突返回。写入依次使用同目录临时文件、文件同步、原子替换与父目录同步，避免部分写入；Linux 存储路径进一步使用 `openat` 与 `O_NOFOLLOW` 防止祖先目录被符号链接重定向。

Linux 应用目录为 `0700`；配置、用户、会话、恢复、日志和临时文件为 `0600`。Editor 打开的普通文档不被改变权限。

`config.toml` 的 `[editor].vim_mode_enabled` 保存 Vim 开关；旧配置缺少此字段时默认关闭。

`[editor].explorer_open_extensions` 默认包含 106 种后缀，覆盖全部语法着色后缀及原有文本后缀，列表上限为 128 项。启动时将未修改的旧默认列表升级为新列表；自定义列表和空列表保留，其他编辑器设置不变。

外观的 `animation_timings` 保存 `page`、`dialog`、`menu` 三组毫秒值，每组依次为加速、匀速、减速。旧配置分别使用 55/110/55、45/90/45、40/80/40，读取时限制每段为 1～2000ms；未改动的默认值不额外写入。它与动画百分比一起随个人外观保存和恢复。

启动会先校验 schema：**未来 schema 一律拒绝**，以防旧程序覆盖新格式。当前或旧格式无法解析时，原文件会在原位置重命名为 `<文件名>.corrupt.<时间戳>`，随后生成默认文档，并在 Shell 显示恢复提示。旧 `users.v1.json` 会迁移到 `users.v2.json`。

`config.toml` 的 `[automation].enabled` 保存外部 CLI 控制开关，默认 `false`；旧配置缺少此段时保持关闭。此设置属于当前操作系统用户，不是 Tundra 登录账户的个人外观设置。
