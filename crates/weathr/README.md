# weathr

`weathr` 是天气场景绘制库。各平台用于天气应用和空闲画面；Windows/macOS 还用于锁屏。它读取共享快照，不启动系统服务或持有全屏终端生命周期。

`EmbeddedDisplay::new` 接收快照、翻译函数和尺寸，`render` 返回一帧 Ratatui Buffer，支持调整尺寸，不读终端输入、不切换终端模式、不向终端输出。Shell 用它显示空闲天气，继续运行后台任务并保留原页面；画面提示按键或点按返回，认证仍由 Shell 管理。独立锁屏与内嵌画面共用同一绘制代码。

## 天气场景和锁屏

天气来自 [system-services](../system-services/README.md) 的共享快照；提供方选择和缓存由该 crate 说明。Shell 可在启动时预取天气。

天气标准化结果、昼夜、季节和动画共同决定 ASCII 房屋、树木、云、雨雪和月相等场景。锁屏支持 12/24 小时制、终端 resize、任意按键或点击终端任意位置进入；资源尺寸会抬高共同最小终端要求。Shell 锁屏模式提示进入系统，由 Shell 负责创建 watchdog 与恢复终端。锁屏 UI 发生 panic 时直接进入全屏 panic 页面，等待用户重启或退出。

宿主可通过 `run_display_blocking_with_control` 提供 `DisplayControl`，读取已经绘制的帧并送入普通终端输入；Shell 用它让外部 CLI 可以读取和进入锁屏。连接、开关和权限仍由 Shell 与 platform 管理，Weathr 不创建控制服务。
