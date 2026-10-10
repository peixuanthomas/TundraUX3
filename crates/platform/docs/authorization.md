# Linux 管理授权与助手连接

`platform::management::client` 和 `authorization` 为 Shell、CLI 共用：发送固定操作、接收事件、维护授权连接和重新连接任务。AA 决定是否批准；Linux 决定进程实际能做什么。

## 首次授权

1. 需要内置提权时，通过 sudo 启动 `tundra-cli __privilege-session`，提示授权保留到本次 Shell 退出。
2. 密码只经标准输入传给 sudo，验证后清除，不写入参数、文件或日志。
3. sudo 使用带命令的 `-k`，既不读取，也不刷新其他程序可使用的密码缓存。
4. 授权程序继承私有双向连接，后续系统管理、账户、电源和任务重连复用它；每次请求仍经过 AA。

这条连接不允许执行任意命令。普通用户本人资料、密码及系统已经允许的电源操作仍保留原有系统授权流程，见[系统集成](integration.md#系统服务和授权)。

## 连接保护与撤销

| 对象 | 要求 |
| --- | --- |
| 授权程序 | 只持有启动时继承的连接，不创建其他进程可连接的会话 socket，不保存密码或令牌文件 |
| Shell | 禁止同用户进程通过 ptrace 或 `/proc` 复制授权 |
| 连接描述符 | 带 close-on-exec 标记，不泄漏给外部命令 |
| 退出、注销或通信失效 | 撤销连接，授权程序退出；已启动的独立任务继续由助手管理 |
| 正常服务错误或过期任务 | 返回具体错误，不因此撤销整个有效授权会话 |

任务的 `control.sock` 保持 root 所有且只接受 root。授权程序核对固定路径 `/run/tundraux3-management/<actor>/<operation>/control.sock` 的所有者、权限和任务身份，再把已经连接的描述符交给当前 Shell；不能仅凭知道路径接管任务。新 Shell 需要重新授权才能重连。

## 固定操作与结果

`ManagementAction` 使用稳定动作名及 `primary`、`group`、固定 `values` 描述操作，不能由按钮位置决定执行什么。`OperationProblem` 返回稳定错误码、下一步和技术详情，不能把未知结果当成成功或自动重试写入。

进程信号使用 pidfd 并核对启动标识，避免 PID 复用后操作错误进程。网络配置先保存原配置，再由 systemd 启动独立的 `__network-rollback` 助手；确认与恢复规则见[网络和磁盘](network-disks.md)。

软件包写入的确认、断线和重连见 [Linux 管理](linux-management.md)。配置会话经私有事件交给现有编辑器，内容不得进入普通恢复草稿或运行日志，完整保存要求见[系统配置](system-config.md)。

固定命令和输入校验测试通过，不等于真实 sudo/polkit 授权交互已验收；必须分别记录对应主机的结果。
