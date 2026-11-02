# ui

`ui` 将界面数据转换为 Ratatui 布局和画面，提供共享控件、主题、焦点、命中检查、动画和自动化快照。它不管理宿主终端，不执行系统操作，也不在绘制过程中修改应用状态。

## 模块结构

| 位置 | 内容 |
| --- | --- |
| `src/foundation/` | 输入事件、焦点、命中检查和共享基础类型 |
| `src/components/` | 按钮、列表、表格、表单、弹窗和滚动条 |
| `src/screens/` | 页面数据模型、布局与绘制 |
| `src/theme/` | 主题、颜色、控件状态和动画调度 |
| `src/motion.rs`、`surface_motion.rs` | 页面、弹窗、详情和键盘的过渡计算 |
| `src/automation.rs` | 从实际绘制帧采集控件树与文字 |
| `src/terminal_images.rs` | 子终端图片裁切、缓存与回退 |
| `assets/` | 图标和时区地图，来源见[资源说明](assets/README.md) |

页面只绘制 `ShellFrameLayout.main` 中的内容和自身弹层。Shell 负责全局标题栏、状态栏、Toast 和模态弹窗，绘制与点击使用同一份布局。

## 共享控件

### 表单

`components::FormDialog` 统一处理填写弹窗的布局、输入、选项、开关、滚动和按钮。调用方提供字段与按钮，组件返回确认、取消或自定义动作，业务检查和保存由调用方完成。

```rust
let mut form = FormDialog::new("create", title, vec![
    FormField::text("name", name_label, ""),
    FormField::toggle("keep", keep_label, false),
]);
form.buttons = vec![
    FormButton::new("save", save_label, FormAction::Submit),
    FormButton::new("cancel", cancel_label, FormAction::Cancel),
];
let event = form.handle_event(input, area);
form.render_frame(frame, area, context);
```

Enter 移到下一栏并在最后一栏确认，空格或点按切换开关，Ctrl+Enter 直接确认，F10 激活默认确认按钮。需要重建表单时，用 `interaction_state()` 和 `with_interaction()` 保留滚动与拖动位置。`ManagementForm` 是兼容名称。

### 列表与页面布局

- `DataTable` 提供表格绘制、选中行、排序和列宽计算。
- `SidebarWorkspaceLayout` 提供导航、列表与详情分栏，窄窗口切换为顶部导航和上下布局。
- `ManagementWorkspace` 组织管理页的搜索、详情、操作栏和高级区域。
- `PageActionBar` 最多直接显示五个操作，支持主色、危险色、禁用和窄窗口换行。
- `ManagementViewModel::selected_rows` 表示全部已选行，`selected` 表示当前位置。
- `ScrollbarOrientation::hit_test` 将指针判定在竖条左右或横条上下扩展一格，视觉布局保持不变。

### 输入与主题

`TextInput` 提供光标、选区、滚动和密码遮罩。按钮的悬停、按下、焦点、当前项和禁用样式由 `TundraTheme` 与 `RenderContext` 统一计算。键盘、鼠标和触屏规则见 [UI 要求](../../docs/UI-requirements.md)。

## 动画

共享动画包括：

- `ease_in_out_cruise` 与 `MotionPhases`：三段时长和连续速度曲线。
- `reveal_rect`、`morph_surface`：页面、弹窗和菜单从入口展开或收回。
- `PanelMotion`、`expand_trailing_pane`：详情分栏、屏幕键盘和中途反向。
- `ANIMATION_FRAME_INTERVAL`：30 FPS 动画调度。
- `RainbowText`：按终端字符宽度着色的持续彩虹文字。

`ui::animate` 导出 animate 0.8 的 `Tween`、`Spring`、`Clock` 和宏，支持 `Rect`、`Color`、`Style` 等 Ratatui 类型：

```rust
use ui::animate::{self, Clock};
```

调用方保存动画状态，每帧使用同一个 `MotionFrame` 推进时钟；键盘操作或减少动态效果时直接到达目标。

## 子终端与图片

`TerminalImageFragment` 描述 RGBA 数据、源裁切、内容区坐标、像素边距和层级。`TerminalImageCache` 按视口裁切并缓存 Kitty、Sixel 和 iTerm2 图片。

文本终端、弹窗遮挡、布局过渡、透明或重叠图片使用字符色块合成。快照保留宽字符、斜体和原生光标形状；编辑器 Vim 区域通过 `editor_vim_terminal_area` 共用绘制、PTY 尺寸和鼠标坐标。

## 自动化快照

`automation::FrameCapture` 在外部控制开启时随实际绘制收集控件树：

1. Shell 用 `begin` 开始帧，并用 `set_group` 标记页面与弹层。
2. 控件按真实布局记录 ID、名称、区域、焦点、选中、禁用和敏感状态。
3. `finish` 读取最终 Buffer，补齐未被控件覆盖的可见文字。

密码区域在控件和整屏文字中统一遮罩，子终端导出解析后的可见文字。采集只描述画面，业务操作仍由 Shell 的正常输入流程执行。

## 验证

```sh
cargo test --locked -p ui
cargo test --locked -p ui --test automation
tundra-cli debug ui-test
```

共享控件测试覆盖按钮状态、表单、模态拦截、密码遮罩、宽字符和缩放。页面命中与完整流程由 Shell 测试和真实终端验收覆盖。

相关文档：[调试预览](../shell/docs/previews.md)、[Shell 会话](../shell/docs/session.md)、[UI 要求](../../docs/UI-requirements.md)。
