# Windows 软件包

Windows 的“启动器 → 软件包”使用已有的 WinGet，提供已安装、更新和搜索三个页面。可以查看详情、安装、卸载、升级单个软件包，以及升级全部可更新软件包。Linux 继续使用原有的发行版软件包后端，macOS 继续通过 [Homebrew](homebrew.md) 管理软件包。

## 使用前准备

系统需要安装提供 `winget.exe` 的 App Installer（应用安装程序），并确保当前 Windows 用户能够在终端运行 WinGet。Tundra 不自动安装、修复或提升 WinGet 本身的权限。

首次使用时，在 Windows 终端运行以下只读命令，检查版本和软件源，并按 WinGet 的提示完成初始化及协议确认：

```powershell
winget --version
winget search --id Microsoft.PowerToys --exact --source winget
```

Tundra 固定使用 `winget` 软件源，不使用 Microsoft Store 的 `msstore` 源。该源的网络访问、组织策略和源协议必须已经满足。缺少 WinGet、软件源不可用或协议尚未接受时，页面显示失败原因，不把失败当作空列表，也不自动接受协议。

当前支持 WinGet 的英文、简体中文列表输出。无法识别的表头、被截断的软件包 ID 或版本会被拒绝，避免对错误目标执行操作。

## 操作与权限

默认打开“已安装”，Tab/Shift+Tab 循环切换已安装、更新、搜索；方向键选择软件包，详情中显示可执行的操作。鼠标和触屏使用同一组可见按钮。Windows 不显示 Linux 专用的软件源配置、软件包数据库修复或配置冲突操作。

软件包修改经过已有 AA 确认，操作输出和退出结果显示在任务窗口。AA 批准不会授予 Windows 管理员权限；安装器需要额外权限时，由 Windows 显示 UAC，部分安装器也会打开自己的图形窗口。

执行过程中应保持 Tundra 运行，等待任务结束。Windows 软件包操作不提供 Linux 授权助手的跨 Shell 退出保活和重新连接保证。失败后先根据输出检查实际安装状态，再决定是否重试；已经完成的安装、卸载或升级不会自动回滚。

## 验证

解析、固定命令参数和失败结果的单元测试可以在 Linux 上运行：

```sh
cargo test --locked -p platform --lib winget
```

Windows CI 同样运行这些单元测试。Linux 测试和 Windows 目标的交叉编译检查不能代替 Windows 实机运行，也不能证明 UAC、安装器窗口和真实软件包修改已验收。

在已经完成上述准备的普通 Windows 用户终端中执行只读实机测试。该测试只查询版本、已安装、搜索、详情和更新列表，不执行安装、卸载或升级。

```powershell
cargo test --locked -p platform --lib windows_winget_readonly_smoke -- --ignored --nocapture
```

真实写入验收需要使用可恢复的 Windows 测试环境，分别检查 AA 确认、UAC 接受与取消、安装器窗口、成功和失败输出，以及修改后列表刷新。键盘、鼠标、触屏和终端条件应分别记录。
