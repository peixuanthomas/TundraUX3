# 编辑器与脚本工作区

内置编辑器提供多文档编辑、语法着色、查找替换、软换行、JSON 格式化和真实 Vim 模式。Linux 脚本工作区复用同一编辑器，并提供 Bash 运行与调试功能。

## 普通编辑器

界面参考 [Microsoft Edit](https://github.com/microsoft/edit/tree/cb6de0a8966aecd01d19a3541dab1829d0e679ca/crates/edit/src/bin/edit)，菜单分为文件、编辑、视图和帮助。查找与替换栏显示在正文上方，底部提供语言、换行、编码、缩进、行列位置和文件切换入口。

- Ctrl+N 新建文档，Ctrl+O 打开文件，Ctrl+P 查看文档列表。
- Ctrl+Tab / Ctrl+Shift+Tab 切换文档，Ctrl+W 关闭当前文档，Ctrl+Q 逐个关闭整组文档。
- 每份文档独立保存撤销历史、光标、搜索选项、文件任务和恢复草稿。
- F10 聚焦菜单，Alt+F/E/V/H 打开对应菜单，Esc 返回正文。
- Linux 系统配置文件入口为 Ctrl+F10，保存时经过差异预览、配置检查、授权和备份。
- 行号旁使用绿色、黄色和红色标记新增、修改和删除位置，保存后清除。

文本由 `ropey::Rope` 保存。界面定位和编辑边界按 Unicode grapheme cluster 处理，搜索与语法标记使用 UTF-8 字节范围。

## 常用操作

| 操作 | 入口 | 说明 |
| --- | --- | --- |
| 查找 | Ctrl+F / 编辑 → 查找 | 支持普通字符串、正则、区分大小写、整词和向前查找 |
| 下一处／上一处 | F3 / Shift+F3 | 循环浏览匹配，包括零宽正则匹配 |
| 替换 | Ctrl+R、Ctrl+H / 编辑 → 替换 | Enter 替换一处，Ctrl+Enter 替换全部；留空替换文本表示删除 |
| 关闭查找栏 | Esc / 关闭按钮 | 保留查询、替换文本和选项 |
| 转到行 | Ctrl+G / 视图 → 转到行 | 输入从 1 开始的行号或 `行:列` |
| 换行与编码 | 底部按钮 | 支持 LF/CRLF/CR，以及 UTF-8、UTF-8 BOM、UTF-16LE、UTF-16BE |
| 缩进 | 底部按钮 / Tab、Shift+Tab | 设置空格或制表符及宽度，可调整多行缩进 |
| 软换行 | Alt+Z / 视图菜单 | 按显示宽度折行，续行显示 `↪`，不修改文件 |
| 格式化 JSON | Ctrl+Shift+J | 使用两空格缩进，一次撤销恢复 |

正则使用 Rust `regex` 语法，默认启用多行锚点，跨行匹配可使用 `(?s)`。替换支持 `$0`、`$1`、`${name}` 和 `$$`。可能切开组合字符、emoji 或 CRLF 的匹配会被跳过。

替换在后台计算，应用前核对文档编号、修改版本和原文，整批修改一次撤销。单次替换最多处理 10,000 处或 8 MiB 数据。系统配置的替换预览显示前 40 处，确认后应用完整结果。

JSON 格式化先验证完整文档，再调整字符串外的空白，保留字段顺序、重复键、数字写法、字符串转义和 BOM。无效 JSON 会报告位置并保留原文。

## 真实 Vim 模式

F8 或 Vim 按钮切换到随包的真实 Vim。编辑区域直接显示 Vim 的模式、选区、寄存器、宏、窗口、命令行、搜索、错误和 `:help`，按键由 Vim 处理。

| 操作 | 行为 |
| --- | --- |
| F8 / VIM模式已启用 | 同步未保存内容并返回普通编辑器，不自动写文件 |
| 再按 F8 | 同步普通编辑器修改并继续原 Vim 进程 |
| `:w`、`:write 文件名` | 通过宿主保存任务写入，成功后确认 |
| `:wq` | 保存成功后退出；失败时保留修改 |
| `:q`、`:q!`、`:qa!` | 按 Vim 的窗口、缓冲和放弃规则处理 |

Vim 写入沿用宿主的异步保存、外部变更检查和权限流程。系统配置仍经过差异、检查、授权和备份；受保护配置接受全文 UTF-8 无 BOM 写入。

普通编辑器支持 UTF-8 和 UTF-16。Vim 中使用其他 `fileencoding` 后，返回普通编辑器前需转换为宿主支持的编码。正常运行会加载用户 Vim 配置和插件。

便携包将完整引擎放在 Shell 旁的 `vim/`。缺少引擎时，安装弹窗可从项目 Release 下载预编译包。开发构建使用：

```sh
python3 scripts/build-vim.py --output target/debug/vim
```

`TUNDRA_VIM` 可指定其他完整 Vim；`TUNDRA_VIM_CLEAN=1` 用于隔离个人配置和插件。构建与发布细节见[随包 Vim](packaging/vim.md)，测试入口见[真实 Vim 回归](testing.md#真实-vim-回归)。

## Linux 脚本工作区

从启动器打开“脚本编辑器”，或在脚本文档中选择“脚本 → 脚本工作区”。新建脚本使用 `#!/usr/bin/env bash`。

运行设置可指定解释器、参数和工作目录。留空时读取脚本首行；`.bash` 默认 Bash，其他无声明脚本默认 `/bin/sh`。参数解析引号和转义，不执行命令替换或变量展开。断点调试需要 Bash 5.2 或更新版本，调试器使用随包 bashdb。

| 操作 | 快捷键 |
| --- | --- |
| 运行／调试或继续 | Ctrl+F5／F5 |
| 单步跳过／步入／跳出 | F7／F11／Shift+F11 |
| 停止／切换断点 | Shift+F5／F9 |
| 切换区域／反向切换 | F6／Shift+F6 |
| 区域内操作／编辑器菜单 | Tab、Enter／F10 |
| 表达式求值／执行 Bash 命令 | Enter／Ctrl+Enter |

- 修改后的脚本保存成功后启动，执行期间当前文档只读。
- 界面提供源码、输出、变量、调用栈、表达式和断点区域，所有操作均有可点击按钮。
- 表达式在暂停时求值；“执行命令”可修改调试中的变量。输出区直接接收脚本按键和粘贴。
- 函数调用和断点列表可定位 `source` 引入的文件。后台命中断点后保持暂停，可从通知返回。
- 运行结束后保留可滚动输出。关闭页面时可继续后台运行、停止并关闭或留在页面。
- 断点和表达式历史属于当前文档页。无效断点显示“未生效”，实际命中后确认有效。

## 实现结构

- `crates/app/src/editor/syntax.rs` 向 LSH 提供逐行文本并维护跨行状态，缓存 token 类型和字节范围。
- `crates/ui/src/theme/mod.rs` 的 `TundraTheme::syntax_style` 将 token 映射到主题颜色。
- `crates/app/src/editor/search.rs` 实现独立搜索引擎，替换通过 Source 事务一次提交。
- `crates/app/src/editor/soft_wrap.rs` 生成逻辑行与显示行映射，光标、鼠标、滚动和行号共用该映射。
- LSH 使用 [Microsoft Edit](https://github.com/microsoft/edit/tree/1be69c85a63bcd475a0b397196d0be3dbed60023/crates/lsh) 的 MIT 许可组件，源码位于 `third_party/lsh`。

## 验证

```sh
cargo test --locked -p app -p ui -p shell --test editor --test editor_advanced
cargo test --locked -p shell --lib controller::management::config_editor::tests
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored
python3 scripts/tests/editor-vim-smoke.py target/debug/tundra-shell target/debug/vim/vim
```
