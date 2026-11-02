# 调试界面预览

预览由 CLI 启动，Shell 和 UI 提供界面。它们使用演示数据，不执行真实管理任务，关闭后恢复终端。

## 全部共享控件

```sh
tundra-cli debug ui-test
# 或从源码运行
cargo run --locked -p cli --bin tundra-cli -- debug ui-test
```

预览使用内置深色主题和英文，不读取或保存用户配置、语言文件和日志。终端至少需要 88 列 × 30 行，推荐 120 × 36。

F1–F6、PageUp/PageDown 或底部按钮切换页面，Tab/Shift+Tab 移动焦点，Esc、Ctrl+C 或退出按钮结束。

| 页面 | 控件 |
| --- | --- |
| 基础 | Button 状态、TextInput、Surface、Panel |
| 选择 | List、DataTable、Tabs、Picker、水平和垂直 Scrollbar |
| 导航 | NavRail、SidebarWorkspaceLayout |
| 提示 | EmptyState、Skeleton、CommandStatus、Toast |
| 弹窗 | Dialog、ContextMenu、CommandPalette |
| 进度 | ProgressGauge、MetricCard、UpdateActivity |

入口为 `crates/shell/src/previews/ui_test.rs`，控件组合位于 `crates/ui/src/ui_test.rs`。

## 彩虹文本

```sh
tundra-cli debug test-rainbow-text
```

该预览展示中文、组合字符和 emoji 的持续彩虹着色。Esc、Ctrl+C、Enter 或退出按钮结束。

## 屏幕键盘

```sh
tundra-cli debug screen-keyboard
```

此演示只编辑本次会话的示例文本，“测试 AA”只展示输入弹窗，不执行命令或申请授权。实际页面的入口见 [AA 与命令行的屏幕键盘](session.md#aa-与命令行的屏幕键盘)。

| 部分 | 行为 |
| --- | --- |
| 布局 | QWERTY 位于下半屏，宽窗口增加功能键和导航区 |
| 修饰键 | 点击锁定，再次点击释放，可组合多个键 |
| 编辑 | Backspace 删除，Enter/Tab 插入换行或制表符 |
| 剪贴板 | 复制全文，粘贴追加内容 |
| 工具栏 | 收起、展开、清空和退出 |
| 设置 | 读取当前主题、动画速度和减少动态效果 |

实体按键高亮与鼠标锁定状态分别保存，失焦、收起或缩放后清除。弹窗和键盘使用同一布局计算绘制与点击位置。

## 界面与动画样式

```sh
tundra-cli debug view-ui-style 2
# Command Line 中使用
/debug view-ui-style 2
```

预览至少需要 60 × 24，建议 100 × 35 或更大。

| 版本 | 风格 |
| --- | --- |
| `1` Glacier | 边框组件、直接进度更新和扫入效果 |
| `2` Tea | 无边框列表与表单、留白和 cubic 进度 |
| `3` Spring | 卡片、弹簧进度和柔和入场，与正式界面共享实现 |

F1–F3 切换版本，F4 切换减少动态效果，F5 重播，Tab 移动焦点，Esc 先关闭弹窗再退出。预览只修改本次会话的演示状态，入口为 `crates/shell/src/previews/style.rs` 和 `crates/ui/src/style_preview.rs`。
