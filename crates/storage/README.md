# storage

`storage` 读取和保存应用文档，检查格式版本，并保留损坏原件供排查。平台路径和文件操作由 `platform` 提供。

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

启动会先校验 schema：**未来 schema 一律拒绝**，以防旧程序覆盖新格式。当前或旧格式无法解析时，原文件会在原位置重命名为 `<文件名>.corrupt.<时间戳>`，随后生成默认文档，并在 Shell 显示恢复提示。旧 `users.v1.json` 会迁移到 `users.v2.json`。
