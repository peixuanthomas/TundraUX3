# i18n

`i18n` 负责 Fluent 语言包加载、校验、默认资源修复和不可变语言快照。它与 `ascii-assets` 共用资源根，语言包位于 `root/locales/<code>/`。

## 语言包

每个语言包包含 `manifest.toml`：

```toml
format_version = 1
code = "zh-CN"
native_name = "简体中文"
```

`common`、`modules` 和 `recovery` 中的 `.ftl` 文件递归加载。`zh-Hans` 是 `zh-CN` 的输入别名，目录与清单使用规范代码。`LanguageCatalog::built_in()` 不访问文件系统即可提供中英文选择信息。

## 快照与消息

```rust
use std::sync::Arc;
use i18n::{LanguageSnapshot, LocalizedText};

let load = LanguageSnapshot::load_startup("assets", "zh-CN", 1);
let snapshot = Arc::new(load.snapshot);
i18n::with_snapshot(&snapshot, || {
    let label = i18n::tr!("notifications-action-ok");
    let retained = i18n::msg!("resources-repaired", count = 1, files = "manifest.toml");
    let raw = LocalizedText::from("external command output");
    assert_eq!(raw.render_current(), "external command output");
});
```

- `msg!` 保留消息 ID 和类型化参数，`tr!` 立即格式化。
- `enter_snapshot` 返回线程绑定的 RAII guard，作用域可嵌套并在线程间隔离。
- `current_snapshot()` 克隆当前线程的快照，后台助手可通过 `export()` / `restore()` 传递语言资源。
- `String` 和 `&str` 转换为 `LocalizedText` 时始终表示原文，不参与翻译。
- 渲染只读取内存，不访问磁盘或写日志。

快照分别保存当前语言、英文回退、内置英文和最小应急资源。缺失消息或格式化失败按这些层级回退，未知 ID 显示为 `[message-id]`。

## 加载与校验

严格加载检查 manifest、Fluent 语法、重复 ID、引用与循环、函数和英文参数契约。支持 `NUMBER`，缺失翻译会记录在加载诊断中并回退英文。

启动加载可跳过损坏的翻译文件，使用内置中英文资源继续运行。已有快照在磁盘文件变化后保持不变，重新加载成功后由调用方发布新快照。

构建脚本从 `crates/ascii-assets/assets/locales/en-US` 嵌入英文，并生成：

- `i18n::ids` 消息常量；
- `MESSAGE_IDS`；
- `MESSAGE_CONTRACTS` 和 `message_contract(id)`。

默认英文和中英文损坏文件通过同目录临时文件与原子重命名修复。健康文件保持原字节，缺失条目追加到合适文件，写入失败时保留内存修正并返回结构化诊断。

`render_diagnostic` 始终使用内置英文，适合错误 `Display` 和日志。`LocalizedError` 保存稳定 `event_code`、消息和可选原因链。

## 文档与验证

使用与资源恢复见[语言资源与恢复](docs/localization.md)。

```sh
cargo test --locked -p i18n
python3 scripts/check-localization.py
```

测试覆盖目录、修复、参数契约、复数回退、快照生命周期、线程隔离和序列化。
