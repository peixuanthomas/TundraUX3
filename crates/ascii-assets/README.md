# ascii-assets

`ascii-assets` 加载和校验主题、ASCII 图案、图标及字体，统计绘制所需尺寸，并提供内置资源用于修复。语言源码也随资源分发；加载和翻译由 `i18n` 负责。

## 资源校验

资源清单固定要求 25 项：20 个文本资源、4 个 TOML art set 和 1 个时钟字体。文本资源只能包含可打印 ASCII；TOML art set 使用 schema v1。图片路径不得是绝对路径或包含路径穿越，并且只允许 GIF、JPEG、PNG、WebP。

资源根可由 `TUNDRA_ASCII_ASSETS_DIR` 指定，也会从二进制同目录查找；Linux 在前两者未命中时再回退至 `/usr/share/tundraux3/assets`。所有资源都会参与尺寸统计，因此资源更新可能改变启动时的最小终端要求。
