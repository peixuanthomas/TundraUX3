# ascii-assets

`ascii-assets` 加载、校验和修复主题、ASCII 图案、图标、字体与语言资源文件，并计算绘制所需尺寸。语言加载与翻译由 [i18n](../i18n/README.md) 负责。

## 模块

| 模块 | 职责 |
| --- | --- |
| `artwork` | 文本图案、art set、主页/启动器/文件图标类型 |
| `asset_manifest` | 必需资源清单、资源类型和默认主题 ID |
| `asset_resolver` | 环境变量、程序目录和安装位置的资源根解析 |
| `asset_store` | 资源加载与绘制尺寸统计 |
| `asset_validation` | 必需资源和默认主题检查 |
| `asset_restore` | 从内置默认内容恢复资源文件 |
| `asset_distribution` | 将规范资源复制到 Cargo 输出目录 |
| `embedded_defaults` | 共享的内置默认文件与图片字节 |
| `clock_font` | 时钟字体资源 |

## 资源格式

默认清单包含 25 项：20 个文本资源、4 个 TOML art set 和 1 个时钟字体。

- 文本资源只包含可打印 ASCII，TOML art set 使用 schema v1。
- 图片支持 GIF、JPEG、PNG 和 WebP，路径必须是资源根内的相对路径。
- 语言资源位于 `locales/<code>/`，包含 `manifest.toml` 和递归读取的 `.ftl` 文件。
- 默认图片字节由静态资源表共享，校验、修复和内存回退复用同一份数据。
- 所有资源参与尺寸统计，更新资源可能改变最小终端尺寸。

资源根按以下顺序解析：

1. `TUNDRA_ASCII_ASSETS_DIR`；
2. 程序同目录的 `assets/`；
3. Linux 的 `/usr/share/tundraux3/assets`。

## 默认资源修复

启动时检查默认图形和中英文语言资源。缺失或损坏的默认文件从内置内容恢复，健康的自定义内容保持不变。默认主题的旧配置可补充新增条目，不移除用户已有图案。

无法写入资源目录时，程序使用内存副本并报告修复结果。语言消息 ID、参数契约和回退规则见[语言资源与恢复](../i18n/docs/localization.md)。

## 验证

```sh
cargo test --locked -p ascii-assets
python scripts/check-embedded-assets.py target/release/tundra-shell.exe target/release/tundra-cli.exe
```

Linux/macOS 使用不带 `.exe` 的程序路径。检查脚本核对每张默认图片是否恰好嵌入一次；某平台完全未使用图片时可添加 `--allow-missing`，仅检查重复项。

资源文件位于 `assets/`，界面预览命令见[调试界面预览](../shell/docs/previews.md)。
