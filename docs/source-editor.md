# 编辑器：Microsoft Edit 风格界面

界面参考 [Microsoft Edit](https://github.com/microsoft/edit/tree/cb6de0a8966aecd01d19a3541dab1829d0e679ca/crates/edit/src/bin/edit)，固定参考提交 `cb6de0a8966aecd01d19a3541dab1829d0e679ca`。菜单为文件、编辑、视图和帮助，取消常驻工具栏及正文外框，查找和替换直接显示在正文上方。底部提供语言、换行、编码、缩进、行列位置和文件切换入口。

普通编辑器使用现有 Ratatui/Shell 界面：按钮、主题、强调色、边框形状、弹窗焦点和全局标题栏由 Tundra 的共享组件管理。Microsoft Edit 的绘制框架未直接复制；已有 LSH 语法高亮代码继续使用其 MIT 许可版本。F8 可进入下文的真实 Vim 界面。

左侧保留行号，新增行绿色、修改行黄色、删除位置红色；保存后清除标记。软换行续行仍显示原行对应的 `↪`。

Vim 关闭时，Ctrl+N 创建独立文档；Ctrl+O 选择文件后保留先前文档，取消选择直接回到原文档。Ctrl+P 或底部文件名打开当前这组文档的列表，Ctrl+Tab / Ctrl+Shift+Tab 切换，Ctrl+W 关闭当前文件，Ctrl+Q 逐个关闭这一组文件。未保存文件仍逐个询问保存、丢弃或取消。每个文档保留独立的撤销、光标、搜索选项、文件任务与恢复草稿。

普通编辑器中，F10 聚焦菜单，Alt+F/E/V/H 打开对应菜单，方向键和 Enter 操作，Esc 返回正文。视图菜单可聚焦底部信息行，再用 Tab、方向键和 Enter 选择设置。Linux 配置文件操作入口改为 Ctrl+F10；配置检查、差异预览、权限确认和私密内容保护继续沿用原流程。

编码、语言、换行和缩进直接显示选项，选择后立即应用，Esc 关闭。弹窗按文字和字段数量决定尺寸；小窗口减少留白，长内容保留滚动，按钮放不下时换行。“关于”只显示关闭按钮。普通文档中的“系统配置文件…”直接打开路径表单，说明要填写系统配置文件的绝对路径，不再经过只有一个选项的中间菜单，也不会自动填入普通文档的路径。

普通 Source 编辑器以 `ropey::Rope` 保存可编辑文本。显示、搜索及语法标记使用原文的 UTF-8 字节范围；界面定位和修改边界遵守 Unicode grapheme cluster。Vim 开启期间，编辑和撤销由 Vim 自己的缓冲处理，宿主同步文档内容和保存状态。

## 使用

以下为普通编辑器的操作；Vim 开启后使用其原生命令和窗口。

| 操作 | 入口 | 行为 |
| --- | --- | --- |
| 查找 | Ctrl+F / 编辑 → 查找 | 普通字符串、正则、区分大小写、整词及向前查找。显示所有匹配，当前匹配成为选区。 |
| 下一处／上一处 | F3 / Shift+F3 | 循环导航，包括零宽正则匹配。 |
| 替换 | Ctrl+R、Ctrl+H / 编辑 → 替换 | 在第二个输入框填写替换文本；留空表示删除。Enter 替换一处，Ctrl+Enter 替换全部，也可点击对应按钮。 |
| 关闭查找栏 | Esc / 关闭按钮 | 保留查找内容、替换内容和选项，返回正文。 |
| 转到行 | Ctrl+G / 视图 → 转到行 | 输入从 1 开始的行号或 `行:列`。 |
| 换行与编码 | 底部换行、编码按钮 | LF / CRLF / CR 转换；UTF-8、UTF-8 BOM、UTF-16LE、UTF-16BE。转换可撤销，UTF-16 保存时写入 BOM。 |
| 缩进 | 底部缩进按钮 / Tab、Shift+Tab | 选择空格或制表符及缩进宽度；选中多行后可一次增加或减少缩进。 |
| 软换行 | Alt+Z / 视图 → 切换软换行 | 默认关闭。按显示宽度折行，续行显示 `↪`；不修改文件、不产生撤销记录。 |
| 格式化 JSON | Ctrl+Shift+J | 两空格缩进，每个字段及数组元素单独成行；一次撤销恢复。 |

正则采用 Rust `regex` 语法，默认启用多行锚点；跨行匹配可用 `(?s)`。替换支持 `$0`、`$1`、`${name}` 和 `$$`；普通字符串模式保留 `$` 原样。不支持正则后向引用及 look-around，非法表达式会留在表单中显示错误。会切开组合字符、emoji 或 CRLF 的匹配被跳过。

普通文档在后台计算替换，计算期间更改查询、文档或关闭查找栏会取消旧请求；应用前再次核对文档编号和修改版本，整批修改一次撤销。单次替换限制为 10,000 处或 8 MiB，超过时提示缩小范围。管理员配置文件保留原来的替换预览确认：显示前 40 处，每处前后文本最多显示 100 个字符；确认时应用完整结果。只读文档允许查找、复制、新建和打开其他文件，拒绝修改当前内容及保存格式。

当前编码选择覆盖 Unicode 文本，没有接入 Microsoft Edit 使用 ICU 提供的全部旧编码。制表符仍沿用现有文本显示规则，缩进宽度控制插入的空格数量；真实触屏和不同终端需要独立验收。

JSON 格式化先验证完整 JSON，再只调整字符串外的空白；保留字段顺序、重复键、数字写法、字符串转义、UTF-8 BOM、首选换行风格及末尾换行状态。逗号仍按合法 JSON 语法保留，显示不再挤在一行。无效 JSON 返回位置错误并保留原文。格式化会将混合换行统一为文档的首选风格；保存仍使用原有保存流程。

## 真实 Vim 模式

F8 或 Vim 按钮开启固定版本的真实终端 Vim。编辑区域直接显示 Vim 的模式提示、待完成按键、命令行、错误、窗口和帮助；Normal、Insert、Replace、Visual、Visual Line、Visual Block、次数、operator/text object、寄存器、宏、搜索和 Ex 命令都由 Vim 处理。`Ctrl+W` 用于 Vim 窗口操作；`:help` 打开随包的原生帮助。

| 操作 | 行为 |
| --- | --- |
| F8 / VIM模式已启用 | 同步当前未保存内容并回到普通编辑器，不自动保存文件；Vim 进程保留。 |
| 再按 F8 | 同步普通编辑器中的修改，继续原 Vim 进程，保留已有寄存器、宏及撤销记录。 |
| `:w`、`:write 文件名` | 将文档写入请求送给宿主，等待现有保存任务完成；保存成功后才确认。 |
| `:wq` | 保存成功后按 Vim 规则退出；失败时保留未保存内容并显示错误。 |
| `:q`、`:q!`、`:qa!` | 按 Vim 窗口与缓冲规则退出；未保存检查、强制放弃由 Vim 处理。 |

通过 Vim 文档命令发起的写入沿用已有保存 worker、外部变更检查和权限处理。普通文件的 `:w!` 可按这次观察到的目标状态覆盖，仍检查文件指纹；若实际写入前又被外部修改，会拒绝覆盖。

受保护系统配置继续走差异预览、配置检查、授权和备份，只读文档继续拒绝修改/保存。受保护配置目前仅接受全文、UTF-8、无 BOM，输出字节须与宿主审核的文本一致；部分写入、追加或写入时额外进行编码、BOM、行尾等字节转换会被拒绝。需要其他编码或格式时，可按 Vim 的写入参数保存到普通路径的副本，仍使用当前用户权限和既有保存流程。

普通编辑器支持 UTF-8（含 BOM）、UTF-16LE 和 UTF-16BE。Vim 支持的其他 `fileencoding` 可在 Vim 内保存，但 F8 返回前，已加载的普通缓冲需改为宿主支持的编码；例如使用 `:set fileencoding=utf-8`。存在其他编码时会提示并保留 Vim，不把它默认为 UTF-8 后继续普通编辑。普通运行按 Vim 的方式加载用户配置和插件，插件所需的外部组件由用户环境提供。

便携包保留 Shell 旁的 `vim/`。缺少随包及用户目录引擎时，开启 Vim 会显示安装弹窗，从项目 Release 下载对应版本的预编译包，显示进度并支持取消；安装成功后自动进入 Vim，失败或取消不改动文档。对应引擎 Release 需要先发布。开发时仍可执行 `python3 scripts/build-vim.py --output target/debug/vim`；release 或自定义 target 将目录放到对应 Shell 旁，普通 Cargo 构建不下载 Vim。调试可用 `TUNDRA_VIM` 指定其他兼容的完整 Vim，无效路径仍直接报错；`TUNDRA_VIM_CLEAN=1` 跳过个人配置和插件，适合隔离回归。依赖和固定版本见[随包 Vim](packaging/vim.md)。

Linux 验收使用真实 PTY 与隔离配置。Windows/macOS 已接入各自构建和运行时 CI，仍需在对应 runner 验证，Linux 结果不能替代。测试入口见[真实 Vim 回归](testing.md#真实-vim-回归)。

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
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored
python3 scripts/tests/editor-vim-smoke.py target/debug/tundra-shell target/debug/vim/vim
```

覆盖跨行语法、主题与选区优先级、UTF-8/grapheme、捕获组替换、零宽匹配、过期预览、只读保护、原子撤销、JSON 保真、软换行点击定位及键盘移动和缩放。
