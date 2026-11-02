# storage

`storage` 读取和保存应用文档，校验格式版本，提供原子写入、迁移、损坏恢复和数据重置。平台路径与底层文件操作由 `platform` 提供。

## 主要文档

| 文件 | 格式 | 内容 |
| --- | --- | --- |
| `config.toml` | TOML，schema 1 | 语言、时区、天气、外观、AA、自动化和应用设置 |
| `users.v2.json` | JSON，schema 2 | Windows/macOS 本地账户与 Linux 个人偏好 |
| `state.v1.json` | JSON，schema 1 | 通用应用状态 |
| `recent-files.v1.json` | JSON，schema 1 | 最近文件 |
| `sessions.v1.json` | JSON，schema 1 | 可恢复会话数据 |
| `clock.v1.json` | JSON，schema 1 | 时钟、闹钟和计时项目 |
| `trash/trash.v1.json` | JSON，schema 1 | 应用回收站清单 |

`StorageManager` 和 `StorageLayout` 管理文档位置与加载，`StorageDescriptor` 描述格式，`StorageLoadReport` 返回迁移和恢复结果。

## 安全保存与恢复

文档默认最多读取 1 GiB，读取前后核对长度、修改时间和路径身份。路径逐级拒绝符号链接、junction 和 reparse point；条件写入使用文档 fingerprint 检查外部修改。

写入依次使用同目录临时文件、文件同步、原子替换和父目录同步。Linux 使用 `openat` 与 `O_NOFOLLOW` 防止祖先目录被重定向，应用目录为 `0700`，私有文档为 `0600`。

未来 schema 直接拒绝。当前或旧格式无法解析时，原件重命名为 `<文件名>.corrupt.<时间戳>`，随后创建默认文档并在 Shell 提示；旧 `users.v1.json` 迁移到 `users.v2.json`。

## 配置要点

- `[automation].enabled` 保存外部 CLI 控制，默认 `false`。
- `screen_timeout_seconds` 保存空闲天气时间，`0` 表示永不，默认 300 秒。
- `[editor].vim_mode_enabled` 保存 Vim 开关，默认关闭。
- `[editor].explorer_open_extensions` 管理交给内置编辑器的后缀，自定义列表和空列表优先。
- `animation_timings` 分别保存 page/dialog/menu 的加速、匀速、减速时长，每段 1～2000ms。
- Launcher `order` 保存统一 ID 顺序，`custom` 保存名称、图标、命令和退出选项，`terminal` 保存参数数组与工作目录。

`reset_saved_content` 用于清除已保存数据，调用方负责备份、确认和重启流程。

## 验证

```sh
cargo test --locked -p storage
```

测试覆盖 schema、迁移、原子写入、路径安全、损坏恢复和配置兼容。
