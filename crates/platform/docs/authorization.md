# Linux 管理授权与助手连接

`platform::management::client` 和 `authorization` 为 Shell、CLI 提供固定操作、事件接收、授权保留和任务重连。AA 决定是否批准，操作系统决定进程实际权限。

## 首次授权

1. 需要内置提权时，通过 sudo 启动 `tundra-cli __privilege-session`。
2. 密码只经标准输入交给 sudo，验证后从内存清除。
3. sudo 使用带命令的 `-k`，不读取或刷新其他程序的密码缓存。
4. 授权程序继承私有双向连接，后续管理、账户、电源和任务重连复用该连接。
5. 每次操作仍遵循 AA 的批准策略。

这条连接只接受固定请求。普通用户本人资料、密码和系统已允许的电源操作沿用各自的系统授权流程。

## 连接保护与撤销

| 对象 | 规则 |
| --- | --- |
| 授权程序 | 只持有启动时继承的连接，不开放其他进程可连接的会话 socket |
| Shell | 防止同用户进程通过 ptrace 或 `/proc` 复制授权 |
| 连接描述符 | 设置 close-on-exec，不传给外部命令 |
| 退出、注销或通信失效 | 撤销连接并结束授权程序 |
| 正常服务错误或过期任务 | 返回错误，保留仍有效的授权会话 |

任务的 `control.sock` 由 root 所有且只接受 root。授权程序核对 `/run/tundraux3-management/<actor>/<operation>/control.sock` 的所有者、权限和任务身份，再将已连接的描述符交给 Shell。新 Shell 需要重新授权后才能重连。

SSH 确认使用授权连接提供的真实调用进程、启动时间、sshd 父进程和内核 TCP 状态，不采用调用者自行填写的进程编号或 `SSH_CONNECTION`。

助手通过请求接收当前语言资源副本，在内存中使用，不以 root 身份读取用户语言文件。命令参数、动作名和对象身份保持原值，事务编号通过独立事件返回。

## 固定操作与结果

`ManagementAction` 使用稳定动作名、`primary`、`group` 和固定 `values` 描述操作，按钮位置不影响执行内容。`OperationProblem` 返回稳定错误码、下一步和技术详情。

- 进程信号使用 pidfd 并核对启动标识。
- 网络配置先保存原配置，再启动独立恢复助手。
- 软件包写入保留原生命令输出和确认流程。
- 系统配置通过私有事件交给编辑器，候选内容不进入普通草稿或运行日志。
- 结果未知时先查询实际状态，不自动重试写入。

相关文档：[系统集成](integration.md)、[Linux 系统管理](linux-management.md)、[系统配置](system-config.md)、[网络与磁盘](network-disks.md)。

## 验证

```sh
cargo test --locked -p platform management:: --lib
```

固定命令、身份和输入校验由自动测试覆盖；真实 sudo/polkit 授权在对应 Linux 测试主机验收。
