# ascii-assets

`ascii-assets` 加载和校验主题、ASCII 图案、图标及字体，统计绘制所需尺寸，并提供内置资源用于修复。语言源码也随资源分发；加载和翻译由 `i18n` 负责。

默认资源表使用一份静态数据，校验、修复和内存回退共用其中的图片字节，避免在同一个可执行文件中重复嵌入 PNG。

语言资源包含防火墙、SSH、配置恢复和定时任务的中英文界面文字；新增启动器入口复用现有图片，并提供各自的 ASCII 图标。旧图标目录会补入四个新入口，保留已有的自定义图案。

定时任务后端的表单、确认和错误提示集中在中英文 `modules/management-scheduled-tasks.ftl`，由 `i18n` 语言快照传给后台线程和授权助手。

中英文资源也包含运行期尺寸提示、可移植表单快捷键、管理能力与备份空状态说明；账户来源和任务名称使用各自的业务标签。

重建发布程序后，可运行 `python scripts/check-embedded-assets.py target/release/tundra-shell.exe target/release/tundra-cli.exe` 检查每张默认图片是否恰好嵌入一次；Linux/macOS 使用不带 `.exe` 的程序路径。脚本逐字节匹配源码中的完整 PNG，同时检查遗漏和重复。

若某个平台的程序没有用到内置图片，编译器可以完全移除这些字节。此时添加 `--allow-missing`，只检查有无重复；脚本仍会打印实际图片数量和字节数。

## 资源校验

资源清单固定要求 25 项：20 个文本资源、4 个 TOML art set 和 1 个时钟字体。文本资源只能包含可打印 ASCII；TOML art set 使用 schema v1。图片路径不得是绝对路径或包含路径穿越，并且只允许 GIF、JPEG、PNG、WebP。

资源根可由 `TUNDRA_ASCII_ASSETS_DIR` 指定，也会从二进制同目录查找；Linux 在前两者未命中时再回退至 `/usr/share/tundraux3/assets`。所有资源都会参与尺寸统计，因此资源更新可能改变启动时的最小终端要求。
