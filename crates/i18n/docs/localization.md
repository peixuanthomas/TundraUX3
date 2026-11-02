# 语言资源与恢复

TundraUX3 提供 `en-US` 和 `zh-CN`，默认语言为 `en-US`。`zh-Hans` 配置会规范化为 `zh-CN`，语言与视觉主题分别保存。

## 切换与重新加载

在“区域与时间”选择语言并确认。再次选择当前语言也会重新读取目录和资源，便于加载更新后的翻译。

加载、校验和配置保存全部成功后，Shell 一次性发布新的不可变快照；失败时保留当前语言和配置。首次设置确认语言后，后续向导使用该语言。

`tundra-cli config set language zh-CN` 使用相同的目录和校验。通过 CLI 修改配置后，现有 Shell 在下次启动或重新确认语言时读取。

## 目录结构

源码资源位于 `crates/ascii-assets/assets/locales`，发布资源位于 `assets/locales`，与 `themes` 平级。

每个语言包包含：

```text
<language>/
  manifest.toml
  common/
  modules/
  recovery/
```

`manifest.toml` 示例：

```toml
format_version = 1
code = "zh-CN"
native_name = "简体中文"
```

目录名与 `code` 使用规范语言代码，`.ftl` 文件按公共组件、业务模块和启动恢复组织。

## 编写消息

使用稳定消息 ID、完整句子和命名参数：

```ftl
files-copied =
    { $count ->
        [one] Copied one file to { $destination }.
       *[other] Copied { $count } files to { $destination }.
    }
```

翻译可使用不同的复数分支，但参数集合必须与英文契约一致。缺失翻译回退英文，构建时生成 `i18n::ids`、`MESSAGE_IDS` 和 `MESSAGE_CONTRACTS`。

Rust 中使用 `i18n::msg!` 保留消息，使用 `i18n::tr!` 在视图模型或绘制阶段格式化。`LocalizedText::Raw` 承载外部原文。

命令、动作和图标使用稳定 ID 或枚举识别，不判断翻译后的标签。配置键、协议字段、路径、用户输入、第三方输出和原始诊断保持原文。日志使用稳定事件代码和固定语言描述。

## 默认资源修复

启动和重新加载时检查默认英文、中文与图形资源：

- 缺失或损坏的默认文件从内置内容恢复，使用同目录临时文件原子替换。
- 健康的自定义资源保持不变，缺失默认条目可追加到现有文件。
- 资源目录不可写时使用内存副本，保存的语言偏好不变。
- 启动修复结果集中显示，语言重载修复随该次操作报告。

恢复界面提供“自动修复并重启”和“退出”，执行前保存编辑器恢复数据。

## 验证

```sh
cargo test --locked -p i18n
python3 scripts/check-localization.py
```

Shell 集成测试覆盖同语言重载、失败回滚、目录发现和配置写入失败；UI 测试覆盖中文宽度、长文本和命中区域。发布包需要携带完整 `assets/` 目录。
