# weathr

`weathr` 是天气场景绘制库。Linux 从天气应用进入；Windows/macOS 还用于锁屏。它读取共享快照，不启动系统服务或持有全屏终端生命周期。

## 天气场景和锁屏

天气来自 [system-services](../system-services/README.md) 的共享快照；提供方选择和缓存由该 crate 说明。Shell 可在启动时预取天气。

天气标准化结果、昼夜、季节和动画共同决定 ASCII 房屋、树木、云、雨雪和月相等场景。锁屏支持 12/24 小时制、终端 resize、任意按键或点击终端任意位置进入；资源尺寸会抬高共同最小终端要求。Shell 锁屏模式提示进入系统，由 Shell 负责创建 watchdog 与恢复终端。锁屏 UI 发生 panic 时直接进入全屏 panic 页面，等待用户重启或退出。
