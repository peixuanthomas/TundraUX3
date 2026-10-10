# 用户管理 / User management

用户管理从启动器打开。Linux 页面读取系统账户查询接口（NSS），包括 root、服务账户和普通账户；目录服务不支持枚举时，可用“按名称查找账户”。来源为远程目录的账户和用户组只读。Windows/macOS 继续使用原有账户页面。

## 查询账户

按名称查找支持 `DOMAIN\user` 和 `user@domain`。列表选中账户后，再用 `id --groups` 读取完整组编号，并按编号查询组名；不要求目录服务支持用户组枚举。查询失败时显示未知，当前账户的组信息未查明时不能修改其附加组或管理员组，避免意外移除管理权限。

## 可修改内容与保护

Linux 中可以修改显示名称、密码、主用户组、附加用户组、登录 Shell、账户有效期和 SSH 公钥，也可以创建、重命名、删除用户组及修改成员。Shell 从 `/etc/shells` 选择；有效期填写 `YYYY-MM-DD` 或 `never`。SSH 公钥位置由 `sshd -T -C` 读取，支持按用户设置的 `AuthorizedKeysFile`；公钥以 SHA256 指纹展示和移除，其他公钥及其选项保持原样。

每次写入都经过 AA 和实际系统授权。保存的 Tundra 角色不能授予 Linux 权限。AccountsService 可用且支持操作时使用该服务；明确缺失或不支持时调用固定的系统账户工具。服务拒绝授权、超时或未确认结果时，刷新账户状态再决定下一步，不换后端重复执行。

root 不能删除；当前账户不能删除、锁定、立即过期或移出已识别的管理员组。删除账户保留主目录。正在被用作主组的用户组不能删除。新账户在密码设置失败时保留账户和文件，页面提示继续设置密码。

## CLI

CLI 与页面共享 `platform::management::users` 的操作及输入检查。可用 `tundra-cli users` 的帮助查看正式子命令；查询输出支持 JSON，密码通过隐藏输入或输入描述符传递。

## English summary

User management opens from Launcher. Linux enumerates NSS accounts, including root and system accounts, and supports exact name lookup for non-enumerating directories. Directory accounts/groups remain read-only. Changes use actual OS authorization; stored desktop roles grant no Linux privileges. Supported AccountsService operations are reused, with native system tools only when the service is explicitly absent or unsupported. A denied or unconfirmed write is never retried through another backend.

Exact lookups accept `DOMAIN\user` and `user@domain`. Full membership is queried only for the selected or explicitly requested account using numeric `id --groups` output and keyed NSS group lookups. Unknown membership is displayed explicitly and blocks changes that could remove the current account's administrator groups.

Account deletion retains homes. The current login and root deletion are protected. SSH public key edits preserve other entries and existing file ownership, permissions, ACLs and extended attributes, reject symlink replacement, and replace the file atomically. Hardware/privileged host acceptance remains separate from automated tests.
