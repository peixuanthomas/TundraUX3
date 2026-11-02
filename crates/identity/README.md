# identity

`identity` 负责应用身份与 UX 权限：Linux 附着当前进程的系统用户，Windows/macOS 使用 Tundra 本地账户。

## 模块

| 模块 | 职责 |
| --- | --- |
| `identity` | `AuthSession`、身份来源和用户账户 |
| `backend` | 身份后端接口 |
| `credentials` | 密码校验、Argon2 哈希与验证 |
| `session_service` | 登录会话、失败计数和锁定 |
| `user_service` | 本地账户管理 |
| `authorization` | Guest/User/Admin 角色与权限检查 |
| `linux` | 当前 Linux 进程用户 |

## 本地账户规则

以下规则用于 Windows/macOS 和测试后端：

- 密码使用随机 salt 的 Argon2 哈希，长度为 10–256 个字符，不能全为空白或等于规范化用户名。
- 用户名匹配不区分大小写，未知用户与错误密码返回相同的无效凭据结果。
- 连续 5 次认证失败锁定 5 分钟，失败与锁定信息持久化。
- 认证会话只保存在内存中。

UX 角色分为 Guest、User 和 Admin。文件与个人资料操作面向 User/Admin；用户管理、Command Line、Launcher 管理、诊断修复和设置管理需要 Admin。

Linux 的系统账户、密码和锁定策略由操作系统管理，保存的 UX 角色不授予 Linux 权限。系统账户通过 `tundra-cli users` 和系统授权流程管理，CLI `config` 不读写身份字段。

## 验证

```sh
cargo test --locked -p identity
```
