# weathr

`weathr` 是天气场景绘制库，用于各平台的天气应用和空闲画面，Windows/macOS 还用于锁屏。它读取共享快照，不启动系统服务，也不持有全屏终端生命周期。

## 内嵌绘制

`EmbeddedDisplay::new` 接收天气快照、翻译函数和尺寸，`render` 返回一帧 Ratatui Buffer，支持调整尺寸。它不会读取终端输入、切换终端模式或直接向终端输出。

天气、昼夜、季节和动画共同决定房屋、树木、云、雨雪和月相等场景。天气数据与缓存由 [system-services](../system-services/README.md) 提供，Shell 可在启动时预取。

Shell 用同一绘制代码显示天气应用、空闲画面和锁屏。空闲时后台任务继续运行，按键或点按返回原页面，认证和页面状态由 Shell 管理。

## 独立锁屏与控制

锁屏支持 12/24 小时制、终端 resize，以及任意按键或点击进入。宿主负责 watchdog、终端模式和 panic 页面。

`run_display_blocking_with_control` 接受 `DisplayControl`，让宿主读取已绘制帧并发送正常终端输入。Shell 用它支持外部 CLI 的天气画面与 `wake`，连接、开关和权限仍由 Shell 与 platform 管理。

## 验证

```sh
cargo test --locked -p weathr
bash scripts/check-weathr-dependency-boundary.sh
```

依赖边界检查确保数据绘制不会反向引入 Shell 或系统运行时。
