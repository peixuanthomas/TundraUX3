# ui

`ui` 把界面数据变成终端布局和画面，提供按钮、列表、输入、焦点、命中检查、主题和动画。它不管理终端模式，不执行系统操作，也不在绘制时修改应用状态。

| 位置 | 内容 |
| --- | --- |
| `src/foundation/` | 输入事件、焦点、命中检查与共享基础类型 |
| `src/components/` | 按钮、列表、表单、弹窗等通用控件 |
| `src/screens/` | 各页面的数据模型、布局与绘制 |
| `src/theme/` | 主题、颜色和控件状态样式 |
| `assets/` | 嵌入图标和时区地图，来源与许可见[资源说明](assets/README.md) |

页面只绘制 `ShellFrameLayout.main` 内的内容与页面弹层。Shell 最终绘制全局标题栏、状态栏、Toast 和模态弹窗。绘制和点击必须使用同一份布局。

所有控件遵守[统一 UI 要求](../../docs/UI-requirements.md)，具体页面合成见 [Shell 会话](../shell/docs/session.md)。通用交互要求只在规范中维护，避免各页面文档重复定义。

验证：`cargo test --locked -p ui`；交互变化还应验证键盘、鼠标/触摸命中及真实终端绘制。
