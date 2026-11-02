# 配置备份与恢复

“配置备份与恢复”按操作列出时间、来源、相关文件和结果。每次修改先保存原件与属性，再记录实际结果；同次操作涉及多个文件时放在一条记录中。

## 记录与恢复

共享记录位于 `/var/lib/tundraux3/config-backups/operations`，目录为 `0700`，文件为 `0600`。内容可包括修改前文本、UID/GID、权限、ACL、扩展属性和修改后版本。

恢复前重新检查当前版本和候选配置，并预览差异。发现后续外部修改时停止；没有检查器时需要明确选择未检查恢复。恢复本身也会保存当前原件，完成后重新读取核对。

配置保存、服务加载和连接验证分别报告结果。SSH 与网络恢复继续使用各自的新连接确认流程。

编辑器、网络、公钥、防火墙、软件源、fstab、服务定义和计划任务都写入统一记录。软件包事务、外部程序修改和普通服务启停不属于文件配置备份。

## 浏览与清理

- 搜索框按文字筛选，筛选面板设置起止日期和来源，日期使用系统时区。
- 每批显示 50 条，可在全部匹配记录中排序。
- 列表读取摘要，打开详情时才读取正文和计算差异；摘要索引可从旧记录重建。
- “保留／取消保留”防止记录被清理，重建索引不影响保留标记。

清理先按当前筛选预览将删除的恢复点、相关文件和预计释放空间，确认后重新核对同一批记录。正在使用、仍用于网络/SSH确认、被其他保留记录引用或已经变化的记录不会删除。

## 账户与用户组

账户恢复只处理所选操作影响的账户、用户组和属性，支持创建、删除、密码、锁定、显示名称、Shell、有效期、主组和附加组等操作。

恢复前检查当前值以及名称、UID/GID 占用，恢复后重新读取系统账户数据。恢复 Shell 不覆盖后来修改的显示名称，恢复某用户的组成员资格时保留其他成员。

删除账户和撤销创建都保留主目录。当前账户、root 和远程目录账户的既有保护仍然有效。

密码只保存受保护的哈希，差异仅显示“密码已变化”。结果未确认的记录需要先检查实际状态，再决定是否可以恢复。

## 验证

```sh
cargo test --locked -p platform management::config_backups --lib
cargo test --locked -p platform management::users::recovery --lib
```

真实账户恢复测试以 root 在独立挂载空间运行：

```sh
unshare --mount --propagation private --fork TEST_BINARY \
  --exact management::users::recovery::tests::real_isolated_account_recovery_preserves_unrelated_changes_and_homes \
  --ignored --nocapture --test-threads=1
```

测试使用临时 `/etc`、`/run`、`/var/lib`、`/var/log`、`/var/spool` 和 `/home`，覆盖属性恢复、主目录保留、组成员、UID/GID 冲突和 subuid/subgid。
