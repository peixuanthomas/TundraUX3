# Source Editor：高亮、搜索、软换行与 JSON 格式化

Source Editor 继续以 `ropey::Rope` 为唯一可编辑文本缓冲。显示、搜索及语法标记使用原文的 UTF-8 字节范围；界面定位和修改边界遵守 Unicode grapheme cluster。

## 使用

| 操作 | 入口 | 行为 |
| --- | --- | --- |
| 查找 | Ctrl+F / 编辑 → 查找 | 普通字符串、正则、区分大小写、整词及向前查找。显示所有匹配，当前匹配成为选区。 |
| 下一处／上一处 | F3 / Shift+F3 | 循环导航，包括零宽正则匹配。 |
| 替换 | Ctrl+H / 编辑 → 替换 | 输入替换文本，选择当前／下一个匹配或全部匹配。留空表示删除。 |
| 提交表单 | Ctrl+Enter，或选择提交按钮 | 替换先生成预览，再次提交才应用；Esc 取消。整批修改可一次撤销。 |
| 软换行 | Alt+Z / 视图 → 切换软换行 | 默认关闭。按显示宽度折行，续行显示 `↪`；不修改文件、不产生撤销记录。 |
| 格式化 JSON | Ctrl+Shift+J / 格式 → 格式化 JSON | 两空格缩进，每个字段及数组元素单独成行；一次撤销恢复。 |

正则采用 Rust `regex` 语法，默认启用多行锚点；跨行匹配可用 `(?s)`。替换支持 `$0`、`$1`、`${name}` 和 `$$`；普通字符串模式保留 `$` 原样。不支持正则后向引用及 look-around，非法表达式会留在表单中显示错误。会切开组合字符、emoji 或 CRLF 的匹配被跳过。

替换预览显示前 40 处，每处原文和替换文本最多显示 100 个字符；应用时使用完整结果。文档在预览后发生修改或更换时，旧预览不会执行。只读文档允许查找，拒绝替换和格式化。

JSON 格式化先验证完整 JSON，再只调整字符串外的空白；保留字段顺序、重复键、数字写法、字符串转义、UTF-8 BOM、首选换行风格及末尾换行状态。逗号仍按合法 JSON 语法保留，显示不再挤在一行。无效 JSON 返回位置错误并保留原文。格式化会将混合换行统一为文档的首选风格；保存仍使用原有保存流程。

## 实现边界

- `crates/app/src/editor/syntax.rs` 从 Rope 逐行向 LSH 提供文本，保持跨行词法状态，仅返回 token 类型和字节范围。结果按缓冲修改与文件路径缓存；主题改变不需重新解析。
- `crates/ui/src/theme/mod.rs` 的 `TundraTheme::syntax_style` 将 token 映射到现有主题角色；选区样式优先于匹配标记及语法样式。
- `crates/app/src/editor/search.rs` 是独立 `SearchEngine`。`EditorCommand::ReplaceMatches` 验证预览版本、原文、排序及 grapheme 边界，再通过已有 Source 事务一次提交；沿用撤销、恢复及保存逻辑。
- `crates/app/src/editor/soft_wrap.rs` 按显示单元宽度生成逻辑行到显示行的映射，保留完整 grapheme。映射按缓冲修改和视口宽度缓存，鼠标、光标、滚动和行号使用同一份数据。保留一个行末光标单元及稳定的滚动条区域；缩放后重新排版。Home/End 仍定位逻辑行首尾。
- 语法分析及换行映射在首次请求或缓存失效后按文档重建；普通视口滚动复用缓存。搜索仅在请求时构建结果，同版本 F3 导航复用匹配范围。

LSH 来自 [Microsoft Edit](https://github.com/microsoft/edit/tree/1be69c85a63bcd475a0b397196d0be3dbed60023/crates/lsh)，固定提交 `1be69c85a63bcd475a0b397196d0be3dbed60023`，保留 MIT 许可证于 `third_party/lsh/LICENSE`。仅复用 LSH 编译器、内置定义、路径匹配与运行时；不引入 Edit 的文本缓冲、颜色或终端绘制代码。

## 验证

```sh
cargo test --locked -p app -p ui -p shell --test editor --test editor_advanced
cargo test --locked -p shell --lib controller::management::config_editor::tests
```

覆盖跨行语法、主题与选区优先级、UTF-8/grapheme、捕获组替换、零宽匹配、过期预览、只读保护、原子撤销、JSON 保真、软换行点击定位及键盘移动和缩放。
