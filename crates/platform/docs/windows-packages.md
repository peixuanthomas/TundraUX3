# Windows 软件包

Windows 的“启动器 → 软件包”使用 WinGet，提供已安装、更新、搜索和详情，支持安装、卸载、升级所选软件包和全部升级。

## 使用前准备

安装提供 `winget.exe` 的 App Installer，并确保当前用户能在终端运行 WinGet。首次使用时检查版本和软件源，按 WinGet 提示完成初始化及协议确认：

```powershell
winget --version
winget search --id Microsoft.PowerToys --exact --source winget
```

Tundra 固定使用 `winget` 源。缺少 WinGet、软件源不可用或协议尚未接受时，页面显示具体失败原因。

列表解析支持英文和简体中文。无法识别的表头、被截断的软件包 ID 或版本会被拒绝，避免对错误目标执行操作。

## 操作与权限

默认打开“已安装”，Tab/Shift+Tab 切换已安装、更新和搜索，详情中显示可用操作。Windows 页面不提供 Linux 的软件源编辑、数据库修复或配置冲突功能。

软件包修改经过 AA 确认，输出和退出结果显示在任务窗口。AA 负责应用内批准，安装器所需的管理员权限由 Windows/UAC 处理，部分安装器会打开自己的图形窗口。

执行过程中保持 Tundra 运行并等待任务结束。失败后先根据输出和列表刷新检查实际状态，已完成的安装、卸载或升级不会自动回滚。

## 验证

```sh
cargo test --locked -p platform --lib winget
```

在已完成 WinGet 准备的普通 Windows 用户终端运行只读实机检查：

```powershell
cargo test --locked -p platform --lib windows_winget_readonly_smoke -- --ignored --nocapture
```

该用例只查询版本、已安装、搜索、详情和更新列表。真实写入验收使用可恢复的 Windows 环境，分别检查 AA、UAC、安装器窗口、成功与失败输出及修改后的列表刷新。
